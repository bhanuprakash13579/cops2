use std::sync::Arc;
use axum::{extract::{Path, Query, State}, http::StatusCode, Json};
use serde_json::{json, Value};
use crate::{auth::AuthUser, db::DbPool};

type Db = State<Arc<DbPool>>;
type Err = (StatusCode, Json<Value>);

fn e400(m: &str) -> Err { (StatusCode::BAD_REQUEST,           Json(json!({ "detail": m }))) }
fn e404(m: &str) -> Err { (StatusCode::NOT_FOUND,             Json(json!({ "detail": m }))) }
fn e500(m: &str) -> Err { (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "detail": m }))) }

// ── Helpers ───────────────────────────────────────────────────────────────────

fn load_session_row(conn: &rusqlite::Connection, id: i64) -> Result<Option<Value>, Err> {
    let row: Option<Value> = conn.query_row(
        "SELECT s.id, s.report_date, s.shift, s.batch_name, s.tariff_id,
                s.created_by, s.created_at, s.submitted_at, s.submitted_by,
                t.effective_from, t.label, t.baggage_rate, t.liquor_duty_rate,
                t.aidc_liquor_rate, t.gold_bcd_rate, t.aidc_gold_rate,
                t.gold_cons_bcd_rate, t.aidc_gold_cons_rate,
                t.silver_bcd_rate, t.aidc_silver_rate,
                t.silver_cons_rate, t.aidc_silver_cons_rate,
                s.challan_no, s.tariff_snapshot
         FROM dcr_sessions s
         LEFT JOIN dcr_tariffs t ON t.id = s.tariff_id
         WHERE s.id = ?",
        rusqlite::params![id],
        |r| {
            // The rates as they were when this shift was worked. If the tariff
            // row has since been edited or deleted the join yields nothing, and
            // this is what keeps the session explainable.
            let frozen: Option<Value> = r
                .get::<_, Option<String>>(23)?
                .and_then(|t| serde_json::from_str(&t).ok());
            let tariff = if r.get::<_, Option<String>>(9)?.is_some() {
                json!({
                    "effective_from":        r.get::<_, Option<String>>(9)?,
                    "label":                 r.get::<_, Option<String>>(10)?,
                    "baggage_rate":          r.get::<_, Option<f64>>(11)?,
                    "liquor_duty_rate":      r.get::<_, Option<f64>>(12)?,
                    "aidc_liquor_rate":      r.get::<_, Option<f64>>(13)?,
                    "gold_bcd_rate":         r.get::<_, Option<f64>>(14)?,
                    "aidc_gold_rate":        r.get::<_, Option<f64>>(15)?,
                    "gold_cons_bcd_rate":    r.get::<_, Option<f64>>(16)?,
                    "aidc_gold_cons_rate":   r.get::<_, Option<f64>>(17)?,
                    "silver_bcd_rate":       r.get::<_, Option<f64>>(18)?,
                    "aidc_silver_rate":      r.get::<_, Option<f64>>(19)?,
                    "silver_cons_rate":      r.get::<_, Option<f64>>(20)?,
                    "aidc_silver_cons_rate": r.get::<_, Option<f64>>(21)?,
                })
            } else {
                // Referenced tariff gone — use the copy frozen onto the session.
                frozen.clone().unwrap_or(Value::Null)
            };
            Ok(json!({
                "id":           r.get::<_, i64>(0)?,
                "report_date":  r.get::<_, String>(1)?,
                "shift":        r.get::<_, String>(2)?,
                "batch_name":   r.get::<_, Option<String>>(3)?,
                "tariff_id":    r.get::<_, Option<i64>>(4)?,
                "created_by":   r.get::<_, Option<String>>(5)?,
                "created_at":   r.get::<_, Option<String>>(6)?,
                "submitted_at": r.get::<_, Option<String>>(7)?,
                "submitted_by": r.get::<_, Option<String>>(8)?,
                "challan_no":   r.get::<_, Option<String>>(22)?,
                "tariff_applied": frozen,
                "tariff":       tariff,
            }))
        },
    ).optional().map_err(|e| e500(&e.to_string()))?;
    Ok(row)
}

fn load_full_session(conn: &rusqlite::Connection, id: i64) -> Result<Option<Value>, Err> {
    let mut session = match load_session_row(conn, id)? {
        Some(s) => s,
        None => return Ok(None),
    };

    // Load main entries
    let mut stmt = conn.prepare(
        "SELECT id, sort_order, sl_no, br_no, os_ref, item_desc,
                dutiable_value, gold_weight_gms,
                baggage_duty, liquor_duty, cigarette_duty, sw_sc,
                gold_duty_bcd, gold_duty_cons, silver_duty_cons,
                sws_on_gold, aidc_gold_silver, sws_on_silver, aidc_on_liquor,
                redemption_fine, reexport_fine, personal_penalty,
                other_charges, fuel_duty, total_duty,
                flight_no, is_sbi_challan, is_offline_br, overrides,
                  cess_on_cig
         FROM dcr_entries WHERE session_id = ? ORDER BY sort_order",
    ).map_err(|e| e500(&e.to_string()))?;

    let entries: Vec<Value> = stmt.query_map(rusqlite::params![id], |r| {
        Ok(json!({
            "id":               r.get::<_, i64>(0)?,
            "sort_order":       r.get::<_, i64>(1)?,
            "sl_no":            r.get::<_, Option<i64>>(2)?,
            "br_no":            r.get::<_, String>(3)?,
            "os_ref":           r.get::<_, String>(4)?,
            "item_desc":        r.get::<_, String>(5)?,
            "dutiable_value":   r.get::<_, f64>(6)?,
            "gold_weight_gms":  r.get::<_, f64>(7)?,
            "baggage_duty":     r.get::<_, f64>(8)?,
            "liquor_duty":      r.get::<_, f64>(9)?,
            "cigarette_duty":   r.get::<_, f64>(10)?,
            "sw_sc":            r.get::<_, f64>(11)?,
            "gold_duty_bcd":    r.get::<_, f64>(12)?,
            "gold_duty_cons":   r.get::<_, f64>(13)?,
            "silver_duty_cons": r.get::<_, f64>(14)?,
            "sws_on_gold":      r.get::<_, f64>(15)?,
            "aidc_gold_silver": r.get::<_, f64>(16)?,
            "sws_on_silver":    r.get::<_, f64>(17)?,
            "aidc_on_liquor":   r.get::<_, f64>(18)?,
            "redemption_fine":  r.get::<_, f64>(19)?,
            "reexport_fine":    r.get::<_, f64>(20)?,
            "personal_penalty": r.get::<_, f64>(21)?,
            "other_charges":    r.get::<_, f64>(22)?,
            "fuel_duty":        r.get::<_, f64>(23)?,
            "total_duty":       r.get::<_, f64>(24)?,
            "flight_no":        r.get::<_, String>(25)?,
            "is_sbi_challan":   r.get::<_, i64>(26)? != 0,
            "is_offline_br":    r.get::<_, i64>(27)? != 0,
            "overrides":        r.get::<_, Option<String>>(28)?,
            // Appended last on purpose: inserting it mid-list would shift
            // every index below it, and those are positional.
            "cess_on_cig":      r.get::<_, f64>(29)?,
        }))
    }).map_err(|e| e500(&e.to_string()))?.filter_map(|r| r.ok()).collect();

    let entries = settle_cases(entries);

    // Load DR entries
    let mut stmt2 = conn.prepare(
        "SELECT id, sort_order, dr_no, amount, item_desc, remarks
         FROM dcr_dr_entries WHERE session_id = ? ORDER BY sort_order",
    ).map_err(|e| e500(&e.to_string()))?;

    let dr_entries: Vec<Value> = stmt2.query_map(rusqlite::params![id], |r| {
        Ok(json!({
            "id":         r.get::<_, i64>(0)?,
            "sort_order": r.get::<_, i64>(1)?,
            "dr_no":      r.get::<_, String>(2)?,
            "amount":     r.get::<_, f64>(3)?,
            "item_desc":  r.get::<_, String>(4)?,
            "remarks":    r.get::<_, String>(5)?,
        }))
    }).map_err(|e| e500(&e.to_string()))?.filter_map(|r| r.ok()).collect();

    // Load OS entries
    let mut stmt3 = conn.prepare(
        "SELECT id, sort_order, os_no, amount, item_desc, remarks
         FROM dcr_os_entries WHERE session_id = ? ORDER BY sort_order",
    ).map_err(|e| e500(&e.to_string()))?;

    let os_entries: Vec<Value> = stmt3.query_map(rusqlite::params![id], |r| {
        Ok(json!({
            "id":         r.get::<_, i64>(0)?,
            "sort_order": r.get::<_, i64>(1)?,
            "os_no":      r.get::<_, String>(2)?,
            "amount":     r.get::<_, f64>(3)?,
            "item_desc":  r.get::<_, String>(4)?,
            "remarks":    r.get::<_, String>(5)?,
        }))
    }).map_err(|e| e500(&e.to_string()))?.filter_map(|r| r.ok()).collect();

    session["entries"]    = json!(entries);
    session["dr_entries"] = json!(dr_entries);
    session["os_entries"] = json!(os_entries);

    Ok(Some(session))
}

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

