use std::sync::Arc;
use axum::{extract::{Path, Query, State}, http::StatusCode, Json};
use serde_json::{json, Value};
use crate::{auth::AuthUser, db::DbPool};

type Db = State<Arc<DbPool>>;
type Err = (StatusCode, Json<Value>);

fn e400(m: &str) -> Err { (StatusCode::BAD_REQUEST,          Json(json!({ "detail": m }))) }
fn e404(m: &str) -> Err { (StatusCode::NOT_FOUND,            Json(json!({ "detail": m }))) }
fn e500(m: &str) -> Err { (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "detail": m }))) }

// ── List BRs ──────────────────────────────────────────────────────────────────

pub async fn list_brs(
    State(pool): Db,
    _auth: AuthUser,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let page     = params.get("page").and_then(|v| v.parse::<i64>().ok()).unwrap_or(1).max(1);
    let per_page = params.get("per_page").and_then(|v| v.parse::<i64>().ok()).unwrap_or(20).clamp(1, 100);
    let offset   = (page - 1) * per_page;
    let search   = params.get("search").map(|s| s.as_str()).unwrap_or("").trim().to_string();

    let (search_sql, search_params): (&str, Vec<String>) = if search.is_empty() {
        ("1=1", vec![])
    } else {
        let p = format!("%{}%", search);
        ("(br_no LIKE ? OR pax_name LIKE ? OR passport_no LIKE ?)", vec![p.clone(), p.clone(), p])
    };

    let total: i64 = conn.query_row(
        &format!("SELECT COUNT(*) FROM br_master WHERE {search_sql}"),
        rusqlite::params_from_iter(search_params.iter()),
        |r| r.get(0)
    ).unwrap_or(0);

    let mut stmt = conn.prepare(&format!(
        "SELECT id, br_no, br_year, br_date, pax_name, passport_no, flight_no,
                total_items_value, total_duty_amount, total_payable, br_printed,
                os_no, os_year, is_legacy
         FROM br_master WHERE {search_sql}
         ORDER BY br_date DESC, br_no DESC
         LIMIT {per_page} OFFSET {offset}"
    )).map_err(|e| e500(&e.to_string()))?;

    let rows: Vec<Value> = stmt.query_map(rusqlite::params_from_iter(search_params.iter()), |r| {
        Ok(json!({
            "id":                r.get::<_, i64>(0)?,
            "br_no":             r.get::<_, String>(1)?,
            "br_year":           r.get::<_, i64>(2)?,
            "br_date":           r.get::<_, Option<String>>(3)?,
            "pax_name":          r.get::<_, Option<String>>(4)?,
            "passport_no":       r.get::<_, Option<String>>(5)?,
            "flight_no":         r.get::<_, Option<String>>(6)?,
            "total_items_value": r.get::<_, Option<f64>>(7)?,
            "total_duty_amount": r.get::<_, Option<f64>>(8)?,
            "total_payable":     r.get::<_, Option<f64>>(9)?,
            "br_printed":        r.get::<_, Option<String>>(10)?,
            "os_no":             r.get::<_, Option<String>>(11)?,
            "os_year":           r.get::<_, Option<i64>>(12)?,
            "is_legacy":         r.get::<_, Option<String>>(13)?,
        }))
    }).map_err(|e| e500(&e.to_string()))?.filter_map(|r| r.ok()).collect();

    Ok(Json(json!({ "total": total, "page": page, "per_page": per_page, "items": rows })))
}

// ── Get single BR ─────────────────────────────────────────────────────────────

pub async fn get_br(
    State(pool): Db,
    _auth: AuthUser,
    Path((br_no, br_year)): Path<(String, i64)>,
) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;

    let case: Option<Value> = conn.query_row(
        "SELECT * FROM br_master WHERE br_no=? AND br_year=?",
        rusqlite::params![br_no, br_year],
        |r| {
            let n = r.as_ref().column_count();
            let mut map = serde_json::Map::new();
            for i in 0..n {
                let name = r.as_ref().column_name(i).unwrap_or("?").to_string();
                let val: Value = match r.get_ref(i)? {
                    rusqlite::types::ValueRef::Null       => Value::Null,
                    rusqlite::types::ValueRef::Integer(n) => json!(n),
                    rusqlite::types::ValueRef::Real(f)    => json!(f),
                    rusqlite::types::ValueRef::Text(s)    => json!(String::from_utf8_lossy(s)),
                    rusqlite::types::ValueRef::Blob(b)    => json!(String::from_utf8_lossy(b)),
                };
                map.insert(name, val);
            }
            Ok(Value::Object(map))
        }
    ).optional().map_err(|e| e500(&e.to_string()))?;

    let mut case = case.ok_or_else(|| e404("BR not found"))?;

    // Load BR items
    let items = load_br_items(&conn, &br_no, br_year).map_err(|e| e500(&e.to_string()))?;
    case["items"] = json!(items);

    Ok(Json(case))
}

// ── Get BRs by passport ───────────────────────────────────────────────────────

