use std::sync::Arc;
use axum::{extract::{Path, Query, State}, http::StatusCode, Json};
use rusqlite::OptionalExtension;
use serde_json::{json, Value};
use crate::{auth::{AuthUser, AdjnUser, SdoUser}, db::DbPool, models::offence::*};

type Db = State<Arc<DbPool>>;
type Err = (StatusCode, Json<Value>);

fn err(s: StatusCode, m: &str) -> Err { (s, Json(json!({ "detail": m }))) }
fn e400(m: &str) -> Err { err(StatusCode::BAD_REQUEST, m) }
fn e404(m: &str) -> Err { err(StatusCode::NOT_FOUND, m) }
fn e500(m: &str) -> Err { err(StatusCode::INTERNAL_SERVER_ERROR, m) }

// ── Helpers ───────────────────────────────────────────────────────────────────

fn load_items(conn: &rusqlite::Connection, os_no: &str, os_year: i64) -> rusqlite::Result<Vec<OsItem>> {
    let mut stmt = conn.prepare(
        "SELECT id, os_no, os_year, items_sno, items_desc, items_qty, items_uqc,
                value_per_piece, items_value, items_fa, items_fa_type, items_fa_qty, items_fa_uqc,
                cumulative_duty_rate, items_duty, items_duty_type, items_category,
                items_release_category, items_sub_category, items_dr_no, items_dr_year, entry_deleted
         FROM cops_items WHERE os_no = ? AND os_year = ? AND entry_deleted = 'N'
         ORDER BY items_sno"
    )?;
    let items: Vec<OsItem> = stmt.query_map(rusqlite::params![os_no, os_year], |r| {
        Ok(OsItem {
            id: r.get(0)?, os_no: r.get(1)?, os_year: r.get(2)?,
            items_sno: r.get(3)?, items_desc: r.get(4)?, items_qty: r.get(5)?,
            items_uqc: r.get(6)?, value_per_piece: r.get(7)?, items_value: r.get(8)?,
            items_fa: r.get(9)?, items_fa_type: r.get(10)?, items_fa_qty: r.get(11)?,
            items_fa_uqc: r.get(12)?, cumulative_duty_rate: r.get(13)?, items_duty: r.get(14)?,
            items_duty_type: r.get(15)?, items_category: r.get(16)?,
            items_release_category: r.get(17)?, items_sub_category: r.get(18)?,
            items_dr_no: r.get(19)?, items_dr_year: r.get(20)?, entry_deleted: r.get(21)?,
        })
    })?.filter_map(|r| r.ok()).collect();
    Ok(items)
}

// ── Business rule validators (mirrors cops1 rules_engine.py) ─────────────────

/// Normalise passport number: trim, uppercase.
/// "DOMESTIC" and "UNCLAIMED" are returned as-is (all-caps sentinel values).
fn normalize_passport(p: Option<String>) -> Option<String> {
    p.map(|s| {
        let s = s.trim().to_uppercase();
        s  // DOMESTIC / UNCLAIMED / real passport — all just upper-cased
    }).filter(|s| !s.is_empty())
}

/// Flight date must not be in the future.
fn validate_flight_date(flight_date: Option<&str>) -> Result<(), Err> {
    if let Some(fd) = flight_date {
        let today = chrono::Local::now().date_naive();
        if let Ok(d) = chrono::NaiveDate::parse_from_str(fd, "%Y-%m-%d") {
            if d > today {
                return Err(e400("Check Flight Date! Cannot be in the future."));
            }
        }
    }
    Ok(())
}

/// Date of birth and date of departure must not be in the future.
fn validate_pax_dates(dob: Option<&str>, departure: Option<&str>) -> Result<(), Err> {
    let today = chrono::Local::now().date_naive();
    if let Some(d) = dob {
        if let Ok(parsed) = chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d") {
            if parsed > today {
                return Err(e400("Date of Birth Should Not be Greater Than Current Batch Date..."));
            }
        }
    }
    if let Some(d) = departure {
        if let Ok(parsed) = chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d") {
            if parsed > today {
                return Err(e400("Date of Departure Should Not be Greater Than Current Batch Date..."));
            }
        }
    }
    Ok(())
}

/// Adjudicating officer remarks must not exceed 3000 chars (legacy VB6 MaxLength).
fn validate_remarks_length(remarks: Option<&str>) -> Result<(), Err> {
    if let Some(r) = remarks {
        if r.len() > crate::config::ADJN_REMARKS_MAX_CHARS {
            return Err(e400(&format!(
                "The Remarks of the Adjudicating Officer exceeds {} Characters. \
                 Please Use the Option of 'Print Adjn. Order On Legal Size Blank Paper'...",
                crate::config::ADJN_REMARKS_MAX_CHARS
            )));
        }
    }
    Ok(())
}

/// Superintendent remarks must not exceed 1500 chars (legacy VB6 MaxLength).
fn validate_supdt_remarks_length(remarks: Option<&str>) -> Result<(), Err> {
    if let Some(r) = remarks {
        if r.len() > crate::config::SUPDT_REMARKS_MAX_CHARS {
            return Err(e400(&format!(
                "Superintendent's Remarks exceed {} characters.",
                crate::config::SUPDT_REMARKS_MAX_CHARS
            )));
        }
    }
    Ok(())
}

fn within_edit_window(adjudication_time: &Option<String>) -> bool {
    match adjudication_time {
        None => true,
        Some(ts) => {
            if let Ok(t) = chrono::NaiveDateTime::parse_from_str(ts, "%Y-%m-%d %H:%M:%S") {
                let now = chrono::Local::now().naive_local();
                (now - t).num_hours() < 24
            } else { true }
        }
    }
}

// ── Endpoints ─────────────────────────────────────────────────────────────────

pub async fn sidebar_counts(State(pool): Db, _auth: AdjnUser) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;

    // Mirrors cops1 _pending_filters() exactly:
    //   is_draft='N', adjudication_date IS NULL, adj_offr_name IS NULL,
    //   adjn_offr_remarks IS NULL or '', quashed!='Y', rejected!='Y',
    //   not offline, not legacy
    // PLUS: must have at least one item in 'Under OS' or 'Under Duty' release category.
    let pending: i64 = conn.query_row(
        "SELECT COUNT(*) FROM cops_master
         WHERE entry_deleted='N' AND is_draft='N'
           AND adjudication_date IS NULL AND adj_offr_name IS NULL
           AND (adjn_offr_remarks IS NULL OR adjn_offr_remarks = '')
           AND (quashed IS NULL OR quashed != 'Y')
           AND (rejected IS NULL OR rejected != 'Y')
           AND (is_offline_adjudication IS NULL OR is_offline_adjudication != 'Y')
           AND (is_legacy IS NULL OR is_legacy != 'Y')
           AND EXISTS (
               SELECT 1 FROM cops_items ci
               WHERE ci.os_no = cops_master.os_no
                 AND ci.os_year = cops_master.os_year
                 AND ci.items_release_category IN ('Under OS', 'Under Duty')
                 AND (ci.entry_deleted IS NULL OR ci.entry_deleted != 'Y')
           )",
        [], |r| r.get(0)
    ).unwrap_or(0);

    let offline_pending: i64 = conn.query_row(
        "SELECT COUNT(*) FROM cops_master WHERE entry_deleted='N' AND is_draft='N'
         AND is_offline_adjudication='Y' AND adj_offr_name IS NULL",
        [], |r| r.get(0)
    ).unwrap_or(0);

    Ok(Json(json!({ "pending": pending, "offline_pending": offline_pending })))
}

// 5-minute in-process cache for item descriptions — avoids a DB round-trip on
// every keystroke in the item form.  Uses double-checked locking: fast path
// reads the atomic timestamp without acquiring the mutex; slow path re-checks
// under lock to prevent a thundering herd when the cache expires.
static ITEM_DESC_CACHE: std::sync::OnceLock<std::sync::Mutex<(Vec<String>, std::time::Instant)>> = std::sync::OnceLock::new();
const ITEM_DESC_TTL_SECS: u64 = 300;

pub async fn item_descriptions(State(pool): Db) -> Result<Json<Value>, Err> {
    let cache = ITEM_DESC_CACHE.get_or_init(|| {
        std::sync::Mutex::new((Vec::new(), std::time::Instant::now() - std::time::Duration::from_secs(ITEM_DESC_TTL_SECS + 1)))
    });

    // Fast path: if cache is fresh, clone and return without hitting DB
    {
        let guard = cache.lock().unwrap();
        if !guard.0.is_empty() && guard.1.elapsed().as_secs() < ITEM_DESC_TTL_SECS {
            return Ok(Json(json!(guard.0.clone())));
        }
    }

    // Slow path: refresh from DB, then store under lock
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let mut stmt = conn.prepare(
        "SELECT UPPER(items_desc) FROM cops_items
         WHERE items_desc IS NOT NULL AND items_desc != '' AND entry_deleted='N'
         GROUP BY UPPER(items_desc) ORDER BY COUNT(*) DESC LIMIT 300"
    ).map_err(|e| e500(&e.to_string()))?;
    let descs: Vec<String> = stmt.query_map([], |r| r.get(0))
        .map_err(|e| e500(&e.to_string()))?
        .filter_map(|r| r.ok()).collect();

    {
        let mut guard = cache.lock().unwrap();
        *guard = (descs.clone(), std::time::Instant::now());
    }
    Ok(Json(json!(descs)))
}

pub async fn list_os(State(pool): Db, auth: AuthUser, Query(params): Query<OsListParams>) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;

    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(20).clamp(1, 100);
    let offset = (page - 1) * per_page;
    let status = params.status.as_deref().unwrap_or("pending");

    let search_filter = params.search.as_deref().unwrap_or("").trim().to_string();
    let year_filter = params.year;

    let br_dr_pending = params.br_dr_pending.unwrap_or(false);
    let (count_sql, list_sql, search_params) = build_list_query(status, &search_filter, year_filter, br_dr_pending, offset, per_page, auth.0.is_sdo());

    let total: i64 = conn.query_row(&count_sql, rusqlite::params_from_iter(search_params.iter()), |r| r.get(0)).unwrap_or(0);

    let mut stmt = conn.prepare(&list_sql).map_err(|e| e500(&e.to_string()))?;
    let cases: Vec<Value> = stmt.query_map(rusqlite::params_from_iter(search_params.iter()), |r| {
        Ok(json!({
            "id": r.get::<_, i64>(0)?,
            "os_no": r.get::<_, String>(1)?,
            "os_date": r.get::<_, Option<String>>(2)?,
            "os_year": r.get::<_, Option<i64>>(3)?,
            "pax_name": r.get::<_, Option<String>>(4)?,
            "passport_no": r.get::<_, Option<String>>(5)?,
            "flight_no": r.get::<_, Option<String>>(6)?,
            "total_items_value": r.get::<_, Option<f64>>(7)?,
            "total_payable": r.get::<_, Option<f64>>(8)?,
            "adjudication_date": r.get::<_, Option<String>>(9)?,
            "adj_offr_name": r.get::<_, Option<String>>(10)?,
            "is_draft": r.get::<_, Option<String>>(11)?,
            "is_offline_adjudication": r.get::<_, Option<String>>(12)?,
            "entry_deleted": r.get::<_, Option<String>>(13)?,
            "online_adjn": r.get::<_, Option<String>>(14)?,
            "closure_ind": r.get::<_, Option<String>>(15)?,
            "adjudication_time": r.get::<_, Option<String>>(16)?,
            "post_adj_br_entries": r.get::<_, Option<String>>(17)?,
            "post_adj_dr_no": r.get::<_, Option<String>>(18)?,
            "booked_by": r.get::<_, Option<String>>(19)?,
            "location_code": r.get::<_, Option<String>>(20)?,
            "total_items": r.get::<_, Option<i64>>(21)?,
        }))
    }).map_err(|e| e500(&e.to_string()))?.filter_map(|r| r.ok()).collect();

    Ok(Json(json!({ "total": total, "page": page, "per_page": per_page, "items": cases })))
}

