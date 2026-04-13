use std::sync::Arc;
use axum::{extract::{Path, State}, http::StatusCode, Json};
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

pub async fn set_mode(State(pool): Db, _auth: AuthUser, Json(req): Json<serde_json::Value>) -> Result<Json<Value>, Err> {
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

pub async fn set_features(State(pool): Db, _auth: AuthUser, Json(req): Json<serde_json::Value>) -> Result<Json<Value>, Err> {
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
        "SELECT id, template_key, template_value, effective_from, created_by
         FROM print_template_config ORDER BY effective_from DESC"
    ).map_err(|e| e500(&e.to_string()))?;

    let rows: Vec<Value> = stmt.query_map([], |r| {
        Ok(json!({
            "id":             r.get::<_, i64>(0)?,
            "template_key":   r.get::<_, String>(1)?,
            "template_value": r.get::<_, Option<String>>(2)?,
            "effective_from": r.get::<_, Option<String>>(3)?,
            "created_by":     r.get::<_, Option<String>>(4)?,
        }))
    }).map_err(|e| e500(&e.to_string()))?.filter_map(|r| r.ok()).collect();

    Ok(Json(json!(rows)))
}

pub async fn upsert_print_template(State(pool): Db, auth: AuthUser, Json(req): Json<serde_json::Value>) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let key = req.get("template_key").and_then(|v| v.as_str()).ok_or_else(|| e400("template_key required"))?;
    let val = req.get("template_value").and_then(|v| v.as_str());
    let from = req.get("effective_from").and_then(|v| v.as_str())
        .unwrap_or_else(|| Box::leak(chrono::Local::now().format("%Y-%m-%d").to_string().into_boxed_str()));

    conn.execute(
        "INSERT INTO print_template_config (template_key, template_value, effective_from, created_by)
         VALUES (?,?,?,?)
         ON CONFLICT(template_key) DO UPDATE SET template_value=excluded.template_value,
         effective_from=excluded.effective_from, created_by=excluded.created_by",
        rusqlite::params![key, val, from, auth.0.sub],
    ).map_err(|e| e500(&e.to_string()))?;

    Ok(Json(json!({ "message": "Template config saved." })))
}

// ── Baggage rules config ──────────────────────────────────────────────────────

pub async fn get_baggage_rules(State(pool): Db, _auth: AuthUser) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let mut stmt = conn.prepare(
        "SELECT id, rule_key, rule_value, pax_type, item_category, effective_from
         FROM baggage_rules_config ORDER BY effective_from DESC"
    ).map_err(|e| e500(&e.to_string()))?;

    let rows: Vec<Value> = stmt.query_map([], |r| {
        Ok(json!({
            "id":             r.get::<_, i64>(0)?,
            "rule_key":       r.get::<_, String>(1)?,
            "rule_value":     r.get::<_, Option<String>>(2)?,
            "pax_type":       r.get::<_, Option<String>>(3)?,
            "item_category":  r.get::<_, Option<String>>(4)?,
            "effective_from": r.get::<_, Option<String>>(5)?,
        }))
    }).map_err(|e| e500(&e.to_string()))?.filter_map(|r| r.ok()).collect();

    Ok(Json(json!(rows)))
}

pub async fn upsert_baggage_rules(State(pool): Db, _auth: AuthUser, Json(req): Json<serde_json::Value>) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let key = req.get("rule_key").and_then(|v| v.as_str()).ok_or_else(|| e400("rule_key required"))?;
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let from = req.get("effective_from").and_then(|v| v.as_str()).unwrap_or(&today);

    conn.execute(
        "INSERT INTO baggage_rules_config (rule_key, rule_value, pax_type, item_category, effective_from)
         VALUES (?,?,?,?,?)
         ON CONFLICT(rule_key) DO UPDATE SET rule_value=excluded.rule_value,
         pax_type=excluded.pax_type, item_category=excluded.item_category,
         effective_from=excluded.effective_from",
        rusqlite::params![
            key,
            req.get("rule_value").and_then(|v| v.as_str()),
            req.get("pax_type").and_then(|v| v.as_str()),
            req.get("item_category").and_then(|v| v.as_str()),
            from,
        ],
    ).map_err(|e| e500(&e.to_string()))?;

    Ok(Json(json!({ "message": "Baggage rule saved." })))
}

// ── Special item allowances ───────────────────────────────────────────────────

