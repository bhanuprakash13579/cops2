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

// ── DCR column lists (id columns excluded — remapped by natural key on restore) ───

const DCR_TARIFF_COLS: &[&str] = &[
    "effective_from", "label",
    "baggage_rate", "liquor_duty_rate", "aidc_liquor_rate",
    "gold_bcd_rate", "aidc_gold_rate", "gold_cons_bcd_rate", "aidc_gold_cons_rate",
    "silver_bcd_rate", "aidc_silver_rate", "silver_cons_rate", "aidc_silver_cons_rate",
    "created_at",
];

// Sessions export includes _tariff_eff for FK remapping on restore
const DCR_SESSION_COLS: &[&str] = &[
    "report_date", "shift", "batch_name", "created_by", "created_at",
    "submitted_at", "submitted_by", "_tariff_eff",
];

// Entry tables export include _sess_date/_sess_shift/_sess_batch for session FK remapping
const DCR_ENTRY_COLS: &[&str] = &[
    "sort_order", "sl_no", "br_no", "os_ref", "item_desc",
    "dutiable_value", "gold_weight_gms", "baggage_duty", "liquor_duty",
    "cigarette_duty", "sw_sc", "gold_duty_bcd", "gold_duty_cons", "silver_duty_cons",
    "sws_on_gold", "aidc_gold_silver", "sws_on_silver", "aidc_on_liquor",
    "redemption_fine", "reexport_fine", "personal_penalty", "other_charges",
    "fuel_duty", "total_duty", "flight_no", "is_sbi_challan", "is_offline_br", "overrides",
    "_sess_date", "_sess_shift", "_sess_batch",
];

const DCR_DR_ENTRY_COLS: &[&str] = &[
    "sort_order", "dr_no", "amount", "item_desc", "remarks",
    "_sess_date", "_sess_shift", "_sess_batch",
];

const DCR_OS_ENTRY_COLS: &[&str] = &[
    "sort_order", "os_no", "amount", "item_desc", "remarks",
    "_sess_date", "_sess_shift", "_sess_batch",
];

const DCR_ITEM_TYPE_COLS: &[&str] = &["name", "usage_count", "is_system"];

const DCR_FORMULA_RULE_COLS: &[&str] = &[
    "sort_order", "target_column", "column_label", "condition_type",
    "condition_items", "expression", "is_active", "notes",
];

const DCR_SETTINGS_COLS: &[&str] = &["station_name", "officer_name", "designation"];

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

