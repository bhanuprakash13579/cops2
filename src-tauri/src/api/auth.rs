use std::sync::Arc;
use axum::{extract::{Path, State}, http::StatusCode, Json};
use bcrypt::{hash, verify, DEFAULT_COST};
use serde_json::{json, Value};

use crate::{
    auth::{create_token, create_admin_token, AuthUser, AdminUser, ADMIN_USERNAME, ADMIN_PWD_HASH},
    db::DbPool,
    models::user::*,
};

// ── Login rate limiting ───────────────────────────────────────────────────────
// 10 failed attempts per user_id within a 5-minute rolling window.

static LOGIN_ATTEMPTS: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<String, (u32, std::time::Instant)>>
> = std::sync::OnceLock::new();

const MAX_LOGIN_ATTEMPTS: u32 = 10;
const LOGIN_WINDOW_SECS: u64 = 300;

// The administrator credential is a single shared password that does not
// rotate, and it can restore over the database. Tighter than an officer's, and
// counted globally rather than per name — there is only one admin account, so
// per-name counting would let an attacker reset the counter by varying the name.
static ADMIN_ATTEMPTS: std::sync::OnceLock<
    std::sync::Mutex<(u32, std::time::Instant)>
> = std::sync::OnceLock::new();
const MAX_ADMIN_ATTEMPTS: u32 = 5;
const ADMIN_WINDOW_SECS: u64 = 900;   // 15 minutes

type Db = State<Arc<DbPool>>;

pub async fn login(State(pool): Db, Json(req): Json<LoginRequest>) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    // ── Rate limit check ─────────────────────────────────────────────────────
    let limiter = LOGIN_ATTEMPTS.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
    {
        let mut map = limiter.lock().unwrap();
        let entry = map.entry(req.user_id.clone()).or_insert((0, std::time::Instant::now()));
        if entry.1.elapsed().as_secs() >= LOGIN_WINDOW_SECS {
            *entry = (0, std::time::Instant::now());
        }
        if entry.0 >= MAX_LOGIN_ATTEMPTS {
            return Err(err429("Too many login attempts. Please wait 5 minutes before trying again."));
        }
        entry.0 += 1;
    }

    let conn = pool.get().map_err(|e| err500(&e.to_string()))?;

    let user: Option<(String, String, String, Option<String>, Option<String>, i64)> = conn
        .query_row(
            "SELECT user_id, user_pwd, user_role, user_desig, user_status, COALESCE(is_user_admin, 0) FROM users WHERE user_id = ? AND (user_status IS NULL OR user_status != 'CLOSED')",
            [&req.user_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        )
        .optional()
        .map_err(|e| err500(&e.to_string()))?;

    let (user_id, pwd_hash, role, desig, status, is_user_admin) = user
        .ok_or_else(|| err401("Invalid credentials"))?;

    let status = status.unwrap_or_else(|| "ACTIVE".to_string());

    if !verify(&req.password, &pwd_hash).unwrap_or(false) {
        return Err(err401("Invalid credentials"));
    }

    // ── Module-type access control ───────────────────────────────────────────
    // Prevent SDOs logging into the adjudication module and vice-versa.
    if let Some(ref mt) = req.module_type {
        let allowed: &[&str] = match mt.to_lowercase().as_str() {
            "sdo"          => &["SDO"],
            "adjudication" => &["DC", "AC"],
            _              => &["SDO", "DC", "AC"],
        };
        if !allowed.contains(&role.as_str()) {
            return Err(err403("Your role is not permitted to access this module."));
        }
    }

    // ── Successful login: reset rate-limit counter ───────────────────────────
    if let Ok(mut map) = limiter.lock() {
        map.remove(&req.user_id);
    }

    let name: String = conn
        .query_row("SELECT user_name FROM users WHERE user_id = ?", [&user_id], |r| r.get(0))
        .map_err(|e| err500(&e.to_string()))?;

    let token = create_token(&user_id, &role, &name, desig.as_deref(), &status)
        .map_err(|e| err500(&e.to_string()))?;

    Ok(Json(json!({
        "access_token": token,
        "token_type": "bearer",
        "user_name": name,
        "user_id": user_id,
        "user_role": role,
        "user_desig": desig,
        "user_status": status,
        "is_user_admin": is_user_admin == 1,
    })))
}

