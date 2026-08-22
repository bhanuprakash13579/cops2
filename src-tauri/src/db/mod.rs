use anyhow::Result;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use std::path::Path;

pub type DbPool = Pool<SqliteConnectionManager>;

pub fn create_pool(db_path: &Path) -> Result<DbPool> {
    let manager = SqliteConnectionManager::file(db_path)
        .with_flags(
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE
                | rusqlite::OpenFlags::SQLITE_OPEN_CREATE
                | rusqlite::OpenFlags::SQLITE_OPEN_URI,
        )
        .with_init(|conn| {
            // MUST be first: set the SQLCipher encryption key before any other operation.
            // Raw-key format (x'hex') skips PBKDF2 entirely → <0.1ms overhead per connection.
            conn.execute_batch(&crate::security::sqlcipher_pragma())?;
            // Performance pragmas — applied once per connection on open
            conn.execute_batch(
                "PRAGMA journal_mode = WAL;
                 PRAGMA synchronous   = NORMAL;
                 PRAGMA cache_size    = -32000;   -- 32 MB page cache
                 PRAGMA foreign_keys  = ON;
                 PRAGMA temp_store    = MEMORY;
                 PRAGMA mmap_size     = 268435456; -- 256 MB memory-mapped I/O

                 -- Cap the write-ahead log. The default is -1, meaning the -wal
                 -- file grows to its high-water mark and NEVER returns the space:
                 -- one bulk import or CSV upload inflates it for the life of the
                 -- database. Measured in the sibling project, a single bulk
                 -- transaction grew the WAL to 210 MB and held it until this was
                 -- added, after which a checkpoint returned all of it.
                 -- Purely a space cap; checkpointing is routine and loses nothing.
                 PRAGMA journal_size_limit = 8388608;   -- 8 MB ceiling

                 -- Wait for a lock instead of failing instantly. The default is
                 -- 0, so a second writer gets SQLITE_BUSY immediately. With one
                 -- officer that is invisible; it stops being invisible as soon as
                 -- anything else opens the database alongside normal work — a
                 -- backup, a report, a second window.
                 PRAGMA busy_timeout  = 5000;           -- wait up to 5 s",
            )
        });

    let pool = Pool::builder()
        .max_size(8)
        .build(manager)?;

    Ok(pool)
}

