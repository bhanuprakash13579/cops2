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
                 PRAGMA mmap_size     = 268435456; -- 256 MB memory-mapped I/O",
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

    Ok(())
}
