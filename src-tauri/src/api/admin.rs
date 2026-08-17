use std::sync::Arc;
use axum::{extract::{Path, Query, State}, http::StatusCode, Json};
use serde_json::{json, Value};
use crate::{auth::{AuthUser, AdminUser, ADMIN_USERNAME, ADMIN_PWD_HASH}, db::DbPool};

type Db = State<Arc<DbPool>>;
type Err = (StatusCode, Json<Value>);

fn e400(m: &str) -> Err { (StatusCode::BAD_REQUEST,          Json(json!({ "detail": m }))) }
fn e404(m: &str) -> Err { (StatusCode::NOT_FOUND,            Json(json!({ "detail": m }))) }
fn e500(m: &str) -> Err { (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "detail": m }))) }

// ── Mode (SDO / ADJN / QUERY / APIS) ─────────────────────────────────────────

pub async fn get_mode(State(pool): Db) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let mode: String = conn.query_row(
        "SELECT config_value FROM feature_flags WHERE config_key='APP_MODE'",
        [], |r| r.get(0)
    ).unwrap_or_else(|_| "sdo".to_string());

    // Two different things, told apart at last.
    //
    // `mode` is which module this installation runs as — sdo, adjudication,
    // query, apis. `prod_mode` is whether this is a built-and-installed copy or
    // one running from a developer's machine.
    //
    // The screens read the second and were given the first: anything set to the
    // SDO module was labelled "DEVELOPMENT MODE — security restrictions are
    // relaxed", on every office machine, permanently, with nothing an officer
    // could do about it. Registering a device did not help because the label
    // never had anything to do with devices.
    //
    // A release build is the office's copy; a debug build is a developer's.
    Ok(Json(json!({ "mode": mode, "prod_mode": !cfg!(debug_assertions) })))
}

pub async fn set_mode(State(pool): Db, _admin: AdminUser, Json(req): Json<serde_json::Value>) -> Result<Json<Value>, Err> {
    let mode = req.get("mode").and_then(|v| v.as_str()).ok_or_else(|| e400("mode required"))?;
    let valid = ["sdo", "adjudication", "query", "apis"];
    if !valid.contains(&mode) {
        return Err(e400("mode must be one of: sdo, adjudication, query, apis"));
    }
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    conn.execute(
        "INSERT INTO feature_flags (config_key, config_value) VALUES ('APP_MODE', ?)
         ON CONFLICT(config_key) DO UPDATE SET config_value=excluded.config_value",
        rusqlite::params![mode],
    ).map_err(|e| e500(&e.to_string()))?;
    Ok(Json(json!({ "mode": mode })))
}

// ── Feature flags ─────────────────────────────────────────────────────────────

pub async fn get_features(State(pool): Db) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let mut stmt = conn.prepare(
        "SELECT config_key, config_value FROM feature_flags WHERE config_key != 'APP_MODE'"
    ).map_err(|e| e500(&e.to_string()))?;

    let mut map = serde_json::Map::new();
    let rows = stmt.query_map([], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    }).map_err(|e| e500(&e.to_string()))?;

    for row in rows.filter_map(|r| r.ok()) {
        // Store as JSON booleans so frontend `!!value` comparisons work correctly.
        // The DB stores "true"/"false" strings; keep non-boolean values as strings.
        let val = match row.1.to_lowercase().as_str() {
            "true"  => json!(true),
            "false" => json!(false),
            _       => json!(row.1),
        };
        map.insert(row.0, val);
    }
    Ok(Json(Value::Object(map)))
}

pub async fn set_features(State(pool): Db, _admin: AdminUser, Json(req): Json<serde_json::Value>) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    if let Some(obj) = req.as_object() {
        for (k, v) in obj {
            if k == "APP_MODE" { continue; }
            let val = v.as_str().unwrap_or_else(|| if v.as_bool().unwrap_or(false) { "true" } else { "false" });
            conn.execute(
                "INSERT INTO feature_flags (config_key, config_value) VALUES (?, ?)
                 ON CONFLICT(config_key) DO UPDATE SET config_value=excluded.config_value",
                rusqlite::params![k, val],
            ).map_err(|e| e500(&e.to_string()))?;
        }
    }
    Ok(Json(json!({ "message": "Features updated." })))
}

// ── Print template config ─────────────────────────────────────────────────────

