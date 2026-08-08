//! Automatic backups.
//!
//! Ported from the sibling project, where every rule below exists because
//! something went wrong without it. The comments record *why*, so a future
//! change does not quietly undo a protection whose purpose is not obvious from
//! the code alone.
//!
//! The two costs are deliberately separated, because conflating them makes a
//! backup scheme either wasteful or useless:
//!
//! ```text
//!     RETENTION costs disk    — how many copies are kept
//!     FREQUENCY costs seconds — how often one is taken
//! ```
//!
//! So retention is small and fixed (two copies) while frequency is high (every
//! 30 minutes), and the exposure window is half an hour rather than a day.
//!
//! What gets written is NOT a copy of the database file. It is a compressed,
//! encrypted export built by `backup_export` — on the real Chennai data, 44.4 MB
//! against 242.6 MB for the same 827,140 rows. Two copies therefore cost about
//! 88 MB per folder rather than 486 MB, which is what makes it reasonable to
//! keep them on machines that are not this one.
//!
//! Rules
//! -----
//! 1. Verify before distributing — row-count the export before it is compressed
//!    (in `backup_export`), and prove the finished archive decrypts and matches
//!    its CRC. Nothing unverified reaches a destination.
//! 2. Two generations, always. Never destroy the last good copy for an
//!    unverified new one.
//! 3. `.partial` then atomic rename. No half-written file may look complete.
//! 4. Probe each destination with a hard timeout. A disconnected share blocks
//!    *inside* the OS call; in the sibling project this stalled application
//!    startup until every folder walk was routed through the probe.
//! 5. No destinations configured means do nothing, silently.
//! 6. Bounded disk: copies × archive size, never more. A destination folder
//!    cannot grow without limit no matter how long the app runs.
//! 7. One backup at a time.
//! 8. Skip when the data has not changed — but still catch up a folder missing
//!    the current copy. Without the second half, a machine switched off for a
//!    week never catches up if nobody books a case after it returns.
//! 9. The safety floor: refuse to overwrite good backups when the database
//!    appears to have lost records. Compared against the newest *usable*
//!    existing backup — not a value stored in the database, which is destroyed
//!    by the very event it guards against, and not merely the newest file,
//!    because a 1400-byte junk file defeated exactly that check.

use anyhow::Result;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use crate::db::DbPool;

/// Tables whose row counts are compared between the live database and a copy.
const VERIFY_TABLES: &[&str] = &[
    "cops_master", "cops_items", "br_master", "br_items",
    "dr_master", "dr_items", "users",
];

const DEFAULT_INTERVAL_MIN: u64 = 30;
const DEFAULT_KEEP: usize = 2;
/// A dead network path can block for a minute; this bounds it.
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
/// A copy smaller than this fraction of the last one is treated as suspect.
const SIZE_TOLERANCE: f64 = 0.60;
/// A table losing more than this fraction of its rows is treated as suspect.
const SHRINK_TOLERANCE: f64 = 0.95;

#[derive(Clone, Debug, Serialize, Default)]
pub struct DestinationResult {
    pub path: String,
    pub ok: bool,
    pub skipped: bool,
    pub detail: String,
    pub size_mb: f64,
    pub pruned: usize,
}

#[derive(Clone, Debug, Serialize, Default)]
pub struct BackupOutcome {
    pub ok: bool,
    pub skipped: bool,
    pub refused: bool,
    pub reason: String,
    pub file: String,
    pub destinations: Vec<DestinationResult>,
}

/// Guards rule 7 — one backup at a time. `try_lock` rather than `lock`, so a
/// manual run during a scheduled one is reported instead of queueing behind it.
static RUN_LOCK: Mutex<()> = Mutex::new(());

// ─────────────────────────────────────────────────────────────────────────────
// Settings, read from app_settings so the admin panel can change them with no
// restart, falling back to the environment and then to the defaults.
// ─────────────────────────────────────────────────────────────────────────────

