use std::{io::{Cursor, Read, Write}, sync::Arc, time::Duration};
use axum::{
    body::Bytes,
    extract::{Multipart, Query, State},
    http::{header, StatusCode},
    response::IntoResponse,
    Json,
};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use zip::{write::SimpleFileOptions, CompressionMethod};

use crate::{auth::{AdjnUser, AdminUser, AuthUser}, db::DbPool};

type Db = State<Arc<DbPool>>;
type Err = (StatusCode, Json<Value>);

fn e400(m: &str) -> Err { (StatusCode::BAD_REQUEST, Json(json!({ "detail": m }))) }
fn e500(m: &str) -> Err { (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "detail": m }))) }

// ── Column lists ──────────────────────────────────────────────────────────────

const MASTER_COLS: &[&str] = &[
    "os_no","os_year","os_date","location_code","shift","booked_by","case_type",
    "pax_name","pax_name_modified_by_vig","pax_nationality","passport_no","passport_date",
    "pp_issue_place","pax_address1","pax_address2","pax_address3","pax_date_of_birth",
    "pax_status","residence_at","country_of_departure","port_of_dep_dest",
    "date_of_departure","stay_abroad_days","father_name","old_passport_no","previous_visits",
    "flight_no","flight_date",
    "total_items","total_items_value","total_fa_value","dutiable_value",
    "redeemed_value","re_export_value","confiscated_value",
    "total_duty_amount","rf_amount","pp_amount","ref_amount",
    "br_amount","wh_amount","other_amount","total_payable",
    "br_no_str","br_no_num","br_date_str","br_amount_str",
    "is_draft","is_legacy","is_offline_adjudication","file_spot",
    "os_printed","os_category","online_os",
    "adjudication_date","adjudication_time","adj_offr_name","adj_offr_designation",
    "adjn_offr_remarks","adjn_offr_remarks1","adjn_section_ref","online_adjn",
    "supdts_remarks","supdt_remarks2",
    "unique_no","entry_deleted","bkup_taken",
    "detained_by","seal_no","nationality","seizure_date",
    "dr_no","dr_year","total_drs","previous_os_details","total_pkgs","closure_ind",
    "post_adj_br_entries","post_adj_dr_no","post_adj_dr_date",
    "deleted_by","deleted_reason","deleted_on","quashed","rejected",
];

const ITEMS_COLS: &[&str] = &[
    "os_no","os_year","items_sno","items_desc","items_qty","items_uqc",
    "value_per_piece","items_value","items_fa","cumulative_duty_rate","items_duty",
    "items_duty_type","items_category","items_sub_category","items_release_category",
    "items_dr_no","items_dr_year","items_fa_type","items_fa_qty","items_fa_uqc",
    "unique_no","entry_deleted",
];

// ── CSV helpers ───────────────────────────────────────────────────────────────

fn val_to_str(v: rusqlite::types::ValueRef<'_>) -> String {
    match v {
        rusqlite::types::ValueRef::Null => String::new(),
        rusqlite::types::ValueRef::Integer(n) => n.to_string(),
        rusqlite::types::ValueRef::Real(f) => {
            let s = format!("{f:.6}");
            s.trim_end_matches('0').trim_end_matches('.').to_string()
        }
        rusqlite::types::ValueRef::Text(t) => String::from_utf8_lossy(t).into_owned(),
        rusqlite::types::ValueRef::Blob(b) => String::from_utf8_lossy(b).into_owned(),
    }
}

fn query_to_csv(conn: &rusqlite::Connection, sql: &str, headers: &[&str]) -> rusqlite::Result<Vec<u8>> {
    let mut buf = Vec::new();
    {
        let mut wtr = csv::Writer::from_writer(&mut buf);
        wtr.write_record(headers).ok();
        let mut stmt = conn.prepare(sql)?;
        let col_count = headers.len();
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let record: Vec<String> = (0..col_count)
                .map(|i| val_to_str(row.get_ref(i).unwrap_or(rusqlite::types::ValueRef::Null)))
                .collect();
            wtr.write_record(&record).ok();
        }
        wtr.flush().ok();
    }
    Ok(buf)
}

fn db_path(conn: &rusqlite::Connection) -> rusqlite::Result<String> {
    conn.query_row("SELECT file FROM pragma_database_list WHERE seq=0", [], |r| r.get(0))
}

fn post_import_optimise(conn: &rusqlite::Connection) {
    let _ = conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS ix_cops_master_os_no_year ON cops_master (os_no, os_year);
         CREATE INDEX IF NOT EXISTS ix_cops_master_draft_deleted ON cops_master (entry_deleted, is_draft);
         CREATE INDEX IF NOT EXISTS ix_cops_master_adjudication_date ON cops_master (adjudication_date);
         CREATE INDEX IF NOT EXISTS ix_cops_master_os_year ON cops_master (os_year);
         CREATE INDEX IF NOT EXISTS ix_cops_master_adj_offr_name ON cops_master (adj_offr_name);
         CREATE INDEX IF NOT EXISTS ix_cops_master_pending ON cops_master (entry_deleted, is_draft, adjudication_date, adj_offr_name);
         CREATE INDEX IF NOT EXISTS ix_cops_items_os_no_year ON cops_items (os_no, os_year);
         ANALYZE cops_master; ANALYZE cops_items;",
    );
}

fn parse_float(s: &str) -> f64 {
    s.trim().parse::<f64>().unwrap_or(0.0)
}