pub async fn get_print_template(State(pool): Db, _auth: AuthUser) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let mut stmt = conn.prepare(
        "SELECT id, field_key, field_label, field_value, effective_from, created_by
         FROM print_template_config ORDER BY field_key, effective_from DESC"
    ).map_err(|e| e500(&e.to_string()))?;

    let rows: Vec<Value> = stmt.query_map([], |r| {
        Ok(json!({
            "id":             r.get::<_, i64>(0)?,
            "field_key":      r.get::<_, String>(1)?,
            "field_label":    r.get::<_, Option<String>>(2)?,
            "field_value":    r.get::<_, Option<String>>(3)?,
            "effective_from": r.get::<_, Option<String>>(4)?,
            "created_by":     r.get::<_, Option<String>>(5)?,
        }))
    }).map_err(|e| e500(&e.to_string()))?.filter_map(|r| r.ok()).collect();

    Ok(Json(json!(rows)))
}

pub async fn upsert_print_template(State(pool): Db, auth: AuthUser, Json(req): Json<serde_json::Value>) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let key = req.get("field_key")
        .and_then(|v| v.as_str()).ok_or_else(|| e400("field_key required"))?;
    let label = req.get("field_label").and_then(|v| v.as_str());
    let val   = req.get("field_value").and_then(|v| v.as_str()).unwrap_or("");
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let from  = req.get("effective_from").and_then(|v| v.as_str()).unwrap_or(&today);

    conn.execute(
        "INSERT INTO print_template_config (field_key, field_label, field_value, effective_from, created_by)
         VALUES (?,?,?,?,?)",
        rusqlite::params![key, label, val, from, auth.0.sub],
    ).map_err(|e| e500(&e.to_string()))?;

    Ok(Json(json!({ "message": "Template config saved." })))
}

// ── Baggage rules config ──────────────────────────────────────────────────────

pub async fn get_baggage_rules(State(pool): Db, _auth: AuthUser) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let mut stmt = conn.prepare(
        "SELECT id, rule_key, rule_label, rule_value, rule_uqc, effective_from, created_by
         FROM baggage_rules_config ORDER BY rule_key, effective_from DESC"
    ).map_err(|e| e500(&e.to_string()))?;

    let rows: Vec<Value> = stmt.query_map([], |r| {
        Ok(json!({
            "id":             r.get::<_, i64>(0)?,
            "rule_key":       r.get::<_, String>(1)?,
            "rule_label":     r.get::<_, Option<String>>(2)?,
            "rule_value":     r.get::<_, Option<f64>>(3)?,
            "rule_uqc":       r.get::<_, Option<String>>(4)?,
            "effective_from": r.get::<_, Option<String>>(5)?,
            "created_by":     r.get::<_, Option<String>>(6)?,
        }))
    }).map_err(|e| e500(&e.to_string()))?.filter_map(|r| r.ok()).collect();

    Ok(Json(json!(rows)))
}

pub async fn upsert_baggage_rules(State(pool): Db, auth: AuthUser, Json(req): Json<serde_json::Value>) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let key = req.get("rule_key").and_then(|v| v.as_str()).ok_or_else(|| e400("rule_key required"))?;
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let from = req.get("effective_from").and_then(|v| v.as_str()).unwrap_or(&today);

    // rule_value is NOT NULL — say which field is missing rather than letting the
    // constraint surface as a database error the officer cannot act on.
    if req.get("rule_value").and_then(|v| v.as_f64()).is_none() {
        return Err(e400("A value for the rule is required."));
    }

    // Versioned: INSERT a new row (multiple rows per key supported for history)
    conn.execute(
        "INSERT INTO baggage_rules_config (rule_key, rule_label, rule_value, rule_uqc, effective_from, created_by)
         VALUES (?,?,?,?,?,?)",
        rusqlite::params![
            key,
            req.get("rule_label").and_then(|v| v.as_str()),
            req.get("rule_value").and_then(|v| v.as_f64()),
            req.get("rule_uqc").and_then(|v| v.as_str()),
            from,
            auth.0.sub,
        ],
    ).map_err(|e| e500(&e.to_string()))?;

    Ok(Json(json!({ "message": "Baggage rule saved." })))
}

// ── Special item allowances ───────────────────────────────────────────────────

