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

    let pending: i64 = conn.query_row(
        "SELECT COUNT(*) FROM cops_master WHERE entry_deleted='N' AND is_draft='N'
         AND adjudication_date IS NULL AND adj_offr_name IS NULL
         AND (quashed IS NULL OR quashed != 'Y') AND (rejected IS NULL OR rejected != 'Y')
         AND (is_offline_adjudication IS NULL OR is_offline_adjudication != 'Y')
         AND (is_legacy IS NULL OR is_legacy != 'Y')",
        [], |r| r.get(0)
    ).unwrap_or(0);

    let offline_pending: i64 = conn.query_row(
        "SELECT COUNT(*) FROM cops_master WHERE entry_deleted='N' AND is_draft='N'
         AND is_offline_adjudication='Y' AND adj_offr_name IS NULL",
        [], |r| r.get(0)
    ).unwrap_or(0);

    Ok(Json(json!({ "pending": pending, "offline_pending": offline_pending })))
}

pub async fn item_descriptions(State(pool): Db) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let mut stmt = conn.prepare(
        "SELECT DISTINCT UPPER(items_desc) FROM cops_items
         WHERE items_desc IS NOT NULL AND items_desc != '' AND entry_deleted='N'
         GROUP BY UPPER(items_desc) ORDER BY COUNT(*) DESC LIMIT 300"
    ).map_err(|e| e500(&e.to_string()))?;
    let descs: Vec<String> = stmt.query_map([], |r| r.get(0))
        .map_err(|e| e500(&e.to_string()))?
        .filter_map(|r| r.ok()).collect();
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

    let (count_sql, list_sql, search_params) = build_list_query(status, &search_filter, year_filter, offset, per_page, auth.0.is_sdo());

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

fn build_list_query(status: &str, search: &str, year: Option<i64>, offset: i64, per_page: i64, _is_sdo: bool) -> (String, String, Vec<String>) {
    let (search_sql, search_params): (&str, Vec<String>) = if search.is_empty() {
        ("1=1", vec![])
    } else {
        let p = format!("%{}%", search);
        ("(os_no LIKE ? OR pax_name LIKE ? OR passport_no LIKE ? OR flight_no LIKE ?)",
         vec![p.clone(), p.clone(), p.clone(), p])
    };
    // year is an i64 — safe to format directly as a numeric literal
    let year_sql = year.map_or("1=1".to_string(), |y| format!("os_year = {y}"));

    let base = match status {
        "draft"       => "entry_deleted='N' AND is_draft='Y'".to_string(),
        "adjudicated" => "entry_deleted='N' AND is_draft='N' AND adjudication_date IS NOT NULL AND adj_offr_name IS NOT NULL".to_string(),
        "offline-pending" => "entry_deleted='N' AND is_draft='N' AND is_offline_adjudication='Y' AND adj_offr_name IS NULL".to_string(),
        _             => "entry_deleted='N' AND is_draft='N' AND adjudication_date IS NULL AND adj_offr_name IS NULL AND (quashed IS NULL OR quashed!='Y') AND (rejected IS NULL OR rejected!='Y') AND (is_offline_adjudication IS NULL OR is_offline_adjudication!='Y') AND (is_legacy IS NULL OR is_legacy!='Y')".to_string(),
    };

    let where_clause = format!("{base} AND {search_sql} AND {year_sql}");
    let cols = "id, os_no, os_date, os_year, pax_name, passport_no, flight_no, total_items_value, total_payable, adjudication_date, adj_offr_name, is_draft, is_offline_adjudication, entry_deleted, online_adjn, closure_ind, adjudication_time, post_adj_br_entries, post_adj_dr_no, booked_by, location_code, total_items";
    let count_sql = format!("SELECT COUNT(*) FROM cops_master WHERE {where_clause}");
    let list_sql  = format!("SELECT {cols} FROM cops_master WHERE {where_clause} ORDER BY os_date DESC, os_no DESC LIMIT {per_page} OFFSET {offset}");
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

    // Recalculate totals from saved items
    recalc_totals(&conn, &req.os_no, os_year).map_err(|e| e500(&e.to_string()))?;

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

    let case_json = get_case_json(&conn, &req.os_no, os_year).map_err(|e| e500(&e.to_string()))?;
    Ok(Json(case_json))
}

