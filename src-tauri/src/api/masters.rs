use std::sync::Arc;
use axum::{extract::{Path, State}, http::StatusCode, Json};
use serde_json::{json, Value};
use crate::{auth::AuthUser, db::DbPool};

type Db  = State<Arc<DbPool>>;
type Err = (StatusCode, Json<Value>);

fn e400(m: &str) -> Err { (StatusCode::BAD_REQUEST,           Json(json!({ "detail": m }))) }
fn e404(m: &str) -> Err { (StatusCode::NOT_FOUND,             Json(json!({ "detail": m }))) }
fn e500(m: &str) -> Err { (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "detail": m }))) }

// ── Master data 10-minute TTL cache ──────────────────────────────────────────
// Master tables are read-only in normal operation and loaded on every form mount.
// Cache each list for 600 s to avoid repeated full-table scans.

type MasterCacheSlot = std::sync::Mutex<Option<(Vec<Value>, std::time::Instant)>>;
const MASTERS_TTL_SECS: u64 = 600;

static NAT_CACHE:      std::sync::OnceLock<MasterCacheSlot> = std::sync::OnceLock::new();
static AIRLINE_CACHE:  std::sync::OnceLock<MasterCacheSlot> = std::sync::OnceLock::new();
static FLIGHT_CACHE:   std::sync::OnceLock<MasterCacheSlot> = std::sync::OnceLock::new();
static ITEMCAT_CACHE:  std::sync::OnceLock<MasterCacheSlot> = std::sync::OnceLock::new();

fn cache_get(slot: &MasterCacheSlot) -> Option<Vec<Value>> {
    if let Ok(guard) = slot.lock() {
        if let Some((ref data, ts)) = *guard {
            if ts.elapsed().as_secs() < MASTERS_TTL_SECS {
                return Some(data.clone());
            }
        }
    }
    None
}

fn cache_set(slot: &MasterCacheSlot, data: Vec<Value>) {
    if let Ok(mut guard) = slot.lock() {
        *guard = Some((data, std::time::Instant::now()));
    }
}


// ── Nationalities ─────────────────────────────────────────────────────────────

pub async fn nationalities(State(pool): Db, _auth: AuthUser) -> Result<Json<Value>, Err> {
    let cache = NAT_CACHE.get_or_init(|| std::sync::Mutex::new(None));
    if let Some(cached) = cache_get(cache) {
        return Ok(Json(json!(cached)));
    }
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let mut stmt = conn.prepare("SELECT id, nationality FROM nationality_master ORDER BY nationality")
        .map_err(|e| e500(&e.to_string()))?;
    let rows: Vec<Value> = stmt.query_map([], |r| {
        Ok(json!({ "id": r.get::<_,i64>(0)?, "nationality": r.get::<_,String>(1)? }))
    }).map_err(|e| e500(&e.to_string()))?.filter_map(|r| r.ok()).collect();
    cache_set(cache, rows.clone());
    Ok(Json(json!(rows)))
}

pub async fn create_nationality(State(pool): Db, _auth: AuthUser, Json(req): Json<Value>) -> Result<Json<Value>, Err> {
    let nat = req.get("nationality").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    if nat.is_empty() { return Err(e400("nationality is required")); }
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    conn.execute("INSERT OR IGNORE INTO nationality_master (nationality) VALUES (?)", rusqlite::params![nat])
        .map_err(|e| e400(&e.to_string()))?;
    let id: i64 = conn.query_row("SELECT id FROM nationality_master WHERE nationality = ?", [&nat], |r| r.get(0))
        .map_err(|e| e500(&e.to_string()))?;
    if let Some(c) = NAT_CACHE.get() { if let Ok(mut g) = c.lock() { *g = None; } }
    Ok(Json(json!({ "id": id, "nationality": nat })))
}

// ── Airlines ──────────────────────────────────────────────────────────────────

pub async fn airlines(State(pool): Db, _auth: AuthUser) -> Result<Json<Value>, Err> {
    let cache = AIRLINE_CACHE.get_or_init(|| std::sync::Mutex::new(None));
    if let Some(cached) = cache_get(cache) {
        return Ok(Json(json!(cached)));
    }
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let mut stmt = conn.prepare("SELECT id, airline_code, airline_name FROM airlines_mast ORDER BY airline_name")
        .map_err(|e| e500(&e.to_string()))?;
    let rows: Vec<Value> = stmt.query_map([], |r| {
        Ok(json!({ "id": r.get::<_,i64>(0)?, "airline_code": r.get::<_,String>(1)?, "airline_name": r.get::<_,String>(2)? }))
    }).map_err(|e| e500(&e.to_string()))?.filter_map(|r| r.ok()).collect();
    cache_set(cache, rows.clone());
    Ok(Json(json!(rows)))
}