fn build_list_query(status: &str, search: &str, year: Option<i64>, br_dr_pending: bool, offset: i64, per_page: i64, _is_sdo: bool) -> (String, String, Vec<String>) {
    let (search_sql, search_params): (&str, Vec<String>) = if search.is_empty() {
        ("1=1", vec![])
    } else {
        let p = format!("%{}%", search);
        // Include old_passport_no so renewing a passport doesn't hide prior cases
        ("(os_no LIKE ? OR pax_name LIKE ? OR passport_no LIKE ? OR old_passport_no LIKE ? OR flight_no LIKE ?)",
         vec![p.clone(), p.clone(), p.clone(), p.clone(), p])
    };
    // year is an i64 — safe to format directly as a numeric literal
    let year_sql = year.map_or("1=1".to_string(), |y| format!("os_year = {y}"));

    let base = match status {
        "draft"       => "entry_deleted='N' AND is_draft='Y'".to_string(),
        // Adjudicated = EITHER adjudication_date OR adj_offr_name is set (mirrors cops1 OR logic).
        // Old MDB-imported records may have adj_offr_name without a date — using AND would make them
        // invisible in both pending and adjudicated views.
        "adjudicated" => "entry_deleted='N' AND is_draft='N' AND (adjudication_date IS NOT NULL OR adj_offr_name IS NOT NULL) AND (quashed IS NULL OR quashed!='Y') AND (rejected IS NULL OR rejected!='Y')".to_string(),
        "offline-pending" => "entry_deleted='N' AND is_draft='N' AND is_offline_adjudication='Y' AND adj_offr_name IS NULL".to_string(),
        _             => "entry_deleted='N' AND is_draft='N' AND adjudication_date IS NULL AND adj_offr_name IS NULL AND (adjn_offr_remarks IS NULL OR adjn_offr_remarks='') AND (quashed IS NULL OR quashed!='Y') AND (rejected IS NULL OR rejected!='Y') AND (is_offline_adjudication IS NULL OR is_offline_adjudication!='Y') AND (is_legacy IS NULL OR is_legacy!='Y') AND EXISTS (SELECT 1 FROM cops_items ci WHERE ci.os_no=cops_master.os_no AND ci.os_year=cops_master.os_year AND ci.items_release_category IN ('Under OS','Under Duty') AND (ci.entry_deleted IS NULL OR ci.entry_deleted!='Y'))".to_string(),
    };

    // br_dr_pending: adjudicated cases where SDO has not yet recorded BR/DR receipt data
    let br_dr_sql = if br_dr_pending {
        " AND adjudication_date IS NOT NULL AND post_adj_br_entries IS NULL AND post_adj_dr_no IS NULL"
    } else {
        ""
    };

    let where_clause = format!("{base}{br_dr_sql} AND {search_sql} AND {year_sql}");
    let cols = "id, os_no, os_date, os_year, pax_name, passport_no, flight_no, total_items_value, total_payable, adjudication_date, adj_offr_name, is_draft, is_offline_adjudication, entry_deleted, online_adjn, closure_ind, adjudication_time, post_adj_br_entries, post_adj_dr_no, booked_by, location_code, total_items";
    let count_sql = format!("SELECT COUNT(*) FROM cops_master WHERE {where_clause}");
    // CAST os_no to INTEGER for correct numeric order (os_no is VARCHAR; "9" > "10" lexicographically)
    let list_sql  = format!("SELECT {cols} FROM cops_master WHERE {where_clause} ORDER BY os_date DESC, CAST(os_no AS INTEGER) DESC LIMIT {per_page} OFFSET {offset}");
    (count_sql, list_sql, search_params)
}

pub async fn get_os(State(pool): Db, auth: AuthUser, Path((os_no, os_year)): Path<(String, i64)>) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;

    let case: Option<Value> = conn.query_row(
        "SELECT * FROM cops_master WHERE os_no=? AND os_year=? AND entry_deleted='N'",
        rusqlite::params![os_no, os_year],
        |r| {
            // Build a JSON object from all columns dynamically
            let col_count = r.as_ref().column_count();
            let mut map = serde_json::Map::new();
            for i in 0..col_count {
                let name = r.as_ref().column_name(i).unwrap_or("?").to_string();
                let val: Value = match r.get_ref(i)? {
                    rusqlite::types::ValueRef::Null => Value::Null,
                    rusqlite::types::ValueRef::Integer(n) => json!(n),
                    rusqlite::types::ValueRef::Real(f) => json!(f),
                    rusqlite::types::ValueRef::Text(s) => json!(String::from_utf8_lossy(s)),
                    rusqlite::types::ValueRef::Blob(b) => json!(String::from_utf8_lossy(b)),
                };
                map.insert(name, val);
            }
            Ok(Value::Object(map))
        }
    ).optional().map_err(|e| e500(&e.to_string()))?;

    let mut case = case.ok_or_else(|| e404("O.S. not found"))?;

    let items = load_items(&conn, &os_no, os_year).map_err(|e| e500(&e.to_string()))?;
    case["items"] = json!(items);

    Ok(Json(case))
}

pub async fn create_os(State(pool): Db, auth: AuthUser, Json(mut req): Json<CreateOsRequest>) -> Result<Json<Value>, Err> {
    // ── Business rule validations ─────────────────────────────────────────────
    req.passport_no = normalize_passport(req.passport_no);
    validate_flight_date(req.flight_date.as_deref())?;
    validate_pax_dates(req.pax_date_of_birth.as_deref(), req.date_of_departure.as_deref())?;
    validate_supdt_remarks_length(req.supdts_remarks.as_deref())?;

    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;

    // Uniqueness check
    let exists: i64 = conn.query_row(
        "SELECT COUNT(*) FROM cops_master WHERE os_no=? AND os_year=? AND entry_deleted='N'",
        rusqlite::params![req.os_no, req.os_year.unwrap_or(chrono::Local::now().year() as i64)],
        |r| r.get(0)
    ).unwrap_or(0);
    if exists > 0 { return Err(e400("O.S. No. already exists for this year.")); }

    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let os_date = req.os_date.clone().unwrap_or_else(|| today.clone());
    let os_year = req.os_year.unwrap_or_else(|| chrono::Local::now().year() as i64);
    let is_draft = req.is_draft.as_deref().unwrap_or("N");

    // Wrap all writes in a single transaction — reduces N+2 individual fsyncs
    // (1 per item + master INSERT + recalc UPDATE) down to 1.
    conn.execute_batch("BEGIN").map_err(|e| e500(&e.to_string()))?;
    let write_result: Result<(), Err> = (|| {
        conn.execute(
            "INSERT INTO cops_master (os_no, os_date, os_year, location_code, shift, booked_by,
             pax_name, pax_nationality, passport_no, passport_date, pp_issue_place,
             pax_address1, pax_address2, pax_address3, pax_date_of_birth, pax_status,
             residence_at, country_of_departure, port_of_dep_dest, date_of_departure,
             stay_abroad_days, flight_no, flight_date, detained_by, seal_no, seizure_date,
             father_name, old_passport_no, total_pkgs, supdts_remarks, supdt_remarks2,
             previous_os_details, previous_visits, case_type, file_spot,
             is_offline_adjudication, is_draft, entry_deleted, bkup_taken,
             total_items, total_items_value, total_fa_value, total_duty_amount, total_payable)
             VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
            rusqlite::params![
                req.os_no, os_date, os_year, req.location_code, req.shift, req.booked_by,
                req.pax_name, req.pax_nationality, req.passport_no, req.passport_date, req.pp_issue_place,
                req.pax_address1, req.pax_address2, req.pax_address3, req.pax_date_of_birth, req.pax_status,
                req.residence_at, req.country_of_departure, req.port_of_dep_dest, req.date_of_departure,
                req.stay_abroad_days, req.flight_no, req.flight_date, req.detained_by, req.seal_no, req.seizure_date,
                req.father_name, req.old_passport_no, req.total_pkgs, req.supdts_remarks, req.supdt_remarks2,
                req.previous_os_details, req.previous_visits, req.case_type, req.file_spot,
                req.is_offline_adjudication.as_deref().unwrap_or("N"), is_draft, "N", "N",
                req.items.len() as i64, 0.0f64, 0.0f64, 0.0f64, 0.0f64,
            ],
        ).map_err(|e| e500(&e.to_string()))?;
        save_items(&conn, &req.os_no, os_year, &os_date, req.location_code.as_deref(), &req.items)?;
        recalc_totals(&conn, &req.os_no, os_year).map_err(|e| e500(&e.to_string()))?;
        Ok(())
    })();
    match write_result {
        Ok(()) => conn.execute_batch("COMMIT").map_err(|e| e500(&e.to_string()))?,
        Err(e) => { let _ = conn.execute_batch("ROLLBACK"); return Err(e); }
    }

    let case_json = get_case_json(&conn, &req.os_no, os_year).map_err(|e| e500(&e.to_string()))?;
    Ok(Json(case_json))
}

pub async fn create_offline(State(pool): Db, _auth: SdoUser, Json(mut req): Json<CreateOsRequest>) -> Result<Json<Value>, Err> {
    // Same as create_os but forces is_offline_adjudication='Y'
    req.passport_no = normalize_passport(req.passport_no);
    validate_flight_date(req.flight_date.as_deref())?;
    validate_pax_dates(req.pax_date_of_birth.as_deref(), req.date_of_departure.as_deref())?;
    validate_supdt_remarks_length(req.supdts_remarks.as_deref())?;

    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let os_date = req.os_date.clone().unwrap_or_else(|| today.clone());
    let os_year = req.os_year.unwrap_or_else(|| chrono::Local::now().year() as i64);
    let is_draft = req.is_draft.as_deref().unwrap_or("N");

    let exists: i64 = conn.query_row(
        "SELECT COUNT(*) FROM cops_master WHERE os_no=? AND os_year=? AND entry_deleted='N'",
        rusqlite::params![req.os_no, os_year], |r| r.get(0)
    ).unwrap_or(0);
    if exists > 0 { return Err(e400("O.S. No. already exists for this year.")); }

    conn.execute_batch("BEGIN").map_err(|e| e500(&e.to_string()))?;
    let write_result: Result<(), Err> = (|| {
        conn.execute(
            "INSERT INTO cops_master (os_no, os_date, os_year, location_code, shift, booked_by,
             pax_name, pax_nationality, passport_no, passport_date, pp_issue_place,
             pax_address1, pax_address2, pax_address3, pax_date_of_birth, pax_status,
             residence_at, country_of_departure, port_of_dep_dest, date_of_departure,
             stay_abroad_days, flight_no, flight_date, detained_by, seal_no, seizure_date,
             father_name, old_passport_no, total_pkgs, supdts_remarks, supdt_remarks2,
             previous_os_details, previous_visits, case_type, file_spot,
             is_offline_adjudication, is_draft, entry_deleted, bkup_taken,
             total_items, total_items_value, total_fa_value, total_duty_amount, total_payable)
             VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
            rusqlite::params![
                req.os_no, os_date, os_year, req.location_code, req.shift, req.booked_by,
                req.pax_name, req.pax_nationality, req.passport_no, req.passport_date, req.pp_issue_place,
                req.pax_address1, req.pax_address2, req.pax_address3, req.pax_date_of_birth, req.pax_status,
                req.residence_at, req.country_of_departure, req.port_of_dep_dest, req.date_of_departure,
                req.stay_abroad_days, req.flight_no, req.flight_date, req.detained_by, req.seal_no, req.seizure_date,
                req.father_name, req.old_passport_no, req.total_pkgs, req.supdts_remarks, req.supdt_remarks2,
                req.previous_os_details, req.previous_visits, req.case_type, req.file_spot,
                "Y", is_draft, "N", "N",
                req.items.len() as i64, 0.0f64, 0.0f64, 0.0f64, 0.0f64,
            ],
        ).map_err(|e| e500(&e.to_string()))?;
        save_items(&conn, &req.os_no, os_year, &os_date, req.location_code.as_deref(), &req.items)?;
        recalc_totals(&conn, &req.os_no, os_year).map_err(|e| e500(&e.to_string()))?;
        Ok(())
    })();
    match write_result {
        Ok(()) => conn.execute_batch("COMMIT").map_err(|e| e500(&e.to_string()))?,
        Err(e) => { let _ = conn.execute_batch("ROLLBACK"); return Err(e); }
    }

    let case_json = get_case_json(&conn, &req.os_no, os_year).map_err(|e| e500(&e.to_string()))?;
    Ok(Json(case_json))
}

