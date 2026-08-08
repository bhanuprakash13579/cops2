# Plan — bringing cops2 to parity with the cops-web backup work

Nothing here is implemented. It is written to be argued with first.

---

## 0. Two bugs already present, both one-line fixes

Found while surveying `src-tauri/src/db/mod.rs`. Neither is speculative.

### 0.1 The write-ahead log grows and never gives the space back

```rust
PRAGMA journal_mode = WAL;
PRAGMA synchronous   = NORMAL;
PRAGMA cache_size    = -32000;
PRAGMA foreign_keys  = ON;
PRAGMA temp_store    = MEMORY;
PRAGMA mmap_size     = 268435456;
```

`journal_size_limit` is absent, so it defaults to **-1**: the `-wal` file grows
to its high-water mark and stays there for the life of the database. One bulk
import or CSV upload inflates it permanently.

This is the same defect found and fixed in cops-web, where it was measured
directly — a bulk transaction grew the WAL to **210 MB**, and after the cap was
added a checkpoint returned all of it. It is almost certainly the disk growth
described as "saving some cache".

```rust
PRAGMA journal_size_limit = 8388608;   // 8 MB ceiling
```

### 0.2 No `busy_timeout` — concurrent writes fail instead of waiting

cops-web sets `PRAGMA busy_timeout=5000`, so a writer waits up to five seconds
for a lock. cops2 sets nothing, which means the default of **0**: a second
writer gets `SQLITE_BUSY` immediately rather than waiting.

With one officer this is invisible. With the automatic backup added — which
opens the database alongside normal work — it becomes a real source of spurious
failures.

```rust
PRAGMA busy_timeout = 5000;
```

Both belong in the same `execute_batch`. They are independent of everything
below and could ship on their own.

---

## 1. What cops2 already has

Genuinely good foundations, and better than cops-web in two places:

| | |
|---|---|
| `rusqlite` with **`backup` feature already compiled in** | the snapshot mechanism exists |
| `r2d2` connection pool, max 8 | concurrency handled |
| **Raw-key PRAGMA, no PBKDF2 per connection** | cops-web pays 100,000 iterations; this does not |
| axum + `AdminUser` / `AuthUser` extractors | clean place to add endpoints |
| `tokio` with `features = ["full"]` | a timer task needs no new dependency |
| React pages mirroring cops-web, incl. `pages/backup/` | the UI ports nearly as-is |

`src-tauri/src/api/backup.rs` already exists and is substantial — 1674 lines
covering CSV and database export, restore, legacy and MDB import, custom
reports and the adjudication PDF. That is the same feature set cops-web had
*before* this work, and it is the right place to extend rather than a new
module.

(An earlier survey of this repository missed that file because the search
covered only `db/mod.rs` and `api/admin.rs`. The uncommitted changes currently
in it are DCR work, unrelated to backups.)

The gap is specifically the **automatic** service: there is no scheduler, no
destinations, no retention, no verification. The only `tokio::time::sleep` in
the codebase is a startup delay.

---

## 2. What has to be written in Rust

The rules matter more than the code; each exists because something went wrong
without it in cops-web.

1. **Verify before distributing.** Reopen the written file, row-count it against
   the source. A snapshot that cannot be reopened never reaches a destination.
2. **Two generations, always.** Never destroy the last good copy for an
   unverified new one.
3. **`.partial` then atomic rename.** No half-written file may look complete.
4. **Probe each destination with a hard timeout.** A disconnected share blocks
   *inside* the OS call; in cops-web this stalled application startup until every
   destination walk was routed through the probe. In Rust this is
   `tokio::time::timeout` around a blocking call on a worker thread.
5. **No destinations configured → do nothing, silently.**
6. **Bounded disk**: copies × database size, never more.
7. **One backup at a time** — a `tokio::sync::Mutex::try_lock`.
8. **Skip when the data has not changed**, but still catch up a folder that is
   missing the current copy. Both halves are needed: without the second, a
   machine switched off for a week never catches up if nobody books a case after
   it returns.
9. **The safety floor.** Refuse to overwrite good backups when the database
   appears to have lost records — comparing against the newest *usable* existing
   backup, not a value stored in the database, and not merely the newest file.
   In cops-web, storing the reference in the database failed for exactly the case
   it guarded, and a 1400-byte junk file defeated the check entirely.
10. **Failure must be noticed** — surfaced in the panel; email if cops2 gains SMTP.

Then the endpoints: status, run-now, settings, test-folders, archive,
download-token, archive-status — mirroring cops-web so the React components
port with little change.

---

## 3. Order

**Phase 0** — the two PRAGMA fixes. Independent, immediate.

**Phase 1** — `backup.rs`: snapshot via `rusqlite::backup`, verify, probe,
retention, safety floor, the tokio timer. This is the substance.

**Phase 2** — axum endpoints added to the existing `api/backup.rs`, plus an
`app_settings` table. The manual export and restore already there stay as they
are; this adds the automatic side alongside them.

**Phase 3** — port `AutoBackupStatus.tsx` and `ArchiveReminder.tsx`. Mostly
copying; the API shapes are already designed.

**Phase 4** — the archive: `VACUUM INTO`, then a compressed encrypted zip.
Rust has no pyzipper equivalent, so this needs `zip` with AES, or `flate2` plus
`aes-gcm` in a documented container. **The format must stay openable by whoever
holds the password, without cops2** — a backup only that program can read is a
liability, so a standard AES zip is worth the extra work over a custom format.

**Phase 5** — the same aggressive review that found eight bugs in cops-web.
Assume this port has its own.

---

## 4. Honest assessment

Phases 0–3 are a faithful translation of working, tested logic — the thinking is
done, the traps are known and written down. Phase 4 is the only part with a
genuine unknown, in the zip-encryption crate choice.

The risk is not the Rust. It is that **the same class of bug will appear in the
translation**, and the ones that hurt were never crashes — they were paths where
the system appeared to work while doing the wrong thing: a skip that never
un-skipped, a safety check defeated by a junk file, a stall during startup.
Phase 5 is not optional.

Everything here is testable on Linux except the Windows behaviour, which remains
the standing gap for both projects.

---

## 5. Decision needed

Whether cops2 is actually the office's future. If it is, this is worth doing
properly. If cops-web stays in service, Phase 0 is still worth applying to cops2
on its own — those two lines are real defects today.
