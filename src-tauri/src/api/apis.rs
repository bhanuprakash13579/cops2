/// APIS module — passenger manifest upload + fuzzy name matching against COPS DB
///
/// POST /apis/match  — upload Excel manifest, returns matched COPS cases
/// POST /apis/export — same but returns a downloadable CSV
use std::sync::Arc;
use axum::{extract::State, http::StatusCode, Json};
use axum::extract::Multipart;
use serde_json::{json, Value};
use crate::{auth::AuthUser, db::DbPool};

type Db = State<Arc<DbPool>>;
type Err = (StatusCode, Json<Value>);

fn e400(m: &str) -> Err { (StatusCode::BAD_REQUEST,          Json(json!({ "detail": m }))) }
fn e500(m: &str) -> Err { (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "detail": m }))) }

// ── Match manifest against COPS DB ───────────────────────────────────────────

pub async fn match_manifest(
    State(pool): Db,
    _auth: AuthUser,
    mut multipart: Multipart,
) -> Result<Json<Value>, Err> {
    let passengers = parse_manifest_multipart(&mut multipart).await?;
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;

    let matches = match_passengers(&conn, &passengers)?;

    Ok(Json(json!({
        "total_passengers": passengers.len(),
        "total_matches":    matches.len(),
        "matches":          matches,
    })))
}

pub async fn export_manifest(
    State(pool): Db,
    _auth: AuthUser,
    mut multipart: Multipart,
) -> Result<axum::response::Response, Err> {
    use axum::response::IntoResponse;
    use axum::http::header;

    let passengers = parse_manifest_multipart(&mut multipart).await?;
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;

    let matches = match_passengers(&conn, &passengers)?;

    // Build CSV
    let mut csv_out = String::from("Passenger Name,Passport No,DOB,Flight No,OS No,OS Year,OS Date,Adjudication Date,Total Payable,Match Score\n");
    for m in &matches {
        csv_out.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{:.2}\n",
            csv_escape(m.get("pax_name").and_then(|v| v.as_str()).unwrap_or("")),
            csv_escape(m.get("passport_no").and_then(|v| v.as_str()).unwrap_or("")),
            csv_escape(m.get("dob").and_then(|v| v.as_str()).unwrap_or("")),
            csv_escape(m.get("flight_no").and_then(|v| v.as_str()).unwrap_or("")),
            csv_escape(m.get("os_no").and_then(|v| v.as_str()).unwrap_or("")),
            m.get("os_year").and_then(|v| v.as_i64()).unwrap_or(0),
            csv_escape(m.get("os_date").and_then(|v| v.as_str()).unwrap_or("")),
            csv_escape(m.get("adjudication_date").and_then(|v| v.as_str()).unwrap_or("")),
            m.get("total_payable").and_then(|v| v.as_f64()).unwrap_or(0.0),
            m.get("match_score").and_then(|v| v.as_f64()).unwrap_or(0.0),
        ));
    }

    let ts = chrono::Local::now().format("%Y%m%d_%H%M%S");
    Ok((
        [(header::CONTENT_TYPE, "text/csv"),
         (header::CONTENT_DISPOSITION, &format!("attachment; filename=\"apis_matches_{ts}.csv\""))],
        csv_out,
    ).into_response())
}

// ── Manifest parsing (CSV or Excel via calamine) ──────────────────────────────

struct Passenger {
    name:        String,
    passport_no: String,
    dob:         String,
    flight_no:   String,
}

async fn parse_manifest_multipart(multipart: &mut Multipart) -> Result<Vec<Passenger>, Err> {
    while let Some(field) = multipart.next_field().await.map_err(|e| e400(&e.to_string()))? {
        let name = field.name().unwrap_or("").to_string();
        if name != "file" { continue; }

        let filename = field.file_name().unwrap_or("upload.csv").to_lowercase();
        let bytes = field.bytes().await.map_err(|e| e500(&e.to_string()))?;

        if filename.ends_with(".csv") {
            return parse_csv_manifest(&bytes);
        } else if filename.ends_with(".xlsx") || filename.ends_with(".xls") {
            return parse_excel_manifest(&bytes);
        } else {
            return Err(e400("Unsupported file format. Please upload CSV or Excel file."));
        }
    }
    Err(e400("No file uploaded. Send a multipart field named 'file'."))
}

