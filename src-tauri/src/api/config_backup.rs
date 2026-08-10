//! Admin configuration backup and restore.
//!
//! WHAT THIS IS FOR
//! Moving an office's SETTINGS to another installation without carrying a single
//! case record: print templates, baggage rules, special allowances, statutes,
//! the masters, and the user accounts. It is how a new machine is seeded so it
//! prints and calculates exactly like the one already in service, and it is the
//! reason the file is small enough to email.
//!
//! It is NOT a data backup. `backup_export` does that. Keeping the two apart is
//! deliberate: this one is meant to be carried between machines, so it must
//! never contain passengers' details.
//!
//! COMPATIBLE WITH THE PYTHON VERSION
//! The format matches cops-web's `/admin/config/backup` exactly — same
//! `format_version`, same table keys, same singleton keys — so a file exported
//! from the office today restores into COPS2. That is the whole point: the
//! settings already in service are the ones worth keeping.
//!
//! RESTORE IS INSERT-ONLY
//! A row whose natural key already exists is left alone, and singletons are
//! written only when missing. Restoring must never overwrite settings that have
//! been tuned on the destination — an import that silently replaced a live
//! print template with an older one would be discovered at the counter.
//!
//! SCHEMA DRIFT IS EXPECTED, NOT AN ERROR
//! Columns are read from the DESTINATION with `PRAGMA table_info` and only the
//! ones that exist on both sides are written. The two programs' schemas have
//! already diverged — cops-web keeps `feature_flags` as one wide row while
//! COPS2 keeps key/value pairs — and that is handled explicitly below rather
//! than left to fail at the first mismatched column.

use std::sync::Arc;

use axum::{extract::State, http::StatusCode, Json};
use serde_json::{json, Map, Value};

use crate::{auth::AdminUser, db::DbPool};

type Db = State<Arc<DbPool>>;
type Err = (StatusCode, Json<Value>);

fn e400(m: &str) -> Err { (StatusCode::BAD_REQUEST, Json(json!({ "detail": m }))) }
fn e500(m: &str) -> Err { (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "detail": m }))) }

/// Format version, matching cops-web. Bump only on a breaking change.
const FORMAT_VERSION: i64 = 1;

/// (table, natural key columns). The natural key is what makes restore
/// idempotent — without one, importing the same file twice doubles every row.
const CONFIG_TABLES: &[(&str, &[&str])] = &[
    // Versioned content — keyed by what it is PLUS when it took effect, so a
    // new rate for the same rule is a new row rather than a replacement.
    ("print_template_config",   &["field_key", "effective_from"]),
    ("baggage_rules_config",    &["rule_key", "effective_from"]),
    ("special_item_allowances", &["item_name", "effective_from"]),
    // Lookups
    ("legal_statutes",          &["keyword"]),
    ("dc_master",               &["dc_code"]),
    ("airlines_mast",           &["airline_code"]),
    ("arrival_flight_master",   &["flight_no", "airline_code"]),
    ("airport_master",          &["airport_name"]),
    ("nationality_master",      &["nationality"]),
    ("port_master",             &["port_of_departure"]),
    ("item_cat_master",         &["category_code"]),
    ("duty_rate_master",        &["duty_category", "from_date"]),
    ("br_no_limits",            &["br_type"]),
    // Accounts. Passwords are already bcrypt hashes; the plaintext is not here
    // and cannot be recovered from this file.
    ("users",                   &["user_id"]),
];

/// Single-row tables, exported as one object rather than a list.
const CONFIG_SINGLETONS: &[&str] = &["feature_flags", "shift_timing_master", "margin_master"];

/// Columns a destination table actually has, excluding the surrogate id.
fn columns_of(conn: &rusqlite::Connection, table: &str) -> Vec<String> {
    let Ok(mut stmt) = conn.prepare(&format!("PRAGMA table_info(\"{table}\")")) else {
        return Vec::new();
    };
    stmt.query_map([], |r| r.get::<_, String>(1))
        .map(|rows| rows.filter_map(|r| r.ok()).filter(|c| c != "id").collect())
        .unwrap_or_default()
}

fn cell(row: &rusqlite::Row, idx: usize) -> Value {
    use rusqlite::types::ValueRef;
    match row.get_ref(idx) {
        Ok(ValueRef::Null) => Value::Null,
        Ok(ValueRef::Integer(i)) => json!(i),
        Ok(ValueRef::Real(f)) => json!(f),
        Ok(ValueRef::Text(t)) => json!(String::from_utf8_lossy(t)),
        Ok(ValueRef::Blob(_)) | Err(_) => Value::Null,
    }
}

/// Is this table COPS2's key/value flavour rather than one wide row?
fn is_key_value(cols: &[String]) -> bool {
    cols.iter().any(|c| c == "config_key") && cols.iter().any(|c| c == "config_value")
}

