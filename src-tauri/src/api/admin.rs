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
    Ok(Json(json!({ "mode": mode })))
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
        map.insert(row.0, json!(row.1));
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

    conn.execute(
        "INSERT INTO allowed_devices (label, ip_address, mac_address, hostname, is_active, added_on, notes)
         VALUES (?,?,?,?,1,?,?)",
        rusqlite::params![
            req.get("label").and_then(|v| v.as_str()),
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

    // BR/DR linked via post_adj fields (best-effort)
    del("DELETE FROM br_items WHERE br_no IN (SELECT br_no FROM br_master WHERE os_no=?)", &[&os_no]);
    del("DELETE FROM br_master WHERE os_no=?", &[&os_no]);
    del("DELETE FROM dr_items WHERE dr_no IN (SELECT dr_no FROM dr_master WHERE os_no=?)", &[&os_no]);
    del("DELETE FROM dr_master WHERE os_no=?", &[&os_no]);

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