fn setting(pool: &DbPool, key: &str) -> Option<String> {
    let conn = pool.get().ok()?;
    conn.query_row(
        "SELECT value FROM app_settings WHERE key = ?1",
        [key],
        |r| r.get::<_, String>(0),
    )
    .ok()
    .filter(|s| !s.trim().is_empty())
}

pub fn destinations(pool: &DbPool) -> Vec<String> {
    let raw = setting(pool, "backup_dirs")
        .or_else(|| std::env::var("COPS_BACKUP_DIRS").ok())
        .unwrap_or_default();
    raw.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

pub fn interval_minutes(pool: &DbPool) -> u64 {
    setting(pool, "backup_interval_minutes")
        .and_then(|s| s.parse::<u64>().ok())
        .map(|v| v.max(5))
        .unwrap_or(DEFAULT_INTERVAL_MIN)
}

pub fn keep_copies(pool: &DbPool) -> usize {
    setting(pool, "backup_keep")
        .and_then(|s| s.parse::<usize>().ok())
        // Never fewer than two: the second copy is the only thing standing
        // between a corrupted database and having nothing to restore from.
        .map(|v| v.max(2))
        .unwrap_or(DEFAULT_KEEP)
}

/// Is this folder on another machine? A UNC path, or any drive letter that is
/// not the system drive. A backup that only ever lands on this computer does
/// not survive this computer failing, which is the entire point of taking one.
pub fn is_remote(path: &str) -> bool {
    let p = path.trim();
    if p.starts_with("\\\\") || p.starts_with("//") {
        return true;
    }
    let bytes = p.as_bytes();
    if bytes.len() > 1 && bytes[1] == b':' {
        let sysdrive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
        let sysdrive = sysdrive.trim_end_matches('\\').to_uppercase();
        return p[..2].to_uppercase() != sysdrive;
    }
    false
}

// ─────────────────────────────────────────────────────────────────────────────
// Rule 4 — reachability with a hard timeout
// ─────────────────────────────────────────────────────────────────────────────

/// Can this folder be written to, within the timeout?
///
/// Runs the filesystem calls on a separate thread and abandons it on timeout,
/// because a disconnected share blocks *inside* the OS call — `create_dir_all`
/// on a dead UNC path hangs exactly as long as the copy would. Abandoning
/// rather than joining is what keeps an unplugged machine to a fixed cost.
pub fn probe_destination(path: &str) -> (bool, String) {
    let owned = path.to_string();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = (|| -> Result<()> {
            std::fs::create_dir_all(&owned)?;
            let probe = Path::new(&owned).join(".cops_write_test");
            std::fs::write(&probe, b"cops")?;
            std::fs::remove_file(&probe)?;
            Ok(())
        })();
        let _ = tx.send(match result {
            Ok(()) => (true, "writable".to_string()),
            Err(e) => (false, format!("{e}")),
        });
    });
    match rx.recv_timeout(PROBE_TIMEOUT) {
        Ok(v) => v,
        Err(_) => (
            false,
            format!("not reachable (no response in {}s)", PROBE_TIMEOUT.as_secs()),
        ),
    }
}

/// Destinations that answered. Every folder walk must go through this — a raw
/// `read_dir` against a dead share is what turns a switched-off PC into a
/// minutes-long stall.
fn reachable(pool: &DbPool) -> Vec<String> {
    destinations(pool)
        .into_iter()
        .filter(|d| probe_destination(d).0)
        .collect()
}

// ─────────────────────────────────────────────────────────────────────────────
// Snapshot and verification
// ─────────────────────────────────────────────────────────────────────────────

// A `snapshot()` using SQLite's online backup API lived here, copying the
// encrypted database page by page. It was correct but produced a 242.6 MB file
// where `backup_export` produces 44.4 MB of the same data, because encrypted
// bytes cannot be compressed afterwards. Its verify step moved into
// `write_archive`, which checks the export before it is compressed. Removed
// rather than left unused, so nothing can wire it back in by accident.

