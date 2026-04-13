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
    Ok(())
}