/// Bulk-import offline adjudication cases from a parsed Excel upload.
///
/// Matches the ZIP-restore contract: rows whose (os_no, os_year) already exist
/// in the DB are silently *skipped* — not treated as errors.  Rows that fail
/// for other reasons (bad data, DB error) are collected in `failed[]`.
///
/// Returns: `{ imported, skipped, failed: [{os_no, error}] }`
pub async fn bulk_import_offline(
    State(pool): Db,
    _auth: SdoUser,
    Json(rows): Json<Vec<Value>>,
) -> Result<Json<Value>, Err> {
    if rows.is_empty() { return Err(e400("No rows to import.")); }
    if rows.len() > 500 { return Err(e400("Maximum 500 rows per import batch.")); }

    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;

    // Pre-load every existing (os_no, os_year) so we can skip duplicates in O(1)
    let mut existing: std::collections::HashSet<(String, i64)> = std::collections::HashSet::new();
    {
        let mut stmt = conn.prepare(
            "SELECT os_no, os_year FROM cops_master WHERE entry_deleted='N'"
        ).map_err(|e| e500(&e.to_string()))?;
        let pairs: Vec<(String, i64)> = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })
        .map(|mapped| mapped.filter_map(|r| r.ok()).collect())
        .unwrap_or_default();
        for k in pairs { existing.insert(k); }
    }

    let mut imported = 0i64;
    let mut skipped  = 0i64;
    let mut failed: Vec<Value> = vec![];

    for row in &rows {
        let os_no = row.get("os_no").and_then(|v| v.as_str())
            .unwrap_or("").trim().to_string();
        if os_no.is_empty() {
            failed.push(json!({"os_no": "", "error": "OS No. is required."}));
            continue;
        }

        let os_date = row.get("os_date").and_then(|v| v.as_str())
            .unwrap_or("").trim().to_string();
        let os_year: i64 = row.get("os_year").and_then(|v| v.as_i64())
            .unwrap_or_else(|| {
                os_date.split('-').next()
                    .and_then(|y| y.parse().ok())
                    .unwrap_or(chrono::Local::now().year() as i64)
            });

        // Skip duplicates (same as ZIP restore)
        if existing.contains(&(os_no.clone(), os_year)) {
            skipped += 1;
            continue;
        }

        let s = |key: &str| -> String {
            row.get(key).and_then(|v| v.as_str()).unwrap_or("").trim().to_string()
        };
        let f = |key: &str| -> f64 {
            row.get(key).and_then(|v| v.as_f64()).unwrap_or(0.0)
        };

        let booked_by           = s("booked_by");
        let flight_no           = s("flight_no");
        let pax_name            = s("pax_name");
        let pax_nationality     = s("pax_nationality");
        let passport_no         = normalize_passport(Some(s("passport_no")));
        let pax_address1        = s("pax_address1");
        let file_spot           = if s("file_spot").is_empty() { "Spot".to_string() } else { s("file_spot") };
        let case_type           = row.get("case_type").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(|s| s.to_string());
        let supdts_remarks      = row.get("supdts_remarks").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(|s| s.to_string());
        let adj_offr_name       = row.get("adj_offr_name").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(|s| s.to_string());
        let adj_offr_desig      = row.get("adj_offr_designation").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(|s| s.to_string());
        let adjudication_date   = row.get("adjudication_date").and_then(|v| v.as_str()).filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .or_else(|| if adj_offr_name.is_some() { Some(os_date.clone()) } else { None });
        let adjn_remarks        = row.get("adjn_offr_remarks").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(|s| s.to_string());
        let rf_amount           = f("rf_amount");
        let ref_amount          = f("ref_amount");
        let pp_amount           = f("pp_amount");
        let confiscated_value   = f("confiscated_value");
        let total_duty_amount   = f("total_duty_amount");
        let total_payable       = f("total_payable");
        let br_entries_json     = row.get("post_adj_br_entries").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(|s| s.to_string());

        let items_arr = row.get("items").and_then(|v| v.as_array()).cloned().unwrap_or_default();
        let n_items   = items_arr.len() as i64;
        // total_items_value from Excel; will be updated from actual items below
        let total_items_value   = f("total_items_value");

        conn.execute_batch("BEGIN").map_err(|e| e500(&e.to_string()))?;
        let result: Result<(), String> = (|| {
            conn.execute(
                "INSERT INTO cops_master (
                     os_no, os_date, os_year, booked_by, pax_name, pax_nationality,
                     passport_no, pax_address1, flight_no, file_spot, case_type, supdts_remarks,
                     is_offline_adjudication, is_draft, entry_deleted, bkup_taken,
                     adj_offr_name, adj_offr_designation, adjudication_date, adjn_offr_remarks,
                     rf_amount, ref_amount, pp_amount, confiscated_value,
                     post_adj_br_entries,
                     total_items, total_items_value, total_fa_value,
                     total_duty_amount, total_payable)
                 VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
                rusqlite::params![
                    os_no, os_date, os_year, booked_by, pax_name, pax_nationality,
                    passport_no, pax_address1, flight_no, file_spot, case_type, supdts_remarks,
                    "Y", "N", "N", "N",
                    adj_offr_name, adj_offr_desig, adjudication_date, adjn_remarks,
                    rf_amount, ref_amount, pp_amount, confiscated_value,
                    br_entries_json,
                    n_items, total_items_value, 0.0f64,
                    total_duty_amount, total_payable,
                ],
            ).map_err(|e| e.to_string())?;

            // Insert items
            for (i, item) in items_arr.iter().enumerate() {
                let desc      = item.get("items_desc").and_then(|v| v.as_str()).unwrap_or("").trim();
                let qty       = item.get("items_qty").and_then(|v| v.as_f64()).unwrap_or(1.0);
                let uqc       = item.get("items_uqc").and_then(|v| v.as_str()).unwrap_or("NOS");
                let val       = item.get("items_value").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let duty_type = item.get("items_duty_type").and_then(|v| v.as_str()).unwrap_or("Miscellaneous-22");
                conn.execute(
                    "INSERT INTO cops_items (os_no, os_year, os_date, location_code,
                     items_sno, items_desc, items_qty, items_uqc, items_value,
                     items_duty_type, entry_deleted)
                     VALUES (?,?,?,?,?,?,?,?,?,?,?)",
                    rusqlite::params![
                        os_no, os_year, os_date, "",
                        (i + 1) as i64, desc, qty, uqc, val, duty_type, "N",
                    ],
                ).map_err(|e| e.to_string())?;
            }

            // Reconcile total_items_value from actual items (Excel value used as fallback)
            conn.execute(
                "UPDATE cops_master SET
                     total_items = (SELECT COUNT(*) FROM cops_items
                                    WHERE os_no=? AND os_year=? AND entry_deleted='N'),
                     total_items_value = CASE
                         WHEN (SELECT COALESCE(SUM(items_value),0) FROM cops_items
                               WHERE os_no=? AND os_year=? AND entry_deleted='N') > 0
                         THEN (SELECT COALESCE(SUM(items_value),0) FROM cops_items
                               WHERE os_no=? AND os_year=? AND entry_deleted='N')
                         ELSE total_items_value
                     END
                 WHERE os_no=? AND os_year=?",
                rusqlite::params![os_no, os_year, os_no, os_year, os_no, os_year, os_no, os_year],
            ).map_err(|e| e.to_string())?;

            Ok(())
        })();

        match result {
            Ok(()) => {
                conn.execute_batch("COMMIT").map_err(|e| e500(&e.to_string()))?;
                // Add to local set so subsequent rows in the same batch can't duplicate it
                existing.insert((os_no, os_year));
                imported += 1;
            }
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK");
                failed.push(json!({ "os_no": os_no, "error": e }));
            }
        }
    }

    Ok(Json(json!({
        "imported": imported,
        "skipped":  skipped,
        "failed":   failed,
        "total":    rows.len(),
    })))
}

pub async fn update_os(State(pool): Db, auth: AuthUser, Path((os_no, os_year)): Path<(String, i64)>, Json(mut req): Json<CreateOsRequest>) -> Result<Json<Value>, Err> {
    req.passport_no = normalize_passport(req.passport_no);
    validate_flight_date(req.flight_date.as_deref())?;
    validate_pax_dates(req.pax_date_of_birth.as_deref(), req.date_of_departure.as_deref())?;
    validate_supdt_remarks_length(req.supdts_remarks.as_deref())?;

    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;

    let (adj_date, adj_time, is_draft_db, os_printed): (Option<String>, Option<String>, Option<String>, Option<String>) = conn.query_row(
        "SELECT adjudication_date, adjudication_time, is_draft, os_printed FROM cops_master WHERE os_no=? AND os_year=? AND entry_deleted='N'",
        rusqlite::params![os_no, os_year], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
    ).map_err(|_| e404("O.S. not found"))?;

    // Role/window checks (mirrors cops1 update_os logic)
    if auth.0.is_adjn() {
        // DC/AC: can only edit adjudicated cases within 24h window
        if adj_date.is_some() && !within_edit_window(&adj_time) {
            return Err(e400("Modification window has expired (24h from adjudication)."));
        }
    } else if auth.0.is_sdo() {
        // SDO: can always edit pending cases; adjudicated = 24h window;
        // non-adjudicated but already printed = blocked
        if adj_date.is_some() {
            if !within_edit_window(&adj_time) {
                return Err(e400("Modification window has expired (24h from adjudication)."));
            }
        } else if os_printed.as_deref() == Some("Y") {
            return Err(e400("Print Out Has Already Been Taken for The Entered O.S.No. Cannot Modify its details!"));
        }
    } else {
        return Err((StatusCode::FORBIDDEN, Json(json!({ "detail": "Permission denied." }))));
    }

    let is_draft = req.is_draft.as_deref().unwrap_or(is_draft_db.as_deref().unwrap_or("N"));

    // Wrap all writes in a single transaction — saves N+3 individual fsyncs
    // (optional adjudication reset + master UPDATE + item DELETE + N item INSERTs + recalc).
    conn.execute_batch("BEGIN").map_err(|e| e500(&e.to_string()))?;
    let write_result: Result<(), Err> = (|| {
        // If adjudicated, reset adjudication fields before rewriting
        if adj_date.is_some() {
            conn.execute(
                "UPDATE cops_master SET online_adjn='N', adjudication_date=NULL, adjudication_time=NULL,
                 adj_offr_name=NULL, adj_offr_designation=NULL, adjn_offr_remarks=NULL,
                 adjn_section_ref=NULL, os_printed='N', confiscated_value=0, redeemed_value=0,
                 re_export_value=0, rf_amount=0, pp_amount=0, ref_amount=0, total_payable=0,
                 closure_ind=NULL WHERE os_no=? AND os_year=?",
                rusqlite::params![os_no, os_year],
            ).map_err(|e| e500(&e.to_string()))?;
        }

        conn.execute(
            "UPDATE cops_master SET pax_name=?, pax_nationality=?, passport_no=?, passport_date=?,
             pp_issue_place=?, pax_address1=?, pax_address2=?, pax_address3=?, pax_date_of_birth=?,
             pax_status=?, residence_at=?, country_of_departure=?, port_of_dep_dest=?,
             date_of_departure=?, stay_abroad_days=?, flight_no=?, flight_date=?,
             detained_by=?, seal_no=?, seizure_date=?, father_name=?, old_passport_no=?,
             total_pkgs=?, supdts_remarks=?, supdt_remarks2=?, previous_os_details=?,
             previous_visits=?, case_type=?, file_spot=?, booked_by=?, shift=?, is_draft=?
             WHERE os_no=? AND os_year=? AND entry_deleted='N'",
            rusqlite::params![
                req.pax_name, req.pax_nationality, req.passport_no, req.passport_date,
                req.pp_issue_place, req.pax_address1, req.pax_address2, req.pax_address3, req.pax_date_of_birth,
                req.pax_status, req.residence_at, req.country_of_departure, req.port_of_dep_dest,
                req.date_of_departure, req.stay_abroad_days, req.flight_no, req.flight_date,
                req.detained_by, req.seal_no, req.seizure_date, req.father_name, req.old_passport_no,
                req.total_pkgs, req.supdts_remarks, req.supdt_remarks2, req.previous_os_details,
                req.previous_visits, req.case_type, req.file_spot, req.booked_by, req.shift, is_draft,
                os_no, os_year,
            ],
        ).map_err(|e| e500(&e.to_string()))?;

        // Replace items
        let os_date: String = conn.query_row(
            "SELECT os_date FROM cops_master WHERE os_no=? AND os_year=?",
            rusqlite::params![os_no, os_year], |r| r.get(0)
        ).unwrap_or_else(|_| chrono::Local::now().format("%Y-%m-%d").to_string());

        conn.execute("DELETE FROM cops_items WHERE os_no=? AND os_year=?",
            rusqlite::params![os_no, os_year]).map_err(|e| e500(&e.to_string()))?;

        let loc: Option<String> = conn.query_row(
            "SELECT location_code FROM cops_master WHERE os_no=? AND os_year=?",
            rusqlite::params![os_no, os_year], |r| r.get(0)
        ).unwrap_or(None);

        save_items(&conn, &os_no, os_year, &os_date, loc.as_deref(), &req.items)?;
        recalc_totals(&conn, &os_no, os_year).map_err(|e| e500(&e.to_string()))?;
        Ok(())
    })();
    match write_result {
        Ok(()) => conn.execute_batch("COMMIT").map_err(|e| e500(&e.to_string()))?,
        Err(e) => { let _ = conn.execute_batch("ROLLBACK"); return Err(e); }
    }

    let case_json = get_case_json(&conn, &os_no, os_year).map_err(|e| e500(&e.to_string()))?;
    Ok(Json(case_json))
}