pub async fn create_airline(State(pool): Db, _auth: AuthUser, Json(req): Json<Value>) -> Result<Json<Value>, Err> {
    let code = req.get("airline_code").and_then(|v| v.as_str()).unwrap_or("").trim().to_uppercase();
    let name = req.get("airline_name").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    if code.is_empty() || name.is_empty() { return Err(e400("airline_code and airline_name are required")); }
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    conn.execute("INSERT OR IGNORE INTO airlines_mast (airline_code, airline_name) VALUES (?,?)", rusqlite::params![code, name])
        .map_err(|e| e400(&e.to_string()))?;
    let id: i64 = conn.query_row("SELECT id FROM airlines_mast WHERE airline_code = ?", [&code], |r| r.get(0))
        .map_err(|e| e500(&e.to_string()))?;
    if let Some(c) = AIRLINE_CACHE.get() { if let Ok(mut g) = c.lock() { *g = None; } }
    Ok(Json(json!({ "id": id, "airline_code": code, "airline_name": name })))
}

// ── Flights ───────────────────────────────────────────────────────────────────

pub async fn flights(State(pool): Db, _auth: AuthUser) -> Result<Json<Value>, Err> {
    let cache = FLIGHT_CACHE.get_or_init(|| std::sync::Mutex::new(None));
    if let Some(cached) = cache_get(cache) {
        return Ok(Json(json!(cached)));
    }
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let mut stmt = conn.prepare("SELECT id, flight_no, airline_code FROM arrival_flight_master ORDER BY flight_no")
        .map_err(|e| e500(&e.to_string()))?;
    let rows: Vec<Value> = stmt.query_map([], |r| {
        Ok(json!({ "id": r.get::<_,i64>(0)?, "flight_no": r.get::<_,String>(1)?, "airline_code": r.get::<_,String>(2)? }))
    }).map_err(|e| e500(&e.to_string()))?.filter_map(|r| r.ok()).collect();
    cache_set(cache, rows.clone());
    Ok(Json(json!(rows)))
}

pub async fn create_flight(State(pool): Db, _auth: AuthUser, Json(req): Json<Value>) -> Result<Json<Value>, Err> {
    let flight_no    = req.get("flight_no").and_then(|v| v.as_str()).unwrap_or("").trim().to_uppercase();
    let airline_code = req.get("airline_code").and_then(|v| v.as_str()).unwrap_or("").trim().to_uppercase();
    if flight_no.is_empty() { return Err(e400("flight_no is required")); }
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    conn.execute("INSERT INTO arrival_flight_master (flight_no, airline_code) VALUES (?,?)", rusqlite::params![flight_no, airline_code])
        .map_err(|e| e400(&e.to_string()))?;
    let id: i64 = conn.last_insert_rowid();
    if let Some(c) = FLIGHT_CACHE.get() { if let Ok(mut g) = c.lock() { *g = None; } }
    Ok(Json(json!({ "id": id, "flight_no": flight_no, "airline_code": airline_code })))
}

// ── Airport ───────────────────────────────────────────────────────────────────

pub async fn airports(State(pool): Db, _auth: AuthUser) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let mut stmt = conn.prepare("SELECT id, airport_name, airport_status FROM airport_master ORDER BY airport_name")
        .map_err(|e| e500(&e.to_string()))?;
    let rows: Vec<Value> = stmt.query_map([], |r| {
        Ok(json!({ "id": r.get::<_,i64>(0)?, "airport_name": r.get::<_,Option<String>>(1)?, "airport_status": r.get::<_,Option<String>>(2)? }))
    }).map_err(|e| e500(&e.to_string()))?.filter_map(|r| r.ok()).collect();
    Ok(Json(json!(rows)))
}

pub async fn close_all_airports(State(pool): Db, _auth: AuthUser) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    conn.execute("UPDATE airport_master SET airport_status = 'Closed' WHERE airport_status = 'Active'", [])
        .map_err(|e| e500(&e.to_string()))?;
    Ok(Json(json!({ "message": "All airports closed successfully" })))
}

// ── Item Categories ───────────────────────────────────────────────────────────