pub async fn get_special_allowances(State(pool): Db, _auth: AuthUser) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let mut stmt = conn.prepare(
        "SELECT id, item_name, keywords, allowance_qty, allowance_uqc, effective_from, active
         FROM special_item_allowances ORDER BY item_name"
    ).map_err(|e| e500(&e.to_string()))?;

    let rows: Vec<Value> = stmt.query_map([], |r| {
        Ok(json!({
            "id":            r.get::<_, i64>(0)?,
            "item_name":     r.get::<_, Option<String>>(1)?,
            "keywords":      r.get::<_, Option<String>>(2)?,
            "allowance_qty": r.get::<_, Option<f64>>(3)?,
            "allowance_uqc": r.get::<_, Option<String>>(4)?,
            "effective_from":r.get::<_, Option<String>>(5)?,
            "active":        r.get::<_, Option<String>>(6)?,
        }))
    }).map_err(|e| e500(&e.to_string()))?.filter_map(|r| r.ok()).collect();

    Ok(Json(json!(rows)))
}

pub async fn create_special_allowance(State(pool): Db, auth: AuthUser, Json(req): Json<serde_json::Value>) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();

    // Both NOT NULL. Same reasoning as above: name the field, not the constraint.
    if req.get("item_name").and_then(|v| v.as_str()).map(str::trim).unwrap_or("").is_empty() {
        return Err(e400("An item name is required."));
    }
    if req.get("allowance_qty").and_then(|v| v.as_f64()).is_none() {
        return Err(e400("An allowance quantity is required."));
    }

    conn.execute(
        "INSERT INTO special_item_allowances (item_name, keywords, allowance_qty, allowance_uqc,
         effective_from, active, created_by) VALUES (?,?,?,?,?,?,?)",
        rusqlite::params![
            req.get("item_name").and_then(|v| v.as_str()),
            req.get("keywords").and_then(|v| v.as_str()),
            req.get("allowance_qty").and_then(|v| v.as_f64()),
            req.get("allowance_uqc").and_then(|v| v.as_str()),
            req.get("effective_from").and_then(|v| v.as_str()).unwrap_or(&today),
            "Y",
            auth.0.sub,
        ],
    ).map_err(|e| e500(&e.to_string()))?;

    Ok(Json(json!({ "message": "Special allowance created." })))
}

pub async fn delete_special_allowance(State(pool): Db, _auth: AuthUser, Path(id): Path<i64>) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let affected = conn.execute("DELETE FROM special_item_allowances WHERE id=?", rusqlite::params![id])
        .map_err(|e| e500(&e.to_string()))?;
    if affected == 0 { return Err(e404("Allowance not found.")); }
    Ok(Json(json!({ "message": "Allowance deleted." })))
}

// ── PIT (Point-in-time config snapshot) ──────────────────────────────────────

pub async fn get_pit_config(
    State(pool): Db,
    _auth: AuthUser,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let ref_date = params.get("ref_date").map(|s| s.as_str()).unwrap_or(today.as_str()).to_string();

    // Latest effective row per field_key as of ref_date
    let mut stmt = conn.prepare(
        "SELECT field_key, field_label, field_value, effective_from
         FROM print_template_config
         WHERE effective_from <= ?
           AND effective_from = (
               SELECT MAX(p2.effective_from)
               FROM print_template_config p2
               WHERE p2.field_key = print_template_config.field_key
                 AND p2.effective_from <= ?
           )"
    ).map_err(|e| e500(&e.to_string()))?;

    let mut map = serde_json::Map::new();
    let rows = stmt.query_map(rusqlite::params![ref_date, ref_date], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, Option<String>>(2)?,
            r.get::<_, Option<String>>(3)?,
        ))
    }).map_err(|e| e500(&e.to_string()))?;

    for row in rows.filter_map(|r| r.ok()) {
        let (key, label, val, eff_from) = row;
        map.insert(key.clone(), json!({
            "field_key":      key,
            "field_label":    label,
            "field_value":    val.unwrap_or_default(),
            "effective_from": eff_from,
        }));
    }

    Ok(Json(json!({ "print_template": map })))
}

// ── Allowed devices ───────────────────────────────────────────────────────────