fn parse_date(s: &str) -> String {
    let s = s.trim().trim_matches('"');
    // Try YYYY-MM-DD first
    if s.len() == 10 && s.as_bytes()[4] == b'-' { return s.to_string(); }
    // Try M/D/YY and M/D/YYYY
    for fmt in &["%m/%d/%y", "%m/%d/%Y", "%d/%m/%Y"] {
        if let Ok(d) = chrono::NaiveDate::parse_from_str(s, fmt) {
            return d.format("%Y-%m-%d").to_string();
        }
    }
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

// ── Export CSV (full database → ZIP) ─────────────────────────────────────────

async fn inner_export_csv(pool: Arc<DbPool>) -> Result<impl IntoResponse, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let master_sql = format!("SELECT {} FROM cops_master ORDER BY os_date, os_no", MASTER_COLS.join(","));
    let items_sql  = format!("SELECT {} FROM cops_items  ORDER BY os_no, os_year, items_sno", ITEMS_COLS.join(","));

    let master_csv = query_to_csv(&conn, &master_sql, MASTER_COLS).map_err(|e| e500(&e.to_string()))?;
    let items_csv  = query_to_csv(&conn, &items_sql,  ITEMS_COLS).map_err(|e| e500(&e.to_string()))?;

    let mut zip_buf = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(Cursor::new(&mut zip_buf));
        let opts = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .with_aes_encryption(zip::AesMode::Aes256, crate::security::zip_password());
        zip.start_file("cops_master.csv", opts).map_err(|e| e500(&e.to_string()))?;
        zip.write_all(&master_csv).map_err(|e| e500(&e.to_string()))?;
        let opts = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .with_aes_encryption(zip::AesMode::Aes256, crate::security::zip_password());
        zip.start_file("cops_items.csv", opts).map_err(|e| e500(&e.to_string()))?;
        zip.write_all(&items_csv).map_err(|e| e500(&e.to_string()))?;
        zip.finish().map_err(|e| e500(&e.to_string()))?;
    }

    let today = chrono::Local::now().format("%Y-%m-%d");
    let filename = format!("cops_full_backup_{today}.zip");
    Ok((
        [
            (header::CONTENT_TYPE, "application/zip".to_string()),
            (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{filename}\"")),
        ],
        zip_buf,
    ))
}

pub async fn export_csv(State(pool): Db, _auth: AuthUser) -> Result<impl IntoResponse, Err> {
    inner_export_csv(pool).await
}

pub async fn admin_export_csv(State(pool): Db, _admin: AdminUser) -> Result<impl IntoResponse, Err> {
    inner_export_csv(pool).await
}

// ── Export DB (SQLite binary backup) ─────────────────────────────────────────

async fn inner_export_db(pool: Arc<DbPool>) -> Result<impl IntoResponse, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let src_path = db_path(&conn).map_err(|e| e500(&e.to_string()))?;
    drop(conn); // release before backup

    let tmp_path = std::env::temp_dir().join(format!("cops2_backup_{}.db", uuid::Uuid::new_v4()));

    // rusqlite backup: src → tmp copy
    {
        let src = rusqlite::Connection::open(&src_path).map_err(|e| e500(&e.to_string()))?;
        let mut dst = rusqlite::Connection::open(&tmp_path).map_err(|e| e500(&e.to_string()))?;
        let backup = rusqlite::backup::Backup::new(&src, &mut dst).map_err(|e| e500(&e.to_string()))?;
        backup.run_to_completion(5, Duration::from_millis(100), None).map_err(|e| e500(&e.to_string()))?;
    }

    let bytes = tokio::fs::read(&tmp_path).await.map_err(|e| e500(&e.to_string()))?;
    let _ = tokio::fs::remove_file(&tmp_path).await;

    let today = chrono::Local::now().format("%Y-%m-%d");
    let filename = format!("cops_fulldb_{today}.db");
    Ok((
        [
            (header::CONTENT_TYPE, "application/octet-stream".to_string()),
            (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{filename}\"")),
        ],
        bytes,
    ))
}

pub async fn export_db(State(pool): Db, _auth: AuthUser) -> Result<impl IntoResponse, Err> {
    inner_export_db(pool).await
}

pub async fn admin_export_db(State(pool): Db, _admin: AdminUser) -> Result<impl IntoResponse, Err> {
    inner_export_db(pool).await
}

// ── Restore full DB from uploaded .db file ────────────────────────────────────

pub async fn admin_restore_fulldb(
    State(pool): Db,
    _admin: AdminUser,
    mut multipart: Multipart,
) -> Result<Json<Value>, Err> {
    let mut file_bytes: Option<Vec<u8>> = None;
    while let Some(field) = multipart.next_field().await.map_err(|e| e400(&e.to_string()))? {
        if field.name().unwrap_or("") == "file" {
            file_bytes = Some(field.bytes().await.map_err(|e| e400(&e.to_string()))?.to_vec());
        }
    }
    let bytes = file_bytes.ok_or_else(|| e400("No file uploaded"))?;
    if bytes.is_empty() { return Err(e400("Empty file")); }

    // Write uploaded bytes to a temp file, then backup into main DB
    let tmp = std::env::temp_dir().join(format!("cops2_restore_{}.db", uuid::Uuid::new_v4()));
    tokio::fs::write(&tmp, &bytes).await.map_err(|e| e500(&e.to_string()))?;

    let result: Result<(), String> = tokio::task::spawn_blocking({
        let tmp = tmp.clone();
        let pool = Arc::clone(&pool);
        move || {
            let src = rusqlite::Connection::open(&tmp).map_err(|e| e.to_string())?;
            // Set the encryption key — cops2 databases are SQLCipher encrypted.
            src.execute_batch(&crate::security::sqlcipher_pragma()).map_err(|_| "Cannot unlock uploaded database — wrong password or not a cops2 database".to_string())?;
            // Verify it's a valid COPS database
            src.query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name='cops_master'",
                [], |r| r.get::<_, i64>(0))
                .map_err(|_| "Uploaded file is not a valid COPS database".to_string())?;

            let mut conn = pool.get().map_err(|e| e.to_string())?;
            let backup = rusqlite::backup::Backup::new(&src, &mut *conn).map_err(|e| e.to_string())?;
            backup.run_to_completion(5, Duration::from_millis(100), None).map_err(|e| e.to_string())
        }
    }).await.map_err(|e| e500(&e.to_string()))?;

    let _ = tokio::fs::remove_file(&tmp).await;

    result.map_err(|e| (StatusCode::BAD_REQUEST, Json(json!({ "detail": e }))))?;
    Ok(Json(json!({ "message": "Database restored successfully. Please restart the app to reload data." })))
}

// ── Legacy master CSV upload ──────────────────────────────────────────────────