fn read_table(conn: &rusqlite::Connection, table: &str) -> Vec<Value> {
    let cols = columns_of(conn, table);
    if cols.is_empty() {
        return Vec::new();
    }
    let list = cols.iter().map(|c| format!("\"{c}\"")).collect::<Vec<_>>().join(", ");
    let Ok(mut stmt) = conn.prepare(&format!("SELECT {list} FROM \"{table}\"")) else {
        return Vec::new();
    };
    stmt.query_map([], |r| {
        let mut o = Map::new();
        for (i, c) in cols.iter().enumerate() {
            o.insert(c.clone(), cell(r, i));
        }
        Ok(Value::Object(o))
    })
    .map(|rows| rows.filter_map(|r| r.ok()).collect())
    .unwrap_or_default()
}

/// A singleton as one flat object, whichever shape the table has.
fn read_singleton(conn: &rusqlite::Connection, table: &str) -> Value {
    let cols = columns_of(conn, table);
    if cols.is_empty() {
        return Value::Null;
    }
    if is_key_value(&cols) {
        // COPS2's shape. Flattened to the same object cops-web produces, so the
        // exported file is interchangeable between the two programs.
        let Ok(mut stmt) = conn.prepare(
            &format!("SELECT config_key, config_value FROM \"{table}\""),
        ) else { return Value::Null };
        let mut o = Map::new();
        if let Ok(rows) = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        }) {
            for (k, v) in rows.flatten() {
                // Keep numbers and booleans as themselves so a round trip does
                // not turn 480 into "480".
                let parsed = serde_json::from_str::<Value>(&v).unwrap_or(Value::String(v));
                o.insert(k, parsed);
            }
        }
        return Value::Object(o);
    }
    read_table(conn, table).into_iter().next().unwrap_or(Value::Null)
}

/// Download every admin-editable setting as JSON. No case data.
pub async fn config_backup(State(pool): Db, _admin: AdminUser) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;

    let mut tables = Map::new();
    for (t, _) in CONFIG_TABLES {
        tables.insert((*t).to_string(), Value::Array(read_table(&conn, t)));
    }
    let mut singletons = Map::new();
    for t in CONFIG_SINGLETONS {
        singletons.insert((*t).to_string(), read_singleton(&conn, t));
    }

    Ok(Json(json!({
        "format_version": FORMAT_VERSION,
        "exported_at":    chrono::Utc::now().to_rfc3339(),
        "kind":           "admin_config_only",
        "note":           "Admin-editable config + masters + users + settings. \
                           No OS/BR/DR/warehouse case data. \
                           Restore is INSERT-only by natural key.",
        "tables":         tables,
        "singletons":     singletons,
    })))
}

