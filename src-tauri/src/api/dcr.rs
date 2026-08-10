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
                s.challan_no
         FROM dcr_sessions s
         LEFT JOIN dcr_tariffs t ON t.id = s.tariff_id
         WHERE s.id = ?",
        rusqlite::params![id],
        |r| {
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
                Value::Null
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
                s.challan_no
         FROM dcr_sessions s
         LEFT JOIN dcr_tariffs t ON t.id = s.tariff_id
         {where_sql}
         ORDER BY s.report_date DESC, s.shift ASC"
    );

    let mut stmt = conn.prepare(&sql).map_err(|e| e500(&e.to_string()))?;

    let rows: Vec<Value> = stmt.query_map(
        rusqlite::params_from_iter(where_params.iter()),
        |r| {
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
                Value::Null
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
                "tariff":       tariff,
            }))
        },
    ).map_err(|e| e500(&e.to_string()))?.filter_map(|r| r.ok()).collect();

    Ok(Json(json!({ "items": rows })))
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
        "INSERT INTO dcr_sessions (report_date, shift, batch_name, tariff_id, created_by)
         VALUES (?, ?, ?, ?, ?)",
        rusqlite::params![report_date, shift, batch_name, tariff_id, created_by],
    ).map_err(|e| e500(&e.to_string()))?;

    let new_id = conn.last_insert_rowid();

    let session = load_session_row(&conn, new_id)?
        .ok_or_else(|| e500("Failed to load created session"))?;

    Ok(Json(session))
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

    let session = load_full_session(&conn, id)?
        .ok_or_else(|| e500("Failed to reload session after save"))?;

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