pub async fn admin_upload_legacy(
    State(pool): Db,
    _admin: AdminUser,
    mut multipart: Multipart,
) -> Result<Json<Value>, Err> {
    let bytes = extract_file(&mut multipart).await?;
    let raw = decode_csv_bytes(&bytes).map_err(|e| e400(&e))?;

    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let existing = existing_os_keys(&conn).map_err(|e| e500(&e.to_string()))?;
    let mut existing = existing;

    let mut rdr = csv::ReaderBuilder::new().has_headers(true).from_reader(raw.as_bytes());
    let headers = rdr.headers().cloned().unwrap_or_default();
    let mut inserted = 0i64; let mut skipped = 0i64; let mut invalid = 0i64;

    // Bulk-load pragmas: disable fsync and use in-memory journal for the duration of import.
    // Safe here because we're inserting new rows — losing partial progress on crash is acceptable.
    conn.execute_batch(
        "PRAGMA synchronous = OFF; PRAGMA journal_mode = MEMORY;"
    ).ok();
    conn.execute_batch("BEGIN").map_err(|e| e500(&e.to_string()))?;

    for result in rdr.records() {
        let rec = match result { Ok(r) => r, Err(_) => { invalid += 1; continue; } };
        let get = |name: &str| -> &str {
            headers.iter().position(|f| f.eq_ignore_ascii_case(name))
                .and_then(|i| rec.get(i))
                .unwrap_or("")
        };

        let os_no = get("os_no").trim().to_string();
        if os_no.is_empty() { invalid += 1; continue; }
        let os_year = match get("os_year").trim().parse::<i64>() {
            Ok(y) if y > 0 => y, _ => { invalid += 1; continue; }
        };
        let location_code = get("location_code").trim().to_string();
        let key = (os_no.clone(), os_year, location_code.clone());
        if existing.contains(&key) { skipped += 1; continue; }

        let os_date = parse_date(get("os_date"));
        conn.execute(
            "INSERT OR IGNORE INTO cops_master (os_no, os_year, os_date, location_code, booked_by, pax_name,
             passport_no, total_items_value, total_duty_amount, total_payable, is_draft, entry_deleted)
             VALUES (?,?,?,?,?,?,?,?,?,?,'N','N')",
            rusqlite::params![
                os_no, os_year, os_date, location_code,
                get("booked_by").trim(), get("pax_name").trim(), get("passport_no").trim(),
                parse_float(get("total_items_value")),
                parse_float(get("total_duty_amount")),
                parse_float(get("total_payable")),
            ],
        ).map_err(|e| e500(&e.to_string()))?;
        existing.insert(key);
        inserted += 1;
    }

    conn.execute_batch("COMMIT").map_err(|e| e500(&e.to_string()))?;
    // Restore normal durability settings
    conn.execute_batch(
        "PRAGMA synchronous = NORMAL; PRAGMA journal_mode = WAL;"
    ).ok();

    post_import_optimise(&conn);
    Ok(Json(json!({ "inserted": inserted, "skipped": skipped, "invalid": invalid })))
}

// ── Legacy items CSV upload ───────────────────────────────────────────────────

pub async fn admin_upload_legacy_items(
    State(pool): Db,
    _admin: AdminUser,
    mut multipart: Multipart,
) -> Result<Json<Value>, Err> {
    let bytes = extract_file(&mut multipart).await?;
    let raw = decode_csv_bytes(&bytes).map_err(|e| e400(&e))?;

    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;

    // Build existing item key set (os_no, os_year, items_sno)
    let mut existing: std::collections::HashSet<(String, i64, i64)> = std::collections::HashSet::new();
    {
        let mut stmt = conn.prepare("SELECT os_no, os_year, items_sno FROM cops_items").map_err(|e| e500(&e.to_string()))?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_,String>(0)?, r.get::<_,i64>(1)?, r.get::<_,i64>(2)?))
        }).map_err(|e| e500(&e.to_string()))?;
        for row in rows.flatten() { existing.insert((row.0.trim().to_string(), row.1, row.2)); }
    }

    let mut rdr = csv::ReaderBuilder::new().has_headers(true).from_reader(raw.as_bytes());
    let headers = rdr.headers().cloned().unwrap_or_default();
    let mut inserted = 0i64; let mut skipped = 0i64; let mut invalid = 0i64;

    conn.execute_batch(
        "PRAGMA synchronous = OFF; PRAGMA journal_mode = MEMORY;"
    ).ok();
    conn.execute_batch("BEGIN").map_err(|e| e500(&e.to_string()))?;

    for result in rdr.records() {
        let rec = match result { Ok(r) => r, Err(_) => { invalid += 1; continue; } };
        let get = |name: &str| -> &str {
            headers.iter().position(|f| f.eq_ignore_ascii_case(name))
                .and_then(|i| rec.get(i)).unwrap_or("")
        };

        let os_no = get("os_no").trim().to_string();
        if os_no.is_empty() { invalid += 1; continue; }
        let os_year = match get("os_year").trim().parse::<i64>() {
            Ok(y) if y > 0 => y, _ => { invalid += 1; continue; }
        };
        let items_sno = match get("items_sno").trim().parse::<i64>() {
            Ok(s) if s > 0 => s, _ => { invalid += 1; continue; }
        };
        let key = (os_no.clone(), os_year, items_sno);
        if existing.contains(&key) { skipped += 1; continue; }

        conn.execute(
            "INSERT OR IGNORE INTO cops_items (os_no, os_year, items_sno, items_desc, items_qty, items_uqc,
             value_per_piece, items_value, items_fa, cumulative_duty_rate, items_duty,
             items_duty_type, items_category, items_release_category, entry_deleted)
             VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,'N')",
            rusqlite::params![
                os_no, os_year, items_sno,
                get("items_desc").trim(),
                parse_float(get("items_qty")), get("items_uqc").trim(),
                parse_float(get("value_per_piece")),
                parse_float(get("items_value")), parse_float(get("items_fa")),
                parse_float(get("cumulative_duty_rate")), parse_float(get("items_duty")),
                get("items_duty_type").trim(), get("items_category").trim(),
                get("items_release_category").trim(),
            ],
        ).map_err(|e| e500(&e.to_string()))?;
        existing.insert(key);
        inserted += 1;
    }

    conn.execute_batch("COMMIT").map_err(|e| e500(&e.to_string()))?;
    conn.execute_batch(
        "PRAGMA synchronous = NORMAL; PRAGMA journal_mode = WAL;"
    ).ok();

    post_import_optimise(&conn);
    Ok(Json(json!({ "inserted": inserted, "skipped": skipped, "invalid": invalid })))
}

// ── Restore from backup ZIP ───────────────────────────────────────────────────