/// Restore settings from a file produced by either program.
pub async fn config_restore(
    State(pool): Db,
    _admin: AdminUser,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, Err> {
    let version = payload.get("format_version").and_then(|v| v.as_i64()).unwrap_or(0);
    if version != FORMAT_VERSION {
        return Err(e400(&format!(
            "This file is format version {version}; this build reads version {FORMAT_VERSION}."
        )));
    }
    if payload.get("kind").and_then(|v| v.as_str()) != Some("admin_config_only") {
        return Err(e400(
            "This is not an admin configuration file. Use Restore Backup for case data.",
        ));
    }

    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let mut added = Map::new();
    let mut skipped = Map::new();

    for (table, keys) in CONFIG_TABLES {
        let rows = payload
            .get("tables")
            .and_then(|t| t.get(*table))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        if rows.is_empty() {
            continue;
        }
        let dest_cols = columns_of(&conn, table);
        if dest_cols.is_empty() {
            // The destination has never heard of this table. Report it rather
            // than fail the whole import — a newer file into an older build.
            skipped.insert((*table).to_string(), json!("table not present in this version"));
            continue;
        }

        // The source may hold several rows sharing one natural key — the same
        // template saved five times on the same day, which is exactly what the
        // office's file contains. Keep the LAST, because that is the most recent
        // edit for that effective date and therefore the one the original
        // program resolves to. Taking the first would restore a superseded
        // version of a print template, and the difference only shows up on
        // paper, after it has been signed.
        let mut deduped: Vec<Value> = Vec::new();
        {
            let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
            for row in rows {
                let Some(obj) = row.as_object() else { continue };
                let sig = keys
                    .iter()
                    .map(|k| obj.get(*k).map(|v| v.to_string()).unwrap_or_default())
                    .collect::<Vec<_>>()
                    .join("\u{1}");
                match seen.get(&sig) {
                    Some(&i) => deduped[i] = row.clone(),
                    None => { seen.insert(sig, deduped.len()); deduped.push(row.clone()); }
                }
            }
        }
        let rows = deduped;

        let (mut ins, mut skip) = (0i64, 0i64);
        for row in rows {
            let Some(obj) = row.as_object() else { continue };

            // Already here? Compare on the natural key only.
            let mut where_parts = Vec::new();
            let mut where_vals: Vec<String> = Vec::new();
            let mut key_ok = true;
            for k in *keys {
                match obj.get(*k) {
                    Some(v) if !v.is_null() => {
                        where_parts.push(format!("\"{k}\" = ?"));
                        where_vals.push(match v {
                            Value::String(s) => s.clone(),
                            other => other.to_string(),
                        });
                    }
                    _ => { key_ok = false; break; }
                }
            }
            if !key_ok {
                skip += 1;
                continue;
            }
            let exists: i64 = conn
                .query_row(
                    &format!(
                        "SELECT COUNT(*) FROM \"{table}\" WHERE {}",
                        where_parts.join(" AND ")
                    ),
                    rusqlite::params_from_iter(where_vals.iter()),
                    |r| r.get(0),
                )
                .unwrap_or(0);
            if exists > 0 {
                skip += 1;
                continue;
            }

            // Write only the columns BOTH sides have.
            let mut names = Vec::new();
            let mut binds: Vec<rusqlite::types::Value> = Vec::new();
            for c in &dest_cols {
                let Some(v) = obj.get(c) else { continue };
                names.push(format!("\"{c}\""));
                binds.push(match v {
                    Value::Null => rusqlite::types::Value::Null,
                    Value::Bool(b) => rusqlite::types::Value::Integer(*b as i64),
                    Value::Number(n) if n.is_i64() => rusqlite::types::Value::Integer(n.as_i64().unwrap()),
                    Value::Number(n) => rusqlite::types::Value::Real(n.as_f64().unwrap_or(0.0)),
                    Value::String(s) => rusqlite::types::Value::Text(s.clone()),
                    other => rusqlite::types::Value::Text(other.to_string()),
                });
            }
            if names.is_empty() {
                skip += 1;
                continue;
            }
            let placeholders = vec!["?"; names.len()].join(", ");
            let sql = format!(
                "INSERT INTO \"{table}\" ({}) VALUES ({placeholders})",
                names.join(", ")
            );
            match conn.execute(&sql, rusqlite::params_from_iter(binds.iter())) {
                Ok(_) => ins += 1,
                Err(_) => skip += 1,
            }
        }
        added.insert((*table).to_string(), json!(ins));
        if skip > 0 {
            skipped.insert((*table).to_string(), json!(skip));
        }
    }

    // Singletons: written only where missing, so settings tuned on THIS machine
    // are never replaced by an older file's values.
    for table in CONFIG_SINGLETONS {
        let Some(obj) = payload
            .get("singletons")
            .and_then(|s| s.get(*table))
            .and_then(|v| v.as_object())
        else { continue };
        let cols = columns_of(&conn, table);
        if cols.is_empty() {
            continue;
        }

        if is_key_value(&cols) {
            // cops-web's wide row becoming COPS2's key/value pairs. The two
            // schemas diverged; this is where they are reconciled instead of
            // the import failing on the first unknown column.
            let mut n = 0i64;
            for (k, v) in obj {
                let text = match v {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                if conn
                    .execute(
                        &format!(
                            "INSERT OR IGNORE INTO \"{table}\" (config_key, config_value) VALUES (?1, ?2)"
                        ),
                        rusqlite::params![k, text],
                    )
                    .unwrap_or(0)
                    > 0
                {
                    n += 1;
                }
            }
            added.insert((*table).to_string(), json!(n));
            continue;
        }

        let present: i64 = conn
            .query_row(&format!("SELECT COUNT(*) FROM \"{table}\""), [], |r| r.get(0))
            .unwrap_or(0);
        if present > 0 {
            skipped.insert((*table).to_string(), json!("already configured; left as it is"));
            continue;
        }
        let mut names = Vec::new();
        let mut binds: Vec<rusqlite::types::Value> = Vec::new();
        for c in &cols {
            let Some(v) = obj.get(c) else { continue };
            names.push(format!("\"{c}\""));
            binds.push(match v {
                Value::Null => rusqlite::types::Value::Null,
                Value::Bool(b) => rusqlite::types::Value::Integer(*b as i64),
                Value::Number(n) if n.is_i64() => rusqlite::types::Value::Integer(n.as_i64().unwrap()),
                Value::Number(n) => rusqlite::types::Value::Real(n.as_f64().unwrap_or(0.0)),
                Value::String(s) => rusqlite::types::Value::Text(s.clone()),
                other => rusqlite::types::Value::Text(other.to_string()),
            });
        }
        if names.is_empty() {
            continue;
        }
        let placeholders = vec!["?"; names.len()].join(", ");
        let _ = conn.execute(
            &format!("INSERT INTO \"{table}\" ({}) VALUES ({placeholders})", names.join(", ")),
            rusqlite::params_from_iter(binds.iter()),
        );
        added.insert((*table).to_string(), json!(1));
    }

    let total: i64 = added.values().filter_map(|v| v.as_i64()).sum();
    Ok(Json(json!({
        "message": format!(
            "Settings restored — {total} new record(s). Anything already present was left unchanged."
        ),
        "added":   added,
        "skipped": skipped,
    })))
}