pub async fn item_categories(State(pool): Db, _auth: AuthUser) -> Result<Json<Value>, Err> {
    let cache = ITEMCAT_CACHE.get_or_init(|| std::sync::Mutex::new(None));
    if let Some(cached) = cache_get(cache) {
        return Ok(Json(json!(cached)));
    }
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let mut stmt = conn.prepare(
        "SELECT id, category_code, category_desc, active_ind, bcd_adv_rate, cvd_adv_rate FROM item_cat_master WHERE active_ind = 'A' ORDER BY category_desc"
    ).map_err(|e| e500(&e.to_string()))?;
    let rows: Vec<Value> = stmt.query_map([], |r| {
        Ok(json!({
            "id": r.get::<_,i64>(0)?,
            "category_code": r.get::<_,String>(1)?,
            "category_desc": r.get::<_,String>(2)?,
            "active_ind": r.get::<_,Option<String>>(3)?,
            "bcd_adv_rate": r.get::<_,Option<f64>>(4)?,
            "cvd_adv_rate": r.get::<_,Option<f64>>(5)?,
        }))
    }).map_err(|e| e500(&e.to_string()))?.filter_map(|r| r.ok()).collect();
    cache_set(cache, rows.clone());
    Ok(Json(json!(rows)))
}

pub async fn create_item_category(State(pool): Db, _auth: AuthUser, Json(req): Json<Value>) -> Result<Json<Value>, Err> {
    let code = req.get("category_code").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    let desc = req.get("category_desc").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    if code.is_empty() || desc.is_empty() { return Err(e400("category_code and category_desc are required")); }
    let bcd = req.get("bcd_adv_rate").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let cvd = req.get("cvd_adv_rate").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    conn.execute(
        "INSERT INTO item_cat_master (category_code, category_desc, active_ind, bcd_adv_rate, cvd_adv_rate) VALUES (?,?,?,?,?)",
        rusqlite::params![code, desc, "A", bcd, cvd],
    ).map_err(|e| e400(&e.to_string()))?;
    let id: i64 = conn.last_insert_rowid();
    if let Some(c) = ITEMCAT_CACHE.get() { if let Ok(mut g) = c.lock() { *g = None; } }
    Ok(Json(json!({ "id": id, "category_code": code, "category_desc": desc, "active_ind": "A", "bcd_adv_rate": bcd, "cvd_adv_rate": cvd })))
}

pub async fn deactivate_item_category(State(pool): Db, _auth: AuthUser, Path(code): Path<String>) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let n = conn.execute("UPDATE item_cat_master SET active_ind = 'C' WHERE category_code = ?", rusqlite::params![code])
        .map_err(|e| e500(&e.to_string()))?;
    if n == 0 { return Err(e404("Category not found")); }
    if let Some(c) = ITEMCAT_CACHE.get() { if let Ok(mut g) = c.lock() { *g = None; } }
    Ok(Json(json!({ "message": "Category deactivated" })))
}

// ── Duty Rates ────────────────────────────────────────────────────────────────

pub async fn duty_rates(State(pool): Db, _auth: AuthUser) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let mut stmt = conn.prepare(
        "SELECT id, duty_category, from_date, to_date, active_ind, bcd_rate, cvd_rate FROM duty_rate_master WHERE active_ind = 'A' ORDER BY duty_category"
    ).map_err(|e| e500(&e.to_string()))?;
    let rows: Vec<Value> = stmt.query_map([], |r| {
        Ok(json!({
            "id": r.get::<_,i64>(0)?,
            "duty_category": r.get::<_,String>(1)?,
            "from_date": r.get::<_,Option<String>>(2)?,
            "to_date": r.get::<_,Option<String>>(3)?,
            "active_ind": r.get::<_,Option<String>>(4)?,
            "bcd_rate": r.get::<_,Option<f64>>(5)?,
            "cvd_rate": r.get::<_,Option<f64>>(6)?,
        }))
    }).map_err(|e| e500(&e.to_string()))?.filter_map(|r| r.ok()).collect();
    Ok(Json(json!(rows)))
}