pub async fn admin_restore(
    State(pool): Db,
    _admin: AdminUser,
    mut multipart: Multipart,
) -> Result<Json<Value>, Err> {
    let bytes = extract_file(&mut multipart).await?;

    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| e400(&format!("Invalid ZIP: {e}")))?;

    let zip_pass = crate::security::zip_password();
    let mut master_csv: Option<Vec<u8>> = None;
    let mut items_csv:  Option<Vec<u8>> = None;

    for i in 0..archive.len() {
        // Peek at the file to get name and encryption status, then release the borrow.
        let (name, is_encrypted) = {
            let f = archive.by_index(i).map_err(|e| e500(&e.to_string()))?;
            (f.name().to_lowercase(), f.encrypted())
        };
        let mut buf = Vec::new();
        if is_encrypted {
            // New-style AES-256 encrypted backup
            archive.by_index_decrypt(i, zip_pass.as_bytes())
                .map_err(|e| e500(&e.to_string()))?
                .read_to_end(&mut buf)
                .map_err(|e| e500(&e.to_string()))?;
        } else {
            // Legacy unencrypted backup
            archive.by_index(i)
                .map_err(|e| e500(&e.to_string()))?
                .read_to_end(&mut buf)
                .map_err(|e| e500(&e.to_string()))?;
        }
        if name.contains("cops_master") { master_csv = Some(buf); }
        else if name.contains("cops_items") { items_csv = Some(buf); }
    }

    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let mut master_inserted = 0i64;
    let mut master_skipped  = 0i64;
    let mut items_inserted  = 0i64;
    let mut items_skipped   = 0i64;

    // Bulk-load mode for the entire restore operation
    conn.execute_batch(
        "PRAGMA synchronous = OFF; PRAGMA journal_mode = MEMORY;"
    ).ok();

    // ── Restore cops_master ───────────────────────────────────────────────────
    if let Some(csv_bytes) = master_csv {
        let raw = decode_csv_bytes(&csv_bytes).map_err(|e| e400(&e))?;
        let mut existing = existing_os_keys(&conn).map_err(|e| e500(&e.to_string()))?;
        let mut rdr = csv::ReaderBuilder::new().has_headers(true).from_reader(raw.as_bytes());
        let headers = rdr.headers().cloned().unwrap_or_default();

        conn.execute_batch("BEGIN").map_err(|e| e500(&e.to_string()))?;
        for result in rdr.records() {
            let rec = match result { Ok(r) => r, Err(_) => continue };
            let get = |name: &str| -> &str {
                headers.iter().position(|f| f.eq_ignore_ascii_case(name))
                    .and_then(|i| rec.get(i)).unwrap_or("")
            };
            let os_no = get("os_no").trim().to_string();
            if os_no.is_empty() { continue; }
            let os_year = match get("os_year").trim().parse::<i64>() { Ok(y) => y, Err(_) => continue };
            let loc = get("location_code").trim().to_string();
            let key = (os_no.clone(), os_year, loc.clone());
            if existing.contains(&key) { master_skipped += 1; continue; }

            // Build a flexible insert with all known columns
            conn.execute(
                "INSERT OR IGNORE INTO cops_master (
                    os_no, os_year, os_date, location_code, shift, booked_by, case_type,
                    pax_name, pax_nationality, passport_no, passport_date, pp_issue_place,
                    pax_address1, pax_address2, pax_address3, pax_date_of_birth, pax_status,
                    residence_at, country_of_departure, port_of_dep_dest, date_of_departure,
                    stay_abroad_days, father_name, old_passport_no, previous_visits,
                    flight_no, flight_date,
                    total_items, total_items_value, total_fa_value, dutiable_value,
                    redeemed_value, re_export_value, confiscated_value,
                    total_duty_amount, rf_amount, pp_amount, ref_amount,
                    br_amount, wh_amount, other_amount, total_payable,
                    br_no_str, br_no_num, br_date_str, br_amount_str,
                    is_draft, is_legacy, is_offline_adjudication, file_spot,
                    os_printed, os_category, online_os,
                    adjudication_date, adjudication_time, adj_offr_name, adj_offr_designation,
                    adjn_offr_remarks, adjn_offr_remarks1, adjn_section_ref, online_adjn,
                    supdts_remarks, supdt_remarks2, unique_no, entry_deleted, bkup_taken,
                    detained_by, seal_no, nationality, seizure_date,
                    dr_no, dr_year, total_drs, previous_os_details, total_pkgs, closure_ind,
                    post_adj_br_entries, post_adj_dr_no, post_adj_dr_date,
                    deleted_by, deleted_reason, deleted_on
                ) VALUES (
                    ?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,
                    ?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,
                    ?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?
                )",
                rusqlite::params![
                    os_no, os_year,
                    if get("os_date").is_empty() { None } else { Some(parse_date(get("os_date"))) },
                    if loc.is_empty() { None } else { Some(loc.clone()) },
                    opt(get("shift")), opt(get("booked_by")), opt(get("case_type")),
                    opt(get("pax_name")), opt(get("pax_nationality")), opt(get("passport_no")),
                    opt(get("passport_date")), opt(get("pp_issue_place")),
                    opt(get("pax_address1")), opt(get("pax_address2")), opt(get("pax_address3")),
                    opt(get("pax_date_of_birth")), opt(get("pax_status")),
                    opt(get("residence_at")), opt(get("country_of_departure")),
                    opt(get("port_of_dep_dest")), opt(get("date_of_departure")),
                    parse_i64(get("stay_abroad_days")), opt(get("father_name")),
                    opt(get("old_passport_no")), opt(get("previous_visits")),
                    opt(get("flight_no")), opt(get("flight_date")),
                    parse_i64(get("total_items")),
                    parse_float_opt(get("total_items_value")),
                    parse_float_opt(get("total_fa_value")),
                    parse_float_opt(get("dutiable_value")),
                    parse_float_opt(get("redeemed_value")),
                    parse_float_opt(get("re_export_value")),
                    parse_float_opt(get("confiscated_value")),
                    parse_float_opt(get("total_duty_amount")),
                    parse_float_opt(get("rf_amount")),
                    parse_float_opt(get("pp_amount")),
                    parse_float_opt(get("ref_amount")),
                    parse_float_opt(get("br_amount")),
                    parse_float_opt(get("wh_amount")),
                    parse_float_opt(get("other_amount")),
                    parse_float_opt(get("total_payable")),
                    opt(get("br_no_str")), parse_float_opt(get("br_no_num")),
                    opt(get("br_date_str")), opt(get("br_amount_str")),
                    or_n(get("is_draft")), opt(get("is_legacy")), opt(get("is_offline_adjudication")),
                    opt(get("file_spot")), opt(get("os_printed")), opt(get("os_category")),
                    opt(get("online_os")), opt(get("adjudication_date")), opt(get("adjudication_time")),
                    opt(get("adj_offr_name")), opt(get("adj_offr_designation")),
                    opt(get("adjn_offr_remarks")), opt(get("adjn_offr_remarks1")),
                    opt(get("adjn_section_ref")),
                    opt(get("online_adjn")), opt(get("supdts_remarks")), opt(get("supdt_remarks2")),
                    parse_i64(get("unique_no")), or_n(get("entry_deleted")), opt(get("bkup_taken")),
                    opt(get("detained_by")), opt(get("seal_no")), opt(get("nationality")),
                    opt(get("seizure_date")), parse_i64(get("dr_no")), parse_i64(get("dr_year")),
                    parse_i64(get("total_drs")), opt(get("previous_os_details")),
                    parse_i64(get("total_pkgs")), opt(get("closure_ind")),
                    opt(get("post_adj_br_entries")), opt(get("post_adj_dr_no")), opt(get("post_adj_dr_date")),
                    opt(get("deleted_by")), opt(get("deleted_reason")), opt(get("deleted_on")),
                ],
            ).map_err(|e| e500(&e.to_string()))?;
            existing.insert(key);
            master_inserted += 1;
        }
        conn.execute_batch("COMMIT").map_err(|e| e500(&e.to_string()))?;
    }

    // ── Restore cops_items ────────────────────────────────────────────────────
    if let Some(csv_bytes) = items_csv {
        let raw = decode_csv_bytes(&csv_bytes).map_err(|e| e400(&e))?;
        let mut rdr = csv::ReaderBuilder::new().has_headers(true).from_reader(raw.as_bytes());
        let headers = rdr.headers().cloned().unwrap_or_default();

        conn.execute_batch("BEGIN").map_err(|e| e500(&e.to_string()))?;
        for result in rdr.records() {
            let rec = match result { Ok(r) => r, Err(_) => continue };
            let get = |name: &str| -> &str {
                headers.iter().position(|f| f.eq_ignore_ascii_case(name))
                    .and_then(|i| rec.get(i)).unwrap_or("")
            };
            let os_no = get("os_no").trim().to_string();
            if os_no.is_empty() { continue; }
            let os_year = match get("os_year").trim().parse::<i64>() { Ok(y) => y, Err(_) => continue };
            let items_sno = match get("items_sno").trim().parse::<i64>() { Ok(s) => s, Err(_) => continue };

            // Check if item already exists
            let exists: bool = conn.query_row(
                "SELECT 1 FROM cops_items WHERE os_no=? AND os_year=? AND items_sno=?",
                rusqlite::params![os_no, os_year, items_sno],
                |_| Ok(true),
            ).optional().map_err(|e| e500(&e.to_string()))?.unwrap_or(false);

            if exists { items_skipped += 1; continue; }

            conn.execute(
                "INSERT OR IGNORE INTO cops_items (
                    os_no, os_year, items_sno, items_desc, items_qty, items_uqc,
                    value_per_piece, items_value, items_fa, cumulative_duty_rate, items_duty,
                    items_duty_type, items_category, items_sub_category, items_release_category,
                    items_dr_no, items_dr_year, items_fa_type, items_fa_qty, items_fa_uqc,
                    unique_no, entry_deleted
                ) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
                rusqlite::params![
                    os_no, os_year, items_sno,
                    opt(get("items_desc")), parse_float_opt(get("items_qty")),
                    opt(get("items_uqc")), parse_float_opt(get("value_per_piece")),
                    parse_float_opt(get("items_value")), parse_float_opt(get("items_fa")),
                    parse_float_opt(get("cumulative_duty_rate")), parse_float_opt(get("items_duty")),
                    opt(get("items_duty_type")), opt(get("items_category")),
                    opt(get("items_sub_category")), opt(get("items_release_category")),
                    parse_i64(get("items_dr_no")), parse_i64(get("items_dr_year")),
                    opt(get("items_fa_type")), parse_float_opt(get("items_fa_qty")),
                    opt(get("items_fa_uqc")), parse_i64(get("unique_no")),
                    or_n(get("entry_deleted")),
                ],
            ).map_err(|e| e500(&e.to_string()))?;
            items_inserted += 1;
        }
        conn.execute_batch("COMMIT").map_err(|e| e500(&e.to_string()))?;
    }

    // Restore normal durability
    conn.execute_batch(
        "PRAGMA synchronous = NORMAL; PRAGMA journal_mode = WAL;"
    ).ok();

    post_import_optimise(&conn);
    Ok(Json(json!({
        "master_inserted": master_inserted,
        "master_skipped":  master_skipped,
        "items_inserted":  items_inserted,
        "items_skipped":   items_skipped,
        "br_inserted": 0, "br_skipped": 0, "br_items_inserted": 0,
        "dr_inserted": 0, "dr_skipped": 0, "dr_items_inserted": 0,
        "users_inserted": 0,
    })))
}

