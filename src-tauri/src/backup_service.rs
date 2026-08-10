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

/// This computer's name, reduced to something safe in a filename.
///
/// Backups carry it because several machines will point at the SAME shared
/// folder. Retention keeps the newest few and deletes the rest, so without a
/// name in the file every machine prunes every other machine's copies: three PCs
/// backing up to one share leave two backups between them instead of two each,
/// and nobody sees it happen. Worse, the machine doing the pruning may be the
/// one whose data is stale.
///
/// It also answers the question actually asked during a recovery — which PC did
/// this come from — without opening the file.
pub fn machine_name() -> String {
    let raw = std::env::var("COMPUTERNAME")            // Windows
        .or_else(|_| std::env::var("HOSTNAME"))         // some shells export it
        .ok()
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
        .unwrap_or_default();
    let cleaned: String = raw
        .trim()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .take(24)
        .collect::<String>()
        .to_lowercase();
    if cleaned.is_empty() { "pc".to_string() } else { cleaned }
}

/// Is this backup one of OURS, or another machine's copy in a shared folder?
///
/// Another machine's backups are never pruned and never used as the safety
/// floor's baseline. Both would be wrong: its retention is its own business, and
/// its row counts describe its database, not this one.
fn is_ours(name: &str) -> bool {
    name.starts_with(&format!("cops_auto_{}_", machine_name())) && name.ends_with(ARCHIVE_EXT)
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
            if is_ours(name) {
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

/// What the last backup is actually made of — the biggest tables, by row count.
///
/// Exists so the decision "is this table worth backing up?" can be re-made from
/// evidence later, instead of from an estimate made once. Revenue data was
/// judged small enough to keep (about 1.2 MB a year at 60 BRs a day); if that
/// ever stops being true, this is where it becomes visible, without anyone
/// having to open an archive to find out.
///
/// Read from the manifest, so it costs kilobytes rather than unpacking 177 MB.
pub fn last_backup_composition(pool: &DbPool) -> Vec<(String, i64)> {
    let Some(latest) = newest_usable_backup(pool) else { return Vec::new() };
    let Ok(mut counts) = crate::backup_export::read_counts(&latest) else { return Vec::new() };
    counts.retain(|(_, n)| *n > 0);
    counts.sort_by(|a, b| b.1.cmp(&a.1));
    counts.truncate(8);
    counts
}

/// Rule 9 — is the live database plausibly intact, compared with the newest
/// backup? Cheap size comparison first, row counts only when that looks wrong.
fn check_shrink(
    pool: &DbPool,
    current: &[(String, i64)],
    snapshot_path: &Path,
) -> std::result::Result<(), String> {
    let Some(baseline) = newest_usable_backup(pool) else {
        // No baseline. There are two very different reasons for that, and
        // treating them the same is how a serious problem stays quiet.
        //
        // No backup files at all is ordinary — a first run, or a folder just
        // configured. Files that EXIST but none of which can be opened is not
        // ordinary: every copy has been corrupted, replaced, or encrypted since
        // it was written, which is exactly what ransomware leaves behind. In
        // that state the floor has nothing to measure against and would wave
        // through anything, so the one moment it matters most is the one moment
        // it stops working. Say so.
        let present = all_backups_newest_first(pool).len();
        if present > 0 {
            return Err(format!(
                "{present} backup file(s) are present but NOT ONE of them can be \
                 opened. They may have been corrupted, replaced, or encrypted by \
                 ransomware. Refusing to overwrite them until someone has looked"
            ));
        }
        return Ok(()); // genuinely nothing to compare against yet
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

    // EVERY table the last backup recorded, not just the handful in
    // VERIFY_TABLES. The manifest carries a count for all of them, so limiting
    // the comparison to a fixed list means any other table — valuables,
    // warehouse, appeals, anything added later — can lose every row and the
    // backup proceeds happily, overwriting the copy that still held them. The
    // list is fine for confirming a copy looks right; it is the wrong thing to
    // decide whether the DATABASE has been damaged.
    let live = pool.get().ok();
    for (table, was) in previous {
        if was <= 0 {
            continue;
        }
        // Prefer the counts already gathered; fall back to asking, so tables
        // outside VERIFY_TABLES are still checked.
        let now = match current.iter().find(|(t, _)| *t == table) {
            Some((_, n)) => *n,
            None => {
                let Some(conn) = live.as_ref() else { continue };
                match conn.query_row(
                    &format!("SELECT COUNT(*) FROM \"{table}\""), [], |r| r.get::<_, i64>(0)
                ) {
                    Ok(n) => n,
                    // The table is gone entirely. That is a schema change, not
                    // rows being lost, and refusing every backup afterwards
                    // would leave the office with no backups at all.
                    Err(_) => continue,
                }
            }
        };
        if now == 0 {
            return Err(format!(
                "{table} now has 0 rows but the last backup holds {was} — \
                 the database looks empty or replaced"
            ));
        }
        if (now as f64) < was as f64 * SHRINK_TOLERANCE {
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
            // Ours only. Another machine sharing this folder keeps its own
            // copies; deleting them would quietly halve the office's redundancy.
            if !p.is_file() || !is_ours(&name) {
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

    // Row counts and MAX(id) between them catch every INSERT and DELETE, and no
    // UPDATE at all. data_revision is bumped by an AFTER UPDATE trigger on each
    // case table, so an adjudication or a correction changes this even though
    // nothing was added or removed. Reading it is one row, not a scan — the
    // check stays O(1) while becoming correct.
    let rev: i64 = conn
        .query_row("SELECT n FROM data_revision WHERE id = 1", [], |r| r.get(0))
        .unwrap_or(-1);
    parts.push(format!("rev#{rev}"));

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
    match std::fs::copy(src, &partial)
        .and_then(|_| crate::backup_export::replace_file(&partial, &final_path))
    {
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
        //
        // Found ONCE. This used to call newest_usable_backup twice, for the name
        // and then again for the path, and validating a candidate decompresses
        // the whole archive through AES to prove it opens. That is a few seconds
        // of work done twice, every interval, on the path that runs when there
        // is nothing to do — which is most of the time.
        if let Some(src) = newest_usable_backup(pool) {
            let latest = src.file_name().map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let behind = folders_missing_latest(pool, &latest);
            if !behind.is_empty() {
                let keep = keep_copies(pool);
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
    let name = format!("cops_auto_{}_{stamp}.{ARCHIVE_EXT}", machine_name());

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
    // Every temp name this program creates, matched by PREFIX rather than by
    // prefix-and-extension. The previous version matched "cops_bk_*.db", which
    // stopped matching anything the day backups became .cops — so nothing was
    // ever swept, and each archive an officer saved left ~44 MB in the temp
    // directory permanently. Listing prefixes only means a future change to an
    // extension cannot silently switch the sweep off again.
    const TEMP_PREFIXES: &[&str] = &[
        "cops_bk_",             // scheduler's working copy
        "cops_archive_",        // an archive built for download
        "cops_plain_",          // the plaintext intermediate
        "cops_up_",             // an upload being restored
    ];
    if let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) {
        for e in entries.flatten() {
            let p = e.path();
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if TEMP_PREFIXES.iter().any(|pre| name.starts_with(pre))
                && older_than(&p)
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
        loop {
            // Swept every cycle, not once at startup. Sweeping only at startup
            // meant an archive saved at midday left its working file in the temp
            // directory until the app was next restarted — on a machine left
            // running for weeks, every download accumulating. The sweep only
            // touches this program's own temp files, and only ones older than an
            // hour, so it cannot disturb a backup in progress.
            let p = pool.clone();
            let _ = tokio::task::spawn_blocking(move || sweep_orphans(&p)).await;

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
    /// A pool over a throwaway database built from the REAL migrations.
    ///
    /// It used to hand-create eight stub tables. That is why the edit-detection
    /// fault survived: the fixture had no data_revision table and no triggers,
    /// so a test could not have noticed them missing. A fixture that invents its
    /// own schema can only ever test itself.
    fn test_pool(dir: &Path, rows: i64) -> DbPool {
        let db = dir.join("live.db");
        let pool = crate::db::create_pool(&db).unwrap();
        crate::db::run_migrations(&pool).unwrap();
        let conn = pool.get().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS app_settings(key TEXT PRIMARY KEY, value TEXT);",
        ).unwrap();
        let mut st = conn
            .prepare("INSERT INTO cops_master(os_no, os_date, os_year, pax_name) VALUES (?1,?2,?3,?4)")
            .unwrap();
        for i in 1..=rows {
            // pax_name carries the padding the size checks rely on.
            st.execute(params![format!("{i}"), "2026-08-09", 2026, "x".repeat(300)]).unwrap();
        }
        drop(st); drop(conn);
        pool
    }

    /// Backdate a file so the sweep's one-hour cutoff applies to it.
    fn filetime_set(path: &Path, when: std::time::SystemTime) {
        let f = std::fs::OpenOptions::new().write(true).open(path).unwrap();
        let _ = f.set_modified(when);
    }

    fn count_backups(dir: &Path) -> usize {
        std::fs::read_dir(dir).map(|es| es.flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("cops_auto_"))
            .count()).unwrap_or(0)
    }

    #[test]
    fn an_edited_record_still_produces_a_backup() {
        // The change detector compared row counts and MAX(id). An UPDATE changes
        // neither, so adjudicating a case — or correcting a name, or recording an
        // outcome — left the fingerprint identical and the run was skipped. The
        // decision never reached a backup until somebody happened to book a new
        // case, which in a quiet week could be days.
        let dir = tmpdir("editonly");
        let dest = dir.join("d1");
        std::fs::create_dir_all(&dest).unwrap();
        let pool = test_pool(&dir, 200);
        put_setting(&pool, "backup_dirs", dest.to_str().unwrap());

        assert!(run_once(&pool, true, false).ok, "seed backup");
        assert!(run_once(&pool, false, false).skipped, "unchanged must skip");

        std::thread::sleep(Duration::from_millis(1100));
        // No new row, no higher id — exactly what adjudication does.
        pool.get().unwrap()
            .execute("UPDATE cops_master SET pax_name = 'adjudicated' WHERE id = 1", [])
            .unwrap();

        let r = run_once(&pool, false, false);
        assert!(!r.skipped, "an edited record MUST produce a backup: {r:?}");
        assert!(r.ok, "and it must succeed: {r:?}");
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
            .execute("INSERT INTO cops_master(os_no, os_date, os_year) VALUES ('new','2026-08-09',2026)", []).unwrap();
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
                .execute("INSERT INTO cops_master(os_no, os_date, os_year) VALUES (?1,?2,?3)",
                         params![i.to_string(), "2026-08-09", 2026])
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
    fn backups_that_have_all_become_unreadable_are_reported_not_ignored() {
        // What ransomware leaves behind: the files are still there, none opens.
        // The floor has nothing to measure against, so without this it would
        // wave everything through — failing silently at the one moment it is
        // most needed, and overwriting whatever might still be recoverable.
        let dir = tmpdir("ransom");
        let dest = dir.join("d1");
        std::fs::create_dir_all(&dest).unwrap();
        let pool = test_pool(&dir, 500);
        put_setting(&pool, "backup_dirs", dest.to_str().unwrap());
        assert!(run_once(&pool, true, false).ok, "seed backup");

        // Every existing backup replaced with something that will not open,
        // keeping the names so they still look like backups.
        for e in std::fs::read_dir(&dest).unwrap().flatten() {
            let p = e.path();
            if is_ours(&e.file_name().to_string_lossy()) {
                std::fs::write(&p, b"encrypted-by-someone-else").unwrap();
            }
        }

        std::thread::sleep(Duration::from_millis(1100));
        pool.get().unwrap()
            .execute("INSERT INTO cops_master(os_no, os_date, os_year) VALUES ('new','2026-08-09',2026)", []).unwrap();

        let r = run_once(&pool, false, false);
        assert!(r.refused, "unreadable backups must stop the run: {r:?}");
        assert!(r.reason.contains("NOT ONE"), "must say what is wrong: {}", r.reason);
    }

    #[test]
    fn an_empty_folder_is_not_mistaken_for_ransomware() {
        // The ordinary first run must not be reported as an attack.
        let dir = tmpdir("firstrun");
        let dest = dir.join("d1");
        std::fs::create_dir_all(&dest).unwrap();
        let pool = test_pool(&dir, 100);
        put_setting(&pool, "backup_dirs", dest.to_str().unwrap());

        let r = run_once(&pool, true, false);
        assert!(r.ok && !r.refused, "a first backup must simply work: {r:?}");
    }

    #[test]
    fn one_machine_never_prunes_another_machines_backups() {
        // Three PCs will point at the same share. Retention keeps the newest few
        // and deletes the rest, so without a machine name in the file each PC
        // prunes the others: three machines leave two backups between them
        // instead of two each, and nothing reports it.
        let dir = tmpdir("shared");
        let dest = dir.join("share");
        std::fs::create_dir_all(&dest).unwrap();
        let pool = test_pool(&dir, 300);
        put_setting(&pool, "backup_dirs", dest.to_str().unwrap());

        // Another PC's copies, already sitting in the shared folder.
        let theirs: Vec<PathBuf> = (1..=3)
            .map(|i| {
                let p = dest.join(format!("cops_auto_otherpc_2026-01-0{i}_120000.{ARCHIVE_EXT}"));
                std::fs::write(&p, vec![7u8; 2048]).unwrap();
                p
            })
            .collect();

        // Enough of our own runs to trigger pruning.
        for i in 0..3 {
            if i > 0 {
                std::thread::sleep(Duration::from_millis(1100));
                pool.get().unwrap()
                    .execute("INSERT INTO cops_master(os_no, os_date, os_year) VALUES ('more','2026-08-09',2026)", []).unwrap();
            }
            assert!(run_once(&pool, true, false).ok);
        }

        for p in &theirs {
            assert!(p.exists(), "another machine's backup was deleted: {p:?}");
        }
        let ours = std::fs::read_dir(&dest).unwrap().flatten()
            .filter(|e| is_ours(&e.file_name().to_string_lossy()))
            .count();
        assert_eq!(ours, 2, "our own copies must still be capped at two");
    }

    #[test]
    fn a_machine_name_is_always_usable_in_a_filename() {
        let n = machine_name();
        assert!(!n.is_empty());
        assert!(n.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'),
                "must be filename-safe on Windows and Linux alike, got {n:?}");
    }

    #[test]
    fn losing_a_table_outside_the_verify_list_is_still_refused() {
        // The floor used to compare only VERIFY_TABLES, so any other table could
        // be emptied and the backup would proceed, overwriting the copy that
        // still held the rows. valuables_master is deliberately NOT in that list.
        let dir = tmpdir("othertable");
        let dest = dir.join("d1");
        std::fs::create_dir_all(&dest).unwrap();
        let pool = test_pool(&dir, 400);
        pool.get().unwrap().execute_batch(
            "CREATE TABLE IF NOT EXISTS valuables_master(id INTEGER PRIMARY KEY, item TEXT);"
        ).unwrap();
        {
            let c = pool.get().unwrap();
            for i in 0..300 {
                c.execute("INSERT INTO valuables_master(item) VALUES (?1)",
                          rusqlite::params![format!("gold {i}")]).unwrap();
            }
        }
        put_setting(&pool, "backup_dirs", dest.to_str().unwrap());
        assert!(run_once(&pool, true, false).ok, "seed backup should succeed");

        std::thread::sleep(Duration::from_millis(1100));
        // Case records untouched; only the valuables register is wiped.
        pool.get().unwrap().execute("DELETE FROM valuables_master", []).unwrap();

        let r = run_once(&pool, true, false);
        assert!(r.refused, "losing a whole register must be refused: {r:?}");
        assert!(r.reason.contains("valuables_master"),
                "the message must name the table that lost rows: {}", r.reason);
    }

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
    fn every_kind_of_temp_file_this_program_leaves_is_swept() {
        // The sweep matched "cops_bk_*.db", which stopped matching anything the
        // day backups became .cops — so nothing was swept and every archive an
        // officer saved left ~44 MB behind for good. Named for the leak, and
        // covers each prefix, so adding a new temp file without adding it here
        // is the only way to reintroduce it.
        let dir = tmpdir("sweep");
        let pool = test_pool(&dir, 10);
        let old = std::time::SystemTime::now() - Duration::from_secs(7200);

        let names = [
            "cops_bk_2026-01-01_000000.cops",
            "cops_archive_deadbeef.cops",
            "cops_plain_1234_5678.tmp",
            "cops_up_abcd.cops",
        ];
        let made: Vec<PathBuf> = names.iter()
            .map(|n| {
                let p = std::env::temp_dir().join(n);
                std::fs::write(&p, vec![0u8; 1024]).unwrap();
                filetime_set(&p, old);
                p
            })
            .collect();

        // Something recent must survive — sweeping a backup mid-run would be
        // worse than leaving a stale file.
        let fresh = std::env::temp_dir().join("cops_archive_inflight.cops");
        std::fs::write(&fresh, b"still being written").unwrap();

        // And a file that is not ours is never touched.
        let theirs = std::env::temp_dir().join("someone_elses_file.cops");
        std::fs::write(&theirs, b"not ours").unwrap();

        sweep_orphans(&pool);

        for p in &made {
            assert!(!p.exists(), "{p:?} should have been swept");
        }
        assert!(fresh.exists(), "a file younger than an hour must be left alone");
        assert!(theirs.exists(), "files this program did not create must be left alone");
        let _ = std::fs::remove_file(&fresh);
        let _ = std::fs::remove_file(&theirs);
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