fn parse_date(s: &str) -> Option<String> {
    let s = s.trim().trim_matches('"');
    if s.is_empty() { return None; }
    // Try YYYY-MM-DD first
    if s.len() == 10 && s.as_bytes()[4] == b'-' { return Some(s.to_string()); }
    // Try M/D/YY and M/D/YYYY
    for fmt in &["%m/%d/%y", "%m/%d/%Y", "%d/%m/%Y"] {
        if let Ok(d) = chrono::NaiveDate::parse_from_str(s, fmt) {
            return Some(d.format("%Y-%m-%d").to_string());
        }
    }
    None // never substitute today's date for invalid input
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

        // ── DCR tables ────────────────────────────────────────────────────────
        // dcr_tariffs — configuration reference table (no FK dependencies)
        let dcr_tariff_sql = "SELECT effective_from,label,baggage_rate,liquor_duty_rate,\
            aidc_liquor_rate,gold_bcd_rate,aidc_gold_rate,gold_cons_bcd_rate,aidc_gold_cons_rate,\
            silver_bcd_rate,aidc_silver_rate,silver_cons_rate,aidc_silver_cons_rate,created_at \
            FROM dcr_tariffs ORDER BY effective_from";
        if let Ok(csv) = query_to_csv(&conn, dcr_tariff_sql, DCR_TARIFF_COLS) {
            let opts2 = SimpleFileOptions::default()
                .compression_method(CompressionMethod::Deflated)
                .with_aes_encryption(zip::AesMode::Aes256, crate::security::zip_password());
            zip.start_file("dcr_tariffs.csv", opts2).map_err(|e| e500(&e.to_string()))?;
            zip.write_all(&csv).map_err(|e| e500(&e.to_string()))?;
        }

        // dcr_item_types — user-defined item categories
        let dcr_item_types_sql = "SELECT name,usage_count,is_system FROM dcr_item_types ORDER BY name";
        if let Ok(csv) = query_to_csv(&conn, dcr_item_types_sql, DCR_ITEM_TYPE_COLS) {
            let opts2 = SimpleFileOptions::default()
                .compression_method(CompressionMethod::Deflated)
                .with_aes_encryption(zip::AesMode::Aes256, crate::security::zip_password());
            zip.start_file("dcr_item_types.csv", opts2).map_err(|e| e500(&e.to_string()))?;
            zip.write_all(&csv).map_err(|e| e500(&e.to_string()))?;
        }

        // dcr_formula_rules — auto-calc rules
        let dcr_formula_sql = "SELECT sort_order,target_column,column_label,condition_type,\
            condition_items,expression,is_active,notes FROM dcr_formula_rules ORDER BY sort_order";
        if let Ok(csv) = query_to_csv(&conn, dcr_formula_sql, DCR_FORMULA_RULE_COLS) {
            let opts2 = SimpleFileOptions::default()
                .compression_method(CompressionMethod::Deflated)
                .with_aes_encryption(zip::AesMode::Aes256, crate::security::zip_password());
            zip.start_file("dcr_formula_rules.csv", opts2).map_err(|e| e500(&e.to_string()))?;
            zip.write_all(&csv).map_err(|e| e500(&e.to_string()))?;
        }

        // dcr_settings — station config (single row)
        let dcr_settings_sql = "SELECT station_name,officer_name,designation FROM dcr_settings WHERE id=1";
        if let Ok(csv) = query_to_csv(&conn, dcr_settings_sql, DCR_SETTINGS_COLS) {
            let opts2 = SimpleFileOptions::default()
                .compression_method(CompressionMethod::Deflated)
                .with_aes_encryption(zip::AesMode::Aes256, crate::security::zip_password());
            zip.start_file("dcr_settings.csv", opts2).map_err(|e| e500(&e.to_string()))?;
            zip.write_all(&csv).map_err(|e| e500(&e.to_string()))?;
        }

        // dcr_sessions — joins tariff for effective_from FK remapping
        let dcr_session_sql = "SELECT s.report_date,s.shift,COALESCE(s.batch_name,''),\
            s.created_by,s.created_at,s.submitted_at,s.submitted_by,\
            COALESCE(t.effective_from,'') AS _tariff_eff \
            FROM dcr_sessions s LEFT JOIN dcr_tariffs t ON t.id=s.tariff_id \
            ORDER BY s.report_date,s.shift,s.batch_name";
        if let Ok(csv) = query_to_csv(&conn, dcr_session_sql, DCR_SESSION_COLS) {
            let opts2 = SimpleFileOptions::default()
                .compression_method(CompressionMethod::Deflated)
                .with_aes_encryption(zip::AesMode::Aes256, crate::security::zip_password());
            zip.start_file("dcr_sessions.csv", opts2).map_err(|e| e500(&e.to_string()))?;
            zip.write_all(&csv).map_err(|e| e500(&e.to_string()))?;
        }

        // dcr_entries — joins session for natural key remapping
        let dcr_entry_sql = "SELECT e.sort_order,e.sl_no,e.br_no,e.os_ref,e.item_desc,\
            e.dutiable_value,e.gold_weight_gms,e.baggage_duty,e.liquor_duty,e.cigarette_duty,\
            e.sw_sc,e.gold_duty_bcd,e.gold_duty_cons,e.silver_duty_cons,e.sws_on_gold,\
            e.aidc_gold_silver,e.sws_on_silver,e.aidc_on_liquor,e.redemption_fine,\
            e.reexport_fine,e.personal_penalty,e.other_charges,e.fuel_duty,e.total_duty,\
            e.flight_no,e.is_sbi_challan,e.is_offline_br,e.overrides,\
            s.report_date AS _sess_date,s.shift AS _sess_shift,\
            COALESCE(s.batch_name,'') AS _sess_batch \
            FROM dcr_entries e JOIN dcr_sessions s ON s.id=e.session_id \
            ORDER BY s.report_date,s.shift,s.batch_name,e.sort_order";
        if let Ok(csv) = query_to_csv(&conn, dcr_entry_sql, DCR_ENTRY_COLS) {
            let opts2 = SimpleFileOptions::default()
                .compression_method(CompressionMethod::Deflated)
                .with_aes_encryption(zip::AesMode::Aes256, crate::security::zip_password());
            zip.start_file("dcr_entries.csv", opts2).map_err(|e| e500(&e.to_string()))?;
            zip.write_all(&csv).map_err(|e| e500(&e.to_string()))?;
        }

        // dcr_dr_entries — DR (Detention Register) line items
        let dcr_dr_sql = "SELECT e.sort_order,e.dr_no,e.amount,e.item_desc,e.remarks,\
            s.report_date AS _sess_date,s.shift AS _sess_shift,\
            COALESCE(s.batch_name,'') AS _sess_batch \
            FROM dcr_dr_entries e JOIN dcr_sessions s ON s.id=e.session_id \
            ORDER BY s.report_date,s.shift,s.batch_name,e.sort_order";
        if let Ok(csv) = query_to_csv(&conn, dcr_dr_sql, DCR_DR_ENTRY_COLS) {
            let opts2 = SimpleFileOptions::default()
                .compression_method(CompressionMethod::Deflated)
                .with_aes_encryption(zip::AesMode::Aes256, crate::security::zip_password());
            zip.start_file("dcr_dr_entries.csv", opts2).map_err(|e| e500(&e.to_string()))?;
            zip.write_all(&csv).map_err(|e| e500(&e.to_string()))?;
        }

        // dcr_os_entries — OS (Offence Sheet) line items
        let dcr_os_sql = "SELECT e.sort_order,e.os_no,e.amount,e.item_desc,e.remarks,\
            s.report_date AS _sess_date,s.shift AS _sess_shift,\
            COALESCE(s.batch_name,'') AS _sess_batch \
            FROM dcr_os_entries e JOIN dcr_sessions s ON s.id=e.session_id \
            ORDER BY s.report_date,s.shift,s.batch_name,e.sort_order";
        if let Ok(csv) = query_to_csv(&conn, dcr_os_sql, DCR_OS_ENTRY_COLS) {
            let opts2 = SimpleFileOptions::default()
                .compression_method(CompressionMethod::Deflated)
                .with_aes_encryption(zip::AesMode::Aes256, crate::security::zip_password());
            zip.start_file("dcr_os_entries.csv", opts2).map_err(|e| e500(&e.to_string()))?;
            zip.write_all(&csv).map_err(|e| e500(&e.to_string()))?;
        }

        zip.finish().map_err(|e| e500(&e.to_string()))?;
    }

    let today = chrono::Local::now().format("%Y-%m-%d");
    let filename = format!("cops_full_backup_{today}.zip");
    let zip_len = zip_buf.len();
    Ok((
        [
            (header::CONTENT_TYPE, "application/zip".to_string()),
            (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{filename}\"")),
            (header::CONTENT_LENGTH, zip_len.to_string()),
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
        // Key must be set on BOTH connections: src to read the encrypted source,
        // dst so the backup output is also encrypted with the same key.
        // Without the key on dst the output is plain SQLite and restore will reject it.
        src.execute_batch(&crate::security::sqlcipher_pragma()).map_err(|e| e500(&e.to_string()))?;
        let mut dst = rusqlite::Connection::open(&tmp_path).map_err(|e| e500(&e.to_string()))?;
        dst.execute_batch(&crate::security::sqlcipher_pragma()).map_err(|e| e500(&e.to_string()))?;
        let backup = rusqlite::backup::Backup::new(&src, &mut dst).map_err(|e| e500(&e.to_string()))?;
        // -1 copies all remaining pages in one step (no per-batch sleep) — fastest safe option.
        backup.run_to_completion(-1, Duration::from_millis(0), None).map_err(|e| e500(&e.to_string()))?;
    }

    let bytes = tokio::fs::read(&tmp_path).await.map_err(|e| e500(&e.to_string()))?;
    let _ = tokio::fs::remove_file(&tmp_path).await;

    let today = chrono::Local::now().format("%Y-%m-%d");
    let filename = format!("cops_fulldb_{today}.db");
    let db_len = bytes.len();
    Ok((
        [
            (header::CONTENT_TYPE, "application/octet-stream".to_string()),
            (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{filename}\"")),
            (header::CONTENT_LENGTH, db_len.to_string()),
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
            backup.run_to_completion(-1, Duration::from_millis(0), None).map_err(|e| e.to_string())
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
    let mut dcr_tariffs_csv:      Option<Vec<u8>> = None;
    let mut dcr_item_types_csv:   Option<Vec<u8>> = None;
    let mut dcr_formula_csv:      Option<Vec<u8>> = None;
    let mut dcr_settings_csv:     Option<Vec<u8>> = None;
    let mut dcr_sessions_csv:     Option<Vec<u8>> = None;
    let mut dcr_entries_csv:      Option<Vec<u8>> = None;
    let mut dcr_dr_entries_csv:   Option<Vec<u8>> = None;
    let mut dcr_os_entries_csv:   Option<Vec<u8>> = None;

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
        if name.contains("cops_master")        { master_csv = Some(buf); }
        else if name.contains("cops_items")    { items_csv = Some(buf); }
        else if name.contains("dcr_tariffs")   { dcr_tariffs_csv = Some(buf); }
        else if name.contains("dcr_item_types"){ dcr_item_types_csv = Some(buf); }
        else if name.contains("dcr_formula")   { dcr_formula_csv = Some(buf); }
        else if name.contains("dcr_settings")  { dcr_settings_csv = Some(buf); }
        else if name.contains("dcr_sessions")  { dcr_sessions_csv = Some(buf); }
        else if name.contains("dcr_dr_entries"){ dcr_dr_entries_csv = Some(buf); }
        else if name.contains("dcr_os_entries"){ dcr_os_entries_csv = Some(buf); }
        else if name.contains("dcr_entries")   { dcr_entries_csv = Some(buf); }
    }

    // Return 400 if upload contains no recognized tables at all
    let has_any = master_csv.is_some() || items_csv.is_some()
        || dcr_tariffs_csv.is_some() || dcr_sessions_csv.is_some()
        || dcr_entries_csv.is_some() || dcr_settings_csv.is_some();
    if !has_any {
        return Err(e400("ZIP does not contain any recognised COPS backup files"));
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
                    deleted_by, deleted_reason, deleted_on,
                    pax_name_modified_by_vig, quashed, rejected
                ) VALUES (
                    ?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,
                    ?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,
                    ?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?
                )",
                rusqlite::params![
                    os_no, os_year,
                    parse_date(get("os_date")),
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
                    opt(get("pax_name_modified_by_vig")), opt(get("quashed")), opt(get("rejected")),
                ],
            ).map_err(|e| e500(&e.to_string()))?;
            existing.insert(key);
            if conn.changes() > 0 { master_inserted += 1; } else { master_skipped += 1; }
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
            // changes() == 0 means INSERT OR IGNORE skipped a duplicate — no pre-check needed.
            if conn.changes() > 0 { items_inserted += 1; } else { items_skipped += 1; }
        }
        conn.execute_batch("COMMIT").map_err(|e| e500(&e.to_string()))?;
    }

    // ── Restore DCR (Duty Collection Register) tables ─────────────────────────
    let mut dcr_tariffs_ins  = 0i64;
    let mut dcr_sessions_ins = 0i64;
    let mut dcr_entries_ins  = 0i64;

    if dcr_tariffs_csv.is_some() || dcr_sessions_csv.is_some() || dcr_entries_csv.is_some()
       || dcr_item_types_csv.is_some() || dcr_formula_csv.is_some()
       || dcr_settings_csv.is_some() || dcr_dr_entries_csv.is_some() || dcr_os_entries_csv.is_some()
    {
        conn.execute_batch("BEGIN").map_err(|e| e500(&e.to_string()))?;

        // 1. Tariffs — natural key: effective_from (UNIQUE in schema)
        let mut tariff_eff_to_id: std::collections::HashMap<String, i64> = {
            let mut st = conn.prepare("SELECT effective_from, id FROM dcr_tariffs")
                .map_err(|e| e500(&e.to_string()))?;
            let rows: Vec<(String, i64)> = st.query_map([], |r| Ok((r.get::<_,String>(0)?, r.get::<_,i64>(1)?)))
                .map_err(|e| e500(&e.to_string()))?.filter_map(|r| r.ok()).collect();
            rows.into_iter().collect()
        };
        if let Some(bytes) = dcr_tariffs_csv {
            if let Ok(raw) = decode_csv_bytes(&bytes) {
                let mut rdr = csv::ReaderBuilder::new().has_headers(true).from_reader(raw.as_bytes());
                let hdr = rdr.headers().cloned().unwrap_or_default();
                for result in rdr.records() {
                    let rec = match result { Ok(r) => r, Err(_) => continue };
                    macro_rules! g { ($n:expr) => { hdr.iter().position(|f| f.eq_ignore_ascii_case($n)).and_then(|i| rec.get(i)).unwrap_or("").trim() }; }
                    let eff = g!("effective_from").to_string();
                    if eff.is_empty() || tariff_eff_to_id.contains_key(&eff) { continue; }
                    conn.execute(
                        "INSERT OR IGNORE INTO dcr_tariffs
                          (effective_from,label,baggage_rate,liquor_duty_rate,aidc_liquor_rate,
                           gold_bcd_rate,aidc_gold_rate,gold_cons_bcd_rate,aidc_gold_cons_rate,
                           silver_bcd_rate,aidc_silver_rate,silver_cons_rate,aidc_silver_cons_rate)
                         VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)",
                        rusqlite::params![
                            eff, opt(g!("label")),
                            parse_float(g!("baggage_rate")), parse_float(g!("liquor_duty_rate")),
                            parse_float(g!("aidc_liquor_rate")), parse_float(g!("gold_bcd_rate")),
                            parse_float(g!("aidc_gold_rate")), parse_float(g!("gold_cons_bcd_rate")),
                            parse_float(g!("aidc_gold_cons_rate")), parse_float(g!("silver_bcd_rate")),
                            parse_float(g!("aidc_silver_rate")), parse_float(g!("silver_cons_rate")),
                            parse_float(g!("aidc_silver_cons_rate")),
                        ],
                    ).ok();
                    if conn.changes() > 0 {
                        tariff_eff_to_id.insert(eff, conn.last_insert_rowid());
                        dcr_tariffs_ins += 1;
                    }
                }
            }
        }
        // Sync map with anything already in DB (handles pre-existing rows)
        {
            let mut st = conn.prepare("SELECT effective_from, id FROM dcr_tariffs")
                .map_err(|e| e500(&e.to_string()))?;
            let rows: Vec<(String, i64)> = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .map_err(|e| e500(&e.to_string()))?.filter_map(|r| r.ok()).collect();
            for (eff, id) in rows { tariff_eff_to_id.entry(eff).or_insert(id); }
        }

        // 2. Item types — natural key: name
        if let Some(bytes) = dcr_item_types_csv {
            if let Ok(raw) = decode_csv_bytes(&bytes) {
                let mut rdr = csv::ReaderBuilder::new().has_headers(true).from_reader(raw.as_bytes());
                let hdr = rdr.headers().cloned().unwrap_or_default();
                for result in rdr.records() {
                    let rec = match result { Ok(r) => r, Err(_) => continue };
                    macro_rules! g { ($n:expr) => { hdr.iter().position(|f| f.eq_ignore_ascii_case($n)).and_then(|i| rec.get(i)).unwrap_or("").trim() }; }
                    let name = g!("name");
                    if name.is_empty() { continue; }
                    conn.execute(
                        "INSERT OR IGNORE INTO dcr_item_types (name,usage_count,is_system) VALUES (?,?,?)",
                        rusqlite::params![name, parse_i64(g!("usage_count")), parse_i64(g!("is_system"))],
                    ).ok();
                }
            }
        }

        // 3. Formula rules — natural key: (sort_order, target_column)
        if let Some(bytes) = dcr_formula_csv {
            if let Ok(raw) = decode_csv_bytes(&bytes) {
                let existing_rules: std::collections::HashSet<(i64, String)> = {
                    let mut st = conn.prepare("SELECT sort_order,target_column FROM dcr_formula_rules")
                        .map_err(|e| e500(&e.to_string()))?;
                    let rows: Vec<(i64, String)> = st.query_map([], |r| Ok((r.get::<_,i64>(0)?, r.get::<_,String>(1)?)))
                        .map_err(|e| e500(&e.to_string()))?.filter_map(|r| r.ok()).collect();
                    rows.into_iter().collect()
                };
                let mut rdr = csv::ReaderBuilder::new().has_headers(true).from_reader(raw.as_bytes());
                let hdr = rdr.headers().cloned().unwrap_or_default();
                for result in rdr.records() {
                    let rec = match result { Ok(r) => r, Err(_) => continue };
                    macro_rules! g { ($n:expr) => { hdr.iter().position(|f| f.eq_ignore_ascii_case($n)).and_then(|i| rec.get(i)).unwrap_or("").trim() }; }
                    let so = parse_i64(g!("sort_order")).unwrap_or(0);
                    let tc = g!("target_column").to_string();
                    if tc.is_empty() || existing_rules.contains(&(so, tc.clone())) { continue; }
                    let ct = g!("condition_type");
                    conn.execute(
                        "INSERT INTO dcr_formula_rules
                          (sort_order,target_column,column_label,condition_type,
                           condition_items,expression,is_active,notes)
                         VALUES (?,?,?,?,?,?,?,?)",
                        rusqlite::params![
                            so, tc, opt(g!("column_label")),
                            if ct.is_empty() { "all" } else { ct },
                            g!("condition_items"), g!("expression"),
                            parse_i64(g!("is_active")).unwrap_or(1), opt(g!("notes")),
                        ],
                    ).ok();
                }
            }
        }

        // 4. Settings — single row, insert-only if absent (preserve existing config)
        if let Some(bytes) = dcr_settings_csv {
            if let Ok(raw) = decode_csv_bytes(&bytes) {
                let mut rdr = csv::ReaderBuilder::new().has_headers(true).from_reader(raw.as_bytes());
                let hdr = rdr.headers().cloned().unwrap_or_default();
                if let Some(Ok(rec)) = rdr.records().next() {
                    macro_rules! g { ($n:expr) => { hdr.iter().position(|f| f.eq_ignore_ascii_case($n)).and_then(|i| rec.get(i)).unwrap_or("").trim() }; }
                    conn.execute(
                        "INSERT OR IGNORE INTO dcr_settings (id,station_name,officer_name,designation)
                         VALUES (1,?,?,?)",
                        rusqlite::params![g!("station_name"), opt(g!("officer_name")), opt(g!("designation"))],
                    ).ok();
                }
            }
        }

        // 5. Sessions — natural key: (report_date, shift, COALESCE(batch_name,''))
        let mut sess_nk_to_id: std::collections::HashMap<(String,String,String), i64> = {
            let mut st = conn.prepare(
                "SELECT report_date,shift,COALESCE(batch_name,''),id FROM dcr_sessions"
            ).map_err(|e| e500(&e.to_string()))?;
            let rows: Vec<((String,String,String), i64)> = st.query_map([], |r| Ok(((r.get::<_,String>(0)?, r.get::<_,String>(1)?, r.get::<_,String>(2)?), r.get::<_,i64>(3)?)))
                .map_err(|e| e500(&e.to_string()))?.filter_map(|r| r.ok()).collect();
            rows.into_iter().collect()
        };
        if let Some(bytes) = dcr_sessions_csv {
            if let Ok(raw) = decode_csv_bytes(&bytes) {
                let mut rdr = csv::ReaderBuilder::new().has_headers(true).from_reader(raw.as_bytes());
                let hdr = rdr.headers().cloned().unwrap_or_default();
                for result in rdr.records() {
                    let rec = match result { Ok(r) => r, Err(_) => continue };
                    macro_rules! g { ($n:expr) => { hdr.iter().position(|f| f.eq_ignore_ascii_case($n)).and_then(|i| rec.get(i)).unwrap_or("").trim() }; }
                    let rd = g!("report_date").to_string();
                    let sh = g!("shift").to_string();
                    let bn = g!("batch_name").to_string();
                    if rd.is_empty() || sh.is_empty() { continue; }
                    let nk = (rd.clone(), sh.clone(), bn.clone());
                    if sess_nk_to_id.contains_key(&nk) { continue; }
                    let tariff_id = { let e = g!("_tariff_eff"); if e.is_empty() { None } else { tariff_eff_to_id.get(e).copied() } };
                    conn.execute(
                        "INSERT INTO dcr_sessions
                          (report_date,shift,batch_name,tariff_id,created_by,
                           created_at,submitted_at,submitted_by)
                         VALUES (?,?,?,?,?,?,?,?)",
                        rusqlite::params![
                            rd, sh,
                            if bn.is_empty() { None } else { Some(bn.clone()) },
                            tariff_id, opt(g!("created_by")), opt(g!("created_at")),
                            opt(g!("submitted_at")), opt(g!("submitted_by")),
                        ],
                    ).ok();
                    if conn.changes() > 0 {
                        sess_nk_to_id.insert(nk, conn.last_insert_rowid());
                        dcr_sessions_ins += 1;
                    }
                }
            }
        }
        // Sync map with all sessions now in DB
        {
            let mut st = conn.prepare(
                "SELECT report_date,shift,COALESCE(batch_name,''),id FROM dcr_sessions"
            ).map_err(|e| e500(&e.to_string()))?;
            let rows: Vec<((String,String,String),i64)> = st.query_map([], |r| {
                Ok(((r.get::<_,String>(0)?, r.get::<_,String>(1)?, r.get::<_,String>(2)?), r.get::<_,i64>(3)?))
            }).map_err(|e| e500(&e.to_string()))?.filter_map(|r| r.ok()).collect();
            for (nk, id) in rows { sess_nk_to_id.entry(nk).or_insert(id); }
        }

        // 6. Main duty entries
        if let Some(bytes) = dcr_entries_csv {
            if let Ok(raw) = decode_csv_bytes(&bytes) {
                let mut rdr = csv::ReaderBuilder::new().has_headers(true).from_reader(raw.as_bytes());
                let hdr = rdr.headers().cloned().unwrap_or_default();
                let existing_ent: std::collections::HashSet<(i64,i64)> = {
                    let mut st = conn.prepare("SELECT session_id, sort_order FROM dcr_entries")
                        .map_err(|e| e500(&e.to_string()))?;
                    let rows: Vec<(i64,i64)> = st.query_map([], |r| Ok((r.get::<_,i64>(0)?, r.get::<_,i64>(1)?)))
                        .map_err(|e| e500(&e.to_string()))?.filter_map(|r| r.ok()).collect();
                    rows.into_iter().collect()
                };
                for result in rdr.records() {
                    let rec = match result { Ok(r) => r, Err(_) => continue };
                    let f = |name: &str| -> &str { hdr.iter().position(|h| h.eq_ignore_ascii_case(name)).and_then(|i| rec.get(i)).unwrap_or("").trim() };
                    let sd = f("_sess_date").to_string(); let ss = f("_sess_shift").to_string(); let sb = f("_sess_batch").to_string();
                    let so = parse_i64(f("sort_order")).unwrap_or(0);
                    let sid = match sess_nk_to_id.get(&(sd, ss, sb)) { Some(&id) => id, None => continue };
                    if existing_ent.contains(&(sid, so)) { continue; }
                    conn.execute(
                        "INSERT OR IGNORE INTO dcr_entries
                          (session_id,sort_order,sl_no,br_no,os_ref,item_desc,
                           dutiable_value,gold_weight_gms,baggage_duty,liquor_duty,cigarette_duty,sw_sc,
                           gold_duty_bcd,gold_duty_cons,silver_duty_cons,sws_on_gold,
                           aidc_gold_silver,sws_on_silver,aidc_on_liquor,
                           redemption_fine,reexport_fine,personal_penalty,other_charges,
                           fuel_duty,total_duty,flight_no,is_sbi_challan,is_offline_br,overrides)
                         VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
                        rusqlite::params![
                            sid, so, parse_i64(f("sl_no")),
                            f("br_no"), f("os_ref"), f("item_desc"),
                            parse_float(f("dutiable_value")), parse_float(f("gold_weight_gms")),
                            parse_float(f("baggage_duty")), parse_float(f("liquor_duty")),
                            parse_float(f("cigarette_duty")), parse_float(f("sw_sc")),
                            parse_float(f("gold_duty_bcd")), parse_float(f("gold_duty_cons")),
                            parse_float(f("silver_duty_cons")), parse_float(f("sws_on_gold")),
                            parse_float(f("aidc_gold_silver")), parse_float(f("sws_on_silver")),
                            parse_float(f("aidc_on_liquor")), parse_float(f("redemption_fine")),
                            parse_float(f("reexport_fine")), parse_float(f("personal_penalty")),
                            parse_float(f("other_charges")), parse_float(f("fuel_duty")),
                            parse_float(f("total_duty")), f("flight_no"),
                            parse_i64(f("is_sbi_challan")).unwrap_or(0),
                            parse_i64(f("is_offline_br")).unwrap_or(0),
                            opt(f("overrides")),
                        ],
                    ).ok();
                    if conn.changes() > 0 { dcr_entries_ins += 1; }
                }
            }
        }

        // 7. DR sub-entries
        if let Some(bytes) = dcr_dr_entries_csv {
            if let Ok(raw) = decode_csv_bytes(&bytes) {
                let mut rdr = csv::ReaderBuilder::new().has_headers(true).from_reader(raw.as_bytes());
                let hdr = rdr.headers().cloned().unwrap_or_default();
                let existing_dr: std::collections::HashSet<(i64,i64)> = {
                    let mut st = conn.prepare("SELECT session_id, sort_order FROM dcr_dr_entries")
                        .map_err(|e| e500(&e.to_string()))?;
                    let rows: Vec<(i64,i64)> = st.query_map([], |r| Ok((r.get::<_,i64>(0)?, r.get::<_,i64>(1)?)))
                        .map_err(|e| e500(&e.to_string()))?.filter_map(|r| r.ok()).collect();
                    rows.into_iter().collect()
                };
                for result in rdr.records() {
                    let rec = match result { Ok(r) => r, Err(_) => continue };
                    let f = |name: &str| -> &str { hdr.iter().position(|h| h.eq_ignore_ascii_case(name)).and_then(|i| rec.get(i)).unwrap_or("").trim() };
                    let sd = f("_sess_date").to_string(); let ss = f("_sess_shift").to_string(); let sb = f("_sess_batch").to_string();
                    let so = parse_i64(f("sort_order")).unwrap_or(0);
                    let sid = match sess_nk_to_id.get(&(sd, ss, sb)) { Some(&id) => id, None => continue };
                    if existing_dr.contains(&(sid, so)) { continue; }
                    conn.execute(
                        "INSERT OR IGNORE INTO dcr_dr_entries (session_id,sort_order,dr_no,amount,item_desc,remarks)
                         VALUES (?,?,?,?,?,?)",
                        rusqlite::params![sid, so, f("dr_no"), parse_float(f("amount")), f("item_desc"), f("remarks")],
                    ).ok();
                    if conn.changes() > 0 { dcr_entries_ins += 1; }
                }
            }
        }

        // 8. OS sub-entries
        if let Some(bytes) = dcr_os_entries_csv {
            if let Ok(raw) = decode_csv_bytes(&bytes) {
                let mut rdr = csv::ReaderBuilder::new().has_headers(true).from_reader(raw.as_bytes());
                let hdr = rdr.headers().cloned().unwrap_or_default();
                let existing_os: std::collections::HashSet<(i64,i64)> = {
                    let mut st = conn.prepare("SELECT session_id, sort_order FROM dcr_os_entries")
                        .map_err(|e| e500(&e.to_string()))?;
                    let rows: Vec<(i64,i64)> = st.query_map([], |r| Ok((r.get::<_,i64>(0)?, r.get::<_,i64>(1)?)))
                        .map_err(|e| e500(&e.to_string()))?.filter_map(|r| r.ok()).collect();
                    rows.into_iter().collect()
                };
                for result in rdr.records() {
                    let rec = match result { Ok(r) => r, Err(_) => continue };
                    let f = |name: &str| -> &str { hdr.iter().position(|h| h.eq_ignore_ascii_case(name)).and_then(|i| rec.get(i)).unwrap_or("").trim() };
                    let sd = f("_sess_date").to_string(); let ss = f("_sess_shift").to_string(); let sb = f("_sess_batch").to_string();
                    let so = parse_i64(f("sort_order")).unwrap_or(0);
                    let sid = match sess_nk_to_id.get(&(sd, ss, sb)) { Some(&id) => id, None => continue };
                    if existing_os.contains(&(sid, so)) { continue; }
                    conn.execute(
                        "INSERT OR IGNORE INTO dcr_os_entries (session_id,sort_order,os_no,amount,item_desc,remarks)
                         VALUES (?,?,?,?,?,?)",
                        rusqlite::params![sid, so, f("os_no"), parse_float(f("amount")), f("item_desc"), f("remarks")],
                    ).ok();
                    if conn.changes() > 0 { dcr_entries_ins += 1; }
                }
            }
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
        "dcr_tariffs_inserted":  dcr_tariffs_ins,
        "dcr_sessions_inserted": dcr_sessions_ins,
        "dcr_entries_inserted":  dcr_entries_ins,
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
    "items_release_category","confiscation_type",
    "value_per_piece","cumulative_duty_rate",
];