pub async fn delete_os(State(pool): Db, auth: AuthUser, Path((os_no, os_year)): Path<(String, i64)>, Query(params): Query<std::collections::HashMap<String, String>>) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let reason = params.get("reason").map(|s| s.as_str()).unwrap_or("").trim().to_string();
    if reason.len() < 5 { return Err(e400("Reason must be at least 5 characters.")); }

    let (adj_date, adj_time): (Option<String>, Option<String>) = conn.query_row(
        "SELECT adjudication_date, adjudication_time FROM cops_master WHERE os_no=? AND os_year=? AND entry_deleted='N'",
        rusqlite::params![os_no, os_year], |r| Ok((r.get(0)?, r.get(1)?))
    ).map_err(|_| e404("O.S. not found"))?;

    if adj_date.is_some() && !within_edit_window(&adj_time) {
        return Err(e400("Cannot delete — 24-hour deletion window has expired."));
    }

    // Archive + soft-delete in a single transaction so a crash between the two
    // operations cannot leave a record permanently stuck in limbo.
    conn.execute_batch("BEGIN").map_err(|e| e500(&e.to_string()))?;
    let write_result: Result<(), Err> = (|| {

    // Archive snapshot: explicit column list because cops_master has more columns
    // (shift, detention_date, case_type, is_draft, quashed, rejected, etc.) than
    // cops_master_deleted.  SELECT * would produce a column-count mismatch error.
    // deleted_by/reason/on are NULL here (copied before the update that sets them).
    conn.execute(
        "INSERT INTO cops_master_deleted (
             id, os_no, os_date, os_year, location_code, booked_by, pax_name, pax_nationality,
             passport_no, passport_date, pax_address1, pax_address2, pax_address3, pax_date_of_birth,
             pax_status, residence_at, country_of_departure, flight_no, flight_date, total_items,
             total_items_value, dutiable_value, redeemed_value, re_export_value, confiscated_value,
             total_duty_amount, rf_amount, pp_amount, ref_amount, br_amount, total_payable,
             adjudication_date, adj_offr_name, adj_offr_designation, adjn_offr_remarks, adjn_offr_remarks1,
             adjn_section_ref, online_adjn, os_printed, os_category, online_os, unique_no,
             entry_deleted, bkup_taken, detained_by, seal_no, nationality, seizure_date,
             pax_name_modified_by_vig, pax_image_filename, total_fa_value, wh_amount, other_amount,
             br_no_str, br_no_num, br_date_str, br_amount_str, dr_no, dr_year, total_drs,
             previous_os_details, previous_visits, father_name, old_passport_no, total_pkgs,
             supdts_remarks, supdt_remarks2, closure_ind, adjudication_time,
             deleted_by, deleted_reason, deleted_on,
             post_adj_br_entries, post_adj_dr_no, post_adj_dr_date,
             is_legacy, is_offline_adjudication, file_spot
         )
         SELECT
             id, os_no, os_date, os_year, location_code, booked_by, pax_name, pax_nationality,
             passport_no, passport_date, pax_address1, pax_address2, pax_address3, pax_date_of_birth,
             pax_status, residence_at, country_of_departure, flight_no, flight_date, total_items,
             total_items_value, dutiable_value, redeemed_value, re_export_value, confiscated_value,
             total_duty_amount, rf_amount, pp_amount, ref_amount, br_amount, total_payable,
             adjudication_date, adj_offr_name, adj_offr_designation, adjn_offr_remarks, adjn_offr_remarks1,
             adjn_section_ref, online_adjn, os_printed, os_category, online_os, unique_no,
             entry_deleted, bkup_taken, detained_by, seal_no, nationality, seizure_date,
             pax_name_modified_by_vig, pax_image_filename, total_fa_value, wh_amount, other_amount,
             br_no_str, br_no_num, br_date_str, br_amount_str, dr_no, dr_year, total_drs,
             previous_os_details, previous_visits, father_name, old_passport_no, total_pkgs,
             supdts_remarks, supdt_remarks2, closure_ind, adjudication_time,
             NULL, NULL, NULL,
             post_adj_br_entries, post_adj_dr_no, post_adj_dr_date,
             is_legacy, is_offline_adjudication, file_spot
         FROM cops_master WHERE os_no=? AND os_year=? AND entry_deleted='N'",
        rusqlite::params![os_no, os_year],
    ).map_err(|e| e500(&format!("Audit archive failed — deletion aborted: {e}")))?;

    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    conn.execute(
        "UPDATE cops_master SET entry_deleted='Y', deleted_by=?, deleted_reason=?, deleted_on=? WHERE os_no=? AND os_year=?",
        rusqlite::params![auth.0.sub, reason, today, os_no, os_year],
    ).map_err(|e| e500(&e.to_string()))?;

    conn.execute(
        "UPDATE cops_items SET entry_deleted='Y' WHERE os_no=? AND os_year=?",
        rusqlite::params![os_no, os_year],
    ).map_err(|e| e500(&e.to_string()))?;

    Ok(())
    })();
    match write_result {
        Ok(()) => conn.execute_batch("COMMIT").map_err(|e| e500(&e.to_string()))?,
        Err(e) => { let _ = conn.execute_batch("ROLLBACK"); return Err(e); }
    }

    Ok(Json(json!({ "message": "O/S case deleted.", "os_no": os_no, "os_year": os_year })))
}

pub async fn adjudicate(State(pool): Db, auth: AdjnUser, Path((os_no, os_year)): Path<(String, i64)>, Json(req): Json<AdjudicateRequest>) -> Result<Json<Value>, Err> {
    validate_remarks_length(req.adjn_offr_remarks.as_deref())?;

    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;

    let (adj_date, adj_time): (Option<String>, Option<String>) = conn.query_row(
        "SELECT adjudication_date, adjudication_time FROM cops_master WHERE os_no=? AND os_year=? AND entry_deleted='N' AND is_draft='N'",
        rusqlite::params![os_no, os_year], |r| Ok((r.get(0)?, r.get(1)?))
    ).map_err(|_| e404("Case not found or not submitted."))?;

    if adj_date.is_some() && !within_edit_window(&adj_time) {
        return Err(e400("Modification window expired (24h from adjudication)."));
    }

    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let adj_date_val = req.adjudication_date.clone().unwrap_or(today);
    let now_ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

    // Only stamp adjudication_time on first adjudication
    let stamp_time = if adj_time.is_none() { now_ts.as_str() } else { adj_time.as_deref().unwrap_or(&now_ts) };

    let base_duty: f64 = conn.query_row(
        "SELECT COALESCE(total_duty_amount, 0) FROM cops_master WHERE os_no=? AND os_year=?",
        rusqlite::params![os_no, os_year], |r| r.get(0)
    ).unwrap_or(0.0);

    let rf = req.rf_amount.unwrap_or(0.0);
    let pp = req.pp_amount.unwrap_or(0.0);
    let refv = req.ref_amount.unwrap_or(0.0);
    let total_payable = base_duty + rf + pp + refv;
    let desig = auth.0.desig.as_deref().unwrap_or(&req.adj_offr_designation);

    conn.execute(
        "UPDATE cops_master SET adj_offr_name=?, adj_offr_designation=?, adjudication_date=?,
         adjudication_time=?, adjn_offr_remarks=?, adjn_section_ref=?,
         rf_amount=?, pp_amount=?, ref_amount=?,
         confiscated_value=?, redeemed_value=?, re_export_value=?, total_payable=?,
         online_adjn='Y', closure_ind=?
         WHERE os_no=? AND os_year=? AND entry_deleted='N'",
        rusqlite::params![
            req.adj_offr_name, desig, adj_date_val, stamp_time,
            req.adjn_offr_remarks, req.adjn_section_ref,
            rf, pp, refv,
            req.confiscated_value.unwrap_or(0.0), req.redeemed_value.unwrap_or(0.0), req.re_export_value.unwrap_or(0.0),
            total_payable,
            if req.close_case.unwrap_or(false) { Some("Y") } else { None },
            os_no, os_year,
        ],
    ).map_err(|e| e500(&e.to_string()))?;

    let case_json = get_case_json(&conn, &os_no, os_year).map_err(|e| e500(&e.to_string()))?;
    Ok(Json(case_json))
}

pub async fn complete_offline(State(pool): Db, auth: AdjnUser, Path((os_no, os_year)): Path<(String, i64)>, Json(req): Json<CompleteOfflineRequest>) -> Result<Json<Value>, Err> {
    validate_remarks_length(req.adjn_offr_remarks.as_deref())?;

    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;

    let adj_name: Option<String> = conn.query_row(
        "SELECT adj_offr_name FROM cops_master WHERE os_no=? AND os_year=? AND entry_deleted='N' AND is_offline_adjudication='Y'",
        rusqlite::params![os_no, os_year], |r| r.get(0)
    ).map_err(|_| e404("Offline case not found."))?;

    if adj_name.is_some() { return Err(e400("This offline case is already completed.")); }

    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let now_ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

    // Compute total_payable the same way adjudicate() does
    let base_duty: f64 = conn.query_row(
        "SELECT COALESCE(total_duty_amount, 0) FROM cops_master WHERE os_no=? AND os_year=?",
        rusqlite::params![os_no, os_year], |r| r.get(0)
    ).unwrap_or(0.0);
    let rf   = req.rf_amount.unwrap_or(0.0);
    let pp   = req.pp_amount.unwrap_or(0.0);
    let refv = req.ref_amount.unwrap_or(0.0);
    let total_payable = base_duty + rf + pp + refv;

    conn.execute(
        "UPDATE cops_master SET adj_offr_name=?, adj_offr_designation=?, adjudication_date=?,
         adjudication_time=?, adjn_offr_remarks=?, rf_amount=?, pp_amount=?, ref_amount=?,
         confiscated_value=?, redeemed_value=?, re_export_value=?, total_payable=?, closure_ind=?
         WHERE os_no=? AND os_year=? AND entry_deleted='N'",
        rusqlite::params![
            req.adj_offr_name, req.adj_offr_designation,
            req.adjudication_date.unwrap_or(today), now_ts,
            req.adjn_offr_remarks, rf, pp, refv,
            req.confiscated_value.unwrap_or(0.0),
            req.redeemed_value.unwrap_or(0.0), req.re_export_value.unwrap_or(0.0),
            total_payable,
            if req.close_case.unwrap_or(false) { Some("Y") } else { None },
            os_no, os_year,
        ],
    ).map_err(|e| e500(&e.to_string()))?;

    Ok(Json(json!({ "status": "ok", "os_no": os_no, "os_year": os_year })))
}

pub async fn outcome_update(State(pool): Db, _auth: AuthUser, Path((os_no, os_year)): Path<(String, i64)>, Json(req): Json<CompleteOfflineRequest>) -> Result<Json<Value>, Err> {
    validate_remarks_length(req.adjn_offr_remarks.as_deref())?;

    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;

    let adj_name: Option<String> = conn.query_row(
        "SELECT adj_offr_name FROM cops_master WHERE os_no=? AND os_year=? AND entry_deleted='N' AND is_offline_adjudication='Y'",
        rusqlite::params![os_no, os_year], |r| r.get(0)
    ).map_err(|_| e404("Offline case not found."))?;

    if adj_name.is_some() { return Err(e400("Outcome already recorded for this case.")); }

    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let now_ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

    let base_duty: f64 = conn.query_row(
        "SELECT COALESCE(total_duty_amount, 0) FROM cops_master WHERE os_no=? AND os_year=?",
        rusqlite::params![os_no, os_year], |r| r.get(0)
    ).unwrap_or(0.0);
    let rf   = req.rf_amount.unwrap_or(0.0);
    let pp   = req.pp_amount.unwrap_or(0.0);
    let refv = req.ref_amount.unwrap_or(0.0);
    let total_payable = base_duty + rf + pp + refv;

    conn.execute(
        "UPDATE cops_master SET adj_offr_name=?, adj_offr_designation=?, adjudication_date=?,
         adjudication_time=?, adjn_offr_remarks=?, rf_amount=?, pp_amount=?, ref_amount=?,
         confiscated_value=?, redeemed_value=?, re_export_value=?, total_payable=?, closure_ind=?
         WHERE os_no=? AND os_year=? AND entry_deleted='N'",
        rusqlite::params![
            req.adj_offr_name, req.adj_offr_designation,
            req.adjudication_date.unwrap_or(today), now_ts,
            req.adjn_offr_remarks, rf, pp, refv,
            req.confiscated_value.unwrap_or(0.0),
            req.redeemed_value.unwrap_or(0.0), req.re_export_value.unwrap_or(0.0),
            total_payable,
            if req.close_case.unwrap_or(false) { Some("Y") } else { None },
            os_no, os_year,
        ],
    ).map_err(|e| e500(&e.to_string()))?;

    Ok(Json(json!({ "status": "ok", "os_no": os_no, "os_year": os_year })))
}