// ── Sessions ──────────────────────────────────────────────────────────────────

pub async fn list_sessions(
    State(pool): Db,
    _auth: AuthUser,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let date_filter = params.get("date").map(|s| s.trim().to_string());

    let (where_sql, where_params): (String, Vec<String>) = if let Some(ref d) = date_filter {
        ("WHERE s.report_date = ?".to_string(), vec![d.clone()])
    } else {
        (String::new(), vec![])
    };

    let sql = format!(
        "SELECT s.id, s.report_date, s.shift, s.batch_name, s.tariff_id,
                s.created_by, s.created_at, s.submitted_at, s.submitted_by,
                t.effective_from, t.label, t.baggage_rate, t.liquor_duty_rate,
                t.aidc_liquor_rate, t.gold_bcd_rate, t.aidc_gold_rate,
                t.gold_cons_bcd_rate, t.aidc_gold_cons_rate,
                t.silver_bcd_rate, t.aidc_silver_rate,
                t.silver_cons_rate, t.aidc_silver_cons_rate,
                s.challan_no, s.tariff_snapshot
         FROM dcr_sessions s
         LEFT JOIN dcr_tariffs t ON t.id = s.tariff_id
         {where_sql}
         ORDER BY s.report_date DESC, s.shift ASC"
    );

    let mut stmt = conn.prepare(&sql).map_err(|e| e500(&e.to_string()))?;

    let rows: Vec<Value> = stmt.query_map(
        rusqlite::params_from_iter(where_params.iter()),
        |r| {
            // The rates as they were when this shift was worked. If the tariff
            // row has since been edited or deleted the join yields nothing, and
            // this is what keeps the session explainable.
            let frozen: Option<Value> = r
                .get::<_, Option<String>>(23)?
                .and_then(|t| serde_json::from_str(&t).ok());
            let tariff = if r.get::<_, Option<String>>(9)?.is_some() {
                json!({
                    "effective_from":        r.get::<_, Option<String>>(9)?,
                    "label":                 r.get::<_, Option<String>>(10)?,
                    "baggage_rate":          r.get::<_, Option<f64>>(11)?,
                    "liquor_duty_rate":      r.get::<_, Option<f64>>(12)?,
                    "aidc_liquor_rate":      r.get::<_, Option<f64>>(13)?,
                    "gold_bcd_rate":         r.get::<_, Option<f64>>(14)?,
                    "aidc_gold_rate":        r.get::<_, Option<f64>>(15)?,
                    "gold_cons_bcd_rate":    r.get::<_, Option<f64>>(16)?,
                    "aidc_gold_cons_rate":   r.get::<_, Option<f64>>(17)?,
                    "silver_bcd_rate":       r.get::<_, Option<f64>>(18)?,
                    "aidc_silver_rate":      r.get::<_, Option<f64>>(19)?,
                    "silver_cons_rate":      r.get::<_, Option<f64>>(20)?,
                    "aidc_silver_cons_rate": r.get::<_, Option<f64>>(21)?,
                })
            } else {
                // Referenced tariff gone — use the copy frozen onto the session.
                frozen.clone().unwrap_or(Value::Null)
            };
            Ok(json!({
                "id":           r.get::<_, i64>(0)?,
                "report_date":  r.get::<_, String>(1)?,
                "shift":        r.get::<_, String>(2)?,
                "batch_name":   r.get::<_, Option<String>>(3)?,
                "tariff_id":    r.get::<_, Option<i64>>(4)?,
                "created_by":   r.get::<_, Option<String>>(5)?,
                "created_at":   r.get::<_, Option<String>>(6)?,
                "submitted_at": r.get::<_, Option<String>>(7)?,
                "submitted_by": r.get::<_, Option<String>>(8)?,
                "challan_no":   r.get::<_, Option<String>>(22)?,
                "tariff_applied": frozen,
                "tariff":       tariff,
            }))
        },
    ).map_err(|e| e500(&e.to_string()))?.filter_map(|r| r.ok()).collect();

    Ok(Json(json!({ "items": rows })))
}


/// The full rate set for a tariff row, as JSON — what gets frozen onto a session.
///
/// Copied in rather than referenced because `tariff_id` points at a row that can
/// be edited, superseded, or lost. When that happens the session still has the
/// value and the duty, and nothing to say what rate connected them. An invoice
/// stores the tax rate it charged for exactly this reason.
fn tariff_json(conn: &rusqlite::Connection, id: i64) -> Option<Value> {
    conn.query_row(
        "SELECT effective_from, label,
                baggage_rate, liquor_duty_rate, aidc_liquor_rate,
                gold_bcd_rate, aidc_gold_rate,
                gold_cons_bcd_rate, aidc_gold_cons_rate,
                silver_bcd_rate, aidc_silver_rate,
                silver_cons_rate, aidc_silver_cons_rate
         FROM dcr_tariffs WHERE id = ?1",
        [id],
        |r| {
            Ok(json!({
                "effective_from":        r.get::<_, Option<String>>(0)?,
                "label":                 r.get::<_, Option<String>>(1)?,
                "baggage_rate":          r.get::<_, Option<f64>>(2)?,
                "liquor_duty_rate":      r.get::<_, Option<f64>>(3)?,
                "aidc_liquor_rate":      r.get::<_, Option<f64>>(4)?,
                "gold_bcd_rate":         r.get::<_, Option<f64>>(5)?,
                "aidc_gold_rate":        r.get::<_, Option<f64>>(6)?,
                "gold_cons_bcd_rate":    r.get::<_, Option<f64>>(7)?,
                "aidc_gold_cons_rate":   r.get::<_, Option<f64>>(8)?,
                "silver_bcd_rate":       r.get::<_, Option<f64>>(9)?,
                "aidc_silver_rate":      r.get::<_, Option<f64>>(10)?,
                "silver_cons_rate":      r.get::<_, Option<f64>>(11)?,
                "aidc_silver_cons_rate": r.get::<_, Option<f64>>(12)?,
                "frozen_at":             chrono::Local::now().to_rfc3339(),
            }))
        },
    ).ok()
}

pub async fn create_session(
    State(pool): Db,
    _auth: AuthUser,
    Json(req): Json<Value>,
) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;

    let report_date = req.get("report_date").and_then(|v| v.as_str())
        .ok_or_else(|| e400("report_date required"))?;
    let shift = req.get("shift").and_then(|v| v.as_str())
        .ok_or_else(|| e400("shift required (DAY or NIGHT)"))?;
    let batch_name = req.get("batch_name").and_then(|v| v.as_str());
    let created_by = req.get("created_by").and_then(|v| v.as_str());

    // Look up latest tariff
    let tariff_id: Option<i64> = conn.query_row(
        "SELECT id FROM dcr_tariffs ORDER BY effective_from DESC LIMIT 1",
        [],
        |r| r.get(0),
    ).optional().map_err(|e| e500(&e.to_string()))?;

    conn.execute(
        "INSERT INTO dcr_sessions (report_date, shift, batch_name, tariff_id, created_by, tariff_snapshot)
         VALUES (?, ?, ?, ?, ?,?)",
        rusqlite::params![
            report_date, shift, batch_name, tariff_id, created_by,
            // Frozen at creation: the rates in force for this shift.
            tariff_id.and_then(|t| tariff_json(&conn, t)).map(|v| v.to_string()),
        ],
    ).map_err(|e| e500(&e.to_string()))?;

    let new_id = conn.last_insert_rowid();

    let session = load_session_row(&conn, new_id)?
        .ok_or_else(|| e500("Failed to load created session"))?;

    Ok(Json(session))
}