fn parse_csv_manifest(bytes: &[u8]) -> Result<Vec<Passenger>, Err> {
    let text = encoding_rs::UTF_8.decode(bytes).0.into_owned();
    let mut rdr = csv::Reader::from_reader(text.as_bytes());
    let headers = rdr.headers().map_err(|e| e400(&e.to_string()))?.clone();

    fn col_idx(headers: &csv::StringRecord, candidates: &[&str]) -> Option<usize> {
        let h: Vec<String> = headers.iter().map(|s| s.to_lowercase().trim().to_string()).collect();
        candidates.iter().find_map(|c| h.iter().position(|h| h.contains(c)))
    }

    let name_col    = col_idx(&headers, &["name", "passenger", "pax"]).unwrap_or(0);
    let pp_col      = col_idx(&headers, &["passport", "pp_no", "pp no"]).unwrap_or(1);
    let dob_col     = col_idx(&headers, &["dob", "birth", "date of birth"]);
    let flight_col  = col_idx(&headers, &["flight", "flt"]);

    let mut pax = Vec::new();
    for result in rdr.records() {
        let rec = result.map_err(|e| e400(&e.to_string()))?;
        pax.push(Passenger {
            name:        rec.get(name_col).unwrap_or("").trim().to_string(),
            passport_no: rec.get(pp_col).unwrap_or("").trim().to_string(),
            dob:         dob_col.and_then(|i| rec.get(i)).unwrap_or("").trim().to_string(),
            flight_no:   flight_col.and_then(|i| rec.get(i)).unwrap_or("").trim().to_string(),
        });
    }
    Ok(pax)
}

fn parse_excel_manifest(bytes: &[u8]) -> Result<Vec<Passenger>, Err> {
    use calamine::{open_workbook_from_rs, Reader, Xlsx};
    use std::io::Cursor;

    let cursor = Cursor::new(bytes);
    let mut workbook: Xlsx<_> = open_workbook_from_rs(cursor)
        .map_err(|e| e400(&format!("Cannot open Excel file: {e}")))?;

    let sheet_name = workbook.sheet_names().first()
        .cloned()
        .ok_or_else(|| e400("Excel file has no sheets"))?;

    let range = workbook.worksheet_range(&sheet_name)
        .map_err(|e| e400(&format!("Cannot read sheet: {e}")))?;

    let mut rows = range.rows();
    let header_row = rows.next().ok_or_else(|| e400("Empty Excel file"))?;

    let find_col = |headers: &[calamine::Data], candidates: &[&str]| -> Option<usize> {
        headers.iter().position(|c| {
            let s = c.to_string().to_lowercase();
            candidates.iter().any(|cand| s.contains(cand))
        })
    };

    let name_col   = find_col(header_row, &["name", "passenger", "pax"]).unwrap_or(0);
    let pp_col     = find_col(header_row, &["passport", "pp_no"]).unwrap_or(1);
    let dob_col    = find_col(header_row, &["dob", "birth"]);
    let flight_col = find_col(header_row, &["flight", "flt"]);

    let cell_str = |row: &[calamine::Data], idx: usize| -> String {
        row.get(idx).map(|c| c.to_string().trim().to_string()).unwrap_or_default()
    };

    let pax: Vec<Passenger> = rows.map(|row| Passenger {
        name:        cell_str(row, name_col),
        passport_no: cell_str(row, pp_col),
        dob:         dob_col.map(|i| cell_str(row, i)).unwrap_or_default(),
        flight_no:   flight_col.map(|i| cell_str(row, i)).unwrap_or_default(),
    }).filter(|p| !p.name.is_empty()).collect();

    Ok(pax)
}