fn counts_from(conn: &rusqlite::Connection) -> Vec<(String, i64)> {
    VERIFY_TABLES
        .iter()
        .filter_map(|t| {
            conn.query_row(&format!("SELECT COUNT(*) FROM \"{t}\""), [], |r| {
                r.get::<_, i64>(0)
            })
            .ok()
            .map(|n| (t.to_string(), n))
        })
        .collect()
}

pub fn live_counts(pool: &DbPool) -> Vec<(String, i64)> {
    pool.get().map(|c| counts_from(&c)).unwrap_or_default()
}

// Rule 1's `verify()` moved into `backup_export::write_archive`, which checks
// the export BEFORE compressing it — so a broken export costs nothing to throw
// away and can never reach a folder looking like a good backup.

/// Extension of a backup archive. Not `.db` — the file is a compressed,
/// encrypted archive, and naming it `.db` would invite someone to open it as a
/// database, fail, and conclude the backup is corrupt.
const ARCHIVE_EXT: &str = "cops";

/// Does this file open, decrypt, and hold intact data?
fn is_usable_backup(path: &Path) -> bool {
    crate::backup_export::is_usable_archive(path)
}

fn all_backups_newest_first(pool: &DbPool) -> Vec<PathBuf> {
    let mut found: Vec<(SystemTime, PathBuf)> = Vec::new();
    for d in reachable(pool) {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with("cops_auto_") && name.ends_with(ARCHIVE_EXT) {
                if let Ok(m) = e.metadata().and_then(|m| m.modified()) {
                    found.push((m, p));
                }
            }
        }
    }
    found.sort_by(|a, b| b.0.cmp(&a.0));
    found.into_iter().map(|(_, p)| p).collect()
}

/// The newest USABLE backup — the safety floor's baseline, and the source for
/// catching a machine up.
///
/// Validated rather than merely most-recent. A truncated or unrelated file
/// dropped in a backup folder becomes the newest by timestamp, and then the
/// size comparison measures against junk while the row-count comparison cannot
/// read it and concludes there is nothing to compare — so it allows the run.
/// That was reproduced in the sibling project: a 1400-byte file let an emptied
/// database through.
fn newest_usable_backup(pool: &DbPool) -> Option<PathBuf> {
    for candidate in all_backups_newest_first(pool) {
        if is_usable_backup(&candidate) {
            return Some(candidate);
        }
        tracing::warn!("ignoring unusable backup while choosing a baseline: {candidate:?}");
    }
    None
}

/// Rule 9 — is the live database plausibly intact, compared with the newest
/// backup? Cheap size comparison first, row counts only when that looks wrong.
fn check_shrink(
    pool: &DbPool,
    current: &[(String, i64)],
    snapshot_path: &Path,
) -> std::result::Result<(), String> {
    let Some(baseline) = newest_usable_backup(pool) else {
        return Ok(()); // nothing to compare against yet
    };

    if let (Ok(new_meta), Ok(old_meta)) =
        (std::fs::metadata(snapshot_path), std::fs::metadata(&baseline))
    {
        let (new_size, old_size) = (new_meta.len() as f64, old_meta.len() as f64);
        if old_size > 0.0 && new_size < old_size * SIZE_TOLERANCE {
            return Err(format!(
                "the new copy is {:.1} MB against {:.1} MB for the last backup — \
                 the database looks emptied, replaced, or truncated",
                new_size / 1_048_576.0,
                old_size / 1_048_576.0
            ));
        }
    }

    // Counts come from the archive's manifest, so the baseline costs a few
    // kilobytes to read rather than unpacking the whole database to count rows.
    let Ok(previous) = crate::backup_export::read_counts(&baseline) else { return Ok(()) };
    for (table, was) in previous {
        if was <= 0 {
            continue;
        }
        let Some((_, now)) = current.iter().find(|(t, _)| *t == table) else { continue };
        if *now == 0 {
            return Err(format!(
                "{table} now has 0 rows but the last backup holds {was} — \
                 the database looks empty or replaced"
            ));
        }
        if (*now as f64) < was as f64 * SHRINK_TOLERANCE {
            return Err(format!(
                "{table} has dropped from {was} to {now} ({} records missing)",
                was - now
            ));
        }
    }
    Ok(())
}