/// What this machine looks like to the allow-list, and whether it is on it.
///
/// Identified by HOSTNAME, deliberately, not by the value config::mac_address()
/// returns. That is not a real MAC — it hashes the hostname together with the
/// process id, so it changes every time the application restarts. Registering a
/// device against it would work once and never match again. The hostname is
/// stable, is what an administrator recognises on a network, and is what the
/// allowed_devices table already stores.
pub async fn device_info(State(pool): Db, _admin: AdminUser) -> Result<Json<Value>, Err> {
    let host = crate::config::hostname();
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let registered: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM allowed_devices
             WHERE is_active = 1 AND LOWER(hostname) = LOWER(?1)",
            [&host],
            |r| r.get(0),
        )
        .unwrap_or(0);
    Ok(Json(json!({
        "hostname":   host,
        "registered": registered > 0,
        // Named for what it is. It identifies a RUN, not a machine.
        "note": "Devices are matched on hostname. COPS2 does not read the network \
                 adapter address.",
    })))
}

/// Add THIS machine to the allow-list.
///
/// Idempotent: registering twice reactivates the existing row rather than adding
/// a second one, because the obvious way to use this button is to press it again
/// when unsure whether it worked.
pub async fn register_device(State(pool): Db, _admin: AdminUser) -> Result<Json<Value>, Err> {
    let host = crate::config::hostname();
    if host.trim().is_empty() {
        return Err(e500("This machine has no hostname, so it cannot be registered."));
    }
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;

    let existing: Option<i64> = conn
        .query_row(
            "SELECT id FROM allowed_devices WHERE LOWER(hostname) = LOWER(?1)",
            [&host],
            |r| r.get(0),
        )
        .ok();

    match existing {
        Some(id) => {
            conn.execute("UPDATE allowed_devices SET is_active = 1 WHERE id = ?1", [id])
                .map_err(|e| e500(&e.to_string()))?;
            Ok(Json(json!({
                "message": format!("{host} was already registered; it is active again."),
                "hostname": host, "id": id, "created": false,
            })))
        }
        None => {
            conn.execute(
                "INSERT INTO allowed_devices (label, hostname, is_active, added_by, added_on, notes)
                 VALUES (?1, ?2, 1, 'system_admin', date('now'), 'Registered from this machine')",
                rusqlite::params![&host, &host],
            )
            .map_err(|e| e500(&e.to_string()))?;
            Ok(Json(json!({
                "message": format!("{host} registered."),
                "hostname": host, "id": conn.last_insert_rowid(), "created": true,
            })))
        }
    }
}

pub async fn list_devices(State(pool): Db, _admin: AdminUser) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let mut stmt = conn.prepare(
        "SELECT id, label, ip_address, mac_address, hostname, is_active, added_on, notes
         FROM allowed_devices ORDER BY label"
    ).map_err(|e| e500(&e.to_string()))?;

    let rows: Vec<Value> = stmt.query_map([], |r| {
        Ok(json!({
            "id":          r.get::<_, i64>(0)?,
            "label":       r.get::<_, Option<String>>(1)?,
            "ip_address":  r.get::<_, Option<String>>(2)?,
            "mac_address": r.get::<_, Option<String>>(3)?,
            "hostname":    r.get::<_, Option<String>>(4)?,
            "is_active":   r.get::<_, i64>(5).unwrap_or(1) != 0,
            "added_on":    r.get::<_, Option<String>>(6)?,
            "notes":       r.get::<_, Option<String>>(7)?,
        }))
    }).map_err(|e| e500(&e.to_string()))?.filter_map(|r| r.ok()).collect();

    Ok(Json(json!(rows)))
}

pub async fn create_device(State(pool): Db, _admin: AdminUser, Json(req): Json<serde_json::Value>) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;

    // Reject duplicate active IP addresses.
    if let Some(ip) = req.get("ip_address").and_then(|v| v.as_str()) {
        if !ip.trim().is_empty() {
            let exists: i64 = conn.query_row(
                "SELECT COUNT(*) FROM allowed_devices WHERE ip_address=? AND is_active=1",
                rusqlite::params![ip], |r| r.get(0),
            ).unwrap_or(0);
            if exists > 0 {
                return Err(e400("This IP address is already registered as an active device."));
            }
        }
    }

    let today = chrono::Local::now().format("%Y-%m-%d").to_string();

    // label is NOT NULL. The admin panel's "register this machine" button posts an
    // empty body, so an unchecked req.get() reached SQLite as NULL and the officer
    // got "NOT NULL constraint failed: allowed_devices.label" — a database error
    // for what is really a missing name. Fall back to the hostname, which is what
    // registering this machine means, and only refuse when there is nothing at all
    // to call it.
    let label = req.get("label").and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty())
        .or_else(|| req.get("hostname").and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty()))
        .map(|s| s.to_string())
        .or_else(|| { let h = crate::config::hostname(); if h.trim().is_empty() { None } else { Some(h) } })
        .ok_or_else(|| e400("A name for the device is required."))?;

    conn.execute(
        "INSERT INTO allowed_devices (label, ip_address, mac_address, hostname, is_active, added_on, notes)
         VALUES (?,?,?,?,1,?,?)",
        rusqlite::params![
            label,
            req.get("ip_address").and_then(|v| v.as_str()),
            req.get("mac_address").and_then(|v| v.as_str()),
            req.get("hostname").and_then(|v| v.as_str()),
            today,
            req.get("notes").and_then(|v| v.as_str()),
        ],
    ).map_err(|e| e400(&e.to_string()))?;

    Ok(Json(json!({ "message": "Device registered." })))
}

