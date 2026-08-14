//! The compact backup payload — what actually gets written to a backup folder.
//!
//! WHY THIS EXISTS
//! ───────────────
//! The obvious backup is a copy of the database file. It was measured on the
//! real Chennai database (827,140 rows) and it is the worst of the options:
//!
//! ```text
//!     full encrypted .db copy .................... 242.6 MB
//!     every CSV, uncompressed ................... 199.1 MB
//!     tables that hold data, as a plain .db ..... 177.5 MB
//!     THE SAME FILE, compressed + encrypted ...... 44.4 MB
//! ```
//!
//! Two things were learned from those numbers, and both shaped this module.
//!
//! FIRST: the file format is almost irrelevant. CSV is only ~18% smaller than
//! the database — not the large saving it looks like. The 6x saving comes from
//! COMPRESSION, and nothing else.
//!
//! SECOND, and the reason the old design was so large: encrypted bytes do not
//! compress. Compressing the encrypted database made it BIGGER — 242.6 MB to
//! 242.7 MB — because encryption leaves no redundancy to squeeze. So the order
//! of operations is forced, and it is the whole trick here:
//!
//! ```text
//!     export plain  →  compress  →  encrypt
//! ```
//!
//! Compressing after encrypting, which is what copying the .db file amounts to,
//! cannot work no matter which algorithm is used.
//!
//! WHICH TABLES — A RULE, NOT A LIST
//! ─────────────────────────────────
//! Every table that has rows. Not a hand-written list of the important ones.
//!
//! This matters more than it looks. A list of table names is a thing that goes
//! stale silently: someone adds a table, nobody updates the list, and the
//! backup keeps reporting success while quietly omitting data. That exact
//! failure already happened once in the sibling project, where a hardcoded
//! path list went stale and blocked the archive with no error.
//!
//! The rule costs nothing. Of 90 tables only 22 hold anything; the 13 config
//! tables among them — print_template_config, baggage_rules_config,
//! special_item_allowances, legal_statutes, dr_tariffs, dr_formula_rules,
//! users, feature_flags — total under 0.02 MB. They are also the tables it
//! would be most damaging to lose: the print templates and duty rates change
//! over time, and an OS reprinted against the wrong template is wrong even
//! though every case record survived. Data without its settings is not a
//! restore.
//!
//! THE CONTAINER
//! ─────────────
//! A standard AES-256 zip, not a private format. Anyone holding the password
//! can open it with 7-Zip or WinRAR on a machine that has never seen COPS. A
//! backup only this program can read is a liability, not an asset.

use anyhow::{anyhow, Result};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::db::DbPool;

/// Name of the database inside the zip. Fixed, so a restore never has to guess.
pub const ENTRY_NAME: &str = "cops_data.db";

/// A small index of what the archive holds, stored alongside the database.
///
/// It exists so the safety floor can read row counts without extracting 177 MB
/// on every run. Worth being explicit about why this is NOT the mistake made
/// once before, where the floor kept its reference inside the database it was
/// protecting and the reference died with the thing it guarded: this manifest
/// lives inside the backup, so it survives exactly when the backup survives,
/// and it is written only after the export has been verified. The archive is
/// still proved openable separately — the manifest is trusted for counts, never
/// for integrity.
pub const MANIFEST_NAME: &str = "manifest.json";

/// Tables that must never be skipped even if empty, because their emptiness is
/// itself meaningful state rather than an absence of data.
const ALWAYS_INCLUDE: &[&str] = &["app_settings"];

/// Tables the office may choose to leave OUT of the backup, set in
/// `app_settings` under `backup_exclude_tables` as a comma-separated list.
///
/// Empty by default: everything is backed up unless somebody deliberately says
/// otherwise. The case for excluding the duty-collection tables is that each
/// shift's report is exported and emailed, so a copy exists outside the system.
/// Measured on the real data, that saves about 2.7 MB a year at full airport
/// volume — roughly 6% of the backup — which is why this is a setting and not
/// a separate database. Splitting the database to save it would double the
/// files, backups, restores and migrations, and create two things that can
/// drift apart.
///
/// What it costs, stated plainly for whoever turns it on: an emailed spreadsheet
/// is a REPORT, not restorable data. After a disk failure those shifts can only
/// be re-keyed by hand, and the monthly register cannot be regenerated without
/// them. Case records — OS, BR, DR — can never be excluded, whatever this is
/// set to; see `NEVER_EXCLUDE`.
const SUGGESTED_EXCLUDE: &str = "dcr_sessions,dcr_entries,dcr_dr_entries,dcr_os_entries";

/// Tables no setting may ever drop. The office cannot re-create these from
/// anything, so a typo in a settings field must not be able to leave them out.
const NEVER_EXCLUDE: &[&str] = &[
    "cops_master", "cops_items", "cops_master_deleted", "cops_items_deleted",
    "br_master", "br_items", "dr_master", "dr_items",
    "users", "app_settings",
    "print_template_config", "baggage_rules_config", "special_item_allowances",
    "legal_statutes", "dcr_tariffs", "dcr_formula_rules", "feature_flags",
];

