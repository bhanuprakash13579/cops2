/// CSV register reports: r4 = BR Register, r5 = OS Register, r6 = DR Register
use std::sync::Arc;
use axum::{extract::{Query, State}, http::StatusCode, Json};
use axum::response::IntoResponse;
use axum::http::header;
use serde_json::{json, Value};
use crate::{auth::AuthUser, db::DbPool};

type Db = State<Arc<DbPool>>;
type Err = (StatusCode, Json<Value>);

fn e400(m: &str) -> Err { (StatusCode::BAD_REQUEST, Json(json!({ "detail": m }))) }
fn e500(m: &str) -> Err { (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "detail": m }))) }

pub async fn generate(
    State(pool): Db,
    _auth: AuthUser,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Result<axum::response::Response, Err> {
    let report_id  = params.get("report_id").map(|s| s.as_str()).unwrap_or("");
    let start_date = params.get("start_date").map(|s| s.as_str()).unwrap_or("2000-01-01");
    let end_date   = params.get("end_date").map(|s| s.as_str()).unwrap_or("2099-12-31");

    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;

    let (csv_body, filename) = match report_id {
        "r4" => generate_br_register(&conn, start_date, end_date)?,
        "r5" => generate_os_register(&conn, start_date, end_date)?,
        "r6" => generate_dr_register(&conn, start_date, end_date)?,
        _ => return Err(e400("Invalid report_id. Use r4 (BR), r5 (OS), or r6 (DR).")),
    };

    Ok((
        [(header::CONTENT_TYPE, "text/csv; charset=utf-8"),
         (header::CONTENT_DISPOSITION, &format!("attachment; filename=\"{filename}\""))],
        csv_body,
    ).into_response())
}

fn generate_br_register(conn: &rusqlite::Connection, from: &str, to: &str) -> Result<(String, String), Err> {
    let mut stmt = conn.prepare(
        "SELECT br_no, br_year, br_date, pax_name, passport_no, pax_nationality,
                pax_date_of_birth, flight_no, flight_date, location_code, login_id,
                total_items_value, total_duty_amount, total_payable, br_printed,
                os_no, os_year
         FROM br_master WHERE br_date >= ? AND br_date <= ? AND entry_deleted='N'
         ORDER BY br_date, br_no"
    ).map_err(|e| e500(&e.to_string()))?;

    let mut csv = String::from(
        "BR No,BR Year,Date,Pax Name,Passport No,Nationality,DOB,Flight No,Flight Date,\
         Location,Booked By,Total Value,Total Duty,Total Payable,Printed,OS No,OS Year\n"
    );

    let rows = stmt.query_map(rusqlite::params![from, to], |r| {
        Ok((
            r.get::<_, String>(0)?,  r.get::<_, i64>(1)?,      r.get::<_, Option<String>>(2)?,
            r.get::<_, Option<String>>(3)?, r.get::<_, Option<String>>(4)?,
            r.get::<_, Option<String>>(5)?, r.get::<_, Option<String>>(6)?,
            r.get::<_, Option<String>>(7)?, r.get::<_, Option<String>>(8)?,
            r.get::<_, Option<String>>(9)?, r.get::<_, Option<String>>(10)?,
            r.get::<_, Option<f64>>(11)?,   r.get::<_, Option<f64>>(12)?,
            r.get::<_, Option<f64>>(13)?,   r.get::<_, Option<String>>(14)?,
            r.get::<_, Option<String>>(15)?,r.get::<_, Option<i64>>(16)?,
        ))
    }).map_err(|e| e500(&e.to_string()))?;

    for row in rows.filter_map(|r| r.ok()) {
        csv.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
            row.0, row.1, opt_s(&row.2), opt_s(&row.3), opt_s(&row.4),
            opt_s(&row.5), opt_s(&row.6), opt_s(&row.7), opt_s(&row.8),
            opt_s(&row.9), opt_s(&row.10),
            row.11.unwrap_or(0.0), row.12.unwrap_or(0.0), row.13.unwrap_or(0.0),
            opt_s(&row.14), opt_s(&row.15), row.16.unwrap_or(0),
        ));
    }

    let ts = chrono::Local::now().format("%Y%m%d");
    Ok((csv, format!("BR_Register_{from}_{to}_{ts}.csv")))
}