/// Every session in a month, with its entries — one request, not sixty-two.
///
/// The monthly register is built in the browser, so it needs the whole month's
/// data. Fetching it session by session would be up to 62 round trips for one
/// button press; on a LAN share that is the difference between a report that
/// appears and one an officer gives up waiting for.
///
/// Ordered by date then shift, which is the order the register is read in.
pub async fn month_sessions(
    State(pool): Db,
    _auth: AuthUser,
    Path((year, month)): Path<(i64, u32)>,
) -> Result<Json<Value>, Err> {
    if !(1..=12).contains(&month) || year < 2000 {
        return Err(e400("Invalid year or month."));
    }
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;

    // Text comparison on YYYY-MM-DD is exact and uses the index, and avoids
    // month-length arithmetic entirely.
    let prefix = format!("{year:04}-{month:02}-");
    let mut stmt = conn
        .prepare(
            "SELECT id FROM dcr_sessions
             WHERE report_date LIKE ?1 || '%'
             ORDER BY report_date, CASE shift WHEN 'DAY' THEN 0 ELSE 1 END, id",
        )
        .map_err(|e| e500(&e.to_string()))?;
    let ids: Vec<i64> = stmt
        .query_map([&prefix], |r| r.get(0))
        .map_err(|e| e500(&e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();

    let mut sessions = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(full) = load_full_session(&conn, id)? {
            sessions.push(full);
        }
    }
    Ok(Json(json!({ "year": year, "month": month, "sessions": sessions })))
}

pub async fn get_session(
    State(pool): Db,
    _auth: AuthUser,
    Path(id): Path<i64>,
) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let session = load_full_session(&conn, id)?
        .ok_or_else(|| e404("Session not found"))?;
    Ok(Json(session))
}

pub async fn update_session(
    State(pool): Db,
    _auth: AuthUser,
    Path(id): Path<i64>,
    Json(req): Json<Value>,
) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;

    let batch_name = req.get("batch_name").and_then(|v| v.as_str());
    let created_by = req.get("created_by").and_then(|v| v.as_str());

    conn.execute(
        "UPDATE dcr_sessions SET batch_name = ?, created_by = ? WHERE id = ?",
        rusqlite::params![batch_name, created_by, id],
    ).map_err(|e| e500(&e.to_string()))?;

    let session = load_session_row(&conn, id)?
        .ok_or_else(|| e404("Session not found"))?;

    Ok(Json(session))
}

pub async fn bulk_save_entries(
    State(pool): Db,
    _auth: AuthUser,
    Path(id): Path<i64>,
    Json(req): Json<Value>,
) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;

    // Verify session exists
    let exists: i64 = conn.query_row(
        "SELECT COUNT(*) FROM dcr_sessions WHERE id = ?",
        rusqlite::params![id],
        |r| r.get(0),
    ).unwrap_or(0);
    if exists == 0 { return Err(e404("Session not found")); }

    let entries    = req.get("entries").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    let dr_entries = req.get("dr_entries").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    let os_entries = req.get("os_entries").and_then(|v| v.as_array()).cloned().unwrap_or_default();

    conn.execute_batch("BEGIN").map_err(|e| e500(&e.to_string()))?;
    let write_result: Result<(), Err> = (|| {
        // Delete all existing
        conn.execute("DELETE FROM dcr_entries WHERE session_id = ?",    rusqlite::params![id])
            .map_err(|e| e500(&e.to_string()))?;
        conn.execute("DELETE FROM dcr_dr_entries WHERE session_id = ?", rusqlite::params![id])
            .map_err(|e| e500(&e.to_string()))?;
        conn.execute("DELETE FROM dcr_os_entries WHERE session_id = ?", rusqlite::params![id])
            .map_err(|e| e500(&e.to_string()))?;

        // Insert main entries
        for (i, e) in entries.iter().enumerate() {
            conn.execute(
                "INSERT INTO dcr_entries (
                    session_id, sort_order, sl_no, br_no, os_ref, item_desc,
                    dutiable_value, gold_weight_gms,
                    baggage_duty, liquor_duty, cigarette_duty, sw_sc,
                    gold_duty_bcd, gold_duty_cons, silver_duty_cons,
                    sws_on_gold, aidc_gold_silver, sws_on_silver, aidc_on_liquor,
                    redemption_fine, reexport_fine, personal_penalty,
                    other_charges, fuel_duty, total_duty,
                    flight_no, is_sbi_challan, is_offline_br, overrides,
                      cess_on_cig
                 ) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
                rusqlite::params![
                    id,
                    e.get("sort_order").and_then(|v| v.as_i64()).unwrap_or(i as i64),
                    e.get("sl_no").and_then(|v| v.as_i64()),
                    e.get("br_no").and_then(|v| v.as_str()).unwrap_or(""),
                    e.get("os_ref").and_then(|v| v.as_str()).unwrap_or(""),
                    e.get("item_desc").and_then(|v| v.as_str()).unwrap_or(""),
                    e.get("dutiable_value").and_then(|v| v.as_f64()).unwrap_or(0.0),
                    e.get("gold_weight_gms").and_then(|v| v.as_f64()).unwrap_or(0.0),
                    e.get("baggage_duty").and_then(|v| v.as_f64()).unwrap_or(0.0),
                    e.get("liquor_duty").and_then(|v| v.as_f64()).unwrap_or(0.0),
                    e.get("cigarette_duty").and_then(|v| v.as_f64()).unwrap_or(0.0),
                    e.get("sw_sc").and_then(|v| v.as_f64()).unwrap_or(0.0),
                    e.get("gold_duty_bcd").and_then(|v| v.as_f64()).unwrap_or(0.0),
                    e.get("gold_duty_cons").and_then(|v| v.as_f64()).unwrap_or(0.0),
                    e.get("silver_duty_cons").and_then(|v| v.as_f64()).unwrap_or(0.0),
                    e.get("sws_on_gold").and_then(|v| v.as_f64()).unwrap_or(0.0),
                    e.get("aidc_gold_silver").and_then(|v| v.as_f64()).unwrap_or(0.0),
                    e.get("sws_on_silver").and_then(|v| v.as_f64()).unwrap_or(0.0),
                    e.get("aidc_on_liquor").and_then(|v| v.as_f64()).unwrap_or(0.0),
                    e.get("redemption_fine").and_then(|v| v.as_f64()).unwrap_or(0.0),
                    e.get("reexport_fine").and_then(|v| v.as_f64()).unwrap_or(0.0),
                    e.get("personal_penalty").and_then(|v| v.as_f64()).unwrap_or(0.0),
                    e.get("other_charges").and_then(|v| v.as_f64()).unwrap_or(0.0),
                    e.get("fuel_duty").and_then(|v| v.as_f64()).unwrap_or(0.0),
                    e.get("total_duty").and_then(|v| v.as_f64()).unwrap_or(0.0),
                    e.get("flight_no").and_then(|v| v.as_str()).unwrap_or(""),
                    if e.get("is_sbi_challan").and_then(|v| v.as_bool()).unwrap_or(false) { 1i64 } else { 0i64 },
                    if e.get("is_offline_br").and_then(|v| v.as_bool()).unwrap_or(false) { 1i64 } else { 0i64 },
                    e.get("overrides").and_then(|v| v.as_str()),
                    e.get("cess_on_cig").and_then(|v| v.as_f64()).unwrap_or(0.0),
                ],
            ).map_err(|e| e500(&e.to_string()))?;
        }

        // Insert DR entries
        for (i, e) in dr_entries.iter().enumerate() {
            conn.execute(
                "INSERT INTO dcr_dr_entries (session_id, sort_order, dr_no, amount, item_desc, remarks)
                 VALUES (?, ?, ?, ?, ?, ?)",
                rusqlite::params![
                    id,
                    e.get("sort_order").and_then(|v| v.as_i64()).unwrap_or(i as i64),
                    e.get("dr_no").and_then(|v| v.as_str()).unwrap_or(""),
                    e.get("amount").and_then(|v| v.as_f64()).unwrap_or(0.0),
                    e.get("item_desc").and_then(|v| v.as_str()).unwrap_or(""),
                    e.get("remarks").and_then(|v| v.as_str()).unwrap_or(""),
                ],
            ).map_err(|e| e500(&e.to_string()))?;
        }

        // Insert OS entries
        for (i, e) in os_entries.iter().enumerate() {
            conn.execute(
                "INSERT INTO dcr_os_entries (session_id, sort_order, os_no, amount, item_desc, remarks)
                 VALUES (?, ?, ?, ?, ?, ?)",
                rusqlite::params![
                    id,
                    e.get("sort_order").and_then(|v| v.as_i64()).unwrap_or(i as i64),
                    e.get("os_no").and_then(|v| v.as_str()).unwrap_or(""),
                    e.get("amount").and_then(|v| v.as_f64()).unwrap_or(0.0),
                    e.get("item_desc").and_then(|v| v.as_str()).unwrap_or(""),
                    e.get("remarks").and_then(|v| v.as_str()).unwrap_or(""),
                ],
            ).map_err(|e| e500(&e.to_string()))?;
        }

        Ok(())
    })();

    match write_result {
        Ok(()) => conn.execute_batch("COMMIT").map_err(|e| e500(&e.to_string()))?,
        Err(e) => { let _ = conn.execute_batch("ROLLBACK"); return Err(e); }
    }

    // The sheet knows which receipt settled which case. Carry that across to the
    // register, which otherwise never learns it. Outside the transaction above:
    // a failure to link must not cost the office the shift's figures.
    let linked = link_receipts_to_cases(&conn, id);

    let session = load_full_session(&conn, id)?
        .ok_or_else(|| e500("Failed to reload session after save"))?;
    let _ = linked;

    Ok(Json(session))
}