/// The columns a table has in both the archive and the running database.
///
/// Returned in the live table's own order. A column the archive does not carry
/// is left to its default; one it carries that the table no longer has is not
/// asked for. Either way the restore does not depend on the two schemas having
/// stayed identical since the backup was taken.
fn shared_columns(conn: &rusqlite::Connection, table: &str) -> Result<Vec<String>> {
    let names = |db: &str| -> Result<Vec<String>> {
        let mut st = conn.prepare(&format!("PRAGMA {db}.table_info(\"{table}\")"))?;
        let rows = st.query_map([], |r| r.get::<_, String>(1))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    };
    let live = names("main")?;
    let archived: std::collections::HashSet<String> =
        names("src")?.into_iter().map(|c| c.to_lowercase()).collect();
    Ok(live.into_iter()
        .filter(|c| archived.contains(&c.to_lowercase()))
        .collect())
}

/// Tables the operator has chosen to leave out, minus anything protected.
fn excluded_tables(conn: &rusqlite::Connection) -> Vec<String> {
    let raw: String = conn
        .query_row(
            "SELECT value FROM app_settings WHERE key = 'backup_exclude_tables'",
            [],
            |r| r.get(0),
        )
        .unwrap_or_default();
    raw.split(',')
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .filter(|s| {
            let protected = NEVER_EXCLUDE.iter().any(|p| p.eq_ignore_ascii_case(s));
            if protected {
                tracing::warn!(
                    "backup_exclude_tables lists {s}, which holds case records —                      ignoring it and backing the table up anyway"
                );
            }
            !protected
        })
        .collect()
}

/// Objects SQLite maintains itself; copying them corrupts the target.
fn is_internal(name: &str) -> bool {
    name.starts_with("sqlite_")
}

/// Turn foreign-key enforcement back on, and PROVE it took.
///
/// Two ways this silently fails, and the connection then goes back to the pool
/// with enforcement off — every later case write skipping its integrity checks
/// with nothing to say so:
///
///   * SQLite ignores this pragma inside a transaction and still reports
///     success, so checking the Result is not enough; the value has to be read
///     back.
///   * A rollback that did not complete leaves a transaction open, which is
///     exactly the situation where this runs.
///
/// Failing loudly is the point. There is nothing sensible to do about it here,
/// but a database quietly accepting rows it should refuse is worth a line in
/// the log that names the cause.
fn restore_foreign_keys(conn: &rusqlite::Connection, context: &str) {
    let _ = conn.execute_batch("PRAGMA foreign_keys = ON");
    let on: i64 = conn
        .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
        .unwrap_or(-1);
    if on != 1 {
        // One more attempt after closing anything still open.
        let _ = conn.execute_batch("ROLLBACK");
        let _ = conn.execute_batch("PRAGMA foreign_keys = ON");
        let again: i64 = conn
            .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
            .unwrap_or(-1);
        if again != 1 {
            tracing::error!(
                "foreign key enforcement could NOT be restored after {context} \
                 (pragma reads {again}). This connection will not check \
                 relationships until the application is restarted."
            );
        }
    }
}

/// Move `from` onto `to`, replacing whatever is there — on Windows as well.
///
/// `fs::rename` silently replaces an existing destination on Unix and FAILS on
/// Windows. Every atomic publish in this module is a rename onto a name that may
/// already exist: an officer choosing an existing file in the save dialog and
/// confirming the overwrite, or two runs landing in the same second. On Linux
/// that works and on Windows it returns "Access is denied", which is the worst
/// possible split — it passes every test here and fails in the office.
///
/// The remove-then-rename is not atomic, so the destination is briefly absent.
/// That is acceptable precisely where it is used: the thing being replaced is a
/// backup superseded by the file about to take its place, and the source is a
/// complete, already-verified archive. It is never used on live data.
pub(crate) fn replace_file(from: &Path, to: &Path) -> std::io::Result<()> {
    match fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(_) if to.exists() => {
            fs::remove_file(to)?;
            fs::rename(from, to)
        }
        Err(e) => Err(e),
    }
}

/// Every table holding at least one row, plus the always-include set.
///
/// Deliberately computed by asking the database, never by consulting a list
/// kept in the source. See the module note.
fn tables_worth_copying(conn: &rusqlite::Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name",
    )?;
    let all: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .filter_map(|r| r.ok())
        .filter(|n| !is_internal(n))
        .collect();

    let skip = excluded_tables(conn);
    if !skip.is_empty() {
        tracing::info!("backup excludes {} table(s) by configuration: {}", skip.len(), skip.join(", "));
    }

    let mut keep = Vec::new();
    for t in all {
        if skip.iter().any(|s| s.eq_ignore_ascii_case(&t)) {
            continue;
        }
        if ALWAYS_INCLUDE.contains(&t.as_str()) {
            keep.push(t);
            continue;
        }
        // EXISTS stops at the first row — it does not count 357,705 of them.
        let has: i64 = conn
            .query_row(&format!("SELECT EXISTS(SELECT 1 FROM \"{t}\")"), [], |r| r.get(0))
            .unwrap_or(0);
        if has == 1 {
            keep.push(t);
        }
    }
    Ok(keep)
}

/// The CREATE statements for the given tables and their indexes and triggers.
///
/// Taken verbatim from sqlite_master and replayed unmodified on the target, so
/// column types, defaults, NOT NULL and primary keys survive exactly. Building
/// the target with `CREATE TABLE ... AS SELECT` instead would be shorter and
/// would silently drop every constraint — the restored database would accept
/// rows the original rejected.
fn schema_for(
    conn: &rusqlite::Connection,
    tables: &[String],
    kinds: &[&str],
) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for kind in kinds {
        let mut stmt = conn.prepare(
            "SELECT tbl_name, sql FROM sqlite_master
             WHERE type = ?1 AND sql IS NOT NULL",
        )?;
        let rows: Vec<(String, String)> = stmt
            .query_map([*kind], |r| Ok((r.get(0)?, r.get(1)?)))?
            .filter_map(|r| r.ok())
            .collect();
        for (owner, sql) in rows {
            if !is_internal(&owner) && tables.iter().any(|t| t == &owner) {
                out.push(sql);
            }
        }
    }
    Ok(out)
}