pub async fn quash_os(State(pool): Db, auth: AdjnUser, Path((os_no, os_year)): Path<(String, i64)>) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;

    let (adj_date, adj_time): (Option<String>, Option<String>) = conn.query_row(
        "SELECT adjudication_date, adjudication_time FROM cops_master WHERE os_no=? AND os_year=? AND entry_deleted='N' AND is_draft='N'",
        rusqlite::params![os_no, os_year], |r| Ok((r.get(0)?, r.get(1)?))
    ).map_err(|_| e404("Case not found."))?;

    if adj_date.is_none() { return Err(e400("Cannot quash an un-adjudicated case.")); }
    if !within_edit_window(&adj_time) { return Err(e400("24-hour deletion window has expired.")); }

    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    conn.execute_batch("BEGIN").map_err(|e| e500(&e.to_string()))?;
    let write_result: Result<(), Err> = (|| {
        // Archive to cops_master_deleted before hard-deleting so there is always
        // an audit trail even for quashed cases.
        conn.execute(
            "INSERT INTO cops_master_deleted (
                 id, os_no, os_date, os_year, location_code, booked_by, pax_name, pax_nationality,
                 passport_no, passport_date, pax_address1, pax_address2, pax_address3, pax_date_of_birth,
                 pax_status, residence_at, country_of_departure, flight_no, flight_date, total_items,
                 total_items_value, dutiable_value, redeemed_value, re_export_value, confiscated_value,
                 total_duty_amount, rf_amount, pp_amount, ref_amount, br_amount, total_payable,
                 adjudication_date, adj_offr_name, adj_offr_designation, adjn_offr_remarks, adjn_offr_remarks1,
                 adjn_section_ref, online_adjn, os_printed, os_category, online_os, unique_no,
                 entry_deleted, bkup_taken, detained_by, seal_no, nationality, seizure_date,
                 pax_name_modified_by_vig, pax_image_filename, total_fa_value, wh_amount, other_amount,
                 br_no_str, br_no_num, br_date_str, br_amount_str, dr_no, dr_year, total_drs,
                 previous_os_details, previous_visits, father_name, old_passport_no, total_pkgs,
                 supdts_remarks, supdt_remarks2, closure_ind, adjudication_time,
                 deleted_by, deleted_reason, deleted_on,
                 post_adj_br_entries, post_adj_dr_no, post_adj_dr_date,
                 is_legacy, is_offline_adjudication, file_spot
             )
             SELECT
                 id, os_no, os_date, os_year, location_code, booked_by, pax_name, pax_nationality,
                 passport_no, passport_date, pax_address1, pax_address2, pax_address3, pax_date_of_birth,
                 pax_status, residence_at, country_of_departure, flight_no, flight_date, total_items,
                 total_items_value, dutiable_value, redeemed_value, re_export_value, confiscated_value,
                 total_duty_amount, rf_amount, pp_amount, ref_amount, br_amount, total_payable,
                 adjudication_date, adj_offr_name, adj_offr_designation, adjn_offr_remarks, adjn_offr_remarks1,
                 adjn_section_ref, online_adjn, os_printed, os_category, online_os, unique_no,
                 entry_deleted, bkup_taken, detained_by, seal_no, nationality, seizure_date,
                 pax_name_modified_by_vig, pax_image_filename, total_fa_value, wh_amount, other_amount,
                 br_no_str, br_no_num, br_date_str, br_amount_str, dr_no, dr_year, total_drs,
                 previous_os_details, previous_visits, father_name, old_passport_no, total_pkgs,
                 supdts_remarks, supdt_remarks2, closure_ind, adjudication_time,
                 ?, 'Quashed by adjudicating officer', ?,
                 post_adj_br_entries, post_adj_dr_no, post_adj_dr_date,
                 is_legacy, is_offline_adjudication, file_spot
             FROM cops_master WHERE os_no=? AND os_year=?",
            rusqlite::params![auth.0.sub, today, os_no, os_year],
        ).map_err(|e| e500(&format!("Audit archive failed — quash aborted: {e}")))?;

        conn.execute("DELETE FROM cops_items WHERE os_no=? AND os_year=?", rusqlite::params![os_no, os_year])
            .map_err(|e| e500(&e.to_string()))?;
        conn.execute("DELETE FROM cops_master WHERE os_no=? AND os_year=?", rusqlite::params![os_no, os_year])
            .map_err(|e| e500(&e.to_string()))?;
        Ok(())
    })();
    match write_result {
        Ok(()) => conn.execute_batch("COMMIT").map_err(|e| e500(&e.to_string()))?,
        Err(e) => { let _ = conn.execute_batch("ROLLBACK"); return Err(e); }
    }

    Ok(Json(json!({ "message": "Case permanently deleted.", "os_no": os_no, "os_year": os_year })))
}

pub async fn post_adj(State(pool): Db, auth: AuthUser, Path((os_no, os_year)): Path<(String, i64)>, Json(req): Json<PostAdjRequest>) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;

    // Guard: BR/DR details can only be recorded after the adjudication order is issued
    let adj_date: Option<String> = conn.query_row(
        "SELECT adjudication_date FROM cops_master WHERE os_no=? AND os_year=? AND entry_deleted='N'",
        rusqlite::params![os_no, os_year], |r| r.get(0)
    ).map_err(|_| e404("O.S. case not found."))?;
    if adj_date.is_none() {
        return Err(e400("BR/DR details can only be added after the adjudication order is issued."));
    }

    conn.execute(
        "UPDATE cops_master SET post_adj_br_entries=?, post_adj_dr_no=?, post_adj_dr_date=? WHERE os_no=? AND os_year=? AND entry_deleted='N'",
        rusqlite::params![req.post_adj_br_entries, req.post_adj_dr_no, req.post_adj_dr_date, os_no, os_year],
    ).map_err(|e| e500(&e.to_string()))?;
    Ok(Json(json!({ "message": "Updated." })))
}

pub async fn print_pdf(State(pool): Db, auth: AuthUser, Path((os_no, os_year)): Path<(String, i64)>) -> Result<axum::response::Response, Err> {
    use axum::response::IntoResponse;
    use axum::http::header;

    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let mut case = get_case_json(&conn, &os_no, os_year).map_err(|e| e500(&e.to_string()))?;
    let items = load_items(&conn, &os_no, os_year).map_err(|e| e500(&e.to_string()))?;
    case["items"] = json!(items);

    let pdf_bytes = crate::pdf::generate_os_pdf(&case)
        .map_err(|e| e500(&format!("PDF generation failed: {e}")))?;

    Ok((
        [(header::CONTENT_TYPE, "application/pdf"),
         (header::CONTENT_DISPOSITION, &format!("attachment; filename=\"OS_{os_no}_{os_year}.pdf\""))],
        pdf_bytes,
    ).into_response())
}

pub async fn query_search(State(pool): Db, _auth: AuthUser, Json(body): Json<serde_json::Value>) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;

    // `export=true` lifts the per-page cap so download-all works in one request.
    let is_export = body.get("export").and_then(|v| v.as_bool()).unwrap_or(false);
    let page      = body.get("page").and_then(|v| v.as_i64()).unwrap_or(1).max(1);
    let per_page  = body.get("per_page")
        .or_else(|| body.get("limit"))
        .and_then(|v| v.as_i64())
        .unwrap_or(20)
        .clamp(1, if is_export { 5000 } else { 100 });
    let offset = (page - 1) * per_page;

    // ── Exact lookup by os_no + os_year (OSPrintView path) ────────────────────
    let os_no_str   = body.get("os_no").and_then(|v| v.as_str()).map(|s| s.trim()).filter(|s| !s.is_empty()).map(|s| s.to_string());
    let os_year_val = body.get("os_year").and_then(|v| v.as_i64());

    if let (Some(os_no), Some(os_year)) = (os_no_str.as_deref(), os_year_val) {
        let case = get_case_json(&conn, os_no, os_year)
            .map_err(|e| e500(&e.to_string()))?;
        return Ok(Json(json!({
            "total": 1, "total_count": 1, "page": 1, "per_page": per_page,
            "total_pages": 1, "has_next": false, "has_prev": false, "items": [case]
        })));
    }

    // ── Build WHERE clause from advanced filters ───────────────────────────────
    let mut conditions: Vec<String> = vec!["cm.entry_deleted='N'".to_string()];
    let mut params: Vec<String>     = vec![];

    // Helper: add a LIKE condition on a cops_master column
    macro_rules! like_cond {
        ($col:expr, $key:expr) => {
            if let Some(v) = body.get($key).and_then(|v| v.as_str())
                .map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
                conditions.push(format!("cm.{} LIKE ?", $col));
                params.push(format!("%{}%", v));
            }
        }
    }

    // If os_no provided without os_year it's a partial search
    if let Some(ref no) = os_no_str {
        conditions.push("cm.os_no LIKE ?".to_string());
        params.push(format!("%{}%", no));
    }
    // os_year alone (no os_no) — exact year filter
    if os_no_str.is_none() {
        if let Some(yr) = os_year_val {
            conditions.push("cm.os_year = ?".to_string());
            params.push(yr.to_string());
        }
    }

    like_cond!("pax_name",             "pax_name");
    like_cond!("passport_no",          "passport_no");
    like_cond!("flight_no",            "flight_no");
    like_cond!("country_of_departure", "country_of_departure");

    if let Some(from) = body.get("from_date").and_then(|v| v.as_str()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
        conditions.push("cm.os_date >= ?".to_string());
        params.push(from);
    }
    if let Some(to) = body.get("to_date").and_then(|v| v.as_str()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
        conditions.push("cm.os_date <= ?".to_string());
        params.push(to);
    }
    if let Some(min) = body.get("min_value").and_then(|v| v.as_f64()) {
        conditions.push("cm.total_items_value >= ?".to_string());
        params.push(min.to_string());
    }
    if let Some(max) = body.get("max_value").and_then(|v| v.as_f64()) {
        conditions.push("cm.total_items_value <= ?".to_string());
        params.push(max.to_string());
    }
    if let Some(ct) = body.get("case_type").and_then(|v| v.as_str()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
        conditions.push("cm.case_type = ?".to_string());
        params.push(ct);
    }
    // item_desc — EXISTS sub-query so multi-item cases are returned without duplicates
    if let Some(desc) = body.get("item_desc").and_then(|v| v.as_str()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
        conditions.push(
            "EXISTS (SELECT 1 FROM cops_items ci \
             WHERE ci.os_no=cm.os_no AND ci.os_year=cm.os_year \
             AND ci.items_desc LIKE ? \
             AND (ci.entry_deleted IS NULL OR ci.entry_deleted!='Y'))".to_string()
        );
        params.push(format!("%{}%", desc));
    }

    // Legacy generic text search (OSPrintView compat — only when no specific filter given)
    let search = body.get("search").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    if !search.is_empty() && params.is_empty() {
        let p = format!("%{}%", search);
        conditions.push(
            "(cm.os_no LIKE ? OR cm.pax_name LIKE ? OR cm.passport_no LIKE ? \
              OR cm.flight_no LIKE ? OR cm.adj_offr_name LIKE ?)".to_string()
        );
        for _ in 0..5 { params.push(p.clone()); }
    }

    let where_clause = conditions.join(" AND ");

    // ── Sort ──────────────────────────────────────────────────────────────────
    let sort_col = match body.get("sort_by").and_then(|v| v.as_str()).unwrap_or("os_year") {
        "pax_name"          => "cm.pax_name",
        "flight_date"       => "cm.flight_date",
        "total_items_value" => "cm.total_items_value",
        "adjudication_date" => "cm.adjudication_date",
        _                   => "cm.os_year",
    };
    let sort_dir = if body.get("sort_dir").and_then(|v| v.as_str()).unwrap_or("desc") == "asc" { "ASC" } else { "DESC" };
    // Secondary sort: os_no as integer so 10 > 9
    let order_clause = format!("{sort_col} {sort_dir}, CAST(cm.os_no AS INTEGER) DESC");

    // ── Count ─────────────────────────────────────────────────────────────────
    let total: i64 = conn.query_row(
        &format!("SELECT COUNT(*) FROM cops_master cm WHERE {where_clause}"),
        rusqlite::params_from_iter(params.iter()),
        |r| r.get(0)
    ).unwrap_or(0);

    // ── Main SELECT — includes country_of_departure + item_desc_summary ───────
    let mut stmt = conn.prepare(&format!(
        "SELECT cm.id, cm.os_no, cm.os_date, cm.os_year, cm.pax_name, cm.passport_no,
                cm.flight_no, cm.flight_date, cm.total_items_value, cm.total_payable,
                cm.adjudication_date, cm.adj_offr_name, cm.is_draft, cm.online_adjn,
                cm.entry_deleted, cm.post_adj_br_entries, cm.post_adj_dr_no,
                cm.post_adj_dr_date, cm.total_duty_amount, cm.closure_ind,
                cm.country_of_departure,
                (SELECT GROUP_CONCAT(ci.items_desc, '; ')
                 FROM cops_items ci
                 WHERE ci.os_no=cm.os_no AND ci.os_year=cm.os_year
                   AND (ci.entry_deleted IS NULL OR ci.entry_deleted!='Y')
                 ORDER BY ci.items_sno) AS item_desc_summary
         FROM cops_master cm WHERE {where_clause}
         ORDER BY {order_clause}
         LIMIT {per_page} OFFSET {offset}"
    )).map_err(|e| e500(&e.to_string()))?;

    let rows: Vec<Value> = stmt.query_map(rusqlite::params_from_iter(params.iter()), |r| {
        Ok(json!({
            "id":                   r.get::<_, i64>(0)?,
            "os_no":                r.get::<_, String>(1)?,
            "os_date":              r.get::<_, Option<String>>(2)?,
            "os_year":              r.get::<_, Option<i64>>(3)?,
            "pax_name":             r.get::<_, Option<String>>(4)?,
            "passport_no":          r.get::<_, Option<String>>(5)?,
            "flight_no":            r.get::<_, Option<String>>(6)?,
            "flight_date":          r.get::<_, Option<String>>(7)?,
            "total_items_value":    r.get::<_, Option<f64>>(8)?,
            "total_payable":        r.get::<_, Option<f64>>(9)?,
            "adjudication_date":    r.get::<_, Option<String>>(10)?,
            "adj_offr_name":        r.get::<_, Option<String>>(11)?,
            "is_draft":             r.get::<_, Option<String>>(12)?,
            "online_adjn":          r.get::<_, Option<String>>(13)?,
            "entry_deleted":        r.get::<_, Option<String>>(14)?,
            "post_adj_br_entries":  r.get::<_, Option<String>>(15)?,
            "post_adj_dr_no":       r.get::<_, Option<String>>(16)?,
            "post_adj_dr_date":     r.get::<_, Option<String>>(17)?,
            "total_duty_amount":    r.get::<_, Option<f64>>(18)?,
            "closure_ind":          r.get::<_, Option<String>>(19)?,
            "country_of_departure": r.get::<_, Option<String>>(20)?,
            "item_desc_summary":    r.get::<_, Option<String>>(21)?,
            "items": [],  // not hydrated in list view; OSPrintView exact-lookup path includes items
        }))
    }).map_err(|e| e500(&e.to_string()))?.filter_map(|r| r.ok()).collect();

    let total_pages = (total + per_page - 1) / per_page;
    Ok(Json(json!({
        "total":       total,
        "total_count": total,
        "page":        page,
        "per_page":    per_page,
        "total_pages": total_pages,
        "has_next":    page < total_pages,
        "has_prev":    page > 1,
        "items":       rows,
    })))
}