pub async fn get_brs_by_passport(
    State(pool): Db,
    _auth: AuthUser,
    Path(passport_no): Path<String>,
) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let pp = passport_no.replace('\'', "''").to_uppercase();

    let mut stmt = conn.prepare(&format!(
        "SELECT id, br_no, br_year, br_date, pax_name, passport_no,
                total_items_value, total_duty_amount, total_payable, br_printed
         FROM br_master WHERE UPPER(passport_no) LIKE '%{pp}%'
         ORDER BY br_date DESC LIMIT 50"
    )).map_err(|e| e500(&e.to_string()))?;

    let rows: Vec<Value> = stmt.query_map([], |r| {
        Ok(json!({
            "id":                r.get::<_, i64>(0)?,
            "br_no":             r.get::<_, String>(1)?,
            "br_year":           r.get::<_, i64>(2)?,
            "br_date":           r.get::<_, Option<String>>(3)?,
            "pax_name":          r.get::<_, Option<String>>(4)?,
            "passport_no":       r.get::<_, Option<String>>(5)?,
            "total_items_value": r.get::<_, Option<f64>>(6)?,
            "total_duty_amount": r.get::<_, Option<f64>>(7)?,
            "total_payable":     r.get::<_, Option<f64>>(8)?,
            "br_printed":        r.get::<_, Option<String>>(9)?,
        }))
    }).map_err(|e| e500(&e.to_string()))?.filter_map(|r| r.ok()).collect();

    Ok(Json(json!({ "items": rows })))
}

// ── Create BR ─────────────────────────────────────────────────────────────────

pub async fn create_br(
    State(pool): Db,
    _auth: AuthUser,
    Json(req): Json<serde_json::Value>,
) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;

    let br_no   = req.get("br_no").and_then(|v| v.as_str()).ok_or_else(|| e400("br_no required"))?;
    let br_year = req.get("br_year").and_then(|v| v.as_i64())
        .unwrap_or_else(|| chrono::Local::now().year() as i64);
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let br_date = req.get("br_date").and_then(|v| v.as_str()).unwrap_or(&today);

    // Uniqueness
    let exists: i64 = conn.query_row(
        "SELECT COUNT(*) FROM br_master WHERE br_no=? AND br_year=?",
        rusqlite::params![br_no, br_year], |r| r.get(0)
    ).unwrap_or(0);
    if exists > 0 { return Err(e400("BR No. already exists for this year.")); }

    conn.execute(
        "INSERT INTO br_master (br_no, br_year, br_date, location_code, pax_name, pax_nationality,
         passport_no, passport_date, pax_date_of_birth, flight_no, flight_date, booked_by,
         os_no, os_year, br_printed, total_items_value, total_duty_amount, total_payable, is_legacy)
         VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
        rusqlite::params![
            br_no, br_year, br_date,
            req.get("location_code").and_then(|v| v.as_str()),
            req.get("pax_name").and_then(|v| v.as_str()),
            req.get("pax_nationality").and_then(|v| v.as_str()),
            req.get("passport_no").and_then(|v| v.as_str()),
            req.get("passport_date").and_then(|v| v.as_str()),
            req.get("pax_date_of_birth").and_then(|v| v.as_str()),
            req.get("flight_no").and_then(|v| v.as_str()),
            req.get("flight_date").and_then(|v| v.as_str()),
            req.get("booked_by").and_then(|v| v.as_str()),
            req.get("os_no").and_then(|v| v.as_str()),
            req.get("os_year").and_then(|v| v.as_i64()),
            "N",
            req.get("total_items_value").and_then(|v| v.as_f64()).unwrap_or(0.0),
            req.get("total_duty_amount").and_then(|v| v.as_f64()).unwrap_or(0.0),
            req.get("total_payable").and_then(|v| v.as_f64()).unwrap_or(0.0),
            req.get("is_legacy").and_then(|v| v.as_str()).unwrap_or("N"),
        ],
    ).map_err(|e| e500(&e.to_string()))?;

    // Save items
    if let Some(items) = req.get("items").and_then(|v| v.as_array()) {
        for (i, item) in items.iter().enumerate() {
            conn.execute(
                "INSERT INTO br_items (br_no, br_year, items_sno, items_desc, items_qty, items_uqc,
                 items_value, cumulative_duty_rate, items_duty, items_duty_type, items_category,
                 items_release_category, items_sub_category)
                 VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)",
                rusqlite::params![
                    br_no, br_year, (i + 1) as i64,
                    item.get("items_desc").and_then(|v| v.as_str()),
                    item.get("items_qty").and_then(|v| v.as_f64()),
                    item.get("items_uqc").and_then(|v| v.as_str()),
                    item.get("items_value").and_then(|v| v.as_f64()).unwrap_or(0.0),
                    item.get("cumulative_duty_rate").and_then(|v| v.as_f64()).unwrap_or(0.0),
                    item.get("items_duty").and_then(|v| v.as_f64()).unwrap_or(0.0),
                    item.get("items_duty_type").and_then(|v| v.as_str()),
                    item.get("items_category").and_then(|v| v.as_str()),
                    item.get("items_release_category").and_then(|v| v.as_str()),
                    item.get("items_sub_category").and_then(|v| v.as_str()),
                ],
            ).map_err(|e| e500(&e.to_string()))?;
        }
    }

    Ok(Json(json!({ "message": "BR created.", "br_no": br_no, "br_year": br_year })))
}