/// Rule 6 — keep the newest `keep` copies, delete the rest.
fn prune(folder: &str, keep: usize) -> usize {
    let Ok(entries) = std::fs::read_dir(folder) else { return 0 };
    let mut files: Vec<(SystemTime, PathBuf)> = entries
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            let name = p.file_name()?.to_str()?.to_string();
            if !p.is_file() || !name.starts_with("cops_auto_") {
                return None;
            }
            Some((e.metadata().ok()?.modified().ok()?, p))
        })
        .collect();
    files.sort_by(|a, b| b.0.cmp(&a.0));
    let mut removed = 0;
    for (_, p) in files.into_iter().skip(keep) {
        if std::fs::remove_file(&p).is_ok() {
            removed += 1;
        }
    }
    removed
}

// ─────────────────────────────────────────────────────────────────────────────
// Rule 8 — skip when nothing changed, but still catch a folder up
// ─────────────────────────────────────────────────────────────────────────────

/// Signature of the data: row counts plus highest ids.
///
/// Deliberately NOT file size or modification time — the -wal and -shm files
/// change whenever any connection opens, so a file-level signature never
/// matched and the skip never fired. Counts catch deletions, highest ids catch
/// insertions, and both are cheap because the counts are already being read.
fn fingerprint(pool: &DbPool, counts: &[(String, i64)]) -> Option<String> {
    let conn = pool.get().ok()?;
    let mut parts: Vec<String> = counts.iter().map(|(t, n)| format!("{t}:{n}")).collect();
    for (t, _) in counts {
        let mx: i64 = conn
            .query_row(&format!("SELECT COALESCE(MAX(id),0) FROM \"{t}\""), [], |r| r.get(0))
            .unwrap_or(-1);
        parts.push(format!("{t}#{mx}"));
    }
    Some(format!("{:x}", md5_like(&parts.join("|"))))
}