/// Record the bank deposit challan number against a session.
///
/// One deposit covers a shift's offline BR collections, so it belongs on the
/// SESSION rather than an entry. It is the reference an auditor follows from the
/// register to the bank, and COPS2 had nowhere to put it: the column did not
/// exist and neither did this route, so a figure the office is accountable for
/// simply had no home.
///
/// Editable after the fact on purpose. The number arrives from the bank after
/// the shift is written up, and a mistyped challan has to be correctable without
/// unpicking the session — refusing to change it would only push officers into
/// keeping the real number somewhere outside the system.
/// Find a session by the date and shift it covers, rather than by id.
///
/// An officer opening the duty register thinks in "the night shift on the 9th",
/// not in row numbers. Without this the screen has to list every session and
/// filter client-side, which works until a year of sessions has accumulated.
///
/// Shift is upper-cased before matching: the column is constrained to DAY or
/// NIGHT, and a link carrying `night` would otherwise find nothing and look
/// like the session was missing.
pub async fn session_by_date(
    State(pool): Db,
    _auth: AuthUser,
    Path((report_date, shift)): Path<(String, String)>,
) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let id: Option<i64> = conn
        .query_row(
            "SELECT id FROM dcr_sessions
             WHERE report_date = ?1 AND UPPER(shift) = UPPER(?2)
             ORDER BY id LIMIT 1",
            rusqlite::params![report_date.trim(), shift.trim()],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e500(&e.to_string()))?;

    let id = id.ok_or_else(|| e404("No duty session for that date and shift."))?;
    let row = load_session_row(&conn, id)?.ok_or_else(|| e404("Session not found"))?;
    Ok(Json(row))
}

pub async fn set_challan(
    State(pool): Db,
    _auth: AuthUser,
    Path(id): Path<i64>,
    Json(req): Json<Value>,
) -> Result<Json<Value>, Err> {
    let challan = req
        .get("challan_no")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if challan.is_empty() {
        return Err(e400("Enter the challan number, or skip if there were no offline BRs."));
    }
    if challan.chars().count() > 50 {
        return Err(e400("Challan number is too long (50 characters maximum)."));
    }

    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let affected = conn
        .execute(
            "UPDATE dcr_sessions SET challan_no = ?1 WHERE id = ?2",
            rusqlite::params![challan, id],
        )
        .map_err(|e| e500(&e.to_string()))?;
    if affected == 0 {
        return Err(e404("Session not found"));
    }

    let row = load_session_row(&conn, id)?.ok_or_else(|| e404("Session not found"))?;
    Ok(Json(row))
}

pub async fn submit_session(
    State(pool): Db,
    _auth: AuthUser,
    Path(id): Path<i64>,
    Json(req): Json<Value>,
) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let submitted_by = req.get("submitted_by").and_then(|v| v.as_str());

    conn.execute(
        "UPDATE dcr_sessions SET submitted_at = datetime('now'), submitted_by = ? WHERE id = ?",
        rusqlite::params![submitted_by, id],
    ).map_err(|e| e500(&e.to_string()))?;

    let session = load_session_row(&conn, id)?
        .ok_or_else(|| e404("Session not found"))?;

    Ok(Json(session))
}

pub async fn unsubmit_session(
    State(pool): Db,
    _auth: AuthUser,
    Path(id): Path<i64>,
) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;

    conn.execute(
        "UPDATE dcr_sessions SET submitted_at = NULL, submitted_by = NULL WHERE id = ?",
        rusqlite::params![id],
    ).map_err(|e| e500(&e.to_string()))?;

    let session = load_session_row(&conn, id)?
        .ok_or_else(|| e404("Session not found"))?;

    Ok(Json(session))
}

// ── Tariffs ───────────────────────────────────────────────────────────────────

pub async fn list_tariffs(
    State(pool): Db,
    _auth: AuthUser,
) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;

    let mut stmt = conn.prepare(
        "SELECT id, effective_from, label,
                baggage_rate, liquor_duty_rate, aidc_liquor_rate,
                gold_bcd_rate, aidc_gold_rate,
                gold_cons_bcd_rate, aidc_gold_cons_rate,
                silver_bcd_rate, aidc_silver_rate,
                silver_cons_rate, aidc_silver_cons_rate,
                created_at
         FROM dcr_tariffs ORDER BY effective_from DESC",
    ).map_err(|e| e500(&e.to_string()))?;

    let rows: Vec<Value> = stmt.query_map([], |r| {
        Ok(json!({
            "id":                    r.get::<_, i64>(0)?,
            "effective_from":        r.get::<_, String>(1)?,
            "label":                 r.get::<_, Option<String>>(2)?,
            "baggage_rate":          r.get::<_, f64>(3)?,
            "liquor_duty_rate":      r.get::<_, f64>(4)?,
            "aidc_liquor_rate":      r.get::<_, f64>(5)?,
            "gold_bcd_rate":         r.get::<_, f64>(6)?,
            "aidc_gold_rate":        r.get::<_, f64>(7)?,
            "gold_cons_bcd_rate":    r.get::<_, f64>(8)?,
            "aidc_gold_cons_rate":   r.get::<_, f64>(9)?,
            "silver_bcd_rate":       r.get::<_, f64>(10)?,
            "aidc_silver_rate":      r.get::<_, f64>(11)?,
            "silver_cons_rate":      r.get::<_, f64>(12)?,
            "aidc_silver_cons_rate": r.get::<_, f64>(13)?,
            "created_at":            r.get::<_, Option<String>>(14)?,
        }))
    }).map_err(|e| e500(&e.to_string()))?.filter_map(|r| r.ok()).collect();

    Ok(Json(json!({ "items": rows })))
}

