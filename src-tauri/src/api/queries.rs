/// Cross-reference search — given a passport / name / flight, return matching BR + OS + DR records.
use std::sync::Arc;
use axum::{extract::{Query, State}, http::StatusCode, Json};
use serde_json::{json, Value};
use crate::{auth::AuthUser, db::DbPool};

type Db = State<Arc<DbPool>>;
type Err = (StatusCode, Json<Value>);

fn e500(m: &str) -> Err { (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "detail": m }))) }

pub async fn cross_reference(
    State(pool): Db,
    _auth: AuthUser,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Value>, Err> {
    let passport = params.get("passport").map(|s| s.trim().to_uppercase()).unwrap_or_default();
    let name     = params.get("name").map(|s| s.trim().to_uppercase()).unwrap_or_default();
    let flight   = params.get("flight").map(|s| s.trim().to_uppercase()).unwrap_or_default();

    if passport.is_empty() && name.is_empty() && flight.is_empty() {
        return Ok(Json(json!({ "os": [], "br": [], "dr": [] })));
    }

    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;

    // Build WHERE clause with ? placeholders and LIKE pattern params
    let mut conditions: Vec<&str> = Vec::new();
    let mut like_params: Vec<String> = Vec::new();

    if !passport.is_empty() {
        conditions.push("passport_no LIKE ?");
        like_params.push(format!("%{}%", passport));
    }
    if !name.is_empty() {
        conditions.push("pax_name LIKE ?");
        like_params.push(format!("%{}%", name));
    }
    if !flight.is_empty() {
        conditions.push("flight_no LIKE ?");
        like_params.push(format!("%{}%", flight));
    }
    let where_sql = conditions.join(" OR ");

    // OS cases — only active (not soft-deleted) records
    let where_os = format!("({where_sql}) AND entry_deleted='N'");
    let mut os_stmt = conn.prepare(&format!(
        "SELECT os_no, os_year, os_date, pax_name, passport_no, flight_no,
                total_payable, adjudication_date, adj_offr_name, entry_deleted, is_draft
         FROM cops_master WHERE {where_os} ORDER BY os_date DESC LIMIT 50"
    )).map_err(|e| e500(&e.to_string()))?;

    let os_rows: Vec<Value> = os_stmt.query_map(rusqlite::params_from_iter(like_params.iter()), |r| {
        Ok(json!({
            "os_no":             r.get::<_, String>(0)?,
            "os_year":           r.get::<_, Option<i64>>(1)?,
            "os_date":           r.get::<_, Option<String>>(2)?,
            "pax_name":          r.get::<_, Option<String>>(3)?,
            "passport_no":       r.get::<_, Option<String>>(4)?,
            "flight_no":         r.get::<_, Option<String>>(5)?,
            "total_payable":     r.get::<_, Option<f64>>(6)?,
            "adjudication_date": r.get::<_, Option<String>>(7)?,
            "adj_offr_name":     r.get::<_, Option<String>>(8)?,
            "entry_deleted":     r.get::<_, Option<String>>(9)?,
            "is_draft":          r.get::<_, Option<String>>(10)?,
        }))
    }).map_err(|e| e500(&e.to_string()))?.filter_map(|r| r.ok()).collect();

    // BR cases
    let where_br = format!("({where_sql}) AND entry_deleted='N'");
    let mut br_stmt = conn.prepare(&format!(
        "SELECT br_no, br_year, br_date, pax_name, passport_no, flight_no,
                total_payable, br_printed
         FROM br_master WHERE {where_br} ORDER BY br_date DESC LIMIT 50"
    )).map_err(|e| e500(&e.to_string()))?;

    let br_rows: Vec<Value> = br_stmt.query_map(rusqlite::params_from_iter(like_params.iter()), |r| {
        Ok(json!({
            "br_no":         r.get::<_, String>(0)?,
            "br_year":       r.get::<_, i64>(1)?,
            "br_date":       r.get::<_, Option<String>>(2)?,
            "pax_name":      r.get::<_, Option<String>>(3)?,
            "passport_no":   r.get::<_, Option<String>>(4)?,
            "flight_no":     r.get::<_, Option<String>>(5)?,
            "total_payable": r.get::<_, Option<f64>>(6)?,
            "br_printed":    r.get::<_, Option<String>>(7)?,
        }))
    }).map_err(|e| e500(&e.to_string()))?.filter_map(|r| r.ok()).collect();

    // DR cases
    let where_dr = format!("({where_sql}) AND entry_deleted='N'");
    let mut dr_stmt = conn.prepare(&format!(
        "SELECT dr_no, dr_year, dr_date, pax_name, passport_no, flight_no,
                total_items_value, dr_printed
         FROM dr_master WHERE {where_dr} ORDER BY dr_date DESC LIMIT 50"
    )).map_err(|e| e500(&e.to_string()))?;

    let dr_rows: Vec<Value> = dr_stmt.query_map(rusqlite::params_from_iter(like_params.iter()), |r| {
        Ok(json!({
            "dr_no":             r.get::<_, String>(0)?,
            "dr_year":           r.get::<_, i64>(1)?,
            "dr_date":           r.get::<_, Option<String>>(2)?,
            "pax_name":          r.get::<_, Option<String>>(3)?,
            "passport_no":       r.get::<_, Option<String>>(4)?,
            "flight_no":         r.get::<_, Option<String>>(5)?,
            "total_items_value": r.get::<_, Option<f64>>(6)?,
            "dr_printed":        r.get::<_, Option<String>>(7)?,
        }))
    }).map_err(|e| e500(&e.to_string()))?.filter_map(|r| r.ok()).collect();

    Ok(Json(json!({
        "os": os_rows,
        "br": br_rows,
        "dr": dr_rows,
        "query": { "passport": passport, "name": name, "flight": flight },
    })))
}