// ── MDB import — not supported in cops2 ──────────────────────────────────────

pub async fn admin_import_mdb(_admin: AdminUser, mut multipart: Multipart) -> Result<Json<Value>, Err> {
    // Drain the multipart body so the connection is properly closed
    while multipart.next_field().await.ok().flatten().is_some() {}
    Err((StatusCode::NOT_IMPLEMENTED, Json(json!({
        "detail": "MDB import is not supported in cops2. Export to CSV from cops1 first, then use the CSV restore."
    }))))
}

// ── Custom report ─────────────────────────────────────────────────────────────

const REPORT_MASTER_COLS: &[&str] = &[
    "os_no","os_year","os_date","location_code","case_type","booked_by","os_category",
    "pax_name","pax_nationality","passport_no","passport_date","pp_issue_place",
    "pax_address1","pax_address2","pax_address3","pax_date_of_birth",
    "father_name","residence_at","country_of_departure","date_of_departure",
    "port_of_dep_dest","stay_abroad_days","old_passport_no","pax_status",
    "flight_no","flight_date",
    "total_items","total_items_value","total_fa_value","dutiable_value",
    "redeemed_value","re_export_value","confiscated_value",
    "total_duty_amount","rf_amount","pp_amount","ref_amount",
    "br_amount","wh_amount","other_amount","total_payable",
    "br_no_num","br_date_str","br_amount_str","br_no_str",
    "adjudication_date","adj_offr_name","adj_offr_designation","adjn_offr_remarks",
    "adjn_section_ref",
    "online_adjn","dr_no","dr_year","seizure_date","supdts_remarks",
    "post_adj_br_entries","post_adj_dr_no","post_adj_dr_date",
];

const REPORT_ITEM_COLS: &[&str] = &[
    "items_desc","items_qty","items_uqc","items_value","items_fa",
    "items_duty","items_duty_type","items_category","items_sub_category",
    "items_release_category","value_per_piece","cumulative_duty_rate",
];