/// The tariff in force on a given date — `?as_of=YYYY-MM-DD`, today by default.
///
/// This route was MISSING while the screen that needs it was already calling it:
/// FormulaRulesPage requests /dcr/tariffs/current on load, got a 404, and could
/// not show which rates are actually in force. Found by checking every call the
/// frontend makes against the routes that exist, rather than by reading either
/// side on its own.
///
/// Point-in-time, not "the newest row". Duty rates change at a budget and the
/// office still edits and reprints older sessions, which must be valued at the
/// rates that applied THEN. Picking the latest row regardless of date would
/// silently revalue historical collections — wrong in a way nobody notices until
/// an audit.
///
/// Falls back to the EARLIEST tariff when none is yet effective, matching the
/// original: a session dated before the first tariff row is better valued at the
/// oldest known rates than refused outright.
pub async fn current_tariff(
    State(pool): Db,
    _auth: AuthUser,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Value>, Err> {
    let as_of = q
        .get("as_of")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| chrono::Local::now().format("%Y-%m-%d").to_string());

    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    const COLS: &str = "id, effective_from, label,
                        baggage_rate, liquor_duty_rate, aidc_liquor_rate,
                        gold_bcd_rate, aidc_gold_rate,
                        gold_cons_bcd_rate, aidc_gold_cons_rate,
                        silver_bcd_rate, aidc_silver_rate,
                        silver_cons_rate, aidc_silver_cons_rate,
                        created_at";

    let row = |r: &rusqlite::Row| -> rusqlite::Result<Value> {
        Ok(json!({
            "id":                    r.get::<_, i64>(0)?,
            "effective_from":        r.get::<_, String>(1)?,
            "label":                 r.get::<_, Option<String>>(2)?,
            "baggage_rate":          r.get::<_, f64>(3)?,
            "liquor_duty_rate":      r.get::<_, f64>(4)?,
            "aidc_liquor_rate":      r.get::<_, f64>(5)?,
            "gold_bcd_rate":         r.get::<_, f64>(6)?,
            "aidc_gold_rate":        r.get::<_, f64>(7)?,
            "gold_cons_bcd_rate":    r.get::<_, f64>(8)?,
            "aidc_gold_cons_rate":   r.get::<_, f64>(9)?,
            "silver_bcd_rate":       r.get::<_, f64>(10)?,
            "aidc_silver_rate":      r.get::<_, f64>(11)?,
            "silver_cons_rate":      r.get::<_, f64>(12)?,
            "aidc_silver_cons_rate": r.get::<_, f64>(13)?,
            "created_at":            r.get::<_, Option<String>>(14)?,
        }))
    };

    let found = conn
        .query_row(
            &format!(
                "SELECT {COLS} FROM dcr_tariffs
                 WHERE effective_from <= ?1
                 ORDER BY effective_from DESC LIMIT 1"
            ),
            [&as_of],
            row,
        )
        .or_else(|_| {
            conn.query_row(
                &format!("SELECT {COLS} FROM dcr_tariffs ORDER BY effective_from ASC LIMIT 1"),
                [],
                row,
            )
        });

    match found {
        Ok(v) => Ok(Json(v)),
        Err(_) => Err(e404("No duty rates have been set up yet. Add a tariff first.")),
    }
}

pub async fn create_tariff(
    State(pool): Db,
    _auth: AuthUser,
    Json(req): Json<Value>,
) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;

    let effective_from = req.get("effective_from").and_then(|v| v.as_str())
        .ok_or_else(|| e400("effective_from required"))?;

    conn.execute(
        "INSERT INTO dcr_tariffs (
            effective_from, label,
            baggage_rate, liquor_duty_rate, aidc_liquor_rate,
            gold_bcd_rate, aidc_gold_rate,
            gold_cons_bcd_rate, aidc_gold_cons_rate,
            silver_bcd_rate, aidc_silver_rate,
            silver_cons_rate, aidc_silver_cons_rate
         ) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)",
        rusqlite::params![
            effective_from,
            req.get("label").and_then(|v| v.as_str()),
            req.get("baggage_rate").and_then(|v| v.as_f64()).unwrap_or(0.35),
            req.get("liquor_duty_rate").and_then(|v| v.as_f64()).unwrap_or(0.15),
            req.get("aidc_liquor_rate").and_then(|v| v.as_f64()).unwrap_or(0.035),
            req.get("gold_bcd_rate").and_then(|v| v.as_f64()).unwrap_or(0.125),
            req.get("aidc_gold_rate").and_then(|v| v.as_f64()).unwrap_or(0.05),
            req.get("gold_cons_bcd_rate").and_then(|v| v.as_f64()).unwrap_or(0.125),
            req.get("aidc_gold_cons_rate").and_then(|v| v.as_f64()).unwrap_or(0.05),
            req.get("silver_bcd_rate").and_then(|v| v.as_f64()).unwrap_or(0.35),
            req.get("aidc_silver_rate").and_then(|v| v.as_f64()).unwrap_or(0.05),
            req.get("silver_cons_rate").and_then(|v| v.as_f64()).unwrap_or(0.35),
            req.get("aidc_silver_cons_rate").and_then(|v| v.as_f64()).unwrap_or(0.05),
        ],
    ).map_err(|e| e500(&e.to_string()))?;

    let new_id = conn.last_insert_rowid();

    let row: Value = conn.query_row(
        "SELECT id, effective_from, label,
                baggage_rate, liquor_duty_rate, aidc_liquor_rate,
                gold_bcd_rate, aidc_gold_rate,
                gold_cons_bcd_rate, aidc_gold_cons_rate,
                silver_bcd_rate, aidc_silver_rate,
                silver_cons_rate, aidc_silver_cons_rate,
                created_at
         FROM dcr_tariffs WHERE id = ?",
        rusqlite::params![new_id],
        |r| Ok(json!({
            "id":                    r.get::<_, i64>(0)?,
            "effective_from":        r.get::<_, String>(1)?,
            "label":                 r.get::<_, Option<String>>(2)?,
            "baggage_rate":          r.get::<_, f64>(3)?,
            "liquor_duty_rate":      r.get::<_, f64>(4)?,
            "aidc_liquor_rate":      r.get::<_, f64>(5)?,
            "gold_bcd_rate":         r.get::<_, f64>(6)?,
            "aidc_gold_rate":        r.get::<_, f64>(7)?,
            "gold_cons_bcd_rate":    r.get::<_, f64>(8)?,
            "aidc_gold_cons_rate":   r.get::<_, f64>(9)?,
            "silver_bcd_rate":       r.get::<_, f64>(10)?,
            "aidc_silver_rate":      r.get::<_, f64>(11)?,
            "silver_cons_rate":      r.get::<_, f64>(12)?,
            "aidc_silver_cons_rate": r.get::<_, f64>(13)?,
            "created_at":            r.get::<_, Option<String>>(14)?,
        })),
    ).map_err(|e| e500(&e.to_string()))?;

    Ok(Json(row))
}

// ── Formula Rules ─────────────────────────────────────────────────────────────

fn load_rule_row(r: &rusqlite::Row) -> rusqlite::Result<Value> {
    Ok(json!({
        "id":              r.get::<_, i64>(0)?,
        "sort_order":      r.get::<_, i64>(1)?,
        "target_column":   r.get::<_, String>(2)?,
        "column_label":    r.get::<_, Option<String>>(3)?,
        "condition_type":  r.get::<_, String>(4)?,
        "condition_items": r.get::<_, String>(5)?,
        "expression":      r.get::<_, String>(6)?,
        "is_active":       r.get::<_, i64>(7)? != 0,
        "notes":           r.get::<_, Option<String>>(8)?,
    }))
}

pub async fn list_rules(
    State(pool): Db,
    _auth: AuthUser,
) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;

    let mut stmt = conn.prepare(
        "SELECT id, sort_order, target_column, column_label,
                condition_type, condition_items, expression, is_active, notes
         FROM dcr_formula_rules ORDER BY sort_order ASC",
    ).map_err(|e| e500(&e.to_string()))?;

    let rows: Vec<Value> = stmt.query_map([], load_rule_row)
        .map_err(|e| e500(&e.to_string()))?.filter_map(|r| r.ok()).collect();

    Ok(Json(json!({ "items": rows })))
}