pub async fn monthly_report(State(pool): Db, _auth: AuthUser, Query(params): Query<std::collections::HashMap<String, String>>) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let month = params.get("month").and_then(|v| v.parse::<i64>().ok()).unwrap_or(chrono::Local::now().month() as i64);
    let year  = params.get("year").and_then(|v| v.parse::<i64>().ok()).unwrap_or(chrono::Local::now().year() as i64);

    let from = format!("{year}-{month:02}-01");
    let to   = format!("{year}-{month:02}-31");

    // Load master rows — non-draft, non-deleted, ordered by OS no numerically
    let mut mstmt = conn.prepare(
        "SELECT os_no, os_date, os_year, booked_by, flight_no, pax_name, pax_nationality,
                passport_no, pax_address1, pax_address2, pax_address3,
                total_items_value, total_fa_value, rf_amount, ref_amount, pp_amount,
                total_duty_amount, post_adj_br_entries, post_adj_dr_no, post_adj_dr_date,
                file_spot, adj_offr_name, adj_offr_designation, case_type, adjudication_date,
                wh_amount, other_amount
         FROM cops_master
         WHERE os_date >= ? AND os_date <= ? AND entry_deleted='N' AND is_draft='N'
         ORDER BY CAST(os_no AS INTEGER)"
    ).map_err(|e| e500(&e.to_string()))?;

    struct MasterRow {
        os_no: String, os_date: Option<String>, os_year: Option<i64>,
        booked_by: Option<String>, flight_no: Option<String>,
        pax_name: Option<String>, nationality: Option<String>, passport_no: Option<String>,
        addr1: Option<String>, addr2: Option<String>, addr3: Option<String>,
        total_value: f64, fa_value: f64,
        rf: f64, ref_amt: f64, pp: f64, duty: f64,
        br_entries_json: Option<String>, dr_no: Option<String>, dr_date: Option<String>,
        file_spot: Option<String>, adj_name: Option<String>, adj_desig: Option<String>,
        case_type: Option<String>, adj_date: Option<String>,
        wh: f64, other: f64,
    }

    let masters: Vec<MasterRow> = mstmt.query_map(rusqlite::params![from, to], |r| {
        Ok(MasterRow {
            os_no:           r.get(0)?,
            os_date:         r.get(1)?,
            os_year:         r.get(2)?,
            booked_by:       r.get(3)?,
            flight_no:       r.get(4)?,
            pax_name:        r.get(5)?,
            nationality:     r.get(6)?,
            passport_no:     r.get(7)?,
            addr1:           r.get(8)?,
            addr2:           r.get(9)?,
            addr3:           r.get(10)?,
            total_value:     r.get::<_, Option<f64>>(11)?.unwrap_or(0.0),
            fa_value:        r.get::<_, Option<f64>>(12)?.unwrap_or(0.0),
            rf:              r.get::<_, Option<f64>>(13)?.unwrap_or(0.0),
            ref_amt:         r.get::<_, Option<f64>>(14)?.unwrap_or(0.0),
            pp:              r.get::<_, Option<f64>>(15)?.unwrap_or(0.0),
            duty:            r.get::<_, Option<f64>>(16)?.unwrap_or(0.0),
            br_entries_json: r.get(17)?,
            dr_no:           r.get(18)?,
            dr_date:         r.get(19)?,
            file_spot:       r.get(20)?,
            adj_name:        r.get(21)?,
            adj_desig:       r.get(22)?,
            case_type:       r.get(23)?,
            adj_date:        r.get(24)?,
            wh:              r.get::<_, Option<f64>>(25)?.unwrap_or(0.0),
            other:           r.get::<_, Option<f64>>(26)?.unwrap_or(0.0),
        })
    }).map_err(|e| e500(&e.to_string()))?.filter_map(|r| r.ok()).collect();

    if masters.is_empty() {
        return Ok(Json(json!({ "month": month, "year": year, "items": [] })));
    }

    // Bulk-load items for all cases in this month via a parameterized JOIN (no N+1, no injection)
    let mut items_map: std::collections::HashMap<String, Vec<(f64, String, String, String, String)>> =
        std::collections::HashMap::new();

    if let Ok(mut istmt) = conn.prepare(
        "SELECT i.os_no, i.os_year, i.items_qty, i.items_uqc, i.items_desc,
                i.items_duty_type, i.items_release_category
         FROM cops_items i
         INNER JOIN cops_master m ON i.os_no = m.os_no AND i.os_year = m.os_year
         WHERE m.os_date >= ? AND m.os_date <= ?
           AND m.entry_deleted='N' AND m.is_draft='N'
           AND (i.entry_deleted IS NULL OR i.entry_deleted='N')
         ORDER BY i.os_no, i.os_year, i.items_sno"
    ) {
        let _ = istmt.query_map(rusqlite::params![from, to], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<f64>>(2)?.unwrap_or(0.0),
                r.get::<_, Option<String>>(3)?.unwrap_or_default(),
                r.get::<_, Option<String>>(4)?.unwrap_or_default(),
                r.get::<_, Option<String>>(5)?.unwrap_or_default(),
                r.get::<_, Option<String>>(6)?.unwrap_or_default(),
            ))
        }).map(|rows| {
            for row in rows.filter_map(|r| r.ok()) {
                let (os_no, qty, uqc, desc, duty_type, rel_cat) = row;
                items_map.entry(os_no).or_default()
                    .push((qty, uqc, desc, duty_type, rel_cat));
            }
        });
    }

    let rows: Vec<Value> = masters.into_iter().map(|m| {
        let items = items_map.get(&m.os_no).map(|v| v.as_slice()).unwrap_or(&[]);

        // Address
        let address = [m.addr1.as_deref(), m.addr2.as_deref(), m.addr3.as_deref()]
            .iter().filter_map(|s| *s).map(str::trim).filter(|s| !s.is_empty())
            .collect::<Vec<_>>().join(" ");

        // Item description: "qty uqc desc, ..."
        let item_description = items.iter().map(|(qty, uqc, desc, _, _)| {
            let qty_s = if *qty == 0.0 { "0".to_string() } else { format!("{}", qty) };
            format!("{qty_s} {uqc} {desc}").trim().to_string()
        }).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(", ");

        // Tags
        let mut seen_tags: Vec<String> = Vec::new();
        let mut seen_set: std::collections::HashSet<String> = std::collections::HashSet::new();
        for (_, _, desc, duty_type, _) in items {
            let tag = tag_from_duty_type(duty_type)
                .or_else(|| if !desc.is_empty() { Some(tag_from_desc(desc)) } else { None })
                .unwrap_or_else(|| "Miscellaneous Goods (With Different Unit Qty Codes)".to_string());
            if seen_set.insert(tag.clone()) { seen_tags.push(tag); }
        }
        let tags = seen_tags.join(", ");

        // Confiscation label
        let mut conf_cats: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
        for (_, _, _, _, rel_cat) in items {
            match rel_cat.to_uppercase().trim() {
                "CONFS" => { conf_cats.insert("Absolute Confiscation"); }
                "REF"   => { conf_cats.insert("Re-Export"); }
                "RF"    => { conf_cats.insert("Confiscation"); }
                _ => {}
            }
        }
        let column1 = if conf_cats.is_empty() { None } else { Some(conf_cats.into_iter().collect::<Vec<_>>().join(" & ")) };

        // Financial
        let rf_ref = m.rf + m.ref_amt;
        let total  = rf_ref + m.pp + m.duty;
        let value_in_rs = (m.total_value - m.fa_value).max(0.0);
        let other_charges = m.wh + m.other;

        // BR entries: parse JSON → (br_nos_str, br_dates_str)
        let (br_no, br_date) = parse_br_entries(m.br_entries_json.as_deref());

        // DR remarks
        let remarks = format_dr_remarks(m.dr_no.as_deref(), m.dr_date.as_deref());

        // Export/Import
        let export_import = if m.case_type.as_deref().unwrap_or("").trim().to_uppercase() == "EXPORT CASE" {
            "Export"
        } else {
            "Import"
        };

        // O-in-O (adjudication order number = OS no itself in this system)
        let oino_no   = m.os_no.clone();
        let oino_date = m.adj_date.clone().unwrap_or_default();

        json!({
            "os_no":                m.os_no,
            "os_date":              m.os_date,
            "batch_aiu":            m.booked_by,
            "flt_no":               m.flight_no,
            "pax_name":             m.pax_name,
            "nationality":          m.nationality,
            "passport_no":          m.passport_no,
            "address":              if address.is_empty() { Value::Null } else { json!(address) },
            "item_description":     if item_description.is_empty() { Value::Null } else { json!(item_description) },
            "tags":                 if tags.is_empty() { Value::Null } else { json!(tags) },
            "quantity":             if item_description.is_empty() { Value::Null } else { json!(item_description) },
            "value_in_rs":          value_in_rs,
            "oinO_no":              oino_no,
            "date_of_oinO":         oino_date,
            "rf_ref":               rf_ref,
            "penalty":              m.pp,
            "duty_rs":              m.duty,
            "other_charges":        other_charges,
            "total":                total,
            "br_no":                if br_no.is_empty() { Value::Null } else { json!(br_no) },
            "br_date":              if br_date.is_empty() { Value::Null } else { json!(br_date) },
            "remarks":              if remarks.is_empty() { Value::Null } else { json!(remarks) },
            "file_spot":            m.file_spot.unwrap_or_else(|| "Spot".to_string()),
            "adjudicated_by_ac_dc": m.adj_name.or(m.adj_desig),
            "adjudicated_by_jc_adc": "",
            "export_import":        export_import,
            "column1":              column1,
        })
    }).collect();

    Ok(Json(json!({ "month": month, "year": year, "items": rows })))
}