/// Human-readable label derived from items_release_category (kept for reference).
#[allow(dead_code)]
fn confiscation_label(rc: &str) -> &'static str {
    match rc.trim().to_uppercase().as_str() {
        "CONFS"      => "Absolute Confiscation",
        "RF"         => "Confiscation",
        "REF"        => "Re-Export",
        "UNDER OS"   => "Under OS (Seized)",
        "UNDER DUTY" => "Dutiable",
        _            => "",
    }
}

/// Token-overlap name similarity (0..1). Mirrors Python _name_score() and frontend nameScore().
fn name_score(a: &str, b: &str) -> f64 {
    if a.is_empty() || b.is_empty() { return 0.0; }
    let ta: std::collections::HashSet<String> = a.to_uppercase().split_whitespace().map(String::from).collect();
    let tb: std::collections::HashSet<String> = b.to_uppercase().split_whitespace().map(String::from).collect();
    let overlap = ta.iter().filter(|t| tb.contains(*t)).count() as f64;
    overlap / ta.len().max(tb.len()).max(1) as f64
}

#[derive(Deserialize)]
pub struct OsListItem {
    os_no:    String,
    #[serde(default)]
    os_year:  Option<i64>,   // null = year unknown; backend resolves via pax_name
    #[serde(default)]
    pax_name: Option<String>,
}