pub async fn create_rule(
    State(pool): Db,
    _auth: AuthUser,
    Json(req): Json<Value>,
) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;

    let target_column = req.get("target_column").and_then(|v| v.as_str())
        .ok_or_else(|| e400("target_column required"))?;
    let expression = req.get("expression").and_then(|v| v.as_str())
        .ok_or_else(|| e400("expression required"))?;

    conn.execute(
        "INSERT INTO dcr_formula_rules
            (sort_order, target_column, column_label, condition_type, condition_items, expression, is_active, notes)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        rusqlite::params![
            req.get("sort_order").and_then(|v| v.as_i64()).unwrap_or(0),
            target_column,
            req.get("column_label").and_then(|v| v.as_str()),
            req.get("condition_type").and_then(|v| v.as_str()).unwrap_or("all"),
            req.get("condition_items").and_then(|v| v.as_str()).unwrap_or(""),
            expression,
            if req.get("is_active").and_then(|v| v.as_bool()).unwrap_or(true) { 1i64 } else { 0i64 },
            req.get("notes").and_then(|v| v.as_str()),
        ],
    ).map_err(|e| e500(&e.to_string()))?;

    let new_id = conn.last_insert_rowid();

    let row: Value = conn.query_row(
        "SELECT id, sort_order, target_column, column_label,
                condition_type, condition_items, expression, is_active, notes
         FROM dcr_formula_rules WHERE id = ?",
        rusqlite::params![new_id],
        load_rule_row,
    ).map_err(|e| e500(&e.to_string()))?;

    Ok(Json(row))
}

pub async fn update_rule(
    State(pool): Db,
    _auth: AuthUser,
    Path(id): Path<i64>,
    Json(req): Json<Value>,
) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;

    let target_column = req.get("target_column").and_then(|v| v.as_str())
        .ok_or_else(|| e400("target_column required"))?;
    let expression = req.get("expression").and_then(|v| v.as_str())
        .ok_or_else(|| e400("expression required"))?;

    let updated = conn.execute(
        "UPDATE dcr_formula_rules SET
            sort_order = ?, target_column = ?, column_label = ?,
            condition_type = ?, condition_items = ?, expression = ?,
            is_active = ?, notes = ?
         WHERE id = ?",
        rusqlite::params![
            req.get("sort_order").and_then(|v| v.as_i64()).unwrap_or(0),
            target_column,
            req.get("column_label").and_then(|v| v.as_str()),
            req.get("condition_type").and_then(|v| v.as_str()).unwrap_or("all"),
            req.get("condition_items").and_then(|v| v.as_str()).unwrap_or(""),
            expression,
            if req.get("is_active").and_then(|v| v.as_bool()).unwrap_or(true) { 1i64 } else { 0i64 },
            req.get("notes").and_then(|v| v.as_str()),
            id,
        ],
    ).map_err(|e| e500(&e.to_string()))?;

    if updated == 0 { return Err(e404("Rule not found")); }

    let row: Value = conn.query_row(
        "SELECT id, sort_order, target_column, column_label,
                condition_type, condition_items, expression, is_active, notes
         FROM dcr_formula_rules WHERE id = ?",
        rusqlite::params![id],
        load_rule_row,
    ).map_err(|e| e500(&e.to_string()))?;

    Ok(Json(row))
}

pub async fn delete_rule(
    State(pool): Db,
    _auth: AuthUser,
    Path(id): Path<i64>,
) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;

    let deleted = conn.execute(
        "DELETE FROM dcr_formula_rules WHERE id = ?",
        rusqlite::params![id],
    ).map_err(|e| e500(&e.to_string()))?;

    if deleted == 0 { return Err(e404("Rule not found")); }

    Ok(Json(json!({ "message": "Deleted." })))
}

pub async fn reorder_rules(
    State(pool): Db,
    _auth: AuthUser,
    Json(order): Json<Vec<i64>>,
) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;

    conn.execute_batch("BEGIN").map_err(|e| e500(&e.to_string()))?;
    let write_result: Result<(), Err> = (|| {
        for (pos, rule_id) in order.iter().enumerate() {
            conn.execute(
                "UPDATE dcr_formula_rules SET sort_order = ? WHERE id = ?",
                rusqlite::params![pos as i64, rule_id],
            ).map_err(|e| e500(&e.to_string()))?;
        }
        Ok(())
    })();

    match write_result {
        Ok(()) => conn.execute_batch("COMMIT").map_err(|e| e500(&e.to_string()))?,
        Err(e) => { let _ = conn.execute_batch("ROLLBACK"); return Err(e); }
    }

    let mut stmt = conn.prepare(
        "SELECT id, sort_order, target_column, column_label,
                condition_type, condition_items, expression, is_active, notes
         FROM dcr_formula_rules ORDER BY sort_order ASC",
    ).map_err(|e| e500(&e.to_string()))?;

    let rows: Vec<Value> = stmt.query_map([], load_rule_row)
        .map_err(|e| e500(&e.to_string()))?.filter_map(|r| r.ok()).collect();

    Ok(Json(json!({ "items": rows })))
}

// ── Item Types ────────────────────────────────────────────────────────────────

pub async fn list_item_types(
    State(pool): Db,
    _auth: AuthUser,
) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;

    let mut stmt = conn.prepare(
        "SELECT id, name, usage_count, is_system
         FROM dcr_item_types ORDER BY usage_count DESC, name ASC",
    ).map_err(|e| e500(&e.to_string()))?;

    let rows: Vec<Value> = stmt.query_map([], |r| {
        Ok(json!({
            "id":          r.get::<_, i64>(0)?,
            "name":        r.get::<_, String>(1)?,
            "usage_count": r.get::<_, i64>(2)?,
            "is_system":   r.get::<_, i64>(3)? != 0,
        }))
    }).map_err(|e| e500(&e.to_string()))?.filter_map(|r| r.ok()).collect();

    Ok(Json(json!({ "items": rows })))
}

pub async fn create_item_type(
    State(pool): Db,
    _auth: AuthUser,
    Json(req): Json<Value>,
) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;

    let name = req.get("name").and_then(|v| v.as_str())
        .ok_or_else(|| e400("name required"))?;
    let is_system = if req.get("is_system").and_then(|v| v.as_bool()).unwrap_or(false) { 1i64 } else { 0i64 };

    conn.execute(
        "INSERT INTO dcr_item_types (name, is_system) VALUES (?, ?)",
        rusqlite::params![name, is_system],
    ).map_err(|e| e500(&e.to_string()))?;

    let new_id = conn.last_insert_rowid();

    let row: Value = conn.query_row(
        "SELECT id, name, usage_count, is_system FROM dcr_item_types WHERE id = ?",
        rusqlite::params![new_id],
        |r| Ok(json!({
            "id":          r.get::<_, i64>(0)?,
            "name":        r.get::<_, String>(1)?,
            "usage_count": r.get::<_, i64>(2)?,
            "is_system":   r.get::<_, i64>(3)? != 0,
        })),
    ).map_err(|e| e500(&e.to_string()))?;

    Ok(Json(row))
}

pub async fn use_item_type(
    State(pool): Db,
    _auth: AuthUser,
    Path(id): Path<i64>,
) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;

    let updated = conn.execute(
        "UPDATE dcr_item_types SET usage_count = usage_count + 1 WHERE id = ?",
        rusqlite::params![id],
    ).map_err(|e| e500(&e.to_string()))?;

    if updated == 0 { return Err(e404("Item type not found")); }

    Ok(Json(json!({ "message": "ok" })))
}

// ── Settings ──────────────────────────────────────────────────────────────────

pub async fn get_settings(
    State(pool): Db,
    _auth: AuthUser,
) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;

    let row: Option<Value> = conn.query_row(
        "SELECT id, station_name, officer_name, designation FROM dcr_settings WHERE id = 1",
        [],
        |r| Ok(json!({
            "id":           r.get::<_, i64>(0)?,
            "station_name": r.get::<_, String>(1)?,
            "officer_name": r.get::<_, Option<String>>(2)?,
            "designation":  r.get::<_, Option<String>>(3)?,
        })),
    ).optional().map_err(|e| e500(&e.to_string()))?;

    let settings = row.unwrap_or_else(|| json!({
        "id":           1,
        "station_name": "CUSTOMS, CHENNAI AIRPORT",
        "officer_name": null,
        "designation":  null,
    }));

    Ok(Json(settings))
}