/// Confirms the signed-in caller is the designated **user admin** — an active AC
/// or DC holding the `is_user_admin` flag. Read from the database on every call,
/// never from the token: revoking the flag, handing it over, or closing the
/// account then takes effect at once rather than lingering until a token expires.
fn require_user_admin(conn: &rusqlite::Connection, user_id: &str)
    -> Result<(), (StatusCode, Json<Value>)>
{
    let row: Option<(i64, String, Option<String>)> = conn.query_row(
        "SELECT COALESCE(is_user_admin, 0), user_role, user_status FROM users WHERE user_id = ?",
        [user_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    ).optional().map_err(|e| err500(&e.to_string()))?;

    let (flag, role, status) = row.ok_or_else(|| err403("Only the user administrator may manage user accounts."))?;
    let active = status.as_deref() != Some("CLOSED");
    let acdc   = role == "AC" || role == "DC";
    if flag == 1 && acdc && active {
        Ok(())
    } else {
        Err(err403("Only the user administrator may manage user accounts."))
    }
}

// Listing the office's users is itself a user-admin action now — the in-app
// management screen is the only caller, and no one else needs the roster.
pub async fn list_users(State(pool): Db, auth: AuthUser) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let conn = pool.get().map_err(|e| err500(&e.to_string()))?;
    require_user_admin(&conn, &auth.0.sub)?;
    let mut stmt = conn.prepare(
        "SELECT id, user_name, user_desig, user_id, user_role, user_status, created_on,
                COALESCE(is_user_admin, 0)
         FROM users WHERE (user_status IS NULL OR user_status != 'CLOSED')
         ORDER BY user_role, user_name"
    ).map_err(|e| err500(&e.to_string()))?;

    let users: Vec<Value> = stmt.query_map([], |r| {
        Ok(json!({
            "id": r.get::<_, i64>(0)?,
            "user_name": r.get::<_, String>(1)?,
            "user_desig": r.get::<_, Option<String>>(2)?,
            "user_id": r.get::<_, String>(3)?,
            "user_role": r.get::<_, String>(4)?,
            "user_status": r.get::<_, Option<String>>(5)?,
            "created_on": r.get::<_, Option<String>>(6)?,
            "is_user_admin": r.get::<_, i64>(7)? == 1,
        }))
    }).map_err(|e| err500(&e.to_string()))?
    .filter_map(|r| r.ok())
    .collect();

    Ok(Json(json!(users)))
}

pub async fn create_user(State(pool): Db, auth: AuthUser, Json(req): Json<CreateUserRequest>) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    if !["SDO", "DC", "AC"].contains(&req.user_role.as_str()) {
        return Err(err400("Invalid role. Must be SDO, DC, or AC."));
    }
    // A blank login ID or name makes an account nobody can sign into or identify.
    if req.user_id.trim().is_empty() {
        return Err(err400("A login ID is required."));
    }
    if req.user_name.trim().is_empty() {
        return Err(err400("A name is required."));
    }

    // Creating accounts is the user admin's job, and only theirs. It used to be
    // open to every signed-in officer with per-role rules, which let an SDO make
    // a fresh DC account and sign in as it — the same privilege escalation, one
    // step round. Now one designated AC/DC owns account creation for the office,
    // and may create any of the three roles; no one else reaches this at all.
    let conn = pool.get().map_err(|e| err500(&e.to_string()))?;
    require_user_admin(&conn, &auth.0.sub)?;

    let exists: i64 = conn.query_row(
        "SELECT COUNT(*) FROM users WHERE user_id = ?",
        [&req.user_id], |r| r.get(0),
    ).unwrap_or(0);
    if exists > 0 {
        return Err(err409("A user with this login ID already exists."));
    }

    let pwd_hash = hash(&req.password, DEFAULT_COST).map_err(|e| err500(&e.to_string()))?;
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();

    conn.execute(
        "INSERT INTO users (user_name, user_desig, user_id, user_pwd, user_role, user_status, created_on) VALUES (?,?,?,?,?,?,?)",
        rusqlite::params![req.user_name, req.user_desig, req.user_id, pwd_hash, req.user_role, "ACTIVE", today],
    ).map_err(|e| err400(&e.to_string()))?;

    Ok(Json(json!({ "message": "User created." })))
}

