//! The compact backup payload — what actually gets written to a backup folder.
//!
//! WHY THIS EXISTS
//! ───────────────
//! The obvious backup is a copy of the database file. It was measured on the
//! real Chennai database (827,140 rows) and it is the worst of the options:
//!
//!     full encrypted .db copy .................... 242.6 MB
//!     every CSV, uncompressed ................... 199.1 MB
//!     tables that hold data, as a plain .db ..... 177.5 MB
//!     THE SAME FILE, compressed + encrypted ...... 44.4 MB
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
//!     export plain  →  compress  →  encrypt
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

/// Objects SQLite maintains itself; copying them corrupts the target.
fn is_internal(name: &str) -> bool {
    name.starts_with("sqlite_")
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

    let mut keep = Vec::new();
    for t in all {
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
fn schema_for(conn: &rusqlite::Connection, tables: &[String]) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for kind in ["table", "index", "trigger"] {
        let mut stmt = conn.prepare(
            "SELECT tbl_name, sql FROM sqlite_master
             WHERE type = ?1 AND sql IS NOT NULL",
        )?;
        let rows: Vec<(String, String)> = stmt
            .query_map([kind], |r| Ok((r.get(0)?, r.get(1)?)))?
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
    let ddl = schema_for(&src, &tables)?;

    // Pass 1 — schema, on its own plain (unencrypted) connection.
    {
        let plain = rusqlite::Connection::open(plain_path)?;
        plain.execute_batch("PRAGMA journal_mode = OFF; PRAGMA synchronous = OFF;")?;
        for stmt in &ddl {
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
    // DETACH whether or not the copy worked, or the file stays locked and the
    // next run fails for a reason that has nothing to do with the next run.
    let _ = src.execute_batch("ROLLBACK");
    let _ = src.execute_batch("DETACH DATABASE exp;");
    copy?;

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
    let plain_path: PathBuf = archive_path.with_extension("plain.tmp");
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
        fs::rename(&tmp_archive, archive_path)?;
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
/// The database entry is streamed through the decryptor to a sink rather than
/// written to disk. That still forces every byte through the AES and deflate
/// layers and checks the CRC at the end, so a truncated or altered archive
/// fails here, but it costs no disk and no 177 MB temp file. Writing the file
/// out to open it with SQLite would prove slightly more and cost far more, on a
/// path that runs every backup.
pub fn is_usable_archive(archive_path: &Path) -> bool {
    let Ok(f) = fs::File::open(archive_path) else { return false };
    let Ok(mut za) = zip::ZipArchive::new(f) else { return false };
    let pw = crate::security::zip_password().as_bytes();

    let entry_ok = match za.by_name_decrypt(ENTRY_NAME, pw) {
        Ok(mut e) => std::io::copy(&mut e, &mut std::io::sink()).is_ok(),
        Err(_) => false,
    };
    // A manifest claiming no rows is not a backup worth measuring against.
    entry_ok && read_counts(archive_path).map(|c| !c.is_empty()).unwrap_or(false)
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
        let idx: i64 = r.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='ix_os'",
            [], |x| x.get(0)).unwrap();
        assert_eq!(idx, 1, "indexes must be recreated");
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