pub async fn create_duty_rate(State(pool): Db, _auth: AuthUser, Json(req): Json<Value>) -> Result<Json<Value>, Err> {
    let cat = req.get("duty_category").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    let from = req.get("from_date").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    if cat.is_empty() || from.is_empty() { return Err(e400("duty_category and from_date are required")); }
    let bcd = req.get("bcd_rate").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let cvd = req.get("cvd_rate").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    conn.execute(
        "INSERT INTO duty_rate_master (duty_category, from_date, active_ind, bcd_rate, cvd_rate) VALUES (?,?,?,?,?)",
        rusqlite::params![cat, from, "A", bcd, cvd],
    ).map_err(|e| e400(&e.to_string()))?;
    let id: i64 = conn.last_insert_rowid();
    Ok(Json(json!({ "id": id, "duty_category": cat, "from_date": from, "active_ind": "A", "bcd_rate": bcd, "cvd_rate": cvd })))
}

pub async fn deactivate_duty_rate(State(pool): Db, _auth: AuthUser, Path(id): Path<i64>) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let n = conn.execute(
        "UPDATE duty_rate_master SET active_ind = 'C', to_date = ? WHERE id = ?",
        rusqlite::params![today, id],
    ).map_err(|e| e500(&e.to_string()))?;
    if n == 0 { return Err(e404("Duty rate not found")); }
    Ok(Json(json!({ "message": "Duty rate deactivated" })))
}

// ── DC Master ─────────────────────────────────────────────────────────────────

pub async fn dc_list(State(pool): Db, _auth: AuthUser) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let mut stmt = conn.prepare(
        "SELECT id, dc_code, dc_name, dc_status FROM dc_master WHERE dc_status = 'Active' ORDER BY dc_name"
    ).map_err(|e| e500(&e.to_string()))?;
    let rows: Vec<Value> = stmt.query_map([], |r| {
        Ok(json!({ "id": r.get::<_,i64>(0)?, "dc_code": r.get::<_,String>(1)?, "dc_name": r.get::<_,String>(2)?, "dc_status": r.get::<_,Option<String>>(3)? }))
    }).map_err(|e| e500(&e.to_string()))?.filter_map(|r| r.ok()).collect();
    Ok(Json(json!(rows)))
}

pub async fn create_dc(State(pool): Db, _auth: AuthUser, Json(req): Json<Value>) -> Result<Json<Value>, Err> {
    let code = req.get("dc_code").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    let name = req.get("dc_name").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    if code.is_empty() || name.is_empty() { return Err(e400("dc_code and dc_name are required")); }
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    conn.execute(
        "INSERT OR IGNORE INTO dc_master (dc_code, dc_name, dc_status) VALUES (?,?,?)",
        rusqlite::params![code, name, "Active"],
    ).map_err(|e| e400(&e.to_string()))?;
    let id: i64 = conn.query_row("SELECT id FROM dc_master WHERE dc_code = ?", [&code], |r| r.get(0))
        .map_err(|e| e500(&e.to_string()))?;
    Ok(Json(json!({ "id": id, "dc_code": code, "dc_name": name, "dc_status": "Active" })))
}

// ── BR Number Limits ──────────────────────────────────────────────────────────

pub async fn br_limits(State(pool): Db, _auth: AuthUser) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let mut stmt = conn.prepare("SELECT id, br_type, br_series_from, br_series_to FROM br_no_limits ORDER BY br_type")
        .map_err(|e| e500(&e.to_string()))?;
    let rows: Vec<Value> = stmt.query_map([], |r| {
        Ok(json!({ "id": r.get::<_,i64>(0)?, "br_type": r.get::<_,String>(1)?, "br_series_from": r.get::<_,i64>(2)?, "br_series_to": r.get::<_,i64>(3)? }))
    }).map_err(|e| e500(&e.to_string()))?.filter_map(|r| r.ok()).collect();
    Ok(Json(json!(rows)))
}

pub async fn create_br_limit(State(pool): Db, _auth: AuthUser, Json(req): Json<Value>) -> Result<Json<Value>, Err> {
    let br_type    = req.get("br_type").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    let series_from = req.get("br_series_from").and_then(|v| v.as_i64()).unwrap_or(0);
    let series_to   = req.get("br_series_to").and_then(|v| v.as_i64()).unwrap_or(0);
    if br_type.is_empty() || series_from == 0 { return Err(e400("br_type, br_series_from, and br_series_to are required")); }
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    conn.execute(
        "INSERT INTO br_no_limits (br_type, br_series_from, br_series_to) VALUES (?,?,?)",
        rusqlite::params![br_type, series_from, series_to],
    ).map_err(|e| e400(&e.to_string()))?;
    let id: i64 = conn.last_insert_rowid();
    Ok(Json(json!({ "id": id, "br_type": br_type, "br_series_from": series_from, "br_series_to": series_to })))
}