pub async fn update_device(State(pool): Db, _admin: AdminUser, Path(id): Path<i64>, Json(req): Json<serde_json::Value>) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    // is_active comes from frontend as boolean; convert to INTEGER for SQLite
    let is_active: Option<i64> = req.get("is_active").and_then(|v| v.as_bool())
        .map(|b| if b { 1 } else { 0 });
    let affected = conn.execute(
        "UPDATE allowed_devices SET label=COALESCE(?,label),
         ip_address=COALESCE(?,ip_address), mac_address=COALESCE(?,mac_address),
         hostname=COALESCE(?,hostname), is_active=COALESCE(?,is_active),
         notes=COALESCE(?,notes)
         WHERE id=?",
        rusqlite::params![
            req.get("label").and_then(|v| v.as_str()),
            req.get("ip_address").and_then(|v| v.as_str()),
            req.get("mac_address").and_then(|v| v.as_str()),
            req.get("hostname").and_then(|v| v.as_str()),
            is_active,
            req.get("notes").and_then(|v| v.as_str()),
            id,
        ],
    ).map_err(|e| e500(&e.to_string()))?;
    if affected == 0 { return Err(e404("Device not found.")); }
    Ok(Json(json!({ "message": "Device updated." })))
}

pub async fn delete_device(State(pool): Db, _admin: AdminUser, Path(id): Path<i64>) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let affected = conn.execute(
        "UPDATE allowed_devices SET is_active=0 WHERE id=?",
        rusqlite::params![id],
    ).map_err(|e| e500(&e.to_string()))?;
    if affected == 0 { return Err(e404("Device not found.")); }
    Ok(Json(json!({ "message": "Device deactivated." })))
}

// ── Print template row-level PUT/DELETE (admin-auth) ─────────────────────────

pub async fn update_print_template_row(State(pool): Db, _admin: AdminUser, Path(id): Path<i64>, Json(req): Json<serde_json::Value>) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let affected = conn.execute(
        "UPDATE print_template_config SET field_value=COALESCE(?,field_value),
         field_label=COALESCE(?,field_label),
         effective_from=COALESCE(?,effective_from) WHERE id=?",
        rusqlite::params![
            req.get("field_value").and_then(|v| v.as_str()),
            req.get("field_label").and_then(|v| v.as_str()),
            req.get("effective_from").and_then(|v| v.as_str()),
            id,
        ],
    ).map_err(|e| e500(&e.to_string()))?;
    if affected == 0 { return Err(e404("Template config not found.")); }
    Ok(Json(json!({ "message": "Template updated." })))
}

pub async fn delete_print_template_row(State(pool): Db, _admin: AdminUser, Path(id): Path<i64>) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let affected = conn.execute("DELETE FROM print_template_config WHERE id=?", rusqlite::params![id])
        .map_err(|e| e500(&e.to_string()))?;
    if affected == 0 { return Err(e404("Template config not found.")); }
    Ok(Json(json!({ "message": "Template deleted." })))
}

// ── Baggage rules row-level PUT/DELETE (admin-auth) ───────────────────────────