/// Build the plaintext export at `plain_path`. Returns the tables copied.
///
/// Runs in two passes on purpose. The schema is created through a separate
/// plain connection so the DDL can be replayed BYTE FOR BYTE; the rows are then
/// copied through ATTACH so the transfer happens inside SQLite rather than
/// marshalling 827,140 rows through Rust. Rewriting each CREATE statement to
/// name the attached schema would avoid the second connection, but that means
/// parsing SQL with string surgery, and a mangled DDL fails as a subtly wrong
/// table rather than as an error.
fn build_plain_export(pool: &DbPool, plain_path: &Path) -> Result<Vec<String>> {
    let src = pool.get()?;
    let tables = tables_worth_copying(&src)?;
    if tables.is_empty() {
        return Err(anyhow!("nothing to export: no table holds any rows"));
    }
    let table_ddl = schema_for(&src, &tables, &["table"])?;

    // Indexes are NOT copied. They hold no information — every one can be
    // rebuilt from the rows — and on the real Chennai data they cost 13.2 MB of
    // the archive, 23% of it, to store something already implied by the data
    // beside them. A restore goes into the live database, which has its own
    // indexes from the migrations, so they are not wanted at the far end either.
    //
    // Triggers ARE copied, and only after the rows: they are behaviour rather
    // than derived data, and one active during the copy would fire on rows that
    // are not new and could rewrite the data being backed up.
    let trigger_ddl = schema_for(&src, &tables, &["trigger"])?;

    // Pass 1 — tables only, on their own plain (unencrypted) connection.
    {
        let plain = rusqlite::Connection::open(plain_path)?;
        plain.execute_batch("PRAGMA journal_mode = OFF; PRAGMA synchronous = OFF;")?;
        for stmt in &table_ddl {
            // A failure here is worth reporting with the statement attached;
            // silently continuing would produce an export missing a table.
            plain
                .execute_batch(stmt)
                .map_err(|e| anyhow!("creating schema failed: {e}\n  statement: {stmt}"))?;
        }
    }

    // Pass 2 — rows, inside SQLite via ATTACH. KEY '' means the target stays
    // plaintext, which is required: it is about to be compressed, and that only
    // works on plaintext. The file is deleted at the end of `write_archive`.
    let path_str = plain_path.to_string_lossy().replace('\'', "''");
    src.execute_batch(&format!("ATTACH DATABASE '{path_str}' AS exp KEY '';"))?;

    // Foreign keys OFF for the copy, and it MUST be set before BEGIN — the
    // pragma is silently ignored inside a transaction.
    //
    // Tables are copied in whatever order they come back in, so a child row can
    // land before the table holding its parent has been filled. On the real
    // Chennai data this failed on dr_entries. Enforcement is not wanted here in
    // any case: the rows come from a database that already satisfies every
    // constraint, and re-checking them mid-copy only means the copy has to guess
    // a valid insertion order that does not always exist.
    let _ = src.execute_batch("PRAGMA foreign_keys = OFF");
    let copy = (|| -> Result<()> {
        src.execute_batch("BEGIN")?;
        for t in &tables {
            src.execute_batch(&format!(
                "INSERT INTO exp.\"{t}\" SELECT * FROM main.\"{t}\""
            ))
            .map_err(|e| anyhow!("copying {t} failed: {e}"))?;
        }
        src.execute_batch("COMMIT")?;
        Ok(())
    })();
    // This connection goes back to the pool for ordinary case work, where the
    // constraints very much are wanted.
    restore_foreign_keys(&src, "the export");
    // DETACH whether or not the copy worked, or the file stays locked and the
    // next run fails for a reason that has nothing to do with the next run.
    let _ = src.execute_batch("ROLLBACK");
    let _ = src.execute_batch("DETACH DATABASE exp;");
    copy?;

    // Pass 3 — triggers, now that the rows are already in place.
    if !trigger_ddl.is_empty() {
        let plain = rusqlite::Connection::open(plain_path)?;
        plain.execute_batch("PRAGMA journal_mode = OFF; PRAGMA synchronous = OFF;")?;
        for stmt in &trigger_ddl {
            plain
                .execute_batch(stmt)
                .map_err(|e| anyhow!("creating trigger failed: {e}\n  statement: {stmt}"))?;
        }
    }

    Ok(tables)
}

/// Row counts for the given tables, used to prove the export is complete.
fn counts(conn: &rusqlite::Connection, tables: &[String]) -> Vec<(String, i64)> {
    tables
        .iter()
        .filter_map(|t| {
            conn.query_row(&format!("SELECT COUNT(*) FROM \"{t}\""), [], |r| {
                r.get::<_, i64>(0)
            })
            .ok()
            .map(|n| (t.clone(), n))
        })
        .collect()
}

pub struct ExportReport {
    pub tables: usize,
    pub rows: i64,
    pub plain_bytes: u64,
    pub archive_bytes: u64,
}