// ── Monthly report helpers ────────────────────────────────────────────────────

fn tag_from_duty_type(dt: &str) -> Option<String> {
    let d = dt.to_lowercase();
    if d.is_empty() { return None; }
    Some(if d.contains("gold") || d.contains("jewellery") || d.contains("jewelry") || d.contains("necklace") || d.contains("bangle") || d.contains("yellow metal") {
        "Gold (Primary & Jewellery Forms)"
    } else if d.contains("silver") { "Silver"
    } else if d.contains("liquor") { "Liquor"
    } else if d.contains("e-cig") || d.contains("electronic cig") { "E-CIGARETTES"
    } else if d.contains("cigarette") { "Cigarettes"
    } else if d.contains("cell phone") || d.contains("mobile phone") || d.contains("smartphone") || d.contains("iphone") || d.contains("camera") || d.contains("television") || d.contains("dvd player") || d.contains("tablet") || d.contains("headphone") || d.contains("power bank") || d.contains("smartwatch") {
        "Consumer Electronics (Cameras,Televisions,Cell Phones,DVD Players Etc)"
    } else if d.contains("walkman") || d.contains("calculator") || d.contains("audio cd") || d.contains("video cd") {
        "Misc. Electronic Items (CDs,DVDs,Walkman,Calculator,Digital Diary etc)"
    } else if d.contains("watch part") || d.contains("watch movement") { "Watch_Parts"
    } else if d.contains("watch") { "Watches"
    } else if d.contains("ganja") || d.contains("cannabis") { "Ganja"
    } else if d.contains("heroin") || d.contains("brown sugar") { "Heroin"
    } else if d.contains("cocaine") { "Cocaine"
    } else if d.contains("morphine") { "Morphine"
    } else if d.contains("opium") { "Opium"
    } else if d.contains("hashish") || d.contains("charas") { "Hashish / Charas"
    } else if d.contains("mandrax") || d.contains("methaqualone") { "Mandrax / Methaqualone"
    } else if d.contains("poppy") { "Poppy Seeds"
    } else if d.contains("narcotic") || d.contains("ndps") || d.contains("methamphetamine") { "Other Narcotics"
    } else if d.contains("ficn") || d.contains("counterfeit curr") { "Indian Fake Currency Notes (FICN)- Face Value in Rs."
    } else if d.contains("foreign currency") || d.contains("fema") || d.contains("foreign exchange") || d.contains("currency") {
        "Foreign Currency (Equivalent to Indian Rs.)"
    } else if d.contains("wildlife") || d.contains("ivory") || d.contains("pangolin") || d.contains("coral") { "Wild Life / Flora / Fauna"
    } else if d.contains("precious stone") || d.contains("diamond") || d.contains("sapphire") || d.contains("gemstone") || d.contains("ruby") || d.contains("emerald") || d.contains("pearl") {
        "Diamonds & Precious Stones"
    } else if d.contains("garment") { "Garments"
    } else if d.contains("textile") || d.contains("fabric") { "Fabrics"
    } else if d.contains("antique") { "Antiquities"
    } else if d.contains("medicine") || d.contains("pharmaceutical") || d.contains("drug") || d.contains("capsule") || d.contains("steroid") {
        "Pharmaceutical Drugs / Medicines"
    } else if d.contains("chemical") { "Chemicals"
    } else {
        return None; // fall through to desc-based classification
    }.to_string())
}

fn tag_from_desc(desc: &str) -> String {
    let d = desc.to_lowercase();
    if d.contains("gold") || d.contains("jewel") || d.contains("necklace") || d.contains("bangle") || d.contains("yellow metal") {
        return "Gold (Primary & Jewellery Forms)".to_string();
    }
    if d.contains("silver") { return "Silver".to_string(); }
    if d.contains("liquor") || d.contains("whisky") || d.contains("whiskey") || d.contains("vodka") || d.contains("brandy") || d.contains("rum") { return "Liquor".to_string(); }
    if d.contains("e-cigarette") || d.contains("e cigarette") || d.contains("vape") { return "E-CIGARETTES".to_string(); }
    if d.contains("cigarette") { return "Cigarettes".to_string(); }
    if d.contains("mobile") || d.contains("phone") || d.contains("camera") || d.contains("laptop") || d.contains("tablet") || d.contains("ipad") || d.contains("iphone") || d.contains("television") || d.contains("tv") {
        return "Consumer Electronics (Cameras,Televisions,Cell Phones,DVD Players Etc)".to_string();
    }
    if d.contains("watch") { return "Watches".to_string(); }
    if d.contains("ganja") || d.contains("cannabis") || d.contains("marijuana") { return "Ganja".to_string(); }
    if d.contains("heroin") { return "Heroin".to_string(); }
    if d.contains("cocaine") { return "Cocaine".to_string(); }
    if d.contains("currency") || d.contains("dollars") || d.contains("euro") || d.contains("forex") { return "Foreign Currency (Equivalent to Indian Rs.)".to_string(); }
    if d.contains("diamond") || d.contains("precious stone") || d.contains("sapphire") || d.contains("ruby") { return "Diamonds & Precious Stones".to_string(); }
    if d.contains("garment") || d.contains("clothing") || d.contains("shirt") || d.contains("trouser") { return "Garments".to_string(); }
    "Miscellaneous Goods (With Different Unit Qty Codes)".to_string()
}

fn parse_br_entries(json_str: Option<&str>) -> (String, String) {
    let s = match json_str { Some(s) if !s.is_empty() => s, _ => return (String::new(), String::new()) };
    let Ok(entries) = serde_json::from_str::<Vec<serde_json::Value>>(s) else { return (String::new(), String::new()) };
    let mut numbers: Vec<String> = Vec::new();
    let mut dates:   Vec<String> = Vec::new();
    for e in &entries {
        if let Some(no) = e.get("no").and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty()) {
            numbers.push(no.to_string());
        }
        if let Some(d) = e.get("date").and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty()) {
            // Convert YYYY-MM-DD → DD-MM-YYYY
            let formatted = if d.len() >= 10 && d.as_bytes()[4] == b'-' {
                format!("{}-{}-{}", &d[8..10], &d[5..7], &d[0..4])
            } else { d.to_string() };
            dates.push(formatted);
        }
    }
    (numbers.join(", "), dates.join(", "))
}

fn format_dr_remarks(dr_no: Option<&str>, dr_date: Option<&str>) -> String {
    let no = match dr_no { Some(s) if !s.trim().is_empty() => s.trim(), _ => return String::new() };
    let mut parts = vec![format!("DR.No.{no}")];
    if let Some(d) = dr_date.filter(|s| s.len() >= 10) {
        if d.as_bytes()[4] == b'-' {
            parts.push(format!("dt.{}.{}.{}", &d[8..10], &d[5..7], &d[0..4]));
        }
    }
    parts.join(" ")
}

// ── Internal helpers ──────────────────────────────────────────────────────────

fn save_items(conn: &rusqlite::Connection, os_no: &str, os_year: i64, os_date: &str, loc: Option<&str>, items: &[CreateItemRequest]) -> Result<(), Err> {
    for item in items {
        let fa = item.items_fa.unwrap_or(0.0);
        let rate = item.cumulative_duty_rate.unwrap_or(0.0);
        let val = item.items_value.unwrap_or(0.0);
        let duty = ((val - fa).max(0.0) * rate / 100.0 * 100.0).round() / 100.0;

        conn.execute(
            "INSERT INTO cops_items (os_no, os_date, os_year, location_code, items_sno, items_desc,
             items_qty, items_uqc, value_per_piece, items_value, items_fa, items_fa_type,
             items_fa_qty, items_fa_uqc, cumulative_duty_rate, items_duty, items_duty_type,
             items_category, items_release_category, items_sub_category, entry_deleted)
             VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
            rusqlite::params![
                os_no, os_date, os_year, loc, item.items_sno, item.items_desc,
                item.items_qty, item.items_uqc, item.value_per_piece, item.items_value,
                item.items_fa, item.items_fa_type, item.items_fa_qty, item.items_fa_uqc,
                item.cumulative_duty_rate, duty, item.items_duty_type,
                item.items_category, item.items_release_category, item.items_sub_category, "N",
            ],
        ).map_err(|e| e500(&e.to_string()))?;
    }
    Ok(())
}

fn recalc_totals(conn: &rusqlite::Connection, os_no: &str, os_year: i64) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE cops_master SET
         total_items      = (SELECT COUNT(*) FROM cops_items WHERE os_no=cops_master.os_no AND os_year=cops_master.os_year AND entry_deleted='N'),
         total_items_value= (SELECT COALESCE(SUM(items_value),0) FROM cops_items WHERE os_no=cops_master.os_no AND os_year=cops_master.os_year AND entry_deleted='N'),
         total_duty_amount= (SELECT COALESCE(SUM(items_duty),0) FROM cops_items WHERE os_no=cops_master.os_no AND os_year=cops_master.os_year AND entry_deleted='N')
         WHERE os_no=? AND os_year=?",
        rusqlite::params![os_no, os_year],
    )?;
    Ok(())
}

fn get_case_json(conn: &rusqlite::Connection, os_no: &str, os_year: i64) -> rusqlite::Result<Value> {
    let mut case = conn.query_row(
        "SELECT * FROM cops_master WHERE os_no=? AND os_year=? AND entry_deleted='N'",
        rusqlite::params![os_no, os_year],
        |r| {
            let col_count = r.as_ref().column_count();
            let mut map = serde_json::Map::new();
            for i in 0..col_count {
                let name = r.as_ref().column_name(i).unwrap_or("?").to_string();
                let val: Value = match r.get_ref(i)? {
                    rusqlite::types::ValueRef::Null => Value::Null,
                    rusqlite::types::ValueRef::Integer(n) => json!(n),
                    rusqlite::types::ValueRef::Real(f) => json!(f),
                    rusqlite::types::ValueRef::Text(s) => json!(String::from_utf8_lossy(s)),
                    rusqlite::types::ValueRef::Blob(b) => json!(String::from_utf8_lossy(b)),
                };
                map.insert(name, val);
            }
            Ok(Value::Object(map))
        }
    )?;

    let items = load_items(conn, os_no, os_year)?;
    case["items"] = json!(items);
    Ok(case)
}

// ── classify-item ─────────────────────────────────────────────────────────────

pub async fn classify_item(_auth: AuthUser, Query(params): Query<std::collections::HashMap<String, String>>) -> Json<Value> {
    let desc = params.get("description").map(|s| s.as_str()).unwrap_or("").to_uppercase();
    let (duty_type, uqc) = classify_description(&desc);
    Json(json!({ "duty_type": duty_type, "uqc": uqc }))
}