pub async fn update_baggage_rules_row(State(pool): Db, _admin: AdminUser, Path(id): Path<i64>, Json(req): Json<serde_json::Value>) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let affected = conn.execute(
        "UPDATE baggage_rules_config SET rule_label=COALESCE(?,rule_label),
         rule_value=COALESCE(?,rule_value), rule_uqc=COALESCE(?,rule_uqc),
         effective_from=COALESCE(?,effective_from) WHERE id=?",
        rusqlite::params![
            req.get("rule_label").and_then(|v| v.as_str()),
            req.get("rule_value").and_then(|v| v.as_f64()),
            req.get("rule_uqc").and_then(|v| v.as_str()),
            req.get("effective_from").and_then(|v| v.as_str()),
            id,
        ],
    ).map_err(|e| e500(&e.to_string()))?;
    if affected == 0 { return Err(e404("Baggage rule not found.")); }
    Ok(Json(json!({ "message": "Rule updated." })))
}

pub async fn delete_baggage_rules_row(State(pool): Db, _admin: AdminUser, Path(id): Path<i64>) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let affected = conn.execute("DELETE FROM baggage_rules_config WHERE id=?", rusqlite::params![id])
        .map_err(|e| e500(&e.to_string()))?;
    if affected == 0 { return Err(e404("Baggage rule not found.")); }
    Ok(Json(json!({ "message": "Rule deleted." })))
}

// ── Special allowances row-level PUT (admin-auth) ─────────────────────────────

pub async fn update_special_allowance(State(pool): Db, _admin: AdminUser, Path(id): Path<i64>, Json(req): Json<serde_json::Value>) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let affected = conn.execute(
        "UPDATE special_item_allowances SET item_name=COALESCE(?,item_name),
         keywords=COALESCE(?,keywords), allowance_qty=COALESCE(?,allowance_qty),
         allowance_uqc=COALESCE(?,allowance_uqc), active=COALESCE(?,active),
         effective_from=COALESCE(?,effective_from) WHERE id=?",
        rusqlite::params![
            req.get("item_name").and_then(|v| v.as_str()),
            req.get("keywords").and_then(|v| v.as_str()),
            req.get("allowance_qty").and_then(|v| v.as_f64()),
            req.get("allowance_uqc").and_then(|v| v.as_str()),
            req.get("active").and_then(|v| v.as_str()),
            req.get("effective_from").and_then(|v| v.as_str()),
            id,
        ],
    ).map_err(|e| e500(&e.to_string()))?;
    if affected == 0 { return Err(e404("Allowance not found.")); }
    Ok(Json(json!({ "message": "Allowance updated." })))
}

// ── Remarks templates ─────────────────────────────────────────────────────────

pub async fn get_remarks_templates(State(pool): Db, _auth: AuthUser) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let mut stmt = conn.prepare(
        "SELECT id, template_key, template_text FROM remarks_templates ORDER BY template_key"
    ).map_err(|e| e500(&e.to_string()))?;

    let rows = stmt.query_map([], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?))
    }).map_err(|e| e500(&e.to_string()))?;

    // Return as Record<string, {id, label, value}> keyed by template_key
    let mut map = serde_json::Map::new();
    for row in rows.filter_map(|r| r.ok()) {
        let (id, key, text) = row;
        map.insert(key.clone(), json!({
            "id":    id,
            "label": key,
            "value": text.unwrap_or_default(),
        }));
    }
    Ok(Json(Value::Object(map)))
}

pub async fn upsert_remarks_template(State(pool): Db, _admin: AdminUser, Path(key): Path<String>, Json(req): Json<serde_json::Value>) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let text = req.get("template_text").and_then(|v| v.as_str()).ok_or_else(|| e400("template_text required"))?;
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();

    conn.execute(
        "INSERT INTO remarks_templates (template_key, template_text, updated_on) VALUES (?,?,?)
         ON CONFLICT(template_key) DO UPDATE SET template_text=excluded.template_text, updated_on=excluded.updated_on",
        rusqlite::params![key, text, today],
    ).map_err(|e| e500(&e.to_string()))?;

    Ok(Json(json!({ "message": "Remarks template saved." })))
}

// ── Danger zone: Purge OS (IRREVERSIBLE hard delete) ─────────────────────────