pub async fn update_os(State(pool): Db, auth: AuthUser, Path((os_no, os_year)): Path<(String, i64)>, Json(mut req): Json<CreateOsRequest>) -> Result<Json<Value>, Err> {
    req.passport_no = normalize_passport(req.passport_no);
    validate_flight_date(req.flight_date.as_deref())?;
    validate_pax_dates(req.pax_date_of_birth.as_deref(), req.date_of_departure.as_deref())?;
    validate_supdt_remarks_length(req.supdts_remarks.as_deref())?;

    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;

    let (adj_date, adj_time, is_draft_db): (Option<String>, Option<String>, Option<String>) = conn.query_row(
        "SELECT adjudication_date, adjudication_time, is_draft FROM cops_master WHERE os_no=? AND os_year=? AND entry_deleted='N'",
        rusqlite::params![os_no, os_year], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))
    ).map_err(|_| e404("O.S. not found"))?;

    // Role/window checks
    if auth.0.is_adjn() {
        if adj_date.is_some() && !within_edit_window(&adj_time) {
            return Err(e400("Modification window has expired (24h from adjudication)."));
        }
    } else if !auth.0.is_sdo() {
        return Err((StatusCode::FORBIDDEN, Json(json!({ "detail": "Permission denied." }))));
    }

    // If adjudicated, reset adjudication fields
    if adj_date.is_some() {
        conn.execute(
            "UPDATE cops_master SET online_adjn='N', adjudication_date=NULL, adjudication_time=NULL,
             adj_offr_name=NULL, adj_offr_designation=NULL, adjn_offr_remarks=NULL,
             os_printed='N', confiscated_value=0, redeemed_value=0, re_export_value=0,
             rf_amount=0, pp_amount=0, ref_amount=0, total_payable=0, closure_ind=NULL
             WHERE os_no=? AND os_year=?",
            rusqlite::params![os_no, os_year],
        ).map_err(|e| e500(&e.to_string()))?;
    }

    let is_draft = req.is_draft.as_deref().unwrap_or(is_draft_db.as_deref().unwrap_or("N"));

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

    let case_json = get_case_json(&conn, &os_no, os_year).map_err(|e| e500(&e.to_string()))?;
    Ok(Json(case_json))
}

pub async fn delete_os(State(pool): Db, auth: AuthUser, Path((os_no, os_year)): Path<(String, i64)>, Query(params): Query<std::collections::HashMap<String, String>>) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let reason = params.get("reason").map(|s| s.as_str()).unwrap_or("").trim().to_string();
    if reason.len() < 5 { return Err(e400("Reason must be at least 5 characters.")); }

    let (adj_date,): (Option<String>,) = conn.query_row(
        "SELECT adjudication_date FROM cops_master WHERE os_no=? AND os_year=? AND entry_deleted='N'",
        rusqlite::params![os_no, os_year], |r| Ok((r.get(0)?,))
    ).map_err(|_| e404("O.S. not found"))?;

    if adj_date.is_some() { return Err(e400("Cannot delete an adjudicated case via this route.")); }

    // Archive snapshot
    conn.execute(
        "INSERT INTO cops_master_deleted SELECT *, NULL, NULL, NULL FROM cops_master WHERE os_no=? AND os_year=? AND entry_deleted='N'",
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
         adjudication_time=?, adjn_offr_remarks=?, rf_amount=?, pp_amount=?, ref_amount=?,
         confiscated_value=?, redeemed_value=?, re_export_value=?, total_payable=?,
         online_adjn='Y', closure_ind=?
         WHERE os_no=? AND os_year=? AND entry_deleted='N'",
        rusqlite::params![
            req.adj_offr_name, desig, adj_date_val, stamp_time,
            req.adjn_offr_remarks, rf, pp, refv,
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

    conn.execute(
        "UPDATE cops_master SET adj_offr_name=?, adj_offr_designation=?, adjudication_date=?,
         adjudication_time=?, adjn_offr_remarks=?, rf_amount=?, pp_amount=?, ref_amount=?,
         confiscated_value=?, redeemed_value=?, re_export_value=?, closure_ind=?
         WHERE os_no=? AND os_year=? AND entry_deleted='N'",
        rusqlite::params![
            req.adj_offr_name, req.adj_offr_designation,
            req.adjudication_date.unwrap_or(today), now_ts,
            req.adjn_offr_remarks, req.rf_amount.unwrap_or(0.0), req.pp_amount.unwrap_or(0.0),
            req.ref_amount.unwrap_or(0.0), req.confiscated_value.unwrap_or(0.0),
            req.redeemed_value.unwrap_or(0.0), req.re_export_value.unwrap_or(0.0),
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

    conn.execute("DELETE FROM cops_items WHERE os_no=? AND os_year=?", rusqlite::params![os_no, os_year])
        .map_err(|e| e500(&e.to_string()))?;
    conn.execute("DELETE FROM cops_master WHERE os_no=? AND os_year=?", rusqlite::params![os_no, os_year])
        .map_err(|e| e500(&e.to_string()))?;

    Ok(Json(json!({ "message": "Case permanently deleted.", "os_no": os_no, "os_year": os_year })))
}

pub async fn post_adj(State(pool): Db, auth: AuthUser, Path((os_no, os_year)): Path<(String, i64)>, Json(req): Json<PostAdjRequest>) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
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

pub async fn query_search(State(pool): Db, auth: AuthUser, Json(body): Json<serde_json::Value>) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let search = body.get("search").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    let page = body.get("page").and_then(|v| v.as_i64()).unwrap_or(1);
    let per_page = body.get("per_page").and_then(|v| v.as_i64()).unwrap_or(20).clamp(1, 100);
    let offset = (page - 1) * per_page;

    let (where_clause, search_params): (&str, Vec<String>) = if search.is_empty() {
        ("1=1", vec![])
    } else {
        let p = format!("%{}%", search);
        ("(os_no LIKE ? OR pax_name LIKE ? OR passport_no LIKE ? OR flight_no LIKE ? OR adj_offr_name LIKE ?)",
         vec![p.clone(), p.clone(), p.clone(), p.clone(), p])
    };

    let total: i64 = conn.query_row(
        &format!("SELECT COUNT(*) FROM cops_master WHERE {where_clause}"),
        rusqlite::params_from_iter(search_params.iter()),
        |r| r.get(0)
    ).unwrap_or(0);

    let mut stmt = conn.prepare(&format!(
        "SELECT id, os_no, os_date, os_year, pax_name, passport_no, flight_no, total_items_value,
         total_payable, adjudication_date, adj_offr_name, is_draft, online_adjn, entry_deleted,
         post_adj_br_entries, post_adj_dr_no, post_adj_dr_date, total_duty_amount, closure_ind
         FROM cops_master WHERE {where_clause}
         ORDER BY os_date DESC LIMIT {per_page} OFFSET {offset}"
    )).map_err(|e| e500(&e.to_string()))?;

    let rows: Vec<Value> = stmt.query_map(rusqlite::params_from_iter(search_params.iter()), |r| {
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
            "online_adjn": r.get::<_, Option<String>>(12)?,
            "entry_deleted": r.get::<_, Option<String>>(13)?,
            "post_adj_br_entries": r.get::<_, Option<String>>(14)?,
            "post_adj_dr_no": r.get::<_, Option<String>>(15)?,
            "post_adj_dr_date": r.get::<_, Option<String>>(16)?,
            "total_duty_amount": r.get::<_, Option<f64>>(17)?,
            "closure_ind": r.get::<_, Option<String>>(18)?,
        }))
    }).map_err(|e| e500(&e.to_string()))?.filter_map(|r| r.ok()).collect();

    Ok(Json(json!({ "total": total, "page": page, "per_page": per_page, "items": rows })))
}