pub fn run_migrations(pool: &DbPool) -> Result<()> {
    let conn = pool.get()?;
    conn.execute_batch(include_str!("migrations.sql"))?;

    // Defensive column additions for existing DBs (SQLite ALTER TABLE ADD COLUMN
    // errors if the column already exists, so we silently ignore duplicate-column errors).
    let col_migrations = [
        ("cops_master",         "adjn_section_ref", "TEXT"),
        ("cops_master_deleted", "adjn_section_ref", "TEXT"),
        // dr_master: columns added in cops2 that were missing from the original cops1 schema
        ("dr_master", "pax_nationality", "VARCHAR(100)"),
        ("dr_master", "booked_by",       "VARCHAR(200)"),
        ("dr_master", "os_year",         "INTEGER"),
        // br_master needs these for exactly the same reason dr_master needs
        // os_year, and was missed. The baggage list SELECTs both columns, so
        // without them every request for the register failed with "no such
        // column" — 334,546 receipts unreachable through the page built to show
        // them, while the detention register beside it worked. Creating a BR
        // failed too, since the INSERT names is_legacy as well.
        //
        // Found by a search test that happened to exercise the list. Neither
        // route nor page nor file comparison could see it: everything existed,
        // the query inside was simply wrong.
        ("br_master", "os_year",         "INTEGER"),
        ("br_master", "is_legacy",       "TEXT DEFAULT 'N'"),
        // br_items was short the same way, and its INSERT names all three — so
        // booking a receipt with any item on it failed outright, and reading the
        // items back failed too. dr_items has dr_year and cops_items has the
        // other two; br_items alone had none of them. Types copied from those
        // siblings so the three registers agree.
        ("br_items",  "br_year",              "INTEGER"),
        ("br_items",  "cumulative_duty_rate", "FLOAT DEFAULT 0"),
        ("br_items",  "items_sub_category",   "VARCHAR(100)"),
        // dr_items: columns added in cops2 that were missing from the original cops1 schema
        ("dr_items",  "dr_year",         "INTEGER"),
        ("dr_items",  "items_category",  "VARCHAR(50)"),
        // Bank deposit challan number — added after the first COPS2 builds
        // shipped, so existing databases need it too.
        ("dcr_sessions", "challan_no",     "VARCHAR(50)"),
        // Cess on cigarettes — a duty component that was missing entirely,
        // so existing databases need the column before it can be recorded.
        ("dcr_entries",  "cess_on_cig",    "REAL NOT NULL DEFAULT 0"),
        // Rates as applied, so a session stays explainable if the
        // tariff row is later edited or lost.
        ("dcr_sessions", "tariff_snapshot", "TEXT"),
        // A formula is no longer rewritten in place; a change writes a new
        // version dated from the day it was made. Rules already in a database
        // become the first version of their own lineage, in force from before
        // any shift on record, so every existing sheet keeps computing as it did.
        ("dcr_formula_rules", "lineage_id",     "INTEGER"),
        ("dcr_formula_rules", "effective_from", "TEXT"),
        ("dcr_formula_rules", "changed_by",     "TEXT"),
        // The user admin — one designated AC/DC who alone may add, close, or reset
        // the office's user accounts. A flag, not a role: the person keeps doing
        // their own job and holds this authority beside it, and it moves from one
        // officer to another by being cleared here and set there. Every existing
        // user defaults to 0 — no one holds it until the system admin grants it.
        ("users", "is_user_admin", "INTEGER NOT NULL DEFAULT 0"),
    ];
    for (table, col, col_type) in &col_migrations {
        let sql = format!("ALTER TABLE {} ADD COLUMN {} {}", table, col, col_type);
        if let Err(e) = conn.execute_batch(&sql) {
            let msg = e.to_string();
            if !msg.contains("duplicate column name") {
                return Err(e.into());
            }
        }
    }

    // Every rule that predates the version history is version one of its own
    // line, in force from before any shift on record. Done after the columns
    // above exist, and harmless to repeat.
    conn.execute_batch(
        "UPDATE dcr_formula_rules SET lineage_id = id WHERE lineage_id IS NULL;
         UPDATE dcr_formula_rules SET effective_from = '1900-01-01' WHERE effective_from IS NULL;"
    ).ok();

    // Defensive index additions for existing DBs (CREATE INDEX IF NOT EXISTS is idempotent,
    // but the migrations.sql may not have run again on existing databases).
    let index_migrations = [
        "CREATE INDEX IF NOT EXISTS ix_cops_master_os_date ON cops_master (os_date)",
    ];
    for sql in &index_migrations {
        conn.execute_batch(sql)?;
    }

    // Defensive migration for feature_flags: old schema used boolean columns
    // (apis_enabled INTEGER, session_timeout_minutes INTEGER) instead of a key-value
    // store. Detect old schema and recreate the table correctly.
    let old_schema: bool = conn.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('feature_flags') WHERE name='apis_enabled'",
        [],
        |r| r.get::<_, i64>(0),
    ).unwrap_or(0) > 0;

    if old_schema {
        conn.execute_batch(
            "DROP TABLE IF EXISTS feature_flags;
             CREATE TABLE feature_flags (
                 config_key   TEXT PRIMARY KEY,
                 config_value TEXT NOT NULL DEFAULT ''
             );"
        )?;
    }

    // Seed DCR initial data (idempotent)
    seed_dcr_defaults(&conn)?;

    dedupe_and_protect_items(&conn);
    backfill_search_indexes(&conn);

    Ok(())
}