pub async fn update_user(State(pool): Db, auth: AuthUser, Path(id): Path<i64>, Json(req): Json<serde_json::Value>) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let conn = pool.get().map_err(|e| err500(&e.to_string()))?;
    // Editing someone's status or name is a user-admin action. Left open, this
    // let any signed-in officer set another account — the user admin's included —
    // to CLOSED and lock them out. The screen never called it; the guard costs
    // no legitimate use.
    require_user_admin(&conn, &auth.0.sub)?;

    if let Some(status) = req.get("user_status").and_then(|v| v.as_str()) {
        conn.execute("UPDATE users SET user_status = ? WHERE id = ?", rusqlite::params![status, id])
            .map_err(|e| err500(&e.to_string()))?;
    }
    if let Some(name) = req.get("user_name").and_then(|v| v.as_str()) {
        conn.execute("UPDATE users SET user_name = ? WHERE id = ?", rusqlite::params![name, id])
            .map_err(|e| err500(&e.to_string()))?;
    }
    Ok(Json(json!({ "message": "User updated." })))
}

pub async fn delete_user(State(pool): Db, auth: AuthUser, Path(id): Path<i64>) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let conn = pool.get().map_err(|e| err500(&e.to_string()))?;

    // Anyone may close their OWN account; closing someone else's is the user
    // admin's to do. That is the only widening — an ordinary officer still cannot
    // reach past their own account.
    let target_user_id: Option<String> = conn.query_row(
        "SELECT user_id FROM users WHERE id = ?",
        rusqlite::params![id], |r| r.get(0),
    ).optional().map_err(|e| err500(&e.to_string()))?;
    let target_user_id = target_user_id.ok_or_else(|| err404("User not found"))?;
    if target_user_id != auth.0.sub {
        require_user_admin(&conn, &auth.0.sub)?;
    }

    // The user admin must not be closed out of existence — neither by themselves
    // nor by anyone — while they still hold the role, or the office is left with
    // no one able to manage accounts. The role is handed over first, then the
    // account can be closed.
    let holds_admin: i64 = conn.query_row(
        "SELECT COALESCE(is_user_admin, 0) FROM users WHERE id = ?", [id], |r| r.get(0),
    ).optional().map_err(|e| err500(&e.to_string()))?.unwrap_or(0);
    if holds_admin == 1 {
        return Err(err400("This account is the user admin. Hand the role over first, then close it."));
    }

    conn.execute("UPDATE users SET user_status = 'CLOSED', closed_on = ? WHERE id = ?",
        rusqlite::params![chrono::Local::now().format("%Y-%m-%d").to_string(), id])
        .map_err(|e| err500(&e.to_string()))?;
    Ok(Json(json!({ "message": "User closed." })))
}

pub async fn change_password(State(pool): Db, auth: AuthUser, Json(req): Json<ChangePasswordRequest>) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let conn = pool.get().map_err(|e| err500(&e.to_string()))?;
    let pwd_hash: String = conn
        .query_row("SELECT user_pwd FROM users WHERE user_id = ?", [&auth.0.sub], |r| r.get(0))
        .map_err(|_| err404("User not found"))?;

    if !verify(&req.old_password, &pwd_hash).unwrap_or(false) {
        return Err(err400("Current password is incorrect."));
    }
    let new_hash = hash(&req.new_password, DEFAULT_COST).map_err(|e| err500(&e.to_string()))?;

    conn.execute(
        "UPDATE users SET user_pwd = ?, user_status = 'ACTIVE' WHERE user_id = ?",
        rusqlite::params![new_hash, auth.0.sub],
    ).map_err(|e| err500(&e.to_string()))?;

    Ok(Json(json!({ "message": "Password changed." })))
}