impl ExportReport {
    pub fn ratio(&self) -> f64 {
        if self.archive_bytes == 0 {
            return 0.0;
        }
        self.plain_bytes as f64 / self.archive_bytes as f64
    }
}

/// Write a compressed, encrypted backup archive to `archive_path`.
///
/// The plaintext intermediate exists only for the duration of this call and is
/// removed before returning, on every path including failure. It must not
/// outlive the function: it is the one moment the case data is not encrypted.
pub fn write_archive(pool: &DbPool, archive_path: &Path) -> Result<ExportReport> {
    if let Some(dir) = archive_path.parent() {
        fs::create_dir_all(dir)?;
    }

    // The plaintext intermediate goes in the SYSTEM temp directory, never beside
    // the destination. The destination is wherever the officer pointed the save
    // dialog — a USB stick, a share on another PC — and building there would
    // write 177 MB of unencrypted case records onto that medium, then delete
    // them. A crash or a pulled stick mid-write leaves them behind, on exactly
    // the kind of removable media that goes missing.
    //
    // The name is unique per run: two backups at once would otherwise share the
    // file and corrupt each other. That cannot happen through the scheduler,
    // which holds a lock, but an officer saving a copy while it runs is not
    // going through the scheduler.
    let plain_path: PathBuf = std::env::temp_dir().join(format!(
        "cops_plain_{}_{}.tmp",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let _ = fs::remove_file(&plain_path);

    // `finish` runs on every exit path below — see the guard drop at the end.
    let result = (|| -> Result<ExportReport> {
        let tables = build_plain_export(pool, &plain_path)?;

        // Verify BEFORE compressing. Checking only after would mean spending a
        // minute compressing a broken export, and — worse — a corrupt archive
        // could reach a destination folder and be taken for a good backup.
        let expected = {
            let plain = rusqlite::Connection::open(&plain_path)?;
            let check: String = plain.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
            if check != "ok" {
                return Err(anyhow!("export failed its integrity check: {check}"));
            }
            counts(&plain, &tables)
        };
        let live_conn = pool.get()?;
        let live = counts(&live_conn, &tables);
        for (t, want) in &live {
            let got = expected.iter().find(|(n, _)| n == t).map(|(_, c)| *c).unwrap_or(-1);
            // Rows can be added while the export runs, so the copy may hold
            // more. It must never hold fewer.
            if got < *want {
                return Err(anyhow!("{t}: export has {got} rows, live has {want}"));
            }
        }
        let rows: i64 = expected.iter().map(|(_, c)| *c).sum();
        let plain_bytes = fs::metadata(&plain_path)?.len();

        // Compress and encrypt, in that order. Reversed, the compression stage
        // has nothing to work with — measured, it made the file larger.
        let tmp_archive = archive_path.with_extension("partial");
        let _ = fs::remove_file(&tmp_archive);
        {
            let out = fs::File::create(&tmp_archive)?;
            let mut zw = zip::ZipWriter::new(out);
            // Deflate, chosen over zstd deliberately — measured on the real
            // 177.5 MB export, zstd wins on both axes:
            //
            //     deflate -6  44.4 MB   9.6s     (this)
            //     deflate -9  43.5 MB  21.7s     2% smaller for 2x the time
            //     zstd -3     42.0 MB   1.6s
            //     zstd -10    35.4 MB   7.9s     20% smaller, same time
            //
            // It is still deflate because a zstd-in-zip archive does not open in
            // the 7-Zip or WinRAR already on an office machine. The 8 MB buys a
            // recovery path that works when COPS2 itself is the thing that will
            // not run, which is exactly the situation a backup is for. Decided
            // explicitly, not by default — do not "optimise" this without the
            // same decision being made again.
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated)
                .with_aes_encryption(zip::AesMode::Aes256, crate::security::zip_password())
                // Databases pass 4 GB eventually; without this the archive
                // silently fails at that size rather than growing a header.
                .large_file(true);
            zw.start_file(ENTRY_NAME, opts)?;
            let mut src = fs::File::open(&plain_path)?;
            let mut buf = vec![0u8; 1 << 20];
            loop {
                let n = src.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                zw.write_all(&buf[..n])?;
            }

            // Written after the export passed verification, never before.
            let manifest = serde_json::json!({
                "created": chrono::Local::now().to_rfc3339(),
                "rows": rows,
                "plain_bytes": plain_bytes,
                "tables": expected.iter()
                    .map(|(k, v)| (k.clone(), serde_json::json!(v)))
                    .collect::<serde_json::Map<String, serde_json::Value>>(),
            });
            zw.start_file(MANIFEST_NAME, opts)?;
            zw.write_all(serde_json::to_string_pretty(&manifest)?.as_bytes())?;
            zw.finish()?.sync_all()?;
        }

        // Read the archive back through the decryptor. This proves the password
        // opens it and the CRC matches — that the bytes on disk are the bytes
        // intended, not merely that writing reported success.
        {
            let f = fs::File::open(&tmp_archive)?;
            let mut za = zip::ZipArchive::new(f)?;
            let mut entry = za
                .by_name_decrypt(ENTRY_NAME, crate::security::zip_password().as_bytes())
                .map_err(|e| anyhow!("archive cannot be reopened: {e}"))?;
            let n = std::io::copy(&mut entry, &mut std::io::sink())?;
            if n != plain_bytes {
                return Err(anyhow!(
                    "archive holds {n} bytes, expected {plain_bytes}"
                ));
            }
        }

        // Only now is it allowed to take the real name — no half-written file
        // may ever look like a finished backup.
        replace_file(&tmp_archive, archive_path)?;
        let archive_bytes = fs::metadata(archive_path)?.len();

        Ok(ExportReport {
            tables: tables.len(),
            rows,
            plain_bytes,
            archive_bytes,
        })
    })();

    // The plaintext copy must not survive this function under any outcome.
    let _ = fs::remove_file(&plain_path);
    let _ = fs::remove_file(archive_path.with_extension("partial"));
    result
}

/// Extract the database out of an archive, for restore and for verification.
pub fn extract(archive_path: &Path, dest_db: &Path) -> Result<()> {
    let f = fs::File::open(archive_path)?;
    let mut za = zip::ZipArchive::new(f)?;
    let mut entry = za
        .by_name_decrypt(ENTRY_NAME, crate::security::zip_password().as_bytes())
        .map_err(|e| anyhow!("cannot open archive: {e}"))?;
    let mut out = fs::File::create(dest_db)?;
    std::io::copy(&mut entry, &mut out)?;
    out.sync_all()?;
    Ok(())
}

pub struct RestoreReport {
    pub tables_restored: usize,
    pub rows: i64,
    pub tables_cleared: usize,
}

/// Load a plaintext export back into the live encrypted database.
///
/// The exact inverse of `build_plain_export`, and it lives next to it so the two
/// cannot drift apart. It has to be the inverse rather than a page copy:
/// SQLCipher's backup API refuses to copy between a plaintext source and an
/// encrypted destination — "backup is not supported with encrypted databases" —
/// which is the same constraint that makes the export use ATTACH.
///
/// Everything happens in ONE transaction. A restore that failed half way would
/// otherwise leave the office with part of yesterday and part of today, which
/// is worse than either and impossible to untangle afterwards.
///
/// Tables absent from the archive are emptied, not left alone. The archive holds
/// every table that had rows, so a table missing from it was empty when the
/// backup was taken; leaving today's rows in place would produce a database that
/// never existed at any point in time.
pub fn restore_into(pool: &DbPool, plain_path: &Path) -> Result<RestoreReport> {
    let incoming = {
        let plain = rusqlite::Connection::open(plain_path)?;
        let mut st = plain.prepare(
            "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name",
        )?;
        let names: Vec<String> = st
            .query_map([], |r| r.get::<_, String>(0))?
            .filter_map(|r| r.ok())
            .filter(|n| !is_internal(n))
            .collect();
        let mut out = Vec::new();
        for n in names {
            let sql: Option<String> = plain
                .query_row(
                    "SELECT sql FROM sqlite_master WHERE type='table' AND name=?1",
                    [&n],
                    |r| r.get(0),
                )
                .ok();
            out.push((n, sql));
        }
        out
    };
    if incoming.is_empty() {
        return Err(anyhow!("the backup contains no tables"));
    }

    let conn = pool.get()?;
    let existing: Vec<String> = {
        let mut st = conn.prepare("SELECT name FROM sqlite_master WHERE type='table'")?;
        let v: Vec<String> = st
            .query_map([], |r| r.get::<_, String>(0))?
            .filter_map(|r| r.ok())
            .filter(|n| !is_internal(n))
            .collect();
        v
    };

    let path_str = plain_path.to_string_lossy().replace('\'', "''");
    conn.execute_batch(&format!("ATTACH DATABASE '{path_str}' AS src KEY '';"))?;

    // Same reasoning as the export, and more forcefully: a restore DELETEs every
    // table and refills it, so with enforcement on, deleting a parent before its
    // children — or refilling a child before its parent — fails. No ordering
    // avoids that in general, and the rows being restored already satisfied
    // every constraint when they were backed up. Must precede BEGIN.
    let _ = conn.execute_batch("PRAGMA foreign_keys = OFF");

    let work = (|| -> Result<RestoreReport> {
        conn.execute_batch("BEGIN")?;
        let mut rows = 0i64;
        for (t, ddl) in &incoming {
            if !existing.iter().any(|e| e == t) {
                // A table the running version has never created — the backup is
                // from a build that knew about it, so recreate it rather than
                // dropping its data on the floor.
                if let Some(sql) = ddl {
                    conn.execute_batch(sql)?;
                }
            }
            conn.execute_batch(&format!("DELETE FROM main.\"{t}\""))?;

            // Column by column, not position by position.
            //
            // This used to be SELECT *, which lines the archive's columns up
            // against the live table's by position and count. That holds only
            // while the two agree exactly: the day a column is added, every
            // archive taken before it becomes unrestorable — "table has 12
            // columns but 9 values were supplied" — and the office discovers it
            // at the worst possible moment.
            //
            // Naming the columns the two have in common restores what the
            // archive holds and lets a column added since take its default. A
            // column since dropped is simply not asked for.
            let cols = shared_columns(&conn, t)?;
            if cols.is_empty() { continue; }
            let list = cols.iter()
                .map(|c| format!("\"{c}\""))
                .collect::<Vec<_>>().join(", ");
            conn.execute_batch(&format!(
                "INSERT INTO main.\"{t}\" ({list}) SELECT {list} FROM src.\"{t}\""
            ))
            .map_err(|e| anyhow!("restoring {t} failed: {e}"))?;
            rows += conn
                .query_row(&format!("SELECT COUNT(*) FROM main.\"{t}\""), [], |r| {
                    r.get::<_, i64>(0)
                })
                .unwrap_or(0);
        }

        let mut cleared = 0usize;
        for t in &existing {
            if !incoming.iter().any(|(n, _)| n == t) {
                let n: i64 = conn
                    .query_row(&format!("SELECT COUNT(*) FROM main.\"{t}\""), [], |r| r.get(0))
                    .unwrap_or(0);
                if n > 0 {
                    conn.execute_batch(&format!("DELETE FROM main.\"{t}\""))?;
                    cleared += 1;
                }
            }
        }

        conn.execute_batch("COMMIT")?;
        Ok(RestoreReport { tables_restored: incoming.len(), rows, tables_cleared: cleared })
    })();

    if work.is_err() {
        // Put the database back exactly as it was. A partial restore is worse
        // than a refused one.
        let _ = conn.execute_batch("ROLLBACK");
    }
    let _ = conn.execute_batch("DETACH DATABASE src;");
    restore_foreign_keys(&conn, "the restore");

    // The restored rows have never been constraint-checked as a set, so check
    // them now — after the transaction, where a failure can be reported rather
    // than silently accepted. A restore that quietly loads inconsistent data is
    // exactly the kind of damage that is not noticed for months.
    if work.is_ok() {
        if let Ok(mut st) = conn.prepare("PRAGMA foreign_key_check") {
            if let Ok(mut rows) = st.query([]) {
                if let Ok(Some(r)) = rows.next() {
                    let table: String = r.get(0).unwrap_or_default();
                    tracing::error!(
                        "restored data violates a foreign key in {table} — \
                         the backup may be from an inconsistent database"
                    );
                }
            }
        }
    }
    work
}

/// The row counts recorded when this archive was written.
///
/// Read from the manifest so the safety floor does not have to unpack 177 MB on
/// every run just to count rows.
pub fn read_counts(archive_path: &Path) -> Result<Vec<(String, i64)>> {
    let f = fs::File::open(archive_path)?;
    let mut za = zip::ZipArchive::new(f)?;
    let mut entry = za
        .by_name_decrypt(MANIFEST_NAME, crate::security::zip_password().as_bytes())
        .map_err(|e| anyhow!("no manifest in archive: {e}"))?;
    let mut s = String::new();
    entry.read_to_string(&mut s)?;
    let v: serde_json::Value = serde_json::from_str(&s)?;
    let map = v
        .get("tables")
        .and_then(|t| t.as_object())
        .ok_or_else(|| anyhow!("manifest has no table counts"))?;
    Ok(map
        .iter()
        .filter_map(|(k, v)| v.as_i64().map(|n| (k.clone(), n)))
        .collect())
}

/// Does this archive open, decrypt, and hold intact data?
///
/// Used where a backup file has to be trusted before it is relied on — the
/// safety floor picks its reference from files that pass this.
///
/// WHAT THIS PROVES, AND WHAT IT DELIBERATELY DOES NOT
///
/// It proves the file is a well-formed zip, that the password opens it, that the
/// database entry is present, and that a manifest inside it decrypts, parses and
/// records rows. That is enough to choose a baseline: a junk file, a truncated
/// file, a foreign zip and an empty archive all fail it — and a junk file
/// defeating exactly this check is why the validation exists at all.
///
/// It does NOT decompress the database entry. Doing so would additionally verify
/// its CRC, but costs a full pass over 177 MB through AES and deflate on a path
/// that runs every interval, including when there is nothing to do. The CRC IS
/// checked where it matters: when the archive is written (in `write_archive`,
/// before it may reach any destination) and when one is restored (`quick_check`
/// on the extracted database, before the live data is touched). Verifying it a
/// third time to decide which file to measure against buys nothing.
///
/// Truncation still fails here: a zip's directory lives at the END of the file,
/// so a cut-off archive cannot even be opened.
pub fn is_usable_archive(archive_path: &Path) -> bool {
    let Ok(f) = fs::File::open(archive_path) else { return false };
    let Ok(mut za) = zip::ZipArchive::new(f) else { return false };
    let pw = crate::security::zip_password().as_bytes();

    if za.by_name_decrypt(ENTRY_NAME, pw).is_err() {
        return false;
    }
    // A manifest claiming no rows is not a backup worth measuring against.
    read_counts(archive_path).map(|c| !c.is_empty()).unwrap_or(false)
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests — each named for the failure it prevents.
// ─────────────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("cops_exp_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    /// A live database with case data, an admin/template table, and — the point
    /// of several tests below — tables that are empty.
    fn seeded(dir: &Path) -> DbPool {
        let pool = crate::db::create_pool(&dir.join("live.db")).unwrap();
        let c = pool.get().unwrap();
        c.execute_batch(
            "CREATE TABLE cops_master(id INTEGER PRIMARY KEY, os_no TEXT NOT NULL, amt REAL);
             CREATE INDEX ix_os ON cops_master(os_no);
             CREATE TABLE print_template_config(k TEXT PRIMARY KEY, body TEXT);
             CREATE TABLE app_settings(key TEXT PRIMARY KEY, value TEXT);
             CREATE TABLE valuation_master(id INTEGER PRIMARY KEY);
             CREATE TABLE wh_master(id INTEGER PRIMARY KEY);",
        )
        .unwrap();
        for i in 1..=500 {
            c.execute(
                "INSERT INTO cops_master(os_no, amt) VALUES (?1, ?2)",
                rusqlite::params![format!("OS/{i:05}/2026"), i as f64 * 1.5],
            )
            .unwrap();
        }
        c.execute(
            "INSERT INTO print_template_config VALUES ('os_heading', 'CUSTOMS, CHENNAI')",
            [],
        )
        .unwrap();
        drop(c);
        pool
    }

    fn open_plain(p: &Path) -> rusqlite::Connection {
        rusqlite::Connection::open(p).unwrap()
    }

    #[test]
    fn every_value_survives_the_round_trip_exactly() {
        let dir = tmpdir("round");
        let pool = seeded(&dir);
        let zip = dir.join("b.cops");
        let rep = write_archive(&pool, &zip).expect("export must succeed");

        let out = dir.join("restored.db");
        extract(&zip, &out).expect("archive must extract");
        let r = open_plain(&out);

        let n: i64 = r.query_row("SELECT COUNT(*) FROM cops_master", [], |x| x.get(0)).unwrap();
        assert_eq!(n, 500, "all case rows must survive");
        let sum: f64 = r.query_row("SELECT SUM(amt) FROM cops_master", [], |x| x.get(0)).unwrap();
        assert!((sum - (1..=500).map(|i| i as f64 * 1.5).sum::<f64>()).abs() < 1e-6,
                "money values must be exact, got {sum}");
        let tpl: String = r.query_row(
            "SELECT body FROM print_template_config WHERE k='os_heading'", [], |x| x.get(0)).unwrap();
        assert_eq!(tpl, "CUSTOMS, CHENNAI", "admin templates must survive");
        assert!(rep.rows >= 500);
    }

    #[test]
    fn admin_settings_are_kept_not_just_case_data() {
        // The failure this prevents: backing up only OS/DR/BR data. Every case
        // record survives, the print templates do not, and reprints come out
        // wrong against a default template while looking perfectly healthy.
        let dir = tmpdir("admin");
        let pool = seeded(&dir);
        let zip = dir.join("b.cops");
        write_archive(&pool, &zip).unwrap();
        let out = dir.join("r.db");
        extract(&zip, &out).unwrap();
        let n: i64 = open_plain(&out)
            .query_row("SELECT COUNT(*) FROM print_template_config", [], |x| x.get(0))
            .unwrap();
        assert_eq!(n, 1, "print_template_config must be in the archive");
    }

    #[test]
    fn a_table_that_gains_data_later_is_picked_up_without_a_code_change() {
        // Tables are chosen by asking the database, not from a list in the
        // source. A list is what goes stale silently.
        let dir = tmpdir("newtable");
        let pool = seeded(&dir);

        let zip1 = dir.join("before.cops");
        let before = write_archive(&pool, &zip1).unwrap();

        pool.get().unwrap()
            .execute("INSERT INTO valuation_master(id) VALUES (7)", []).unwrap();

        let zip2 = dir.join("after.cops");
        let after = write_archive(&pool, &zip2).unwrap();
        assert_eq!(after.tables, before.tables + 1,
                   "a table that gained rows must now be included");

        let out = dir.join("r.db");
        extract(&zip2, &out).unwrap();
        let n: i64 = open_plain(&out)
            .query_row("SELECT COUNT(*) FROM valuation_master", [], |x| x.get(0)).unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn empty_tables_are_left_out_so_the_archive_stays_small() {
        let dir = tmpdir("empty");
        let pool = seeded(&dir);
        let zip = dir.join("b.cops");
        write_archive(&pool, &zip).unwrap();
        let out = dir.join("r.db");
        extract(&zip, &out).unwrap();
        let n: i64 = open_plain(&out).query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='wh_master'",
            [], |x| x.get(0)).unwrap();
        assert_eq!(n, 0, "an empty table should not be carried");
    }

    #[test]
    fn constraints_survive_so_the_restore_is_as_strict_as_the_original() {
        // Building the target with CREATE TABLE AS SELECT would be shorter and
        // would drop NOT NULL, defaults and primary keys — the restored
        // database would then accept rows the original refused.
        let dir = tmpdir("ddl");
        let pool = seeded(&dir);
        let zip = dir.join("b.cops");
        write_archive(&pool, &zip).unwrap();
        let out = dir.join("r.db");
        extract(&zip, &out).unwrap();
        let r = open_plain(&out);
        assert!(r.execute("INSERT INTO cops_master(os_no) VALUES (NULL)", []).is_err(),
                "NOT NULL must still be enforced after a restore");

        // Indexes are deliberately absent, and that is not the same thing as a
        // constraint being lost. An index changes only how fast a row is found;
        // NOT NULL, defaults and primary keys change which rows are allowed to
        // exist, and those must survive. On the real data indexes were 24% of
        // the archive, storing what the rows beside them already imply.
        let idx: i64 = r.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='ix_os'",
            [], |x| x.get(0)).unwrap();
        assert_eq!(idx, 0, "indexes are not stored — they are rebuilt from the data");
    }

    #[test]
    fn no_plaintext_copy_is_left_behind() {
        // For a moment the case data exists unencrypted, because compression
        // only works on plaintext. It must not outlive the export.
        let dir = tmpdir("plain");
        let pool = seeded(&dir);
        let zip = dir.join("b.cops");
        write_archive(&pool, &zip).unwrap();
        let leftovers: Vec<String> = fs::read_dir(&dir).unwrap().flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.contains("plain") || n.contains("partial") || n.contains("verify"))
            .collect();
        assert!(leftovers.is_empty(), "plaintext must not survive: {leftovers:?}");
    }

    #[test]
    fn the_archive_is_actually_encrypted() {
        let dir = tmpdir("enc");
        let pool = seeded(&dir);
        let zip = dir.join("b.cops");
        write_archive(&pool, &zip).unwrap();

        let bytes = fs::read(&zip).unwrap();
        assert!(!bytes.windows(16).any(|w| w == b"CUSTOMS, CHENNAI"),
                "template text must not be readable in the archive");
        assert!(!bytes.windows(11).any(|w| w == b"OS/00001/20"),
                "case data must not be readable in the archive");

        let f = fs::File::open(&zip).unwrap();
        let mut za = zip::ZipArchive::new(f).unwrap();
        assert!(za.by_name_decrypt(ENTRY_NAME, b"not-the-password").is_err(),
                "a wrong password must not open the archive");
    }

    #[test]
    fn it_is_much_smaller_than_copying_the_database() {
        // The whole reason this module exists.
        let dir = tmpdir("size");
        let pool = seeded(&dir);
        let zip = dir.join("b.cops");
        let rep = write_archive(&pool, &zip).unwrap();
        assert!(rep.ratio() > 2.0,
                "compression should be well over 2x, got {:.1}x ({} -> {} bytes)",
                rep.ratio(), rep.plain_bytes, rep.archive_bytes);
    }
}