pub async fn get_special_allowances(State(pool): Db, _auth: AuthUser) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let mut stmt = conn.prepare(
        "SELECT id, item_description, allowance_qty, allowance_value, uqc,
                pax_type, duty_rate, effective_from
         FROM special_item_allowances ORDER BY item_description"
    ).map_err(|e| e500(&e.to_string()))?;

    let rows: Vec<Value> = stmt.query_map([], |r| {
        Ok(json!({
            "id":               r.get::<_, i64>(0)?,
            "item_description": r.get::<_, Option<String>>(1)?,
            "allowance_qty":    r.get::<_, Option<f64>>(2)?,
            "allowance_value":  r.get::<_, Option<f64>>(3)?,
            "uqc":              r.get::<_, Option<String>>(4)?,
            "pax_type":         r.get::<_, Option<String>>(5)?,
            "duty_rate":        r.get::<_, Option<f64>>(6)?,
            "effective_from":   r.get::<_, Option<String>>(7)?,
        }))
    }).map_err(|e| e500(&e.to_string()))?.filter_map(|r| r.ok()).collect();

    Ok(Json(json!(rows)))
}

pub async fn create_special_allowance(State(pool): Db, _auth: AuthUser, Json(req): Json<serde_json::Value>) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();

    conn.execute(
        "INSERT INTO special_item_allowances (item_description, allowance_qty, allowance_value,
         uqc, pax_type, duty_rate, effective_from) VALUES (?,?,?,?,?,?,?)",
        rusqlite::params![
            req.get("item_description").and_then(|v| v.as_str()),
            req.get("allowance_qty").and_then(|v| v.as_f64()),
            req.get("allowance_value").and_then(|v| v.as_f64()),
            req.get("uqc").and_then(|v| v.as_str()),
            req.get("pax_type").and_then(|v| v.as_str()),
            req.get("duty_rate").and_then(|v| v.as_f64()),
            req.get("effective_from").and_then(|v| v.as_str()).unwrap_or(&today),
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

pub async fn get_pit_config(State(pool): Db, _auth: AuthUser) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;

    // Return latest versioned config for each key
    let mut stmt = conn.prepare(
        "SELECT config_key, config_value, effective_from
         FROM print_template_config
         ORDER BY config_key, effective_from DESC"
    ).map_err(|e| e500(&e.to_string()))?;

    let rows: Vec<Value> = stmt.query_map([], |r| {
        Ok(json!({
            "config_key":     r.get::<_, String>(0)?,
            "config_value":   r.get::<_, Option<String>>(1)?,
            "effective_from": r.get::<_, Option<String>>(2)?,
        }))
    }).map_err(|e| e500(&e.to_string()))?.filter_map(|r| r.ok()).collect();

    Ok(Json(json!(rows)))
}

// ── Allowed devices ───────────────────────────────────────────────────────────

pub async fn list_devices(State(pool): Db, _auth: AuthUser) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let mut stmt = conn.prepare(
        "SELECT id, device_name, device_mac, device_ip, location_code, is_active, created_on
         FROM allowed_devices ORDER BY device_name"
    ).map_err(|e| e500(&e.to_string()))?;

    let rows: Vec<Value> = stmt.query_map([], |r| {
        Ok(json!({
            "id":            r.get::<_, i64>(0)?,
            "device_name":   r.get::<_, Option<String>>(1)?,
            "device_mac":    r.get::<_, Option<String>>(2)?,
            "device_ip":     r.get::<_, Option<String>>(3)?,
            "location_code": r.get::<_, Option<String>>(4)?,
            "is_active":     r.get::<_, Option<String>>(5)?,
            "created_on":    r.get::<_, Option<String>>(6)?,
        }))
    }).map_err(|e| e500(&e.to_string()))?.filter_map(|r| r.ok()).collect();

    Ok(Json(json!(rows)))
}

pub async fn create_device(State(pool): Db, _auth: AuthUser, Json(req): Json<serde_json::Value>) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();

    conn.execute(
        "INSERT INTO allowed_devices (device_name, device_mac, device_ip, location_code, is_active, created_on)
         VALUES (?,?,?,?,?,?)",
        rusqlite::params![
            req.get("device_name").and_then(|v| v.as_str()),
            req.get("device_mac").and_then(|v| v.as_str()),
            req.get("device_ip").and_then(|v| v.as_str()),
            req.get("location_code").and_then(|v| v.as_str()),
            "Y", today,
        ],
    ).map_err(|e| e400(&e.to_string()))?;

    Ok(Json(json!({ "message": "Device registered." })))
}

pub async fn update_device(State(pool): Db, _auth: AuthUser, Path(id): Path<i64>, Json(req): Json<serde_json::Value>) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let affected = conn.execute(
        "UPDATE allowed_devices SET device_name=COALESCE(?,device_name),
         device_mac=COALESCE(?,device_mac), device_ip=COALESCE(?,device_ip),
         location_code=COALESCE(?,location_code), is_active=COALESCE(?,is_active)
         WHERE id=?",
        rusqlite::params![
            req.get("device_name").and_then(|v| v.as_str()),
            req.get("device_mac").and_then(|v| v.as_str()),
            req.get("device_ip").and_then(|v| v.as_str()),
            req.get("location_code").and_then(|v| v.as_str()),
            req.get("is_active").and_then(|v| v.as_str()),
            id,
        ],
    ).map_err(|e| e500(&e.to_string()))?;
    if affected == 0 { return Err(e404("Device not found.")); }
    Ok(Json(json!({ "message": "Device updated." })))
}