// ── Bootstrap check ──────────────────────────────────────────────────────────
// Called by the login page on mount to detect a first-run (no users in DB).

const MODULE_ROLES: &[(&str, &[&str])] = &[
    ("sdo",          &["SDO"]),
    ("adjudication", &["DC", "AC"]),
    ("query",        &["SDO", "DC", "AC"]),
    ("apis",         &["SDO", "DC", "AC"]),
];

pub async fn bootstrap(State(pool): Db, Path(module_type): Path<String>) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let conn = pool.get().map_err(|e| err500(&e.to_string()))?;

    let roles: Vec<&str> = MODULE_ROLES.iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(&module_type))
        .map(|(_, r)| r.to_vec())
        .unwrap_or_else(|| vec!["SDO", "DC", "AC"]);

    // Build an IN clause dynamically
    let placeholders = roles.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT COUNT(*) FROM users WHERE user_status = 'ACTIVE' AND user_role IN ({})",
        placeholders
    );
    let params: Vec<&dyn rusqlite::ToSql> = roles.iter().map(|r| r as &dyn rusqlite::ToSql).collect();
    let count: i64 = conn.query_row(&sql, params.as_slice(), |r| r.get(0))
        .map_err(|e| err500(&e.to_string()))?;

    if count == 0 {
        return Ok(Json(json!({
            "bootstrap_needed": true,
            "credentials": {
                "username": "sysadmin",
                "password": "(your admin password)",
                "message": "No user accounts have been created yet. Click the lock icon (top-right) to open the Admin Panel, log in with your administrator credentials, then create at least one user."
            }
        })));
    }
    Ok(Json(json!({ "bootstrap_needed": false })))
}

// ── /me — current user profile ───────────────────────────────────────────────

pub async fn me(State(pool): Db, auth: AuthUser) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let conn = pool.get().map_err(|e| err500(&e.to_string()))?;
    let row: Option<(String, Option<String>, String, Option<String>, i64)> = conn.query_row(
        "SELECT user_name, user_desig, user_role, user_status, COALESCE(is_user_admin, 0) FROM users WHERE user_id = ?",
        [&auth.0.sub],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
    ).optional().map_err(|e| err500(&e.to_string()))?;

    let (name, desig, role, status, is_user_admin) = row.ok_or_else(|| err404("User not found"))?;
    Ok(Json(json!({
        "user_id": auth.0.sub,
        "user_name": name,
        "user_desig": desig,
        "user_role": role,
        "user_status": status,
        "is_user_admin": is_user_admin == 1,
    })))
}

// ── upgrade-role ──────────────────────────────────────────────────────────────

pub async fn upgrade_role(
    State(pool): Db,
    auth: AuthUser,
    Path(user_id): Path<String>,
    Json(req): Json<serde_json::Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let new_role = req.get("user_role").and_then(|v| v.as_str())
        .ok_or_else(|| err400("user_role is required"))?;
    if !["SDO", "DC", "AC"].contains(&new_role) {
        return Err(err400("Invalid role. Must be SDO, DC, or AC"));
    }
    let conn = pool.get().map_err(|e| err500(&e.to_string()))?;
    // Changing a role is user management, which now belongs to the user admin
    // alone. It used to be open to any DC, which sat outside the one-keeper model
    // the office asked for — a DC who was not the user admin could still reshape
    // the roster. Only the user admin reaches it now.
    require_user_admin(&conn, &auth.0.sub)?;
    // Never let this be the way the user-admin flag moves — an AC carrying the
    // flag must not be silently demoted, nor a promotion double as a handover.
    // Handover has its own guarded path.
    if new_role == "SDO" {
        let holds: i64 = conn.query_row(
            "SELECT COALESCE(is_user_admin, 0) FROM users WHERE user_id = ?",
            [&user_id], |r| r.get(0),
        ).optional().map_err(|e| err500(&e.to_string()))?.unwrap_or(0);
        if holds == 1 {
            return Err(err400("This user is the user admin. Hand the role over first, then change their role."));
        }
    }
    let affected = conn.execute(
        "UPDATE users SET user_role = ? WHERE user_id = ?",
        rusqlite::params![new_role, user_id],
    ).map_err(|e| err500(&e.to_string()))?;
    if affected == 0 { return Err(err404("User not found")); }
    Ok(Json(json!({ "message": format!("Role updated to {new_role}") })))
}