pub async fn purge_os(State(pool): Db, _admin: AdminUser, Json(req): Json<serde_json::Value>) -> Result<Json<Value>, Err> {
    let os_no   = req.get("os_no").and_then(|v| v.as_str()).ok_or_else(|| e400("os_no required"))?.trim().to_string();
    let os_year = req.get("os_year").and_then(|v| v.as_i64()).ok_or_else(|| e400("os_year required"))?;
    let admin_password = req.get("admin_password").and_then(|v| v.as_str()).ok_or_else(|| e400("admin_password required for purge"))?;

    // Re-verify admin password before destruction
    let hash = ADMIN_PWD_HASH.as_deref().ok_or_else(|| e500("Admin password not configured"))?;
    if !bcrypt::verify(admin_password, hash).unwrap_or(false) {
        return Err((StatusCode::FORBIDDEN, Json(json!({ "detail": "Admin password incorrect." }))));
    }

    if os_no.is_empty() { return Err(e400("OS number cannot be blank.")); }

    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;

    // Verify case exists
    let exists: i64 = conn.query_row(
        "SELECT COUNT(*) FROM cops_master WHERE os_no=? AND os_year=?",
        rusqlite::params![os_no, os_year], |r| r.get(0)
    ).unwrap_or(0);
    if exists == 0 {
        return Err(e404(&format!("OS {os_no}/{os_year} not found.")));
    }

    // Hard delete everything related to this case
    let mut deleted = serde_json::Map::new();

    let del = |sql: &str, params: &[&dyn rusqlite::ToSql]| -> i64 {
        conn.execute(sql, params).unwrap_or(0) as i64
    };

    deleted.insert("cops_items".into(),         json!(del("DELETE FROM cops_items WHERE os_no=? AND os_year=?", &[&os_no, &os_year])));
    deleted.insert("cops_items_deleted".into(),  json!(del("DELETE FROM cops_items_deleted WHERE os_no=? AND os_year=?", &[&os_no, &os_year])));
    deleted.insert("cops_master_deleted".into(), json!(del("DELETE FROM cops_master_deleted WHERE os_no=? AND os_year=?", &[&os_no, &os_year])));
    deleted.insert("cops_master".into(),         json!(del("DELETE FROM cops_master WHERE os_no=? AND os_year=?", &[&os_no, &os_year])));

    // Receipts linked to THIS case, and only this case.
    //
    // These four statements used to match on os_no alone, with no year, and the
    // items by br_no alone. O.S. numbers restart every year and so do receipt
    // numbers, so purging case 100/2026 also destroyed the baggage and detention
    // receipts of case 100/2025, 100/2024, and every receipt in any year that
    // happened to reuse the same number. The one operation in the application
    // that deletes permanently was the one least careful about what it matched.
    //
    // Now the receipts belonging to this case are looked up by (os_no, os_year)
    // and removed by their own key, so nothing outside the case is touched.
    for (master, items, no_col, year_col, date_col) in [
        ("br_master", "br_items", "br_no", "br_year", "br_date"),
        ("dr_master", "dr_items", "dr_no", "dr_year", "dr_date"),
    ] {
        let keys: Vec<(String, String)> = {
            let Ok(mut st) = conn.prepare(&format!(
                "SELECT {no_col}, COALESCE({date_col},'') FROM {master} WHERE os_no=? AND os_year=?"
            )) else { continue };
            let rows = st.query_map(rusqlite::params![os_no, os_year], |r| {
                Ok((crate::api::col_text(r, 0)?.unwrap_or_default(),
                    r.get::<_, String>(1)?))
            });
            match rows {
                Ok(it) => it.filter_map(|r| r.ok()).collect(),
                Err(_) => continue,
            }
        };
        let mut item_rows = 0i64;
        let mut master_rows = 0i64;
        for (no, date) in &keys {
            item_rows += conn.execute(
                &format!("DELETE FROM {items} WHERE {no_col}=? AND {date_col}=?"),
                rusqlite::params![no, date],
            ).unwrap_or(0) as i64;
        }
        master_rows += conn.execute(
            &format!("DELETE FROM {master} WHERE os_no=? AND {year_col} IS NOT NULL AND os_year=?"),
            rusqlite::params![os_no, os_year],
        ).unwrap_or(0) as i64;
        if item_rows  > 0 { deleted.insert(items.into(),  json!(item_rows)); }
        if master_rows > 0 { deleted.insert(master.into(), json!(master_rows)); }
    }

    let total_rows_deleted: i64 = deleted.values()
        .filter_map(|v| v.as_i64())
        .sum();

    tracing::warn!("ADMIN HARD-PURGE: OS {os_no}/{os_year} permanently deleted. Breakdown: {:?}", deleted);

    Ok(Json(json!({
        "message": format!("OS {os_no}/{os_year} permanently purged."),
        "total_rows_deleted": total_rows_deleted,
        "breakdown": deleted,
    })))
}