// ── Fuzzy matching against COPS DB ────────────────────────────────────────────

fn match_passengers(conn: &rusqlite::Connection, passengers: &[Passenger]) -> Result<Vec<Value>, Err> {
    // Fetch all COPS cases (name, passport, DOB) into memory — typically < 50k rows
    let mut stmt = conn.prepare(
        "SELECT os_no, os_year, os_date, pax_name, passport_no, pax_date_of_birth,
                flight_no, adjudication_date, total_payable
         FROM cops_master WHERE entry_deleted='N' AND is_draft='N'"
    ).map_err(|e| e500(&e.to_string()))?;

    struct CopsCase {
        os_no:             String,
        os_year:           Option<i64>,
        os_date:           Option<String>,
        pax_name:          Option<String>,
        passport_no:       Option<String>,
        pax_dob:           Option<String>,
        flight_no:         Option<String>,
        adjudication_date: Option<String>,
        total_payable:     Option<f64>,
    }

    let cases: Vec<CopsCase> = stmt.query_map([], |r| {
        Ok(CopsCase {
            os_no:             r.get(0)?,
            os_year:           r.get(1)?,
            os_date:           r.get(2)?,
            pax_name:          r.get(3)?,
            passport_no:       r.get(4)?,
            pax_dob:           r.get(5)?,
            flight_no:         r.get(6)?,
            adjudication_date: r.get(7)?,
            total_payable:     r.get(8)?,
        })
    }).map_err(|e| e500(&e.to_string()))?.filter_map(|r| r.ok()).collect();

    let mut matches = Vec::new();

    for pax in passengers {
        let query_name = pax.name.to_uppercase();
        let query_pp   = pax.passport_no.to_uppercase();
        let query_dob  = &pax.dob;

        for case in &cases {
            let case_name = case.pax_name.as_deref().unwrap_or("").to_uppercase();
            let case_pp   = case.passport_no.as_deref().unwrap_or("").to_uppercase();
            let case_dob  = case.pax_dob.as_deref().unwrap_or("");

            let mut score: f64 = 0.0;

            // Passport exact match → very high confidence
            if !query_pp.is_empty() && !case_pp.is_empty() && query_pp == case_pp {
                score = 0.95;
            }

            // DOB exact match bonus
            if !query_dob.is_empty() && query_dob == case_dob {
                score += 0.3;
            }

            // Name token overlap
            let name_sc = name_score(&query_name, &case_name);
            score = score.max(name_sc);

            if score >= 0.6 {
                matches.push(json!({
                    "pax_name":         pax.name,
                    "passport_no":      pax.passport_no,
                    "dob":              pax.dob,
                    "flight_no":        pax.flight_no,
                    "os_no":            case.os_no,
                    "os_year":          case.os_year,
                    "os_date":          case.os_date,
                    "case_pax_name":    case.pax_name,
                    "case_passport_no": case.passport_no,
                    "adjudication_date":case.adjudication_date,
                    "total_payable":    case.total_payable,
                    "match_score":      (score * 100.0).round() / 100.0,
                }));
            }
        }
    }

    // Sort by score descending
    matches.sort_by(|a, b| {
        let sa = a.get("match_score").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let sb = b.get("match_score").and_then(|v| v.as_f64()).unwrap_or(0.0);
        sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
    });

    Ok(matches)
}

fn name_score(a: &str, b: &str) -> f64 {
    if a.is_empty() || b.is_empty() { return 0.0; }
    let ta: std::collections::HashSet<&str> = a.split_whitespace().collect();
    let tb: std::collections::HashSet<&str> = b.split_whitespace().collect();
    let common = ta.intersection(&tb).count();
    let max_len = ta.len().max(tb.len());
    if max_len == 0 { 0.0 } else { common as f64 / max_len as f64 }
}

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}