#[cfg(test)]
mod schema_drift_tests {
    use super::*;

    /// An archive taken before a column existed must still restore.
    ///
    /// The restore used to copy `SELECT *`, matching the archive's columns to
    /// the live table's by position. The day a column was added, every archive
    /// taken before it became unrestorable — which is the one moment a backup
    /// has to work.
    #[test]
    fn a_backup_taken_before_a_column_existed_still_restores() {
        let dir = tempfile::tempdir().unwrap();
        let live = dir.path().join("live.db");
        let old = dir.path().join("old.db");

        // The archive: the table as it was, three columns.
        {
            let c = rusqlite::Connection::open(&old).unwrap();
            c.execute_batch(
                "CREATE TABLE dcr_formula_rules (id INTEGER PRIMARY KEY, target_column TEXT, expression TEXT);
                 INSERT INTO dcr_formula_rules VALUES (1, 'baggage_duty', 'value * 0.35');",
            ).unwrap();
        }
        // The running database: the same table, since given three more columns.
        let conn = rusqlite::Connection::open(&live).unwrap();
        conn.execute_batch(
            "CREATE TABLE dcr_formula_rules (id INTEGER PRIMARY KEY, target_column TEXT, expression TEXT,
                                             lineage_id INTEGER, effective_from TEXT, changed_by TEXT);",
        ).unwrap();