/// Remove duplicate item rows, then make them impossible.
///
/// cops_items had no unique constraint of any kind, and the restore inserts with
/// INSERT OR IGNORE — which ignores nothing when there is no constraint to
/// violate. Restoring the same archive twice therefore appended a second copy of
/// every item on every case, and a third restore a third copy. The master rows
/// were safe because they are deduplicated in code; the items were not
/// deduplicated anywhere.
///
/// The index is PARTIAL, on active rows only, exactly as cops_master's is. A
/// deleted case may have its number reused, and the old soft-deleted items stay
/// behind — a plain unique index would refuse the new case's first item.
///
/// The cleanup has to run before the index can be created, and the index cannot
/// live in migrations.sql for the same reason: that file runs as one batch, so a
/// CREATE UNIQUE INDEX that fails on existing duplicates would abort the whole
/// migration and take the application down with it.
fn dedupe_and_protect_items(conn: &rusqlite::Connection) {
    // The revenue tables are restored with INSERT OR IGNORE too, and had no
    // constraint either — so a second restore would have doubled every session's
    // figures. They are keyed on the session and the row's position within it,
    // which is what makes one line of a shift's sheet distinct from the next.
    // Caught while the module still has no data in it, which is the only
    // comfortable time to find this.
    for (table, keys) in [
        ("cops_items",     "os_no, os_year, items_sno"),
        ("br_items",       "br_no, br_date, items_sno"),
        ("dr_items",       "dr_no, dr_date, items_sno"),
        ("dcr_entries",    "session_id, sort_order"),
        ("dcr_dr_entries", "session_id, sort_order"),
        ("dcr_os_entries", "session_id, sort_order"),
        ("dcr_tariffs",    "effective_from"),
        ("dcr_item_types", "name"),
    ] {
        let idx = format!("uq_{table}_active");

        // The index existing means this table was cleaned once and has been
        // protected ever since, so there is nothing to look for. Checking costs
        // a tenth of a millisecond; the cleaning scan it skips costs 227 ms per
        // table on 357,705 rows unencrypted, and several times that through
        // SQLCipher. Without this the office would pay a second or so of every
        // launch, forever, to find nothing.
        let already: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name=?1",
            [&idx], |r| r.get(0),
        ).unwrap_or(0);
        if already > 0 { continue; }

        // Only the case/receipt item tables carry entry_deleted; the revenue
        // tables have no such column, so the predicate has to be dropped for
        // them or every statement below is a syntax error.
        let soft = conn
            .query_row(
                &format!("SELECT COUNT(*) FROM pragma_table_info('{table}') WHERE name='entry_deleted'"),
                [], |r| r.get::<_, i64>(0),
            ).unwrap_or(0) > 0;
        let live = if soft { "WHERE entry_deleted IS NULL OR entry_deleted != 'Y'" } else { "" };
        let also = if soft { "AND (entry_deleted IS NULL OR entry_deleted != 'Y')" } else { "" };

        let removed = conn.execute(
            &format!(
                "DELETE FROM {table} WHERE rowid NOT IN (
                     SELECT MIN(rowid) FROM {table} {live} GROUP BY {keys}) {also}"
            ),
            [],
        );
        match removed {
            Ok(n) if n > 0 => tracing::warn!("removed {n} duplicate rows from {table}"),
            Ok(_) => {}
            Err(e) => {
                tracing::error!("could not clean duplicates from {table}: {e}");
                continue;   // do not attempt the index over rows we failed to clean
            }
        }
        if let Err(e) = conn.execute_batch(&format!(
            "CREATE UNIQUE INDEX IF NOT EXISTS {idx} ON {table} ({keys}) {live}"
        )) {
            tracing::error!("could not create {idx} ({e}) — duplicate items remain possible");
        }
    }
}

/// Populate the full-text indexes on a database that already holds registers.
///
/// The triggers keep them in step from here on, but they only fire on rows
/// written after the trigger exists — a database upgraded from an older build,
/// or one restored before this shipped, has 334,546 receipts the index has never
/// seen. Left alone, search would return nothing for them and look like the
/// records were missing.
///
/// Rebuilding costs about four seconds and runs only when the index is empty
/// while its table is not, so an ordinary start pays two counting queries.
fn backfill_search_indexes(conn: &rusqlite::Connection) {
    for (fts, table) in [("br_search", "br_master"), ("dr_search", "dr_master")] {
        // Existence, not a count. COUNT(*) over an FTS5 index walks the whole
        // index — 48 ms on 334,546 receipts unencrypted, and this ran on both
        // registers at every launch to answer a question that only needs "is
        // there anything in here at all". LIMIT 1 answers it in 0.12 ms.
        let indexed = conn
            .query_row(&format!("SELECT rowid FROM {fts} LIMIT 1"), [], |r| r.get::<_, i64>(0))
            .is_ok();
        let has_rows = conn
            .query_row(&format!("SELECT 1 FROM {table} LIMIT 1"), [], |r| r.get::<_, i64>(0))
            .is_ok();
        if !indexed && has_rows {
            let actual: i64 = conn
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
                .unwrap_or(0);
            tracing::info!("building the {fts} index over {actual} rows (one time)");
            if let Err(e) =
                conn.execute_batch(&format!("INSERT INTO {fts}({fts}) VALUES('rebuild')"))
            {
                // Not fatal: search falls back to the scan it used before, which
                // is slow but correct. Saying so beats a search that is quietly
                // fast and quietly incomplete.
                tracing::error!(
                    "could not build {fts} ({e}) — register search will use the slow path"
                );
            }
        }
    }
}

