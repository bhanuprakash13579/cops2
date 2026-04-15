/// APIS module — passenger manifest upload + fuzzy name matching against COPS DB
///
/// POST /apis/match  — upload Excel/CSV manifest, returns results grouped by passenger
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

    let results = match_passengers(&conn, &passengers)?;

    let matched_passengers = results.len() as i64;
    let total_cases_found: i64 = results.iter()
        .map(|r| r["case_count"].as_i64().unwrap_or(0))
        .sum();

    Ok(Json(json!({
        "total_apis_passengers": passengers.len(),
        "matched_passengers":    matched_passengers,
        "total_cases_found":     total_cases_found,
        "results":               results,
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

    let results = match_passengers(&conn, &passengers)?;

    let mut csv_out = String::from(
        "Passenger Name,Passport No,DOB,Flight No,OS No,OS Year,OS Date,\
         COPS Name,Adjudication Date,Total Payable,Match Type,Match Score\n"
    );

    for pax in &results {
        let apis_name     = pax["apis_name"].as_str().unwrap_or("");
        let apis_passport = pax["apis_passport"].as_str().unwrap_or("");
        let apis_dob      = pax["apis_dob"].as_str().unwrap_or("");
        let apis_flight   = pax["apis_flight"].as_str().unwrap_or("");

        if let Some(matches) = pax["cops_matches"].as_array() {
            for m in matches {
                csv_out.push_str(&format!(
                    "{},{},{},{},{},{},{},{},{},{:.2},{},{:.2}\n",
                    csv_escape(apis_name),
                    csv_escape(apis_passport),
                    csv_escape(apis_dob),
                    csv_escape(apis_flight),
                    csv_escape(m["os_no"].as_str().unwrap_or("")),
                    m["os_year"].as_i64().unwrap_or(0),
                    csv_escape(m["os_date"].as_str().unwrap_or("")),
                    csv_escape(m["cops_name"].as_str().unwrap_or("")),
                    csv_escape(m["adjudication_date"].as_str().unwrap_or("")),
                    m["total_payable"].as_f64().unwrap_or(0.0),
                    csv_escape(m["match_type"].as_str().unwrap_or("")),
                    m["match_score"].as_f64().unwrap_or(0.0),
                ));
            }
        }
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
    sno:         usize,
    name:        String,
    passport_no: String,
    dob:         String,
    flight_no:   String,
    sched_date:  String,
    gender:      String,
    nationality: String,
    pnr:         String,
    route:       String,
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
    let sched_col   = col_idx(&headers, &["sched", "scheduled", "dep_date", "arr_date"]);
    let gender_col  = col_idx(&headers, &["gender", "sex"]);
    let nat_col     = col_idx(&headers, &["nationality", "national"]);
    let pnr_col     = col_idx(&headers, &["pnr", "booking"]);
    let route_col   = col_idx(&headers, &["route", "sector", "origin", "dest"]);

    let mut pax = Vec::new();
    for (i, result) in rdr.records().enumerate() {
        let rec = result.map_err(|e| e400(&e.to_string()))?;
        pax.push(Passenger {
            sno:         i + 1,
            name:        rec.get(name_col).unwrap_or("").trim().to_string(),
            passport_no: rec.get(pp_col).unwrap_or("").trim().to_string(),
            dob:         dob_col.and_then(|i| rec.get(i)).unwrap_or("").trim().to_string(),
            flight_no:   flight_col.and_then(|i| rec.get(i)).unwrap_or("").trim().to_string(),
            sched_date:  sched_col.and_then(|i| rec.get(i)).unwrap_or("").trim().to_string(),
            gender:      gender_col.and_then(|i| rec.get(i)).unwrap_or("").trim().to_string(),
            nationality: nat_col.and_then(|i| rec.get(i)).unwrap_or("").trim().to_string(),
            pnr:         pnr_col.and_then(|i| rec.get(i)).unwrap_or("").trim().to_string(),
            route:       route_col.and_then(|i| rec.get(i)).unwrap_or("").trim().to_string(),
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

    let find_col = |candidates: &[&str]| -> Option<usize> {
        header_row.iter().position(|c| {
            let s = c.to_string().to_lowercase();
            candidates.iter().any(|cand| s.contains(cand))
        })
    };

    let name_col   = find_col(&["name", "passenger", "pax"]).unwrap_or(0);
    let pp_col     = find_col(&["passport", "pp_no"]).unwrap_or(1);
    let dob_col    = find_col(&["dob", "birth", "date of birth"]);
    let flight_col = find_col(&["flight", "flt"]);
    let sched_col  = find_col(&["sched", "scheduled", "dep_date", "arr_date"]);
    let gender_col = find_col(&["gender", "sex"]);
    let nat_col    = find_col(&["nationality", "national"]);
    let pnr_col    = find_col(&["pnr", "booking"]);
    let route_col  = find_col(&["route", "sector", "origin", "dest"]);

    let cell_str = |row: &[calamine::Data], idx: usize| -> String {
        row.get(idx).map(|c| c.to_string().trim().to_string()).unwrap_or_default()
    };

    let pax: Vec<Passenger> = rows.enumerate().map(|(i, row)| Passenger {
        sno:         i + 1,
        name:        cell_str(row, name_col),
        passport_no: cell_str(row, pp_col),
        dob:         dob_col.map(|j| cell_str(row, j)).unwrap_or_default(),
        flight_no:   flight_col.map(|j| cell_str(row, j)).unwrap_or_default(),
        sched_date:  sched_col.map(|j| cell_str(row, j)).unwrap_or_default(),
        gender:      gender_col.map(|j| cell_str(row, j)).unwrap_or_default(),
        nationality: nat_col.map(|j| cell_str(row, j)).unwrap_or_default(),
        pnr:         pnr_col.map(|j| cell_str(row, j)).unwrap_or_default(),
        route:       route_col.map(|j| cell_str(row, j)).unwrap_or_default(),
    }).filter(|p| !p.name.is_empty()).collect();

    Ok(pax)
}

// ── COPS cases in memory ──────────────────────────────────────────────────────

struct CopsCase {
    id:                i64,
    os_no:             String,
    os_year:           Option<i64>,
    os_date:           Option<String>,
    pax_name:          Option<String>,
    passport_no:       Option<String>,
    pax_dob:           Option<String>,
    pax_nationality:   Option<String>,
    location_code:     Option<String>,
    flight_no:         Option<String>,
    adjudication_date: Option<String>,
    adj_offr_name:     Option<String>,
    total_items_value: Option<f64>,
    total_duty_amount: Option<f64>,
    total_payable:     Option<f64>,
}

// ── Fuzzy matching — grouped by APIS passenger ───────────────────────────────

fn match_passengers(conn: &rusqlite::Connection, passengers: &[Passenger]) -> Result<Vec<Value>, Err> {
    // Load all active non-draft COPS cases into memory (typically < 50k rows).
    let mut stmt = conn.prepare(
        "SELECT id, os_no, os_year, os_date, pax_name, passport_no, pax_date_of_birth,
                pax_nationality, location_code, flight_no,
                adjudication_date, adj_offr_name,
                total_items_value, total_duty_amount, total_payable
         FROM cops_master WHERE entry_deleted='N' AND is_draft='N'"
    ).map_err(|e| e500(&e.to_string()))?;

    let cases: Vec<CopsCase> = stmt.query_map([], |r| {
        Ok(CopsCase {
            id:                r.get(0)?,
            os_no:             r.get(1)?,
            os_year:           r.get(2)?,
            os_date:           r.get(3)?,
            pax_name:          r.get(4)?,
            passport_no:       r.get(5)?,
            pax_dob:           r.get(6)?,
            pax_nationality:   r.get(7)?,
            location_code:     r.get(8)?,
            flight_no:         r.get(9)?,
            adjudication_date: r.get(10)?,
            adj_offr_name:     r.get(11)?,
            total_items_value: r.get(12)?,
            total_duty_amount: r.get(13)?,
            total_payable:     r.get(14)?,
        })
    }).map_err(|e| e500(&e.to_string()))?.filter_map(|r| r.ok()).collect();

    let mut results: Vec<Value> = Vec::new();

    for pax in passengers {
        let query_name = pax.name.to_uppercase();
        let query_pp   = pax.passport_no.to_uppercase();
        let query_dob  = pax.dob.trim();

        let mut cops_matches: Vec<Value> = Vec::new();

        for case in &cases {
            let case_name = case.pax_name.as_deref().unwrap_or("").to_uppercase();
            let case_pp   = case.passport_no.as_deref().unwrap_or("").to_uppercase();
            let case_dob  = case.pax_dob.as_deref().unwrap_or("");

            // ── Scoring ──
            let passport_hit = !query_pp.is_empty() && !case_pp.is_empty() && query_pp == case_pp;
            let dob_hit      = !query_dob.is_empty() && query_dob == case_dob;
            let name_sc      = name_score(&query_name, &case_name);

            let (score, match_type) = if passport_hit {
                // Exact passport → high confidence; DOB bonus
                let s = if dob_hit { 1.0f64 } else { 0.95 };
                (s, "PASSPORT")
            } else if dob_hit && name_sc >= 0.5 {
                // DOB + name overlap → possible changed/different passport
                (0.75 + name_sc * 0.2, "DOB_NAME")
            } else if name_sc >= 0.75 {
                // Very strong name overlap alone
                (name_sc * 0.85, "DOB_NAME")
            } else {
                continue; // below threshold
            };

            if score < 0.6 { continue; }

            // Load items for this case
            let items = load_case_items(conn, &case.os_no, case.os_year.unwrap_or(0))
                .unwrap_or_default();

            cops_matches.push(json!({
                "cops_id":           case.id,
                "cops_name":         case.pax_name,
                "cops_passport":     case.passport_no,
                "cops_dob":          case.pax_dob,
                "cops_nationality":  case.pax_nationality,
                "match_type":        match_type,
                "match_score":       (score * 100.0).round() / 100.0,
                "os_no":             case.os_no,
                "os_year":           case.os_year,
                "os_date":           case.os_date,
                "location_code":     case.location_code,
                "flight_no":         case.flight_no,
                "adjudication_date": case.adjudication_date,
                "adj_offr_name":     case.adj_offr_name,
                "total_items_value": case.total_items_value.unwrap_or(0.0),
                "total_duty_amount": case.total_duty_amount.unwrap_or(0.0),
                "total_payable":     case.total_payable.unwrap_or(0.0),
                "items":             items,
            }));
        }

        if cops_matches.is_empty() { continue; }

        // Sort matches by score descending
        cops_matches.sort_by(|a, b| {
            let sa = a["match_score"].as_f64().unwrap_or(0.0);
            let sb = b["match_score"].as_f64().unwrap_or(0.0);
            sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
        });

        let case_count = cops_matches.len() as i64;
        results.push(json!({
            "sno":              pax.sno,
            "apis_name":        pax.name,
            "apis_passport":    pax.passport_no,
            "apis_dob":         pax.dob,
            "apis_flight":      pax.flight_no,
            "apis_sched_date":  pax.sched_date,
            "apis_gender":      pax.gender,
            "apis_nationality": pax.nationality,
            "apis_pnr":         pax.pnr,
            "apis_route":       pax.route,
            "case_count":       case_count,
            "cops_matches":     cops_matches,
        }));
    }

    Ok(results)
}

// ── Load seized items for a single COPS case ─────────────────────────────────

fn load_case_items(conn: &rusqlite::Connection, os_no: &str, os_year: i64) -> rusqlite::Result<Vec<Value>> {
    let mut stmt = conn.prepare(
        "SELECT items_sno, items_desc, items_qty, items_uqc, items_value, items_duty
         FROM cops_items WHERE os_no=? AND os_year=? AND entry_deleted='N'
         ORDER BY items_sno"
    )?;
    let rows: Vec<Value> = stmt.query_map(rusqlite::params![os_no, os_year], |r| {
        Ok(json!({
            "sno":   r.get::<_, Option<i64>>(0)?,
            "desc":  r.get::<_, Option<String>>(1)?,
            "qty":   r.get::<_, Option<f64>>(2)?.unwrap_or(0.0),
            "uqc":   r.get::<_, Option<String>>(3)?,
            "value": r.get::<_, Option<f64>>(4)?.unwrap_or(0.0),
            "duty":  r.get::<_, Option<f64>>(5)?.unwrap_or(0.0),
        }))
    })?.filter_map(|r| r.ok()).collect();
    Ok(rows)
}

// ── Name similarity (Jaccard token overlap) ───────────────────────────────────

fn name_score(a: &str, b: &str) -> f64 {
    if a.is_empty() || b.is_empty() { return 0.0; }
    let ta: std::collections::HashSet<&str> = a.split_whitespace().collect();
    let tb: std::collections::HashSet<&str> = b.split_whitespace().collect();
    let common  = ta.intersection(&tb).count();
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