#[derive(Deserialize)]
pub struct CustomReportRequest {
    master_cols: Vec<String>,
    #[serde(default)]
    item_cols: Vec<String>,
    from_date: Option<String>,
    to_date:   Option<String>,
    case_type: Option<String>,
    // Row-level filters (used when os_list is absent)
    #[serde(default)] os_no:         Option<String>,
    #[serde(default)] os_year:       Option<i64>,
    #[serde(default)] adj_offr_name: Option<String>,
    #[serde(default)] flight_no:     Option<String>,
    #[serde(default)] pax_name:      Option<String>,
    #[serde(default)] passport_no:   Option<String>,
    #[serde(default)] item_desc:     Option<String>,
    // Excel-upload batch: if provided, only these (os_no, os_year) pairs are returned
    #[serde(default)]
    os_list: Vec<OsListItem>,
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

    if body.os_list.len() > 2000 {
        return Err(e400("os_list cannot exceed 2000 items per request."));
    }

    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let include_items = !body.item_cols.is_empty();

    // Build parameterized WHERE clause
    let mut conditions = vec!["cm.entry_deleted = 'N'".to_string()];
    let mut params: Vec<String> = Vec::new();
    let mut resolved_by_name_list: Vec<Value> = Vec::new();
    let mut unresolved_list: Vec<Value> = Vec::new();
    let mut resolved_pairs: Vec<(String, i64)> = Vec::new();