// ── Mark BR printed ───────────────────────────────────────────────────────────

pub async fn mark_br_printed(
    State(pool): Db,
    _auth: AuthUser,
    Path((br_no, br_year)): Path<(String, i64)>,
) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    conn.execute(
        "UPDATE br_master SET br_printed='Y' WHERE br_no=? AND br_year=?",
        rusqlite::params![br_no, br_year],
    ).map_err(|e| e500(&e.to_string()))?;
    Ok(Json(json!({ "message": "BR marked as printed." })))
}

// ── Print BR PDF ──────────────────────────────────────────────────────────────

pub async fn print_br_pdf(
    State(pool): Db,
    _auth: AuthUser,
    Path((br_no, br_year)): Path<(String, i64)>,
) -> Result<axum::response::Response, Err> {
    use axum::response::IntoResponse;
    use axum::http::header;

    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;

    let case: Option<Value> = conn.query_row(
        "SELECT * FROM br_master WHERE br_no=? AND br_year=?",
        rusqlite::params![br_no, br_year],
        |r| {
            let n = r.as_ref().column_count();
            let mut map = serde_json::Map::new();
            for i in 0..n {
                let name = r.as_ref().column_name(i).unwrap_or("?").to_string();
                let val: Value = match r.get_ref(i)? {
                    rusqlite::types::ValueRef::Null       => Value::Null,
                    rusqlite::types::ValueRef::Integer(n) => json!(n),
                    rusqlite::types::ValueRef::Real(f)    => json!(f),
                    rusqlite::types::ValueRef::Text(s)    => json!(String::from_utf8_lossy(s)),
                    rusqlite::types::ValueRef::Blob(b)    => json!(String::from_utf8_lossy(b)),
                };
                map.insert(name, val);
            }
            Ok(Value::Object(map))
        }
    ).optional().map_err(|e| e500(&e.to_string()))?;

    let mut case = case.ok_or_else(|| e404("BR not found"))?;
    let items = load_br_items(&conn, &br_no, br_year).map_err(|e| e500(&e.to_string()))?;
    case["items"] = json!(items);

    let pdf_bytes = crate::pdf::generate_br_pdf(&case)
        .map_err(|e| e500(&format!("PDF generation failed: {e}")))?;

    Ok((
        [(header::CONTENT_TYPE, "application/pdf"),
         (header::CONTENT_DISPOSITION, &format!("attachment; filename=\"BR_{br_no}_{br_year}.pdf\""))],
        pdf_bytes,
    ).into_response())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn load_br_items(conn: &rusqlite::Connection, br_no: &str, br_year: i64) -> rusqlite::Result<Vec<Value>> {
    let mut stmt = conn.prepare(
        "SELECT id, br_no, br_year, items_sno, items_desc, items_qty, items_uqc,
                items_value, cumulative_duty_rate, items_duty, items_duty_type,
                items_category, items_release_category, items_sub_category
         FROM br_items WHERE br_no=? AND br_year=? ORDER BY items_sno"
    )?;
    let rows: Vec<Value> = stmt.query_map(rusqlite::params![br_no, br_year], |r| {
        Ok(json!({
            "id":                    r.get::<_, i64>(0)?,
            "br_no":                 r.get::<_, String>(1)?,
            "br_year":               r.get::<_, i64>(2)?,
            "items_sno":             r.get::<_, i64>(3)?,
            "items_desc":            r.get::<_, Option<String>>(4)?,
            "items_qty":             r.get::<_, Option<f64>>(5)?,
            "items_uqc":             r.get::<_, Option<String>>(6)?,
            "items_value":           r.get::<_, Option<f64>>(7)?,
            "cumulative_duty_rate":  r.get::<_, Option<f64>>(8)?,
            "items_duty":            r.get::<_, Option<f64>>(9)?,
            "items_duty_type":       r.get::<_, Option<String>>(10)?,
            "items_category":        r.get::<_, Option<String>>(11)?,
            "items_release_category":r.get::<_, Option<String>>(12)?,
            "items_sub_category":    r.get::<_, Option<String>>(13)?,
        }))
    })?.filter_map(|r| r.ok()).collect();
    Ok(rows)
}

trait OptYear { fn year(&self) -> i64; }
impl OptYear for chrono::DateTime<chrono::Local> {
    fn year(&self) -> i64 { chrono::Datelike::year(self) as i64 }
}

trait OptionalExt<T> {
    fn optional(self) -> rusqlite::Result<Option<T>>;
}
impl<T> OptionalExt<T> for rusqlite::Result<T> {
    fn optional(self) -> rusqlite::Result<Option<T>> {
        match self {
            Ok(v)  => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}