/// Small non-cryptographic digest — this only has to detect change, not resist
/// an adversary, so it avoids pulling in a hashing dependency.
fn md5_like(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn put_setting(pool: &DbPool, key: &str, value: &str) {
    if let Ok(conn) = pool.get() {
        let _ = conn.execute(
            "INSERT INTO app_settings(key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            rusqlite::params![key, value],
        );
    }
}

/// Folders that do not hold the current backup.
///
/// This is what lets a machine that was switched off be caught up without
/// waiting for somebody to book a case. Unreachable folders are not listed —
/// nothing can be done about those until they return.
fn folders_missing_latest(pool: &DbPool, latest: &str) -> Vec<String> {
    reachable(pool)
        .into_iter()
        .filter(|d| !Path::new(d).join(latest).is_file())
        .collect()
}

fn copy_into(src: &Path, dest_dir: &str, name: &str, keep: usize) -> DestinationResult {
    let mut r = DestinationResult { path: dest_dir.to_string(), ..Default::default() };
    let final_path = Path::new(dest_dir).join(name);
    let partial = final_path.with_extension("partial");

    // Rule 3 — write beside the target then rename. An interrupted copy leaves
    // a .partial, never a truncated file that looks complete.
    match std::fs::copy(src, &partial).and_then(|_| std::fs::rename(&partial, &final_path)) {
        Ok(_) => {
            r.ok = true;
            r.size_mb = std::fs::metadata(&final_path)
                .map(|m| (m.len() as f64 / 1_048_576.0 * 10.0).round() / 10.0)
                .unwrap_or(0.0);
            r.pruned = prune(dest_dir, keep);
        }
        Err(e) => {
            r.detail = format!("{e}");
            let _ = std::fs::remove_file(&partial);
        }
    }
    r
}

// ─────────────────────────────────────────────────────────────────────────────
// One run
// ─────────────────────────────────────────────────────────────────────────────

/// Take one backup and copy it to every reachable destination.
///
/// Never returns Err — a backup problem must not disturb the running
/// application. `force` skips the unchanged check; `allow_shrink` overrides the
/// safety floor and is deliberately NOT implied by `force`, so pressing
/// "Back up now" can never be the action that replaces good backups with an
/// emptied database.
pub fn run_once(pool: &DbPool, force: bool, allow_shrink: bool) -> BackupOutcome {
    let Ok(_guard) = RUN_LOCK.try_lock() else {
        return BackupOutcome {
            skipped: true,
            reason: "a backup is already running".into(),
            ..Default::default()
        };
    };

    let dests = destinations(pool);
    if dests.is_empty() {
        // Rule 5 — nothing configured, nothing to do, no noise.
        return BackupOutcome {
            skipped: true,
            reason: "no destinations configured".into(),
            ..Default::default()
        };
    }

    let counts = live_counts(pool);
    let fp = fingerprint(pool, &counts);
    let unchanged = fp.is_some() && fp == setting(pool, "backup_last_fingerprint");

    if !force && unchanged {
        // Nothing new to snapshot — but a folder may still be behind.
        if let Some(latest) = newest_usable_backup(pool)
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        {
            let behind = folders_missing_latest(pool, &latest);
            if !behind.is_empty() {
                let keep = keep_copies(pool);
                let src = newest_usable_backup(pool).unwrap();
                let results: Vec<DestinationResult> = behind
                    .iter()
                    .map(|d| copy_into(&src, d, &latest, keep))
                    .collect();
                let ok = results.iter().any(|r| r.ok);
                return BackupOutcome {
                    ok,
                    reason: "database unchanged; copied the current backup to folders \
                             that were behind".into(),
                    file: latest,
                    destinations: results,
                    ..Default::default()
                };
            }
        }
        return BackupOutcome {
            ok: true,
            skipped: true,
            reason: "database unchanged".into(),
            ..Default::default()
        };
    }

    let stamp = chrono::Local::now().format("%Y-%m-%d_%H%M%S").to_string();
    // Seconds included: without them two runs inside the same minute produce
    // the same filename and the second overwrites the first, leaving one
    // generation where two were intended.
    let name = format!("cops_auto_{stamp}.{ARCHIVE_EXT}");

    let tmp = std::env::temp_dir().join(format!("cops_bk_{stamp}.{ARCHIVE_EXT}"));
    let _ = std::fs::remove_file(&tmp);

    // Export, compress, encrypt — in that order, and verified before it is
    // allowed anywhere near a destination folder. Copying the encrypted
    // database file instead would be six times larger for the same data: see
    // the measurements in `backup_export`.
    let report = match crate::backup_export::write_archive(pool, &tmp) {
        Ok(r) => r,
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            return BackupOutcome { reason: format!("export failed: {e}"), ..Default::default() };
        }
    };
    tracing::info!(
        "backup export: {} tables, {} rows, {:.1} MB compressed to {:.1} MB ({:.1}x)",
        report.tables, report.rows,
        report.plain_bytes as f64 / 1_048_576.0,
        report.archive_bytes as f64 / 1_048_576.0,
        report.ratio(),
    );

    // Rule 9. The snapshot exists but nothing has been copied yet, so refusing
    // here costs one temp file and protects every existing backup. The danger
    // is never a failed backup — it is a SUCCESSFUL one that copies an emptied
    // database over good copies.
    if !allow_shrink {
        if let Err(why) = check_shrink(pool, &counts, &tmp) {
            let _ = std::fs::remove_file(&tmp);
            tracing::error!("BACKUP REFUSED — {why}. Existing backups left untouched.");
            put_setting(pool, "backup_shrink_blocked", &why);
            return BackupOutcome { refused: true, reason: why, ..Default::default() };
        }
    }
    put_setting(pool, "backup_shrink_blocked", "");

    let keep = keep_copies(pool);
    let mut results = Vec::new();
    for d in &dests {
        let (reachable_ok, why) = probe_destination(d);
        if !reachable_ok {
            // Rule 4 — skipped, reported, never fatal.
            results.push(DestinationResult {
                path: d.clone(), skipped: true, detail: why, ..Default::default()
            });
            continue;
        }
        results.push(copy_into(&tmp, d, &name, keep));
    }
    let _ = std::fs::remove_file(&tmp);

    let ok = results.iter().any(|r| r.ok);
    if ok {
        if let Some(f) = fp {
            put_setting(pool, "backup_last_fingerprint", &f);
        }
        put_setting(pool, "backup_last_success", &chrono::Local::now().to_rfc3339());
        for r in results.iter().filter(|r| r.ok) {
            put_setting(pool, &format!("backup_last_ok::{}", r.path),
                        &chrono::Local::now().to_rfc3339());
        }
        tracing::info!("backup {name} written to {} of {} destination(s)",
                       results.iter().filter(|r| r.ok).count(), results.len());
    } else {
        tracing::error!("BACKUP FAILED — no destination could be written.");
    }

    BackupOutcome { ok, file: name, destinations: results, ..Default::default() }
}