    if !body.os_list.is_empty() {
        // Split into known-year (exact) and unknown-year (fuzzy) items
        let exact: Vec<(String, i64)> = body.os_list.iter()
            .filter_map(|item| item.os_year.map(|yr| (item.os_no.clone(), yr)))
            .collect();
        let fuzzy: Vec<&OsListItem> = body.os_list.iter()
            .filter(|item| item.os_year.is_none())
            .collect();
        resolved_pairs.extend(exact);

        if !fuzzy.is_empty() {
            let fuzzy_nos: Vec<&str> = {
                let mut seen = std::collections::HashSet::new();
                fuzzy.iter().filter_map(|item| {
                    if seen.insert(item.os_no.as_str()) { Some(item.os_no.as_str()) } else { None }
                }).collect()
            };
            let placeholders = fuzzy_nos.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            let cand_sql = format!(
                "SELECT os_no, os_year, pax_name FROM cops_master WHERE entry_deleted='N' AND os_no IN ({placeholders})"
            );
            let mut cstmt = conn.prepare(&cand_sql).map_err(|e| e500(&e.to_string()))?;
            let candidates: Vec<(String, i64, String)> = cstmt.query_map(
                rusqlite::params_from_iter(fuzzy_nos.iter()),
                |row| Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                )),
            ).map_err(|e| e500(&e.to_string()))?.filter_map(|r| r.ok()).collect();