// ── OS Config (arrival vs departure print templates) ─────────────────────────

pub async fn get_os_config(State(pool): Db, _auth: AuthUser) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;

    // Latest arrival and departure template configs
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let mut stmt = conn.prepare(
        "SELECT field_key, field_value, effective_from FROM print_template_config
         WHERE effective_from <= ?
           AND effective_from = (
               SELECT MAX(p2.effective_from) FROM print_template_config p2
               WHERE p2.field_key = print_template_config.field_key
                 AND p2.effective_from <= ?
           )
         ORDER BY field_key"
    ).map_err(|e| e500(&e.to_string()))?;

    let mut arrival = serde_json::Map::new();
    let mut departure = serde_json::Map::new();

    let rows = stmt.query_map(rusqlite::params![today, today], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, Option<String>>(2)?))
    }).map_err(|e| e500(&e.to_string()))?;

    for row in rows.filter_map(|r| r.ok()) {
        let (key, val, _eff_from) = row;
        let v = json!(val.unwrap_or_default());
        if key.starts_with("departure_") || key.contains("export") {
            departure.insert(key, v);
        } else {
            arrival.insert(key, v);
        }
    }

    Ok(Json(json!({ "arrival": arrival, "departure": departure })))
}

/// GET /admin/backup/db-cipher-key
/// Returns the derived 64-char hex DB key for disaster recovery.
/// Use this in DB Browser for SQLite (Raw key format) to open cops.db on any machine.
pub async fn get_db_cipher_key(_admin: AdminUser) -> Json<Value> {
    Json(json!({
        "hex_key": crate::security::get_db_key_hex(),
        "usage": "In DB Browser for SQLite: Open Database → Raw key / Hex key → paste this value"
    }))
}


/// Look for records that reference something no longer there.
///
/// The purge used to delete baggage and detention receipts without matching on
/// the year, so purging one case could remove the receipts of a case in another
/// year that happened to share a number. That scoping is fixed, but a database
/// this ran against would already carry the damage, and the damage has a
/// fingerprint: a case that names a receipt the register no longer holds, or an
/// item whose parent has gone.
///
/// Read-only. It answers "did anything get lost", which is not a question the
/// row counts alone can settle.
pub async fn integrity_check(State(pool): Db, _admin: AdminUser) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let count = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap_or(-1) };

    let orphan_br_items = count(
        "SELECT COUNT(*) FROM br_items i
          WHERE NOT EXISTS (SELECT 1 FROM br_master m
                             WHERE m.br_no = i.br_no AND m.br_date = i.br_date)");
    let orphan_dr_items = count(
        "SELECT COUNT(*) FROM dr_items i
          WHERE NOT EXISTS (SELECT 1 FROM dr_master m
                             WHERE m.dr_no = i.dr_no AND m.dr_date = i.dr_date)");
    let orphan_os_items = count(
        "SELECT COUNT(*) FROM cops_items i
          WHERE NOT EXISTS (SELECT 1 FROM cops_master m
                             WHERE m.os_no = i.os_no AND m.os_year = i.os_year)");
    // A case that recorded a detention receipt which is no longer in the register.
    let cases_missing_dr = count(
        "SELECT COUNT(*) FROM cops_master c
          WHERE c.entry_deleted='N'
            AND c.post_adj_dr_no IS NOT NULL AND TRIM(c.post_adj_dr_no) != ''
            AND NOT EXISTS (SELECT 1 FROM dr_master d
                             WHERE CAST(d.dr_no AS TEXT) = TRIM(c.post_adj_dr_no))");

    let total = [orphan_br_items, orphan_dr_items, orphan_os_items, cases_missing_dr]
        .iter().filter(|n| **n > 0).count();

    Ok(Json(json!({
        "clean": total == 0,
        "orphaned_baggage_items":   orphan_br_items,
        "orphaned_detention_items": orphan_dr_items,
        "orphaned_case_items":      orphan_os_items,
        "cases_naming_a_missing_detention_receipt": cases_missing_dr,
        "note": if total == 0 {
            "Nothing references a record that is missing."
        } else {
            "Some records point at something that is no longer present. \
             This is what an unscoped purge leaves behind."
        },
    })))
}