// Returns (duty_type, uqc) matching the exact strings used in the frontend DUTY_TYPES array
// and the items_uqc select options. "Miscellaneous-22" is the fallback the frontend skips.
fn classify_description(desc: &str) -> (&'static str, &'static str) {
    const NARCOTICS: &[&str] = &[
        "DRUG", "NARCOTIC", "CANNABIS", "GANJA", "MARIJUANA", "HEROIN", "COCAINE",
        "OPIUM", "HASHISH", "KETAMINE", "MDMA", "NDPS", "CHARAS",
    ];
    const GOLD_JWL: &[&str] = &[
        "JEWELLERY", "JEWELRY", "ORNAMENT", "CHAIN", "BANGLE", "RING",
        "NECKLACE", "BRACELET", "EARRING", "PENDANT",
    ];
    const GOLD_PRI: &[&str] = &["GOLD BAR", "GOLD COIN", "GOLD BISCUIT", "GOLD INGOT", "GOLD BULLION", "GOLD BIT"];
    const GOLD: &[&str] = &["GOLD", "GOLDEN"];
    const SILVER: &[&str] = &["SILVER", "SILVERWARE"];
    const CURRENCY: &[&str] = &[
        "CURRENCY", "CASH", "DOLLAR", "EURO", "POUND", "RIYAL", "DIRHAM", "YEN",
        "BANKNOTE", "FOREIGN NOTE",
    ];
    const CELL_PHONE: &[&str] = &[
        "MOBILE", "IPHONE", "SAMSUNG", "REDMI", "ONEPLUS", "OPPO", "VIVO", "REALME",
        "XIAOMI", "HUAWEI", "NOKIA", "MOTOROLA", "SMARTPHONE", "HANDSET",
    ];
    const WATCH: &[&str] = &["WATCH", "WRISTWATCH", "TIMEPIECE", "ROLEX", "OMEGA", "SEIKO"];
    const CAMERA: &[&str] = &["CAMERA", "DSLR", "CAMCORDER", "GOPRO", "WEBCAM"];
    const ELECTRONICS: &[&str] = &[
        "LAPTOP", "COMPUTER", "NOTEBOOK", "TABLET", "IPAD", "MACBOOK",
        "DRONE", "AIRPOD", "EARPHONE", "HEADPHONE", "SPEAKER", "BLUETOOTH",
        "TELEVISION", "TV", "MONITOR", "PROJECTOR", "PLAYSTATION", "XBOX", "NINTENDO",
        "KEYBOARD", "MOUSE", "PRINTER", "SCANNER", "ROUTER", "MODEM", "PHONE",
    ];
    const CIGARETTE: &[&str] = &["CIGARETTE", "CIGAR", "BIDI", "VAPE", "E-CIG", "MARLBORO", "DUNHILL"];
    const TOBACCO: &[&str] = &["TOBACCO", "GUTKHA", "PAN MASALA", "ZARDA"];
    const LIQUOR: &[&str] = &[
        "LIQUOR", "ALCOHOL", "WINE", "WHISKY", "WHISKEY", "VODKA", "RUM", "GIN",
        "BEER", "BRANDY", "SCOTCH", "BOURBON", "COGNAC", "TEQUILA",
    ];
    const TEXTILE: &[&str] = &["GARMENT", "CLOTH", "CLOTHING", "DRESS", "SAREE", "FABRIC", "SHIRT", "TROUSER", "JEANS", "TEXTILE"];
    const ARMS: &[&str] = &["PISTOL", "REVOLVER", "RIFLE", "AMMUNITION", "FIREARM", "GUN", "BULLET", "CARTRIDGE"];
    const RED_SANDERS: &[&str] = &["RED SANDERS", "RED SANDALWOOD", "REDWOOD"];

    // Check multi-word patterns first (longer/more specific wins)
    for phrase in GOLD_PRI { if desc.contains(phrase) { return ("Gold (Primary)-07", "GMS"); } }
    for phrase in GOLD_JWL { if desc.contains(phrase) { return ("Gold (Jewellery)-06", "GMS"); } }
    for phrase in RED_SANDERS { if desc.contains(phrase) { return ("Red Sanders / Timber-36", "KGS"); } }

    if NARCOTICS.iter().any(|k| desc.contains(k))   { return ("Narcotics (Other NDPS)-55", "GMS"); }
    if GOLD.iter().any(|k| desc.contains(k))         { return ("Gold (Primary)-07", "GMS"); }
    if SILVER.iter().any(|k| desc.contains(k))       { return ("Silver-14", "GMS"); }
    if CURRENCY.iter().any(|k| desc.contains(k))     { return ("Currency (Foreign)-04", "NOS"); }
    if WATCH.iter().any(|k| desc.contains(k))        { return ("Watch / Watch Movements-25", "NOS"); }
    if CAMERA.iter().any(|k| desc.contains(k))       { return ("Cameras / Video Cameras-17", "NOS"); }
    if CELL_PHONE.iter().any(|k| desc.contains(k))   { return ("Cell Phones-18", "NOS"); }
    if CIGARETTE.iter().any(|k| desc.contains(k))    { return ("Cigarettes-03", "STK"); }
    if TOBACCO.iter().any(|k| desc.contains(k))      { return ("Tobacco / Gutkha-30", "GMS"); }
    if LIQUOR.iter().any(|k| desc.contains(k))       { return ("Liquor-08", "LTR"); }
    if ELECTRONICS.iter().any(|k| desc.contains(k))  { return ("Electronic Goods-21", "NOS"); }
    if TEXTILE.iter().any(|k| desc.contains(k))      { return ("Textiles / Fabrics-26", "KGS"); }
    if ARMS.iter().any(|k| desc.contains(k))         { return ("Arms & Ammunition-13", "NOS"); }
    ("Miscellaneous-22", "NOS")
}

// ── check-os-no ───────────────────────────────────────────────────────────────

pub async fn check_os_no(State(pool): Db, _auth: AuthUser, Query(params): Query<std::collections::HashMap<String, String>>) -> Json<Value> {
    let os_no = params.get("os_no").map(|s| s.as_str()).unwrap_or("");
    let os_year = params.get("os_year").and_then(|v| v.parse::<i64>().ok())
        .unwrap_or_else(|| chrono::Local::now().year() as i64);

    let exists = if let Ok(conn) = pool.get() {
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM cops_master WHERE os_no=? AND os_year=? AND entry_deleted='N'",
            rusqlite::params![os_no, os_year], |r| r.get(0)
        ).unwrap_or(0);
        count > 0
    } else { false };

    Json(json!({ "exists": exists, "os_no": os_no, "os_year": os_year }))
}

// ── mark-printed ──────────────────────────────────────────────────────────────

pub async fn mark_printed(State(pool): Db, _auth: AuthUser, Path((os_no, os_year)): Path<(String, i64)>) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    conn.execute(
        "UPDATE cops_master SET os_printed='Y' WHERE os_no=? AND os_year=? AND entry_deleted='N'",
        rusqlite::params![os_no, os_year],
    ).map_err(|e| e500(&e.to_string()))?;
    Ok(Json(json!({ "message": "Marked as printed." })))
}

// ── passport search (fuzzy name + DOB) ────────────────────────────────────────

pub async fn passport_search(State(pool): Db, _auth: AuthUser, Json(body): Json<serde_json::Value>) -> Result<Json<Value>, Err> {
    let query_name = body.get("pax_name").and_then(|v| v.as_str()).unwrap_or("").to_uppercase();
    let dob = body.get("pax_date_of_birth").and_then(|v| v.as_str()).unwrap_or("");
    let passport_no = body.get("passport_no").and_then(|v| v.as_str()).unwrap_or("");
    let page = body.get("page").and_then(|v| v.as_i64()).unwrap_or(1).max(1);
    let per_page = body.get("per_page").and_then(|v| v.as_i64()).unwrap_or(20).clamp(1, 100);

    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;

    // Build filter with parameterized placeholders
    let mut conditions = vec!["entry_deleted='N'".to_string()];
    let mut sql_params: Vec<String> = Vec::new();
    if !dob.is_empty() {
        conditions.push("pax_date_of_birth = ?".to_string());
        sql_params.push(dob.to_string());
    }
    if !passport_no.is_empty() {
        conditions.push("passport_no LIKE ?".to_string());
        sql_params.push(format!("%{}%", passport_no));
    }

    let where_sql = conditions.join(" AND ");
    let sql = format!(
        "SELECT os_no, os_date, os_year, pax_name, passport_no, pax_date_of_birth,
                flight_no, adjudication_date, adj_offr_name, total_payable
         FROM cops_master WHERE {where_sql}
         ORDER BY os_date DESC LIMIT 500"
    );

    let mut stmt = conn.prepare(&sql).map_err(|e| e500(&e.to_string()))?;
    let mut rows: Vec<Value> = stmt.query_map(rusqlite::params_from_iter(sql_params.iter()), |r| {
        Ok(json!({
            "os_no":              r.get::<_, String>(0)?,
            "os_date":            r.get::<_, Option<String>>(1)?,
            "os_year":            r.get::<_, Option<i64>>(2)?,
            "pax_name":           r.get::<_, Option<String>>(3)?,
            "passport_no":        r.get::<_, Option<String>>(4)?,
            "pax_date_of_birth":  r.get::<_, Option<String>>(5)?,
            "flight_no":          r.get::<_, Option<String>>(6)?,
            "adjudication_date":  r.get::<_, Option<String>>(7)?,
            "adj_offr_name":      r.get::<_, Option<String>>(8)?,
            "total_payable":      r.get::<_, Option<f64>>(9)?,
        }))
    }).map_err(|e| e500(&e.to_string()))?.filter_map(|r| r.ok()).collect();

    // Apply fuzzy name filter if query_name provided (token-overlap ≥ 60%)
    if !query_name.is_empty() {
        rows.retain(|row| {
            let pax = row.get("pax_name").and_then(|v| v.as_str()).unwrap_or("").to_uppercase();
            name_score(&query_name, &pax) >= 0.6
        });
    }

    let total = rows.len() as i64;
    let offset = ((page - 1) * per_page) as usize;
    let page_rows: Vec<Value> = rows.into_iter().skip(offset).take(per_page as usize).collect();

    Ok(Json(json!({ "total": total, "page": page, "per_page": per_page, "items": page_rows })))
}

pub async fn passport_lookup_by_pp(State(pool): Db, _auth: AuthUser, Json(body): Json<serde_json::Value>) -> Result<Json<Value>, Err> {
    let passport_no = body.get("passport_no").and_then(|v| v.as_str()).unwrap_or("");
    let pax_name = body.get("pax_name").and_then(|v| v.as_str()).unwrap_or("").to_uppercase();

    if passport_no.is_empty() { return Ok(Json(json!({ "items": [] }))); }

    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    // Passport numbers are stored uppercase (normalize_passport on every save), so
    // UPPER() wrapper is redundant and defeats ix_cops_master_passport_no.
    // Use a parameterized LIKE — lets SQLite use the index for the leading characters.
    let pp_like = format!("%{}%", passport_no.to_uppercase());

    let mut stmt = conn.prepare(
        "SELECT os_no, os_date, os_year, pax_name, passport_no, pax_date_of_birth,
                pax_nationality, flight_no, adjudication_date, adj_offr_name, total_payable
         FROM cops_master WHERE passport_no LIKE ? AND entry_deleted='N'
         ORDER BY os_date DESC LIMIT 50"
    ).map_err(|e| e500(&e.to_string()))?;

    let mut rows: Vec<Value> = stmt.query_map(rusqlite::params![pp_like], |r| {
        Ok(json!({
            "os_no":              r.get::<_, String>(0)?,
            "os_date":            r.get::<_, Option<String>>(1)?,
            "os_year":            r.get::<_, Option<i64>>(2)?,
            "pax_name":           r.get::<_, Option<String>>(3)?,
            "passport_no":        r.get::<_, Option<String>>(4)?,
            "pax_date_of_birth":  r.get::<_, Option<String>>(5)?,
            "pax_nationality":    r.get::<_, Option<String>>(6)?,
            "flight_no":          r.get::<_, Option<String>>(7)?,
            "adjudication_date":  r.get::<_, Option<String>>(8)?,
            "adj_offr_name":      r.get::<_, Option<String>>(9)?,
            "total_payable":      r.get::<_, Option<f64>>(10)?,
        }))
    }).map_err(|e| e500(&e.to_string()))?.filter_map(|r| r.ok()).collect();

    if !pax_name.is_empty() {
        rows.retain(|row| {
            let pax = row.get("pax_name").and_then(|v| v.as_str()).unwrap_or("").to_uppercase();
            name_score(&pax_name, &pax) >= 0.6
        });
    }

    Ok(Json(json!({ "items": rows })))
}

// ── Token-overlap name matching (mirrors cops1 _name_score) ──────────────────

fn name_score(a: &str, b: &str) -> f64 {
    if a.is_empty() || b.is_empty() { return 0.0; }
    let tokens_a: std::collections::HashSet<&str> = a.split_whitespace().collect();
    let tokens_b: std::collections::HashSet<&str> = b.split_whitespace().collect();
    let common = tokens_a.intersection(&tokens_b).count();
    let max_len = tokens_a.len().max(tokens_b.len());
    if max_len == 0 { 0.0 } else { common as f64 / max_len as f64 }
}

trait YearExt { fn year(&self) -> i64; fn month(&self) -> u32; }
impl YearExt for chrono::DateTime<chrono::Local> {
    fn year(&self) -> i64 { chrono::Datelike::year(self) as i64 }
    fn month(&self) -> u32 { chrono::Datelike::month(self) }
}
