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

    let user: Option<(String, String, String, Option<String>, Option<String>)> = conn
        .query_row(
            "SELECT user_id, user_pwd, user_role, user_desig, user_status FROM users WHERE user_id = ? AND (user_status IS NULL OR user_status != 'CLOSED')",
            [&req.user_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()
        .map_err(|e| err500(&e.to_string()))?;

    let (user_id, pwd_hash, role, desig, status) = user
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
    })))
}

pub async fn list_users(State(pool): Db, _auth: AuthUser) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let conn = pool.get().map_err(|e| err500(&e.to_string()))?;
    let mut stmt = conn.prepare(
        "SELECT id, user_name, user_desig, user_id, user_role, user_status, created_on
         FROM users WHERE (user_status IS NULL OR user_status = 'ACTIVE')
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
        }))
    }).map_err(|e| err500(&e.to_string()))?
    .filter_map(|r| r.ok())
    .collect();

    Ok(Json(json!(users)))
}

pub async fn create_user(State(pool): Db, _auth: AuthUser, Json(req): Json<CreateUserRequest>) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
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

    let pwd_hash = hash(&req.password, DEFAULT_COST).map_err(|e| err500(&e.to_string()))?;
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();

    conn.execute(
        "INSERT INTO users (user_name, user_desig, user_id, user_pwd, user_role, user_status, created_on) VALUES (?,?,?,?,?,?,?)",
        rusqlite::params![req.user_name, req.user_desig, req.user_id, pwd_hash, req.user_role, "ACTIVE", today],
    ).map_err(|e| err400(&e.to_string()))?;

    Ok(Json(json!({ "message": "User created." })))
}

pub async fn update_user(State(pool): Db, _auth: AuthUser, Path(id): Path<i64>, Json(req): Json<serde_json::Value>) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let conn = pool.get().map_err(|e| err500(&e.to_string()))?;

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

    // Users may only close their own account.
    let target_user_id: Option<String> = conn.query_row(
        "SELECT user_id FROM users WHERE id = ?",
        rusqlite::params![id], |r| r.get(0),
    ).optional().map_err(|e| err500(&e.to_string()))?;
    let target_user_id = target_user_id.ok_or_else(|| err404("User not found"))?;
    if target_user_id != auth.0.sub {
        return Err(err403("You may only close your own account."));
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
    let row: Option<(String, Option<String>, String, Option<String>)> = conn.query_row(
        "SELECT user_name, user_desig, user_role, user_status FROM users WHERE user_id = ?",
        [&auth.0.sub],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    ).optional().map_err(|e| err500(&e.to_string()))?;

    let (name, desig, role, status) = row.ok_or_else(|| err404("User not found"))?;
    Ok(Json(json!({
        "user_id": auth.0.sub,
        "user_name": name,
        "user_desig": desig,
        "user_role": role,
        "user_status": status,
    })))
}

// ── upgrade-role ──────────────────────────────────────────────────────────────

pub async fn upgrade_role(
    State(pool): Db,
    auth: AuthUser,
    Path(user_id): Path<String>,
    Json(req): Json<serde_json::Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    // Only DC users are permitted to change roles.
    if auth.0.role != "DC" {
        return Err(err403("Only DC users can upgrade roles."));
    }
    let new_role = req.get("user_role").and_then(|v| v.as_str())
        .ok_or_else(|| err400("user_role is required"))?;
    if !["SDO", "DC", "AC"].contains(&new_role) {
        return Err(err400("Invalid role. Must be SDO, DC, or AC"));
    }
    let conn = pool.get().map_err(|e| err500(&e.to_string()))?;
    let affected = conn.execute(
        "UPDATE users SET user_role = ? WHERE user_id = ?",
        rusqlite::params![new_role, user_id],
    ).map_err(|e| err500(&e.to_string()))?;
    if affected == 0 { return Err(err404("User not found")); }
    Ok(Json(json!({ "message": format!("Role updated to {new_role}") })))
}

// ── Admin login ───────────────────────────────────────────────────────────────
// Username: "sysadmin" (hardcoded)
// Password: from ADMIN_PASSWORD or ADMIN_PWD_HASH environment variable at startup

pub async fn admin_login(Json(req): Json<serde_json::Value>) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let username = req.get("username").and_then(|v| v.as_str()).unwrap_or("");
    let password = req.get("password").and_then(|v| v.as_str()).unwrap_or("");

    if username != ADMIN_USERNAME {
        return Err(err401("Invalid admin credentials"));
    }

    let hash = ADMIN_PWD_HASH.as_deref().ok_or_else(|| {
        err500("Admin password not configured. Rebuild with ADMIN_PASSWORD env var set.")
    })?;

    if !verify(password, hash).unwrap_or(false) {
        return Err(err401("Invalid admin credentials"));
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
        "SELECT id, user_name, user_desig, user_id, user_role, user_status, created_on, closed_on
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

    let pwd_hash = hash(&req.password, DEFAULT_COST).map_err(|e| err500(&e.to_string()))?;
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();

    conn.execute(
        "INSERT INTO users (user_name, user_desig, user_id, user_pwd, user_role, user_status, created_on) VALUES (?,?,?,?,?,?,?)",
        rusqlite::params![req.user_name, req.user_desig, req.user_id, pwd_hash, req.user_role, "ACTIVE", today],
    ).map_err(|e| err400(&e.to_string()))?;

    Ok(Json(json!({ "message": "User created." })))
}

pub async fn admin_update_user(State(pool): Db, _admin: AdminUser, Path(id): Path<i64>, Json(req): Json<serde_json::Value>) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let conn = pool.get().map_err(|e| err500(&e.to_string()))?;

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