#[derive(Deserialize)]
pub struct CustomReportRequest {
    master_cols: Vec<String>,
    #[serde(default)]
    item_cols: Vec<String>,
    from_date: Option<String>,
    to_date:   Option<String>,
    case_type: Option<String>,
    // Row-level filters
    #[serde(default)] os_no:         Option<String>,
    #[serde(default)] os_year:       Option<i64>,
    #[serde(default)] adj_offr_name: Option<String>,
    #[serde(default)] flight_no:     Option<String>,
    #[serde(default)] pax_name:      Option<String>,
    #[serde(default)] passport_no:   Option<String>,
    #[serde(default)] item_desc:     Option<String>,
}

pub async fn custom_report(
    State(pool): Db,
    _auth: AuthUser,
    Json(body): Json<CustomReportRequest>,
) -> Result<Json<Value>, Err> {
    let invalid_m: Vec<_> = body.master_cols.iter().filter(|c| !REPORT_MASTER_COLS.contains(&c.as_str())).collect();
    let invalid_i: Vec<_> = body.item_cols.iter().filter(|c| !REPORT_ITEM_COLS.contains(&c.as_str())).collect();
    if !invalid_m.is_empty() || !invalid_i.is_empty() {
        return Err(e400(&format!("Unknown columns: {:?}", [invalid_m, invalid_i].concat())));
    }
    if body.master_cols.is_empty() && body.item_cols.is_empty() {
        return Err(e400("Select at least one column."));
    }

    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let include_items = !body.item_cols.is_empty();

    // Build parameterized WHERE clause
    let mut conditions = vec!["cm.entry_deleted = 'N'".to_string()];
    let mut params: Vec<String> = Vec::new();
    if let (Some(fd), Some(td)) = (&body.from_date, &body.to_date) {
        conditions.push("cm.os_date >= ? AND cm.os_date <= ?".to_string());
        params.extend_from_slice(&[fd.clone(), td.clone()]);
    }
    if let Some(ct) = &body.case_type {
        if ct.to_uppercase().contains("EXPORT") {
            conditions.push("upper(cm.case_type) = 'EXPORT CASE'".to_string());
        } else {
            conditions.push("(cm.case_type IS NULL OR upper(cm.case_type) != 'EXPORT CASE')".to_string());
        }
    }
    if let Some(v) = &body.os_no         { conditions.push("cm.os_no = ?".to_string()); params.push(v.clone()); }
    if let Some(v) = body.os_year        { conditions.push("cm.os_year = ?".to_string()); params.push(v.to_string()); }
    if let Some(v) = &body.adj_offr_name { conditions.push("upper(cm.adj_offr_name) LIKE upper(?)".to_string()); params.push(format!("%{v}%")); }
    if let Some(v) = &body.flight_no     { conditions.push("upper(cm.flight_no) LIKE upper(?)".to_string()); params.push(format!("%{v}%")); }
    if let Some(v) = &body.pax_name      { conditions.push("upper(cm.pax_name) LIKE upper(?)".to_string()); params.push(format!("%{v}%")); }
    if let Some(v) = &body.passport_no   { conditions.push("upper(cm.passport_no) LIKE upper(?)".to_string()); params.push(format!("%{v}%")); }
    if let Some(v) = &body.item_desc {
        conditions.push("EXISTS (SELECT 1 FROM cops_items ci2 WHERE ci2.os_no=cm.os_no AND ci2.os_year=cm.os_year AND upper(ci2.items_desc) LIKE upper(?))".to_string());
        params.push(format!("%{v}%"));
    }
    let where_clause = conditions.join(" AND ");

    // Always include os_no and os_year at positions 0 and 1 as keys for the items join.
    let mut query_master_cols = vec!["os_no".to_string(), "os_year".to_string()];
    for c in &body.master_cols {
        if c != "os_no" && c != "os_year" { query_master_cols.push(c.clone()); }
    }
    let master_sel = query_master_cols.iter().map(|c| format!("cm.{c}")).collect::<Vec<_>>().join(", ");
    let master_sql = format!(
        "SELECT {master_sel} FROM cops_master cm WHERE {where_clause} ORDER BY cm.os_year, CAST(cm.os_no AS INTEGER)"
    );

    let qmc_len = query_master_cols.len();
    let mut stmt = conn.prepare(&master_sql).map_err(|e| e500(&e.to_string()))?;
    let master_rows: Vec<Vec<String>> = stmt.query_map(
        rusqlite::params_from_iter(params.iter()),
        |row| Ok((0..qmc_len).map(|i| val_to_str(row.get_ref(i).unwrap_or(rusqlite::types::ValueRef::Null))).collect()),
    ).map_err(|e| e500(&e.to_string()))?.filter_map(|r| r.ok()).collect();

    // Bulk-load items (OR-chain, chunked at 80) and aggregate per OS with \n separator.
    let mut items_map: std::collections::HashMap<(String, String), Vec<Vec<String>>> = std::collections::HashMap::new();
    if include_items && !master_rows.is_empty() {
        let item_sel = body.item_cols.iter().map(|c| format!("ci.{c}")).collect::<Vec<_>>().join(", ");
        let icc = body.item_cols.len();
        for chunk in master_rows.chunks(80) {
            let or_parts = chunk.iter().map(|_| "(ci.os_no=? AND ci.os_year=?)").collect::<Vec<_>>().join(" OR ");
            let isql = format!(
                "SELECT ci.os_no, ci.os_year, {item_sel} FROM cops_items ci
                 WHERE ({or_parts}) AND (ci.entry_deleted IS NULL OR ci.entry_deleted != 'Y')
                 ORDER BY ci.os_no, ci.os_year, ci.items_sno"
            );
            let flat: Vec<String> = chunk.iter().flat_map(|r| [r[0].clone(), r[1].clone()]).collect();
            let mut istmt = conn.prepare(&isql).map_err(|e| e500(&e.to_string()))?;
            let item_rows: Vec<(String, String, Vec<String>)> = istmt.query_map(
                rusqlite::params_from_iter(flat.iter()),
                |row| {
                    let ono: String = row.get(0)?;
                    let oyr = row.get::<_, i64>(1).map(|y| y.to_string()).unwrap_or_default();
                    let vals: Vec<String> = (0..icc).map(|i| val_to_str(row.get_ref(i + 2).unwrap_or(rusqlite::types::ValueRef::Null))).collect();
                    Ok((ono, oyr, vals))
                },
            ).map_err(|e| e500(&e.to_string()))?.filter_map(|r| r.ok()).collect();
            for (ono, oyr, vals) in item_rows {
                items_map.entry((ono, oyr)).or_default().push(vals);
            }
        }
    }

    // Build output columns list and one row per OS.
    let mut all_cols = body.master_cols.clone();
    if include_items { all_cols.extend(body.item_cols.iter().cloned()); }

    let json_rows: Vec<Value> = master_rows.iter().map(|row| {
        let os_no  = &row[0];
        let os_year = &row[1];
        let mut obj = serde_json::Map::new();
        for col in &body.master_cols {
            let idx = query_master_cols.iter().position(|c| c == col).unwrap_or(0);
            obj.insert(col.clone(), Value::String(row[idx].clone()));
        }
        if include_items {
            let item_rows = items_map.get(&(os_no.clone(), os_year.clone())).cloned().unwrap_or_default();
            for (ci, col) in body.item_cols.iter().enumerate() {
                let joined = item_rows.iter().map(|ir| ir[ci].as_str()).filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n");
                obj.insert(col.clone(), Value::String(joined));
            }
        }
        Value::Object(obj)
    }).collect();

    Ok(Json(json!({ "columns": all_cols, "rows": json_rows, "total": json_rows.len() })))
}