pub async fn delete_device(State(pool): Db, _auth: AuthUser, Path(id): Path<i64>) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let affected = conn.execute(
        "UPDATE allowed_devices SET is_active='N' WHERE id=?",
        rusqlite::params![id],
    ).map_err(|e| e500(&e.to_string()))?;
    if affected == 0 { return Err(e404("Device not found.")); }
    Ok(Json(json!({ "message": "Device deactivated." })))
}

// ── Print template row-level PUT/DELETE (admin-auth) ─────────────────────────

pub async fn update_print_template_row(State(pool): Db, _admin: AdminUser, Path(id): Path<i64>, Json(req): Json<serde_json::Value>) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let affected = conn.execute(
        "UPDATE print_template_config SET template_value=COALESCE(?,template_value),
         effective_from=COALESCE(?,effective_from) WHERE id=?",
        rusqlite::params![
            req.get("template_value").and_then(|v| v.as_str()),
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
        "UPDATE baggage_rules_config SET rule_value=COALESCE(?,rule_value),
         pax_type=COALESCE(?,pax_type), item_category=COALESCE(?,item_category),
         effective_from=COALESCE(?,effective_from) WHERE id=?",
        rusqlite::params![
            req.get("rule_value").and_then(|v| v.as_str()),
            req.get("pax_type").and_then(|v| v.as_str()),
            req.get("item_category").and_then(|v| v.as_str()),
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
        "UPDATE special_item_allowances SET item_description=COALESCE(?,item_description),
         allowance_qty=COALESCE(?,allowance_qty), allowance_value=COALESCE(?,allowance_value),
         uqc=COALESCE(?,uqc), pax_type=COALESCE(?,pax_type), duty_rate=COALESCE(?,duty_rate),
         effective_from=COALESCE(?,effective_from) WHERE id=?",
        rusqlite::params![
            req.get("item_description").and_then(|v| v.as_str()),
            req.get("allowance_qty").and_then(|v| v.as_f64()),
            req.get("allowance_value").and_then(|v| v.as_f64()),
            req.get("uqc").and_then(|v| v.as_str()),
            req.get("pax_type").and_then(|v| v.as_str()),
            req.get("duty_rate").and_then(|v| v.as_f64()),
            req.get("effective_from").and_then(|v| v.as_str()),
            id,
        ],
    ).map_err(|e| e500(&e.to_string()))?;
    if affected == 0 { return Err(e404("Allowance not found.")); }
    Ok(Json(json!({ "message": "Allowance updated." })))
}

// ── Remarks templates ─────────────────────────────────────────────────────────

pub async fn get_remarks_templates(State(pool): Db, _admin: AdminUser) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let mut stmt = conn.prepare(
        "SELECT id, template_key, template_text, updated_on FROM remarks_templates ORDER BY template_key"
    ).map_err(|e| e500(&e.to_string()))?;

    let rows: Vec<Value> = stmt.query_map([], |r| {
        Ok(json!({
            "id":            r.get::<_, i64>(0)?,
            "template_key":  r.get::<_, String>(1)?,
            "template_text": r.get::<_, Option<String>>(2)?,
            "updated_on":    r.get::<_, Option<String>>(3)?,
        }))
    }).map_err(|e| e500(&e.to_string()))?.filter_map(|r| r.ok()).collect();

    Ok(Json(json!(rows)))
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

    tracing::warn!("ADMIN HARD-PURGE: OS {os_no}/{os_year} permanently deleted. Breakdown: {:?}", deleted);

    Ok(Json(json!({
        "message": format!("OS {os_no}/{os_year} permanently purged."),
        "deleted": deleted,
    })))
}

// ── OS Config (arrival vs departure print templates) ─────────────────────────

pub async fn get_os_config(State(pool): Db, _auth: AuthUser) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;

    // Latest arrival and departure template configs
    let mut stmt = conn.prepare(
        "SELECT template_key, template_value, effective_from FROM print_template_config
         ORDER BY template_key, effective_from DESC"
    ).map_err(|e| e500(&e.to_string()))?;

    let mut arrival = serde_json::Map::new();
    let mut departure = serde_json::Map::new();
    let seen_keys = std::cell::RefCell::new(std::collections::HashSet::new());

    let rows = stmt.query_map([], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, Option<String>>(2)?))
    }).map_err(|e| e500(&e.to_string()))?;

    for row in rows.filter_map(|r| r.ok()) {
        let (key, val, eff_from) = row;
        if seen_keys.borrow().contains(&key) { continue; }
        seen_keys.borrow_mut().insert(key.clone());
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