// ── Reset a user's password (user admin) ──────────────────────────────────────
// Sets a temporary password and marks the account TEMP, so the user is required
// to choose their own the next time they sign in. The admin never learns the
// permanent password — they set only the one-time value the user replaces.
pub async fn reset_password(
    State(pool): Db,
    auth: AuthUser,
    Path(id): Path<i64>,
    Json(req): Json<serde_json::Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let conn = pool.get().map_err(|e| err500(&e.to_string()))?;
    require_user_admin(&conn, &auth.0.sub)?;

    let temp = req.get("temp_password").and_then(|v| v.as_str()).unwrap_or("");
    if temp.chars().count() < 6 {
        return Err(err400("Temporary password must be at least 6 characters."));
    }
    // A closed account must not be revived by a reset — setting it TEMP would put
    // it back in the sign-in table. Reopening a closed account is the system
    // admin's call, not a password reset.
    let status: Option<Option<String>> = conn.query_row(
        "SELECT user_status FROM users WHERE id = ?", [id], |r| r.get(0),
    ).optional().map_err(|e| err500(&e.to_string()))?;
    match status {
        None => return Err(err404("User not found")),
        Some(s) if s.as_deref() == Some("CLOSED") =>
            return Err(err400("This account is closed. It must be reopened by the system administrator before a password can be set.")),
        _ => {}
    }
    let pwd_hash = hash(temp, DEFAULT_COST).map_err(|e| err500(&e.to_string()))?;
    conn.execute(
        "UPDATE users SET user_pwd = ?, user_status = 'TEMP' WHERE id = ?",
        rusqlite::params![pwd_hash, id],
    ).map_err(|e| err500(&e.to_string()))?;
    Ok(Json(json!({ "message": "Temporary password set. The user must choose a new one at next sign-in." })))
}

// ── Hand over the user-admin role (user admin) ────────────────────────────────
// The holder passes the role to another active AC or DC. Done in one transaction
// so the office is never left with two holders or none.
pub async fn transfer_user_admin(
    State(pool): Db,
    auth: AuthUser,
    Json(req): Json<serde_json::Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let mut conn = pool.get().map_err(|e| err500(&e.to_string()))?;
    require_user_admin(&conn, &auth.0.sub)?;

    let target = req.get("user_id").and_then(|v| v.as_str())
        .ok_or_else(|| err400("user_id is required"))?;
    if target == auth.0.sub {
        return Err(err400("You already hold the user-admin role."));
    }
    let row: Option<(String, Option<String>)> = conn.query_row(
        "SELECT user_role, user_status FROM users WHERE user_id = ?",
        [target], |r| Ok((r.get(0)?, r.get(1)?)),
    ).optional().map_err(|e| err500(&e.to_string()))?;
    let (role, status) = row.ok_or_else(|| err404("The chosen user was not found."))?;
    if !(role == "AC" || role == "DC") {
        return Err(err400("The user admin must be an AC or a DC."));
    }
    if status.as_deref() == Some("CLOSED") {
        return Err(err400("Cannot hand the role to a closed account."));
    }

    let tx = conn.transaction().map_err(|e| err500(&e.to_string()))?;
    tx.execute("UPDATE users SET is_user_admin = 0 WHERE user_id = ?", [&auth.0.sub])
        .map_err(|e| err500(&e.to_string()))?;
    tx.execute("UPDATE users SET is_user_admin = 1 WHERE user_id = ?", [target])
        .map_err(|e| err500(&e.to_string()))?;
    tx.commit().map_err(|e| err500(&e.to_string()))?;
    Ok(Json(json!({ "message": format!("The user-admin role now belongs to {target}.") })))
}