pub async fn monthly_report(State(pool): Db, auth: AuthUser, Query(params): Query<std::collections::HashMap<String, String>>) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let month = params.get("month").and_then(|v| v.parse::<i64>().ok()).unwrap_or(chrono::Local::now().month() as i64);
    let year  = params.get("year").and_then(|v| v.parse::<i64>().ok()).unwrap_or(chrono::Local::now().year() as i64);

    let from = format!("{year}-{month:02}-01");
    let to   = format!("{year}-{month:02}-31");

    let mut stmt = conn.prepare(
        "SELECT os_no, os_date, pax_name, passport_no, flight_no, total_items_value,
         total_duty_amount, total_payable, adj_offr_name, adjudication_date, entry_deleted, is_draft
         FROM cops_master WHERE os_date >= ? AND os_date <= ?
         ORDER BY os_date, os_no"
    ).map_err(|e| e500(&e.to_string()))?;

    let rows: Vec<Value> = stmt.query_map(rusqlite::params![from, to], |r| {
        Ok(json!({
            "os_no": r.get::<_, String>(0)?,
            "os_date": r.get::<_, Option<String>>(1)?,
            "pax_name": r.get::<_, Option<String>>(2)?,
            "passport_no": r.get::<_, Option<String>>(3)?,
            "flight_no": r.get::<_, Option<String>>(4)?,
            "total_items_value": r.get::<_, Option<f64>>(5)?,
            "total_duty_amount": r.get::<_, Option<f64>>(6)?,
            "total_payable": r.get::<_, Option<f64>>(7)?,
            "adj_offr_name": r.get::<_, Option<String>>(8)?,
            "adjudication_date": r.get::<_, Option<String>>(9)?,
            "entry_deleted": r.get::<_, Option<String>>(10)?,
            "is_draft": r.get::<_, Option<String>>(11)?,
        }))
    }).map_err(|e| e500(&e.to_string()))?.filter_map(|r| r.ok()).collect();

    Ok(Json(json!({ "month": month, "year": year, "items": rows })))
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

pub async fn classify_item(Query(params): Query<std::collections::HashMap<String, String>>) -> Json<Value> {
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

pub async fn check_os_no(State(pool): Db, Query(params): Query<std::collections::HashMap<String, String>>) -> Json<Value> {
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
    let pp = passport_no.replace('\'', "''").to_uppercase();

    let mut stmt = conn.prepare(&format!(
        "SELECT os_no, os_date, os_year, pax_name, passport_no, pax_date_of_birth,
                pax_nationality, flight_no, adjudication_date, adj_offr_name, total_payable
         FROM cops_master WHERE UPPER(passport_no) LIKE '%{pp}%' AND entry_deleted='N'
         ORDER BY os_date DESC LIMIT 50"
    )).map_err(|e| e500(&e.to_string()))?;

    let mut rows: Vec<Value> = stmt.query_map([], |r| {
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