pub async fn update_settings(
    State(pool): Db,
    _auth: AuthUser,
    Json(req): Json<Value>,
) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;

    let station_name = req.get("station_name").and_then(|v| v.as_str())
        .ok_or_else(|| e400("station_name required"))?;
    let officer_name = req.get("officer_name").and_then(|v| v.as_str());
    let designation  = req.get("designation").and_then(|v| v.as_str());

    conn.execute(
        "INSERT OR REPLACE INTO dcr_settings (id, station_name, officer_name, designation)
         VALUES (1, ?, ?, ?)",
        rusqlite::params![station_name, officer_name, designation],
    ).map_err(|e| e500(&e.to_string()))?;

    let row: Value = conn.query_row(
        "SELECT id, station_name, officer_name, designation FROM dcr_settings WHERE id = 1",
        [],
        |r| Ok(json!({
            "id":           r.get::<_, i64>(0)?,
            "station_name": r.get::<_, String>(1)?,
            "officer_name": r.get::<_, Option<String>>(2)?,
            "designation":  r.get::<_, Option<String>>(3)?,
        })),
    ).map_err(|e| e500(&e.to_string()))?;

    Ok(Json(row))
}

// ── Receipts for a case, so the revenue sheet does not have to be told twice ──

/// The baggage receipts belonging to an O.S., for an officer who has typed its
/// number into the revenue sheet.
///
/// The association already exists: the case records its receipts when the
/// adjudication is completed. Asking an officer to re-enter them during a shift
/// change is asking them to copy something the register already knows, which is
/// how the column ends up blank. Nothing is written here — the sheet fills a
/// field the officer left empty and never touches one they typed in.
///
/// Accepts "520/2026" or "520" with a separate year.
pub async fn receipts_for_os(
    State(pool): Db,
    _auth: AuthUser,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Value>, Err> {
    let raw = params.get("os_ref").map(|s| s.trim()).unwrap_or("");
    if raw.is_empty() {
        return Ok(Json(json!({ "br_numbers": [], "dr_numbers": [] })));
    }
    // The same reading as the write-back uses, so what the sheet fills in and
    // what the register learns can never disagree about which case was meant.
    let (os_no, os_year) = match parse_os_ref(raw) {
        Some((n, y)) => (n, Some(y)),
        None => (
            raw.trim().trim_start_matches('0').to_string(),
            params.get("os_year").and_then(|y| y.trim().parse::<i64>().ok()),
        ),
    };
    if os_no.is_empty() || !os_no.chars().all(|c| c.is_ascii_digit()) {
        return Ok(Json(json!({ "br_numbers": [], "dr_numbers": [] })));
    }
    let Some(os_year) = os_year else {
        return Ok(Json(json!({ "br_numbers": [], "dr_numbers": [] })));
    };

    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let collect = |table: &str, col: &str| -> Vec<String> {
        let sql = format!(
            "SELECT DISTINCT {col} FROM {table}
              WHERE os_no = ?1 AND os_year = ?2 AND entry_deleted = 'N'
              ORDER BY {col}"
        );
        let Ok(mut st) = conn.prepare(&sql) else { return Vec::new() };
        st.query_map(rusqlite::params![os_no, os_year], |r| {
            Ok(crate::api::col_text(r, 0)?.unwrap_or_default())
        })
        .map(|rows| rows.filter_map(|r| r.ok()).filter(|s| !s.trim().is_empty()).collect())
        .unwrap_or_default()
    };

    let br = collect("br_master", "br_no");
    let dr = collect("dr_master", "dr_no");

    // The case may also carry them as free text from the adjudication screen.
    let from_case: String = conn
        .query_row(
            "SELECT COALESCE(post_adj_br_entries,'') FROM cops_master
              WHERE os_no = ?1 AND os_year = ?2 AND entry_deleted = 'N'",
            rusqlite::params![os_no, os_year],
            |r| r.get(0),
        )
        .unwrap_or_default();

    Ok(Json(json!({
        "os_no": os_no,
        "os_year": os_year,
        "br_numbers": br,
        "dr_numbers": dr,
        "post_adj_br_entries": from_case,
    })))
}

/// The duty proper, out of a line's grand total.
///
/// The "Total Duty" column is the sum of every money column, the fine and the
/// penalty among them — the sheet's own `SUM(G:V)`. Passing it in as the duty
/// made the duty test true whenever anything at all had been paid, so a
/// redemption case with no duty on it still read as settled. This takes the
/// charges back out and leaves what was actually charged as duty.
fn duty_of(e: &Value) -> f64 {
    let f = |k: &str| e.get(k).and_then(|v| v.as_f64()).unwrap_or(0.0);
    (f("total_duty") - f("redemption_fine") - f("reexport_fine")
        - f("personal_penalty") - f("other_charges")).max(0.0)
}

/// Mark each line of the sheet with the standing of the case it names.
///
/// A case is very often settled across more than one receipt on the same shift:
/// the penalty on one, the fine and the duty on another, sometimes on an SBI
/// challan rather than a baggage receipt. Judging each line on its own called
/// the second of those OPEN — the fine had been paid but no penalty appeared on
/// that line — while the case was in fact fully settled. Eleven per cent of this
/// month's linked lines read wrongly that way.
///
/// So the money is added up per case first, and every line naming that case
/// carries the same verdict. A line that names no case carries none at all: an
/// ordinary duty receipt is not an offence awaiting payment, and marking it OPEN
/// was noise on nearly every row of the sheet.
///
/// Receipts against the same case on a different day are not in hand here. A
/// case settled across two shifts still shows OPEN on the first of them, which
/// is what the officer sees on the day and is true when they see it.
fn settle_cases(entries: Vec<Value>) -> Vec<Value> {
    use std::collections::HashMap;
    let text = |e: &Value, k: &str| e.get(k).and_then(|v| v.as_str()).unwrap_or("").trim().to_string();

    // A receipt whose goods run over several lines names its case once, on the
    // first of them. The lines below carry the same receipt number, so that is
    // what ties them to the case.
    let mut by_receipt: HashMap<String, (String, i64)> = HashMap::new();
    for e in &entries {
        let (br, Some(case)) = (text(e, "br_no"), parse_os_ref(&text(e, "os_ref"))) else { continue };
        if br.is_empty() { continue; }
        by_receipt.entry(br).or_insert(case);
    }
    let case_of = |e: &Value| -> Option<(String, i64)> {
        parse_os_ref(&text(e, "os_ref"))
            .or_else(|| by_receipt.get(&text(e, "br_no")).cloned())
    };

    let mut totals: HashMap<(String, i64), (f64, f64, f64)> = HashMap::new();
    for e in &entries {
        let Some(case) = case_of(e) else { continue };
        let g = |k: &str| e.get(k).and_then(|v| v.as_f64()).unwrap_or(0.0);
        let t = totals.entry(case).or_insert((0.0, 0.0, 0.0));
        t.0 += g("personal_penalty");
        t.1 += g("redemption_fine");
        t.2 += duty_of(e);
    }

    entries.into_iter().map(|mut e| {
        let verdict = case_of(&e).and_then(|c| totals.get(&c))
            .map(|&(pp, rf, duty)| entry_status(pp, rf, duty));
        if let Some(obj) = e.as_object_mut() {
            obj.insert("status".into(), match verdict {
                Some(v) => json!(v),
                None    => Value::Null,
            });
        }
        e
    }).collect()
}

/// Whether a revenue line is settled.
///
/// Two shapes, and they settle differently:
///
///   * Absolute confiscation — the goods are gone and only a personal penalty
///     is due. The penalty alone settles it.
///   * Confiscation with redemption — the passenger may take the goods back, so
///     a redemption fine and the duty on them fall due as well as the penalty.
///     All three are needed; two out of three is a case still owing money.
///
/// A redemption fine on the line is what distinguishes the second from the
/// first: nobody is charged one unless redemption was offered.
///
/// The penalty is mandatory either way, so a line without it is open whatever
/// else was collected — which is the case this rule exists to catch.
///
/// Derived on read rather than stored, so it cannot drift away from the figures
/// it describes: correct a penalty and the status corrects itself.
pub fn entry_status(personal_penalty: f64, redemption_fine: f64, duty: f64) -> &'static str {
    if personal_penalty <= 0.0 { return "OPEN"; }
    if redemption_fine > 0.0 {
        // Redemption was offered: the duty on the goods being taken back is due.
        if duty > 0.0 { "CLOSED" } else { "OPEN" }
    } else {
        // Absolute confiscation: the penalty is the whole of it.
        "CLOSED"
    }
}