// ── Admin login ───────────────────────────────────────────────────────────────
// Username: "sysadmin" (hardcoded)
// Password: from ADMIN_PASSWORD or ADMIN_PWD_HASH environment variable at startup

pub async fn admin_login(Json(req): Json<serde_json::Value>) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let username = req.get("username").and_then(|v| v.as_str()).unwrap_or("");
    let password = req.get("password").and_then(|v| v.as_str()).unwrap_or("");

    // ── Rate limit ───────────────────────────────────────────────────────────
    //
    // Officer logins were throttled and this was not, which is backwards: this
    // is ONE shared password that never rotates, and holding it means being able
    // to restore over the database, read every case, and change every setting.
    // It was the most valuable credential in the program and the only one an
    // attacker could guess at unlimited speed.
    //
    // Stricter than the officer limit, and counted globally rather than per
    // username: there is only one admin account, so per-name counting would let
    // an attacker reset the counter by varying the name they send.
    //
    // Checked BEFORE the username comparison, so a wrong username costs an
    // attempt too — otherwise the limit is trivially bypassed.
    {
        let limiter = ADMIN_ATTEMPTS.get_or_init(|| std::sync::Mutex::new((0u32, std::time::Instant::now())));
        let mut guard = limiter.lock().unwrap();
        if guard.1.elapsed().as_secs() >= ADMIN_WINDOW_SECS {
            *guard = (0, std::time::Instant::now());
        }
        if guard.0 >= MAX_ADMIN_ATTEMPTS {
            let wait = ADMIN_WINDOW_SECS.saturating_sub(guard.1.elapsed().as_secs());
            return Err(err429(&format!(
                "Too many administrator sign-in attempts. Try again in {} minutes.",
                wait / 60 + 1
            )));
        }
        guard.0 += 1;
    }

    if username != ADMIN_USERNAME {
        return Err(err401("Invalid admin credentials"));
    }

    let hash = ADMIN_PWD_HASH.as_deref().ok_or_else(|| {
        err500("Admin password not configured. Rebuild with ADMIN_PASSWORD env var set.")
    })?;

    if !verify(password, hash).unwrap_or(false) {
        return Err(err401("Invalid admin credentials"));
    }

    // A correct password clears the counter: an administrator who mistypes a
    // few times then gets it right should not stay locked out.
    if let Some(l) = ADMIN_ATTEMPTS.get() {
        *l.lock().unwrap() = (0, std::time::Instant::now());
    }

    let token = create_admin_token().map_err(|e| err500(&e.to_string()))?;
    Ok(Json(json!({
        "access_token": token,
        "token_type": "bearer",
        "username": ADMIN_USERNAME,
        "role": "system_admin",
    })))
}

// ── Admin user management (requires system_admin JWT) ─────────────────────────

pub async fn admin_list_users(State(pool): Db, _admin: AdminUser) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let conn = pool.get().map_err(|e| err500(&e.to_string()))?;
    let mut stmt = conn.prepare(
        "SELECT id, user_name, user_desig, user_id, user_role, user_status, created_on, closed_on,
                COALESCE(is_user_admin, 0)
         FROM users ORDER BY user_role, user_name"
    ).map_err(|e| err500(&e.to_string()))?;

    let users: Vec<Value> = stmt.query_map([], |r| {
        Ok(json!({
            "id":          r.get::<_, i64>(0)?,
            "user_name":   r.get::<_, String>(1)?,
            "user_desig":  r.get::<_, Option<String>>(2)?,
            "user_id":     r.get::<_, String>(3)?,
            "user_role":   r.get::<_, String>(4)?,
            "user_status": r.get::<_, Option<String>>(5)?,
            "created_on":  r.get::<_, Option<String>>(6)?,
            "closed_on":   r.get::<_, Option<String>>(7)?,
            "is_user_admin": r.get::<_, i64>(8)? == 1,
        }))
    }).map_err(|e| err500(&e.to_string()))?
    .filter_map(|r| r.ok())
    .collect();

    Ok(Json(json!(users)))
}