/// Remove temp files and `.partial` copies left by an interrupted run. Both are
/// database-sized and never reused.
pub fn sweep_orphans(pool: &DbPool) -> usize {
    let mut removed = 0;
    let cutoff = SystemTime::now() - Duration::from_secs(3600);
    let older_than = |p: &Path| {
        std::fs::metadata(p).and_then(|m| m.modified()).map(|t| t < cutoff).unwrap_or(false)
    };
    if let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) {
        for e in entries.flatten() {
            let p = e.path();
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with("cops_bk_") && name.ends_with(".db") && older_than(&p)
                && std::fs::remove_file(&p).is_ok() { removed += 1; }
        }
    }
    for d in reachable(pool) {
        if let Ok(entries) = std::fs::read_dir(&d) {
            for e in entries.flatten() {
                let p = e.path();
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name.starts_with("cops_auto_") && name.ends_with(".partial")
                    && older_than(&p) && std::fs::remove_file(&p).is_ok() { removed += 1; }
            }
        }
    }
    removed
}

/// Start the background timer. Called once at startup.
///
/// The orphan sweep runs on the first tick rather than here: it walks the
/// destination folders, and this is called during application startup, so a
/// switched-off machine would delay the app coming up.
pub fn start(pool: std::sync::Arc<DbPool>) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(120)).await;
        {
            let p = pool.clone();
            let _ = tokio::task::spawn_blocking(move || sweep_orphans(&p)).await;
        }
        loop {
            let p = pool.clone();
            // Blocking filesystem and SQLite work must not run on the async
            // runtime's threads, or it stalls every request while a 240 MB
            // snapshot is taken.
            let _ = tokio::task::spawn_blocking(move || run_once(&p, false, false)).await;

            let p = pool.clone();
            let mins = tokio::task::spawn_blocking(move || interval_minutes(&p))
                .await
                .unwrap_or(DEFAULT_INTERVAL_MIN);
            tokio::time::sleep(Duration::from_secs(mins * 60)).await;
        }
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
//
// These exercise the rules that were learned the hard way, not the happy path.
// Each name states the failure it prevents.
//
// RUN SERIALLY:  cargo test backup_service -- --test-threads=1
//
// RUN_LOCK is process-wide, which is exactly right in production — one
// application, one backup at a time — but cargo runs tests as threads in a
// single process, so parallel tests contend for it and report "a backup is
// already running". That is the lock working, not a defect.
// ─────────────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    fn tmpdir(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("cops_test_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    /// A pool over a throwaway database, seeded with the tables the service checks.
    fn test_pool(dir: &Path, rows: i64) -> DbPool {
        let db = dir.join("live.db");
        let pool = crate::db::create_pool(&db).unwrap();
        let conn = pool.get().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS app_settings(key TEXT PRIMARY KEY, value TEXT);
             CREATE TABLE IF NOT EXISTS cops_master(id INTEGER PRIMARY KEY, pad TEXT);
             CREATE TABLE IF NOT EXISTS cops_items(id INTEGER PRIMARY KEY);
             CREATE TABLE IF NOT EXISTS br_master(id INTEGER PRIMARY KEY);
             CREATE TABLE IF NOT EXISTS br_items(id INTEGER PRIMARY KEY);
             CREATE TABLE IF NOT EXISTS dr_master(id INTEGER PRIMARY KEY);
             CREATE TABLE IF NOT EXISTS dr_items(id INTEGER PRIMARY KEY);
             CREATE TABLE IF NOT EXISTS users(id INTEGER PRIMARY KEY);",
        ).unwrap();
        let mut st = conn.prepare("INSERT INTO cops_master(pad) VALUES (?1)").unwrap();
        for _ in 0..rows { st.execute(params!["x".repeat(300)]).unwrap(); }
        drop(st); drop(conn);
        pool
    }

    fn count_backups(dir: &Path) -> usize {
        std::fs::read_dir(dir).map(|es| es.flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("cops_auto_"))
            .count()).unwrap_or(0)
    }

    #[test]
    fn writes_then_skips_when_unchanged_then_writes_again_on_change() {
        let dir = tmpdir("cycle");
        let dest = dir.join("d1");
        std::fs::create_dir_all(&dest).unwrap();
        let pool = test_pool(&dir, 500);
        put_setting(&pool, "backup_dirs", dest.to_str().unwrap());

        let r1 = run_once(&pool, false, false);
        assert!(r1.ok && !r1.skipped, "first backup should write: {r1:?}");

        let r2 = run_once(&pool, false, false);
        assert!(r2.skipped, "unchanged database must skip: {r2:?}");

        pool.get().unwrap()
            .execute("INSERT INTO cops_master(pad) VALUES ('new')", []).unwrap();
        std::thread::sleep(Duration::from_millis(1100)); // distinct filename
        let r3 = run_once(&pool, false, false);
        assert!(r3.ok && !r3.skipped, "a change must produce a backup: {r3:?}");
    }

    #[test]
    fn retention_is_bounded_so_a_folder_cannot_fill_a_disk() {
        let dir = tmpdir("retain");
        let dest = dir.join("d1");
        std::fs::create_dir_all(&dest).unwrap();
        let pool = test_pool(&dir, 200);
        put_setting(&pool, "backup_dirs", dest.to_str().unwrap());
        put_setting(&pool, "backup_keep", "2");

        for i in 0..4 {
            pool.get().unwrap()
                .execute("INSERT INTO cops_master(pad) VALUES (?1)", params![i.to_string()])
                .unwrap();
            std::thread::sleep(Duration::from_millis(1100));
            run_once(&pool, true, false);
        }
        assert_eq!(count_backups(&dest), 2, "retention must cap at 2 copies");
    }

    #[test]
    fn an_unreachable_folder_is_skipped_and_does_not_fail_the_run() {
        let dir = tmpdir("offline");
        let good = dir.join("good");
        std::fs::create_dir_all(&good).unwrap();
        let pool = test_pool(&dir, 100);
        put_setting(&pool, "backup_dirs",
                    &format!("{},/proc/definitely-not-writable", good.to_str().unwrap()));

        let r = run_once(&pool, true, false);
        assert!(r.ok, "the run must still succeed via the reachable folder: {r:?}");
        assert!(r.destinations.iter().any(|d| d.skipped),
                "the unreachable folder must be reported as skipped");
        assert_eq!(count_backups(&good), 1);
    }

    /// The one that matters most: deleting the live database must not destroy
    /// the backups. Without the floor, an emptied database is copied over every
    /// good copy and verification passes, because it compares empty against
    /// empty.
    #[test]
    fn an_emptied_database_cannot_overwrite_good_backups() {
        let dir = tmpdir("floor");
        let dest = dir.join("d1");
        std::fs::create_dir_all(&dest).unwrap();
        let pool = test_pool(&dir, 4000);
        put_setting(&pool, "backup_dirs", dest.to_str().unwrap());

        assert!(run_once(&pool, true, false).ok, "seed backup should succeed");
        let before = count_backups(&dest);

        // Everything gone, as if the file had been deleted and recreated.
        pool.get().unwrap().execute("DELETE FROM cops_master", []).unwrap();
        std::thread::sleep(Duration::from_millis(1100));

        let r = run_once(&pool, true, false);
        assert!(r.refused, "an emptied database must be refused: {r:?}");
        assert_eq!(count_backups(&dest), before, "existing backups must be untouched");

        // And an explicit override still works, for a deliberate purge.
        let r2 = run_once(&pool, true, true);
        assert!(r2.ok && !r2.refused, "explicit override should proceed: {r2:?}");
    }

    /// A truncated or unrelated file in a backup folder becomes the newest by
    /// timestamp. If the floor trusts it blindly, it measures against junk and
    /// lets an emptied database through — which is exactly what happened once.
    #[test]
    fn a_junk_file_in_the_folder_cannot_defeat_the_safety_floor() {
        let dir = tmpdir("junk");
        let dest = dir.join("d1");
        std::fs::create_dir_all(&dest).unwrap();
        let pool = test_pool(&dir, 4000);
        put_setting(&pool, "backup_dirs", dest.to_str().unwrap());
        assert!(run_once(&pool, true, false).ok);

        std::thread::sleep(Duration::from_millis(1100));
        // The junk must carry the CURRENT backup extension. Named anything else
        // it is filtered out before the safety floor is reached, and the test
        // passes without exercising the thing it claims to — which is exactly
        // what happened when the extension changed from .db to .cops.
        std::fs::write(
            dest.join(format!("cops_auto_2099-12-31_235959.{ARCHIVE_EXT}")),
            b"not an archive",
        )
        .unwrap();
        // A well-formed but empty zip is the subtler case: it opens, so a check
        // that only asked "is this a valid zip?" would accept it as a baseline.
        std::fs::write(
            dest.join(format!("cops_auto_2099-12-30_235959.{ARCHIVE_EXT}")),
            b"PK\x05\x06\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00",
        )
        .unwrap();

        pool.get().unwrap().execute("DELETE FROM cops_master", []).unwrap();
        std::thread::sleep(Duration::from_millis(1100));

        let r = run_once(&pool, true, false);
        assert!(r.refused, "junk must be ignored and the floor still hold: {r:?}");
    }

    #[test]
    fn remote_and_local_paths_are_told_apart() {
        assert!(is_remote(r"\\PC2\COPS-Backups"), "UNC is off-machine");
        assert!(is_remote("//PC3/COPS-Backups"), "forward-slash UNC is off-machine");
        assert!(is_remote(r"E:\Backups"), "a USB drive is off-machine");
        assert!(!is_remote(r"C:\ProgramData\COPS"), "the system drive is this machine");
    }

    #[test]
    fn an_unreachable_folder_costs_seconds_not_minutes() {
        let started = std::time::Instant::now();
        let (ok, _why) = probe_destination("/proc/definitely-not-writable");
        assert!(!ok);
        assert!(started.elapsed() < PROBE_TIMEOUT + Duration::from_secs(2),
                "the probe must be bounded by its timeout");
    }
}