            let mut cands_map: std::collections::HashMap<String, Vec<(i64, String)>> =
                std::collections::HashMap::new();
            for (no, yr, name) in candidates {
                cands_map.entry(no).or_default().push((yr, name));
            }

            for item in &fuzzy {
                let cands = cands_map.get(&item.os_no).map(|v| v.as_slice()).unwrap_or(&[]);
                if cands.is_empty() {
                    unresolved_list.push(json!({ "os_no": item.os_no, "os_year": null, "reason": "not_found" }));
                } else if cands.len() == 1 {
                    resolved_pairs.push((item.os_no.clone(), cands[0].0));
                    resolved_by_name_list.push(json!({
                        "os_no": item.os_no, "resolved_year": cands[0].0,
                        "matched_name": cands[0].1, "method": "unique"
                    }));
                } else if let Some(pax) = &item.pax_name {
                    let best = cands.iter().max_by(|a, b| {
                        name_score(pax, &a.1).partial_cmp(&name_score(pax, &b.1))
                            .unwrap_or(std::cmp::Ordering::Equal)
                    }).unwrap();
                    let score = name_score(pax, &best.1);
                    if score >= 0.3 {
                        resolved_pairs.push((item.os_no.clone(), best.0));
                        resolved_by_name_list.push(json!({
                            "os_no": item.os_no, "resolved_year": best.0,
                            "matched_name": best.1,
                            "score": (score * 100.0).round() / 100.0,
                            "method": "name_match"
                        }));
                    } else {
                        unresolved_list.push(json!({ "os_no": item.os_no, "os_year": null, "reason": "low_confidence" }));
                    }
                } else {
                    unresolved_list.push(json!({ "os_no": item.os_no, "os_year": null, "reason": "ambiguous" }));
                }
            }
        }

        if resolved_pairs.is_empty() {
            let all_cols: Vec<String> = body.master_cols.iter().chain(body.item_cols.iter()).cloned().collect();
            return Ok(Json(json!({
                "columns": all_cols, "rows": [], "total": 0,
                "not_found": unresolved_list, "resolved_by_name": resolved_by_name_list
            })));
        }

        let or_parts = resolved_pairs.iter().map(|_| "(cm.os_no=? AND cm.os_year=?)").collect::<Vec<_>>().join(" OR ");
        conditions.push(format!("({or_parts})"));
        for (no, yr) in &resolved_pairs {
            params.push(no.clone());
            params.push(yr.to_string());
        }
    } else {
        // Filter-based mode
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
    }
    let where_clause = conditions.join(" AND ");

    // Always include os_no and os_year at positions 0 and 1 as keys for the items join.
    let mut query_master_cols = vec!["os_no".to_string(), "os_year".to_string()];
    for c in &body.master_cols {
        if c != "os_no" && c != "os_year" { query_master_cols.push(c.clone()); }
    }
    let master_sel = query_master_cols.iter().map(|c| format!("cm.{c}")).collect::<Vec<_>>().join(", ");
    let master_sql = format!(
        "SELECT {master_sel} FROM cops_master cm WHERE {where_clause} ORDER BY cm.os_year, CAST(cm.os_no AS INTEGER) LIMIT 10000"
    );

    let qmc_len = query_master_cols.len();
    let mut stmt = conn.prepare(&master_sql).map_err(|e| e500(&e.to_string()))?;
    let master_rows: Vec<Vec<String>> = stmt.query_map(
        rusqlite::params_from_iter(params.iter()),
        |row| Ok((0..qmc_len).map(|i| val_to_str(row.get_ref(i).unwrap_or(rusqlite::types::ValueRef::Null))).collect()),
    ).map_err(|e| e500(&e.to_string()))?.filter_map(|r| r.ok()).collect();

    // Bulk-load items (OR-chain, chunked at 80) and aggregate per OS with \n separator.
    // confiscation_type is a computed column — replace with SQL CASE expression.
    let mut items_map: std::collections::HashMap<(String, String), Vec<Vec<String>>> = std::collections::HashMap::new();
    if include_items && !master_rows.is_empty() {
        let item_sel = body.item_cols.iter().map(|c| {
            if c == "confiscation_type" {
                "CASE COALESCE(TRIM(UPPER(ci.items_release_category)),'') \
                  WHEN 'CONFS' THEN 'Absolute Confiscation' \
                  WHEN 'RF' THEN 'Confiscation' \
                  WHEN 'REF' THEN 'Re-Export' \
                  WHEN 'UNDER OS' THEN 'Under OS (Seized)' \
                  WHEN 'UNDER DUTY' THEN 'Dutiable' \
                  ELSE COALESCE(ci.items_release_category,'') END".to_string()
            } else {
                format!("ci.{c}")
            }
        }).collect::<Vec<_>>().join(", ");
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

    // When os_list was provided, report which pairs were not found.
    let not_found: Vec<Value>;
    if !body.os_list.is_empty() {
        let found: std::collections::HashSet<(String, String)> =
            master_rows.iter().map(|r| (r[0].clone(), r[1].clone())).collect();
        let db_not_found: Vec<Value> = resolved_pairs.iter()
            .filter(|(no, yr)| !found.contains(&(no.clone(), yr.to_string())))
            .map(|(no, yr)| json!({ "os_no": no, "os_year": yr, "reason": "not_found" }))
            .collect();
        not_found = unresolved_list.into_iter().chain(db_not_found.into_iter()).collect();
    } else {
        not_found = vec![];
    };

    Ok(Json(json!({
        "columns": all_cols, "rows": json_rows, "total": json_rows.len(),
        "not_found": not_found, "resolved_by_name": resolved_by_name_list
    })))
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