pub async fn admin_create_user(State(pool): Db, _admin: AdminUser, Json(req): Json<CreateUserRequest>) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    if !["SDO", "DC", "AC"].contains(&req.user_role.as_str()) {
        return Err(err400("Invalid role. Must be SDO, DC, or AC."));
    }
    let conn = pool.get().map_err(|e| err500(&e.to_string()))?;

    let exists: i64 = conn.query_row(
        "SELECT COUNT(*) FROM users WHERE user_id = ?",
        [&req.user_id], |r| r.get(0),
    ).unwrap_or(0);
    if exists > 0 {
        return Err(err409("A user with this login ID already exists."));
    }

    // The user-admin flag can only rest on an AC or a DC — the office's
    // account-keeper is an adjudicating officer, never ordinary staff.
    let make_admin = req.is_user_admin.unwrap_or(false);
    if make_admin && !(req.user_role == "AC" || req.user_role == "DC") {
        return Err(err400("Only an AC or a DC can be made the user admin."));
    }

    let pwd_hash = hash(&req.password, DEFAULT_COST).map_err(|e| err500(&e.to_string()))?;
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();

    conn.execute(
        "INSERT INTO users (user_name, user_desig, user_id, user_pwd, user_role, user_status, created_on, is_user_admin) VALUES (?,?,?,?,?,?,?,?)",
        rusqlite::params![req.user_name, req.user_desig, req.user_id, pwd_hash, req.user_role, "ACTIVE", today, make_admin as i64],
    ).map_err(|e| err400(&e.to_string()))?;

    Ok(Json(json!({ "message": "User created." })))
}

pub async fn admin_update_user(State(pool): Db, _admin: AdminUser, Path(id): Path<i64>, Json(req): Json<serde_json::Value>) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let conn = pool.get().map_err(|e| err500(&e.to_string()))?;

    // Validate the user-admin flag against the role this edit will LEAVE the
    // account with — the role being set in this same request, or the current one
    // — before writing anything. Checking after the writes could reject the flag
    // only once the role and password had already been changed, leaving a
    // half-applied edit behind.
    let make_admin = req.get("is_user_admin").and_then(|v| v.as_bool());
    if make_admin == Some(true) {
        let effective_role: String = match req.get("user_role").and_then(|v| v.as_str()) {
            Some(r) => r.to_string(),
            None => conn.query_row("SELECT user_role FROM users WHERE id=?", [id], |r| r.get(0))
                .optional().map_err(|e| err500(&e.to_string()))?
                .ok_or_else(|| err404("User not found"))?,
        };
        if !(effective_role == "AC" || effective_role == "DC") {
            return Err(err400("Only an AC or a DC can be made the user admin."));
        }
    }

    if let Some(status) = req.get("user_status").and_then(|v| v.as_str()) {
        conn.execute("UPDATE users SET user_status=? WHERE id=?", rusqlite::params![status, id])
            .map_err(|e| err500(&e.to_string()))?;
    }
    if let Some(name) = req.get("user_name").and_then(|v| v.as_str()) {
        conn.execute("UPDATE users SET user_name=? WHERE id=?", rusqlite::params![name, id])
            .map_err(|e| err500(&e.to_string()))?;
    }
    if let Some(role) = req.get("user_role").and_then(|v| v.as_str()) {
        conn.execute("UPDATE users SET user_role=? WHERE id=?", rusqlite::params![role, id])
            .map_err(|e| err500(&e.to_string()))?;
    }
    if let Some(desig) = req.get("user_desig").and_then(|v| v.as_str()) {
        conn.execute("UPDATE users SET user_desig=? WHERE id=?", rusqlite::params![desig, id])
            .map_err(|e| err500(&e.to_string()))?;
    }
    if let Some(pwd) = req.get("password").and_then(|v| v.as_str()) {
        if !pwd.is_empty() {
            let pwd_hash = hash(pwd, DEFAULT_COST).map_err(|e| err500(&e.to_string()))?;
            conn.execute("UPDATE users SET user_pwd=? WHERE id=?", rusqlite::params![pwd_hash, id])
                .map_err(|e| err500(&e.to_string()))?;
        }
    }
    // Designate or remove the user admin — already validated against the final
    // role above.
    if let Some(mk) = make_admin {
        conn.execute("UPDATE users SET is_user_admin=? WHERE id=?",
                     rusqlite::params![mk as i64, id])
            .map_err(|e| err500(&e.to_string()))?;
    }
    Ok(Json(json!({ "message": "User updated." })))
}