// ── Carrying the BR ↔ O.S. linkage back to the register ─────────────────────

/// The case a revenue line names, read out of whatever the officer typed.
///
/// Almost every line carries a bare `481/2026`, but the column is free text and
/// the month's reports also hold `OS No. 501/2026` and
/// `OS 527/2025 DATED 22.07.2025` — an older case settled now, with the date of
/// the original order written alongside. Splitting on the first slash finds the
/// year in one of those and nothing usable in the other, so both were passed
/// over in silence.
///
/// A short year is taken as this century — `520/26` is case 520 of 2026, which
/// is the only thing anyone typing it means. Nothing is settled on the strength
/// of that reading alone: the case still has to exist in the register, and a
/// number that matches nothing is passed over.
///
/// The trap this guards against is a date. `22/07/2025` in the case column would
/// otherwise be read as case 7 of 2025 and quietly settle somebody else's case,
/// so a number that follows a slash, dot or dash — as the middle of a date does
/// — is not a case number.
pub fn parse_os_ref(raw: &str) -> Option<(String, i64)> {
    let c: Vec<char> = raw.chars().collect();
    for (i, ch) in c.iter().enumerate() {
        if *ch != '/' { continue; }

        // the digits before the slash
        let mut e = i;
        while e > 0 && c[e - 1].is_whitespace() { e -= 1; }
        let mut b = e;
        while b > 0 && c[b - 1].is_ascii_digit() { b -= 1; }
        if b == e || e - b > 5 { continue; }
        // part of a date, or of something longer
        if b > 0 && matches!(c[b - 1], '/' | '.' | '-') { continue; }

        // and the four-digit year after it
        let mut k = i + 1;
        while k < c.len() && c[k].is_whitespace() { k += 1; }
        let ys = k;
        while k < c.len() && c[k].is_ascii_digit() { k += 1; }
        let digits = k - ys;
        if digits != 4 && digits != 2 { continue; }
        if k < c.len() && matches!(c[k], '/' | '.' | '-') && c.get(k + 1).is_some_and(|x| x.is_ascii_digit()) {
            continue;                                   // a date carries on
        }

        let mut year: i64 = c[ys..k].iter().collect::<String>().parse().ok()?;
        if digits == 2 { year += 2000; }                // "26" is this century
        if !(2000..=2100).contains(&year) { continue; }

        let no: String = c[b..e].iter().collect();
        let no = no.trim_start_matches('0');
        if no.is_empty() { continue; }
        return Some((no.to_string(), year));
    }
    None
}

/// Record, on the O.S. case, the baggage receipts named against it in a shift's
/// revenue sheet.
///
/// The sheet is filled every day and carries both numbers on the same line: the
/// receipt, and the case it belongs to. The case itself is supposed to get that
/// association through the adjudication screen, and during a shift change nobody
/// has time to go back and open it — so the register ends up not knowing which
/// receipt settled which case, while the revenue sheet has known all along.
///
/// This reads that linkage and writes it across. The receipt's date is the date
/// of the report it appeared on, which is the day it was collected.
///
/// It only ever adds. An entry already on the case is left exactly as it is,
/// including one an officer typed by hand with an amount on it — the sheet is a
/// second source for the association, not the authority on it.
fn link_receipts_to_cases(conn: &rusqlite::Connection, session_id: i64) -> usize {
    let report_date: String = conn
        .query_row("SELECT report_date FROM dcr_sessions WHERE id = ?1", [session_id], |r| r.get(0))
        .unwrap_or_default();
    if report_date.trim().is_empty() { return 0; }

    let pairs: Vec<(String, String)> = {
        let Ok(mut st) = conn.prepare(
            "SELECT br_no, os_ref FROM dcr_entries
              WHERE session_id = ?1 AND TRIM(br_no) != '' AND TRIM(os_ref) != ''"
        ) else { return 0 };
        st.query_map([session_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map(|rows| rows.filter_map(|r| r.ok()).collect())
            .unwrap_or_default()
    };

    let mut added_total = 0usize;
    let mut cases: std::collections::HashSet<(String, i64)> = Default::default();
    for (br_no, os_ref) in pairs {
        let Some((os_no, os_year)) = parse_os_ref(&os_ref) else { continue };
        let os_no = os_no.as_str();

        // One revenue cell may name several receipts.
        let numbers: Vec<String> = br_no
            .split(|c| c == ',' || c == ';' || c == '/')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if numbers.is_empty() { continue; }

        let existing: String = match conn.query_row(
            "SELECT COALESCE(post_adj_br_entries,'') FROM cops_master
              WHERE os_no = ?1 AND os_year = ?2 AND entry_deleted = 'N'",
            rusqlite::params![os_no, os_year], |r| r.get(0),
        ) {
            Ok(v) => v,
            Err(_) => continue,          // no such case; the sheet may name a typo
        };

        let mut list: Vec<Value> = serde_json::from_str(&existing).unwrap_or_default();
        let mut added = 0usize;
        for n in numbers {
            let already = list.iter().any(|e| {
                // `{"no": ..., "date": ...}` — the shape the adjudication screen
                // writes and the printed form and the query both read back.
                e.get("no").or_else(|| e.get("br_no"))
                    .and_then(|v| v.as_str()).map(str::trim) == Some(n.as_str())
            });
            if already { continue; }
            list.push(json!({ "no": n, "date": report_date }));
            added += 1;
        }
        if added == 0 { continue; }

        if let Ok(encoded) = serde_json::to_string(&list) {
            if conn.execute(
                "UPDATE cops_master SET post_adj_br_entries = ?1
                  WHERE os_no = ?2 AND os_year = ?3 AND entry_deleted = 'N'",
                rusqlite::params![encoded, os_no, os_year],
            ).is_ok() {
                added_total += added;
                cases.insert((os_no.to_string(), os_year));
            }
        }
    }
    if added_total > 0 {
        tracing::info!("revenue sheet supplied {added_total} receipt(s) to {} case(s)", cases.len());
    }
    cases.len()
}

#[cfg(test)]
mod os_ref_tests {
    use super::parse_os_ref;

    #[test]
    fn the_case_number_is_read_out_of_what_the_officer_actually_typed() {
        // Every form the column takes in this month's reports.
        assert_eq!(parse_os_ref("481/2026"),          Some(("481".into(), 2026)));
        assert_eq!(parse_os_ref("OS No. 501/2026"),   Some(("501".into(), 2026)));
        assert_eq!(parse_os_ref("OS 527/2025 DATED 22.07.2025"),
                                                      Some(("527".into(), 2025)));
        // An old case settled now is read as its own year, not the year it was paid.
        assert_eq!(parse_os_ref("OS 527/2025 DATED 22.07.2025").unwrap().1, 2025);

        // Spacing and leading zeros as they get typed.
        assert_eq!(parse_os_ref(" 520 / 2026 "),      Some(("520".into(), 2026)));
        assert_eq!(parse_os_ref("0520/2026"),         Some(("520".into(), 2026)));

        // A date in the case column must never be read as a case. This one would
        // otherwise settle case 7 of 2025 — somebody else's case entirely.
        assert_eq!(parse_os_ref("22/07/2025"), None);
        assert_eq!(parse_os_ref("PAID ON 1/8/2026"), None);

        // A year written short is this century.
        assert_eq!(parse_os_ref("520/26"), Some(("520".into(), 2026)));
        // But a date is still a date, whichever way the year is written.
        assert_eq!(parse_os_ref("22/07/25"), None);
        assert_eq!(parse_os_ref("PAID 1/8/26"), None);

        // Notes and empty cells name no case.
        assert_eq!(parse_os_ref("* 4 BRS CONTAIN MULTIPLE ITEMS"), None);
        assert_eq!(parse_os_ref("520"), None);
        assert_eq!(parse_os_ref(""), None);
    }
}