fn seed_dcr_defaults(conn: &rusqlite::Connection) -> Result<()> {
    // Seed initial tariff (only if none exists)
    let has_tariff: i64 = conn.query_row(
        "SELECT COUNT(*) FROM dcr_tariffs", [], |r| r.get(0)
    ).unwrap_or(0);
    if has_tariff == 0 {
        conn.execute(
            "INSERT INTO dcr_tariffs (effective_from, label, baggage_rate, liquor_duty_rate,
             aidc_liquor_rate, gold_bcd_rate, aidc_gold_rate, gold_cons_bcd_rate, aidc_gold_cons_rate,
             silver_bcd_rate, aidc_silver_rate, silver_cons_rate, aidc_silver_cons_rate)
             VALUES (date('now'), 'Initial Rates', 0.35, 0.15, 0.035, 0.125, 0.05, 0.125, 0.05,
             0.35, 0.05, 0.35, 0.05)",
            [],
        )?;
    }

    // Seed initial settings (only if none exists)
    conn.execute(
        "INSERT OR IGNORE INTO dcr_settings (id, station_name) VALUES (1, 'CUSTOMS, CHENNAI AIRPORT')",
        [],
    )?;

    // Seed system item types (only if none exist)
    let has_items: i64 = conn.query_row(
        "SELECT COUNT(*) FROM dcr_item_types", [], |r| r.get(0)
    ).unwrap_or(0);
    if has_items == 0 {
        let system_items = [
            "BAGGAGE", "GOLD", "SILVER", "GOLD(C)", "SILVER(C)", "LIQUOR",
            "CIGARETTES", "ELECTRONIC ITEMS", "FOREIGN CURRENCY",
        ];
        for name in &system_items {
            let _ = conn.execute(
                "INSERT OR IGNORE INTO dcr_item_types (name, is_system) VALUES (?, 1)",
                rusqlite::params![name],
            );
        }
    }

    // Seed formula rules (only if none exist)
    let has_rules: i64 = conn.query_row(
        "SELECT COUNT(*) FROM dcr_formula_rules", [], |r| r.get(0)
    ).unwrap_or(0);
    if has_rules == 0 {
        let rules: Vec<(i64, &str, &str, &str, &str, &str)> = vec![
            (1,  "baggage_duty",     "except", "GOLD,SILVER,GOLD(C),SILVER(C)", "value * baggage_rate",          "Baggage Duty"),
            (2,  "liquor_duty",      "only",   "LIQUOR",                         "value * liquor_duty_rate",      "Liquor Duty"),
            (3,  "gold_duty_bcd",    "only",   "GOLD,SILVER",                    "value * gold_bcd_rate",         "Gold/Silver BCD"),
            (4,  "aidc_gold_silver", "only",   "GOLD,SILVER",                    "value * aidc_gold_rate",        "AIDC on Gold/Silver"),
            (5,  "gold_duty_cons",   "only",   "GOLD(C)",                        "value * gold_cons_bcd_rate",    "Gold(C) BCD"),
            (6,  "aidc_gold_silver", "only",   "GOLD(C)",                        "value * aidc_gold_cons_rate",   "AIDC on Gold(C)"),
            (7,  "silver_duty_cons", "only",   "SILVER(C)",                      "value * silver_cons_rate",      "Silver(C) Concessional"),
            (8,  "aidc_gold_silver", "only",   "SILVER(C)",                      "value * aidc_silver_cons_rate", "AIDC on Silver(C)"),
            (9,  "aidc_on_liquor",   "only",   "LIQUOR",                         "value * aidc_liquor_rate",      "AIDC on Liquor"),
        ];
        for (sort_order, target_column, condition_type, condition_items, expression, column_label) in rules {
            let _ = conn.execute(
                "INSERT INTO dcr_formula_rules (sort_order, target_column, column_label, condition_type, condition_items, expression, is_active)
                 VALUES (?, ?, ?, ?, ?, ?, 1)",
                rusqlite::params![sort_order, target_column, column_label, condition_type, condition_items, expression],
            );
        }
    }

    Ok(())
}