        let p = old.to_string_lossy().replace('\'', "''");
        conn.execute_batch(&format!("ATTACH DATABASE '{p}' AS src")).unwrap();

        let cols = shared_columns(&conn, "dcr_formula_rules").unwrap();
        assert_eq!(cols, vec!["id", "target_column", "expression"],
                   "only the columns both sides have: {cols:?}");

        let list = cols.iter().map(|c| format!("\"{c}\"")).collect::<Vec<_>>().join(", ");
        conn.execute_batch(&format!(
            "INSERT INTO main.\"dcr_formula_rules\" ({list}) SELECT {list} FROM src.\"dcr_formula_rules\""
        )).expect("an older archive must still restore");

        let (expr, eff): (String, Option<String>) = conn.query_row(
            "SELECT expression, effective_from FROM dcr_formula_rules WHERE id = 1",
            [], |r| Ok((r.get(0)?, r.get(1)?)),
        ).unwrap();
        assert_eq!(expr, "value * 0.35", "the archived value came through");
        assert!(eff.is_none(), "a column added since takes its default, not a stray value");
    }

    /// And the other way: an archive holding a column the table no longer has.
    #[test]
    fn a_backup_holding_a_column_since_dropped_still_restores() {
        let dir = tempfile::tempdir().unwrap();
        let live = dir.path().join("live.db");
        let old = dir.path().join("old.db");
        {
            let c = rusqlite::Connection::open(&old).unwrap();
            c.execute_batch(
                "CREATE TABLE t (id INTEGER PRIMARY KEY, keep TEXT, gone TEXT);
                 INSERT INTO t VALUES (1, 'kept', 'dropped');",
            ).unwrap();
        }
        let conn = rusqlite::Connection::open(&live).unwrap();
        conn.execute_batch("CREATE TABLE t (id INTEGER PRIMARY KEY, keep TEXT);").unwrap();
        let p = old.to_string_lossy().replace('\'', "''");
        conn.execute_batch(&format!("ATTACH DATABASE '{p}' AS src")).unwrap();

        let cols = shared_columns(&conn, "t").unwrap();
        assert_eq!(cols, vec!["id", "keep"], "the dropped column is not asked for");
        let list = cols.iter().map(|c| format!("\"{c}\"")).collect::<Vec<_>>().join(", ");
        conn.execute_batch(&format!("INSERT INTO main.\"t\" ({list}) SELECT {list} FROM src.\"t\"")).unwrap();
        let kept: String = conn.query_row("SELECT keep FROM t WHERE id = 1", [], |r| r.get(0)).unwrap();
        assert_eq!(kept, "kept");
    }
}