fn generate_os_register(conn: &rusqlite::Connection, from: &str, to: &str) -> Result<(String, String), Err> {
    let mut stmt = conn.prepare(
        "SELECT os_no, os_year, os_date, pax_name, passport_no, pax_nationality,
                pax_date_of_birth, flight_no, flight_date, location_code, booked_by,
                total_items_value, total_duty_amount, total_payable,
                adjudication_date, adj_offr_name, entry_deleted, is_draft,
                is_offline_adjudication, case_type, closure_ind
         FROM cops_master WHERE os_date >= ? AND os_date <= ?
         ORDER BY os_date, os_no"
    ).map_err(|e| e500(&e.to_string()))?;

    let mut csv = String::from(
        "OS No,OS Year,Date,Pax Name,Passport No,Nationality,DOB,Flight No,Flight Date,\
         Location,Booked By,Total Value,Total Duty,Total Payable,\
         Adjudication Date,Adj Officer,Status,Draft,Offline Adj,Case Type,Closure\n"
    );

    let rows = stmt.query_map(rusqlite::params![from, to], |r| {
        Ok((
            r.get::<_, String>(0)?,  r.get::<_, Option<i64>>(1)?,   r.get::<_, Option<String>>(2)?,
            r.get::<_, Option<String>>(3)?,  r.get::<_, Option<String>>(4)?,
            r.get::<_, Option<String>>(5)?,  r.get::<_, Option<String>>(6)?,
            r.get::<_, Option<String>>(7)?,  r.get::<_, Option<String>>(8)?,
            r.get::<_, Option<String>>(9)?,  r.get::<_, Option<String>>(10)?,
            r.get::<_, Option<f64>>(11)?,    r.get::<_, Option<f64>>(12)?,
            r.get::<_, Option<f64>>(13)?,    r.get::<_, Option<String>>(14)?,
            r.get::<_, Option<String>>(15)?, r.get::<_, Option<String>>(16)?,
            r.get::<_, Option<String>>(17)?, r.get::<_, Option<String>>(18)?,
            r.get::<_, Option<String>>(19)?, r.get::<_, Option<String>>(20)?,
        ))
    }).map_err(|e| e500(&e.to_string()))?;

    for row in rows.filter_map(|r| r.ok()) {
        let status = if row.16.as_deref() == Some("Y") { "Deleted" }
                     else if row.17.as_deref() == Some("Y") { "Draft" }
                     else if row.14.is_some() { "Adjudicated" }
                     else { "Pending" };
        csv.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
            row.0, row.1.unwrap_or(0), opt_s(&row.2), opt_s(&row.3), opt_s(&row.4),
            opt_s(&row.5), opt_s(&row.6), opt_s(&row.7), opt_s(&row.8),
            opt_s(&row.9), opt_s(&row.10),
            row.11.unwrap_or(0.0), row.12.unwrap_or(0.0), row.13.unwrap_or(0.0),
            opt_s(&row.14), opt_s(&row.15), status,
            opt_s(&row.17), opt_s(&row.18), opt_s(&row.19), opt_s(&row.20),
        ));
    }

    let ts = chrono::Local::now().format("%Y%m%d");
    Ok((csv, format!("OS_Register_{from}_{to}_{ts}.csv")))
}

fn generate_dr_register(conn: &rusqlite::Connection, from: &str, to: &str) -> Result<(String, String), Err> {
    let mut stmt = conn.prepare(
        "SELECT dr_no, dr_year, dr_date, pax_name, passport_no, pax_nationality,
                pax_date_of_birth, flight_no, flight_date, location_code, login_id,
                total_items_value, dr_printed, os_no, os_year
         FROM dr_master WHERE dr_date >= ? AND dr_date <= ? AND entry_deleted='N'
         ORDER BY dr_date, dr_no"
    ).map_err(|e| e500(&e.to_string()))?;

    let mut csv = String::from(
        "DR No,DR Year,Date,Pax Name,Passport No,Nationality,DOB,Flight No,Flight Date,\
         Location,Booked By,Total Value,Printed,OS No,OS Year\n"
    );

    let rows = stmt.query_map(rusqlite::params![from, to], |r| {
        Ok((
            r.get::<_, String>(0)?,  r.get::<_, i64>(1)?,      r.get::<_, Option<String>>(2)?,
            r.get::<_, Option<String>>(3)?,  r.get::<_, Option<String>>(4)?,
            r.get::<_, Option<String>>(5)?,  r.get::<_, Option<String>>(6)?,
            r.get::<_, Option<String>>(7)?,  r.get::<_, Option<String>>(8)?,
            r.get::<_, Option<String>>(9)?,  r.get::<_, Option<String>>(10)?,
            r.get::<_, Option<f64>>(11)?,    r.get::<_, Option<String>>(12)?,
            r.get::<_, Option<String>>(13)?, r.get::<_, Option<i64>>(14)?,
        ))
    }).map_err(|e| e500(&e.to_string()))?;

    for row in rows.filter_map(|r| r.ok()) {
        csv.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
            row.0, row.1, opt_s(&row.2), opt_s(&row.3), opt_s(&row.4),
            opt_s(&row.5), opt_s(&row.6), opt_s(&row.7), opt_s(&row.8),
            opt_s(&row.9), opt_s(&row.10),
            row.11.unwrap_or(0.0), opt_s(&row.12), opt_s(&row.13), row.14.unwrap_or(0),
        ));
    }

    let ts = chrono::Local::now().format("%Y%m%d");
    Ok((csv, format!("DR_Register_{from}_{to}_{ts}.csv")))
}

fn opt_s(v: &Option<String>) -> String {
    v.as_deref().unwrap_or("").replace(',', ";")
}