pub async fn admin_soft_delete_user(State(pool): Db, _admin: AdminUser, Path(id): Path<i64>) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let conn = pool.get().map_err(|e| err500(&e.to_string()))?;
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    conn.execute(
        "UPDATE users SET user_status='CLOSED', closed_on=? WHERE id=?",
        rusqlite::params![today, id],
    ).map_err(|e| err500(&e.to_string()))?;
    Ok(Json(json!({ "message": "User closed." })))
}

pub async fn admin_hard_delete_user(State(pool): Db, _admin: AdminUser, Path(id): Path<i64>) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let conn = pool.get().map_err(|e| err500(&e.to_string()))?;

    // Only CLOSED users may be permanently deleted — prevents accidental data loss.
    let status: Option<Option<String>> = conn.query_row(
        "SELECT user_status FROM users WHERE id=?",
        rusqlite::params![id], |r| r.get(0),
    ).optional().map_err(|e| err500(&e.to_string()))?;

    match status {
        None => return Err(err404("User not found.")),
        Some(s) if s.as_deref() != Some("CLOSED") => {
            return Err(err400("Only CLOSED users can be permanently deleted. Close the account first."));
        }
        _ => {}
    }

    conn.execute("DELETE FROM users WHERE id=?", rusqlite::params![id])
        .map_err(|e| err500(&e.to_string()))?;
    Ok(Json(json!({ "message": "User permanently deleted." })))
}

// ── Helpers ───────────────────────────────────────────────────────────────────
fn err400(msg: &str) -> (StatusCode, Json<Value>) { (StatusCode::BAD_REQUEST,          Json(json!({ "detail": msg }))) }
fn err401(msg: &str) -> (StatusCode, Json<Value>) { (StatusCode::UNAUTHORIZED,         Json(json!({ "detail": msg }))) }
fn err403(msg: &str) -> (StatusCode, Json<Value>) { (StatusCode::FORBIDDEN,            Json(json!({ "detail": msg }))) }
fn err404(msg: &str) -> (StatusCode, Json<Value>) { (StatusCode::NOT_FOUND,            Json(json!({ "detail": msg }))) }
fn err409(msg: &str) -> (StatusCode, Json<Value>) { (StatusCode::CONFLICT,             Json(json!({ "detail": msg }))) }
fn err429(msg: &str) -> (StatusCode, Json<Value>) { (StatusCode::TOO_MANY_REQUESTS,    Json(json!({ "detail": msg }))) }
fn err500(msg: &str) -> (StatusCode, Json<Value>) { (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "detail": msg }))) }

trait OptionalExt<T> {
    fn optional(self) -> rusqlite::Result<Option<T>>;
}
impl<T> OptionalExt<T> for rusqlite::Result<T> {
    fn optional(self) -> rusqlite::Result<Option<T>> {
        match self {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}