// ── Adjudication summary PDF ──────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct AdjSummaryRequest {
    from_date: String,
    to_date:   String,
}

#[derive(Serialize)]
struct OfficerRow {
    name:        String,
    designation: String,
    cases:       i64,
    total_value: f64,
    dutiable:    f64,
    redeemed:    f64,
    re_export:   f64,
    confiscated: f64,
    duty:        f64,
    rf:          f64,
    refine:      f64,
    pp:          f64,
}

fn fmt_ind(n: f64) -> String {
    let n_int = n.round() as i64;
    if n_int == 0 { return "\u{2014}".to_string(); }  // em dash
    let s = n_int.unsigned_abs().to_string();
    if s.len() <= 3 { return s; }
    let tail = &s[s.len()-3..];
    let front = &s[..s.len()-3];
    let mut parts: Vec<&str> = Vec::new();
    let mut i = front.len();
    while i > 0 { let st = if i>2{i-2}else{0}; parts.push(&front[st..i]); i=st; }
    parts.reverse();
    format!("{},{}", parts.join(","), tail)
}

pub async fn adjudication_summary_pdf(
    State(pool): Db,
    _auth: AuthUser,
    Json(body): Json<AdjSummaryRequest>,
) -> Result<impl IntoResponse, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;

    let mut stmt = conn.prepare(
        "SELECT adj_offr_name, max(adj_offr_designation),
                count(*) as cases,
                coalesce(sum(total_items_value),0),
                coalesce(sum(dutiable_value),0),
                coalesce(sum(redeemed_value),0),
                coalesce(sum(re_export_value),0),
                coalesce(sum(confiscated_value),0),
                coalesce(sum(total_duty_amount),0),
                coalesce(sum(rf_amount),0),
                coalesce(sum(ref_amount),0),
                coalesce(sum(pp_amount),0)
         FROM cops_master
         WHERE entry_deleted='N' AND adj_offr_name IS NOT NULL AND adj_offr_name != ''
           AND adjudication_date >= ? AND adjudication_date <= ?
         GROUP BY adj_offr_name ORDER BY adj_offr_name",
    ).map_err(|e| e500(&e.to_string()))?;

    let officers: Vec<OfficerRow> = stmt.query_map(
        rusqlite::params![body.from_date, body.to_date],
        |r| Ok(OfficerRow {
            name: r.get(0)?, designation: r.get::<_,Option<String>>(1)?.unwrap_or_default(),
            cases: r.get(2)?,
            total_value: r.get(3)?, dutiable: r.get(4)?, redeemed: r.get(5)?,
            re_export: r.get(6)?,   confiscated: r.get(7)?, duty: r.get(8)?,
            rf: r.get(9)?, refine: r.get(10)?, pp: r.get(11)?,
        }),
    ).map_err(|e| e500(&e.to_string()))?.filter_map(|r| r.ok()).collect();

    if officers.is_empty() {
        return Err((StatusCode::NOT_FOUND, Json(json!({
            "detail": "No adjudicated cases found for the selected date range."
        }))));
    }

    // Totals
    let tot_cases: i64  = officers.iter().map(|o| o.cases).sum();
    let tot_val:  f64   = officers.iter().map(|o| o.total_value).sum();
    let tot_dut:  f64   = officers.iter().map(|o| o.dutiable).sum();
    let tot_red:  f64   = officers.iter().map(|o| o.redeemed).sum();
    let tot_ref:  f64   = officers.iter().map(|o| o.re_export).sum();
    let tot_conf: f64   = officers.iter().map(|o| o.confiscated).sum();
    let tot_duty: f64   = officers.iter().map(|o| o.duty).sum();
    let tot_rf:   f64   = officers.iter().map(|o| o.rf).sum();
    let tot_refe: f64   = officers.iter().map(|o| o.refine).sum();
    let tot_pp:   f64   = officers.iter().map(|o| o.pp).sum();

    // Build Typst source
    let mut officer_rows = String::new();
    for (i, o) in officers.iter().enumerate() {
        let desig = if o.designation.is_empty() { String::new() }
                    else { format!("\n#text(size:6.5pt, fill:gray)[{}]", crate::pdf::esc_pub(&o.designation)) };
        officer_rows.push_str(&format!(
            "[{}], [*{}*{}], [{}], [{}], [{}], [{}], [{}], [{}], [{}], [{}], [{}], [{}],\n",
            i+1, crate::pdf::esc_pub(&o.name), desig,
            o.cases, fmt_ind(o.total_value), fmt_ind(o.dutiable),
            fmt_ind(o.redeemed), fmt_ind(o.re_export), fmt_ind(o.confiscated),
            fmt_ind(o.duty), fmt_ind(o.rf), fmt_ind(o.refine), fmt_ind(o.pp)
        ));
    }

    let from_str = body.from_date.replace('-', "/");
    let to_str   = body.to_date.replace('-', "/");
    let gen_dt   = chrono::Local::now().format("%d/%m/%Y %H:%M").to_string();

    let typst_src = format!(r##"
#set page(paper: "a4", flipped: true,
  margin: (top: 10mm, bottom: 14mm, left: 8mm, right: 8mm))
#set text(font: ("Liberation Sans","Noto Sans","Roboto"), size: 8pt)
#set table(inset: (x: 3pt, y: 2.5pt), stroke: 0.5pt + black)

#align(center)[
  #text(size: 11pt, weight: "bold")[ADJUDICATING OFFICERS — PERFORMANCE SUMMARY REPORT]
  \
  #text(size: 9pt, weight: "bold", fill: rgb("#1e4a72"))[Period: {from_str} to {to_str}]
  \
  #text(size: 7.5pt, fill: gray)[Filtered by adjudication date | All amounts in Indian Rupees, rounded to nearest rupee | — denotes zero | Generated: {gen_dt}]
]
#v(4pt)

#table(
  columns: (3%, 12%, 5%, 8%, 8%, 8%, 8%, 8%, 8%, 8%, 8%, 8%),
  align: (center, left, center, right, right, right, right, right, right, right, right, right),
  fill: (_, y) => if y == 0 {{ rgb("#1e4a72") }} else if calc.odd(y) {{ white }} else {{ rgb("#f2f7fc") }},
  table.header(
    text(fill:white, weight:"bold")[S.\ No.],
    text(fill:white, weight:"bold")[Officer Name /\ Designation],
    text(fill:white, weight:"bold")[No. of\ Cases],
    text(fill:white, weight:"bold")[Total Value\ Under OS (₹)],
    text(fill:white, weight:"bold")[Dutiable\ Value (₹)],
    text(fill:white, weight:"bold")[Redeemed\ Value (₹)],
    text(fill:white, weight:"bold")[Re-export\ Value (₹)],
    text(fill:white, weight:"bold")[Abs. Conf.\ Value (₹)],
    text(fill:white, weight:"bold")[Duty\ Levied (₹)],
    text(fill:white, weight:"bold")[R.F.\ Levied (₹)],
    text(fill:white, weight:"bold")[R.E.F.\ Levied (₹)],
    text(fill:white, weight:"bold")[Personal\ Penalty (₹)],
  ),
  {officer_rows}
  table.cell(colspan: 2, fill: rgb("#1e4a72"))[#text(fill:white, weight:"bold")[GRAND TOTAL]],
  table.cell(fill: rgb("#1e4a72"))[#text(fill:white, weight:"bold")[{tot_cases}]],
  table.cell(fill: rgb("#1e4a72"))[#text(fill:white, weight:"bold")[{tv}]],
  table.cell(fill: rgb("#1e4a72"))[#text(fill:white, weight:"bold")[{td}]],
  table.cell(fill: rgb("#1e4a72"))[#text(fill:white, weight:"bold")[{tr}]],
  table.cell(fill: rgb("#1e4a72"))[#text(fill:white, weight:"bold")[{tref}]],
  table.cell(fill: rgb("#1e4a72"))[#text(fill:white, weight:"bold")[{tc}]],
  table.cell(fill: rgb("#1e4a72"))[#text(fill:white, weight:"bold")[{tdu}]],
  table.cell(fill: rgb("#1e4a72"))[#text(fill:white, weight:"bold")[{trf}]],
  table.cell(fill: rgb("#1e4a72"))[#text(fill:white, weight:"bold")[{trfe}]],
  table.cell(fill: rgb("#1e4a72"))[#text(fill:white, weight:"bold")[{tpp}]],
)
"##,
        from_str = from_str, to_str = to_str, gen_dt = gen_dt,
        officer_rows = officer_rows,
        tot_cases = tot_cases,
        tv = fmt_ind(tot_val), td = fmt_ind(tot_dut), tr = fmt_ind(tot_red),
        tref = fmt_ind(tot_ref), tc = fmt_ind(tot_conf), tdu = fmt_ind(tot_duty),
        trf = fmt_ind(tot_rf), trfe = fmt_ind(tot_refe), tpp = fmt_ind(tot_pp),
    );

    let pdf_bytes = crate::pdf::compile_typst(&typst_src)
        .map_err(|e| e500(&format!("PDF error: {e}")))?;

    let filename = format!("adj_summary_{}_to_{}.pdf", body.from_date, body.to_date);
    Ok((
        [
            (header::CONTENT_TYPE, "application/pdf".to_string()),
            (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{filename}\"")),
        ],
        pdf_bytes,
    ))
}

// ── upload/new and upload/legacy — destructive operations, require AdminUser ──

pub async fn upload_new(
    State(pool): Db,
    _admin: AdminUser,
    multipart: Multipart,
) -> Result<Json<Value>, Err> {
    admin_restore(State(pool), _admin, multipart).await
}

pub async fn upload_legacy(
    State(pool): Db,
    _admin: AdminUser,
    multipart: Multipart,
) -> Result<Json<Value>, Err> {
    admin_upload_legacy(State(pool), _admin, multipart).await
}

// ── Shared helpers ────────────────────────────────────────────────────────────

async fn extract_file(multipart: &mut Multipart) -> Result<Vec<u8>, Err> {
    while let Some(field) = multipart.next_field().await.map_err(|e| e400(&e.to_string()))? {
        if matches!(field.name(), Some("file") | None) {
            return Ok(field.bytes().await.map_err(|e| e400(&e.to_string()))?.to_vec());
        }
    }
    Err(e400("No file field in form data"))
}

fn decode_csv_bytes(bytes: &[u8]) -> Result<String, String> {
    // Strip BOM if present
    let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    String::from_utf8(bytes.to_vec())
        .or_else(|_| {
            // Try latin-1 fallback
            Ok(bytes.iter().map(|&b| b as char).collect())
        })
}

fn existing_os_keys(conn: &rusqlite::Connection)
    -> rusqlite::Result<std::collections::HashSet<(String, i64, String)>>
{
    let mut stmt = conn.prepare("SELECT os_no, os_year, coalesce(location_code,'') FROM cops_master")?;
    let rows = stmt.query_map([], |r| {
        Ok((r.get::<_,String>(0)?.trim().to_string(), r.get::<_,i64>(1)?, r.get::<_,String>(2)?.trim().to_string()))
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

fn opt(s: &str) -> Option<String> {
    let s = s.trim();
    if s.is_empty() { None } else { Some(s.to_string()) }
}

fn or_n(s: &str) -> String {
    let s = s.trim();
    if s.is_empty() { "N".to_string() } else { s.to_string() }
}

fn parse_i64(s: &str) -> Option<i64> {
    s.trim().parse::<i64>().ok()
}

fn parse_float_opt(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.is_empty() { None } else { s.parse::<f64>().ok() }
}
