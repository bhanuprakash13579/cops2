mod api;
mod auth;
mod config;
mod db;
mod models;
mod pdf;
mod security;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::Manager;

/// Resolve which SQLite file cops2 should open.
///
/// Priority:
///   1. `cops.db`  — cops2's own database (created on first run, or already exists)
///   2. `cops_br_database.db` — cops1's database in the same app-data directory.
///      If it exists and is a plain (unencrypted) SQLite file, it is copied into
///      `cops.db` via the rusqlite backup API so cops2 starts with all existing data.
///   3. Fresh install — `cops.db` will be created empty by `create_pool`.
fn resolve_db_path(app_data: &Path) -> PathBuf {
    let cops2_db  = app_data.join("cops.db");
    let cops1_db  = app_data.join("cops_br_database.db");

    // Already have a cops2 database — use it directly.
    if cops2_db.exists() {
        tracing::info!("Using existing cops2 database: {:?}", cops2_db);
        return cops2_db;
    }

    // Check for cops1 database (plain SQLite — encrypted databases are skipped).
    if cops1_db.exists() {
        tracing::info!("Found cops1 database at {:?} — attempting migration…", cops1_db);
        match migrate_cops1(&cops1_db, &cops2_db) {
            Ok(()) => {
                tracing::info!("cops1 → cops2 migration complete. Using {:?}", cops2_db);
                return cops2_db;
            }
            Err(e) => {
                tracing::warn!(
                    "cops1 migration skipped ({}). \
                     Likely encrypted with SQLCipher — starting fresh. \
                     Use the admin panel to restore from a cops1 backup.",
                    e
                );
                // Fall through: cops2_db doesn't exist yet, create_pool will create it.
            }
        }
    }

    cops2_db
}

/// Open the cops1 database for reading, handling both plain and encrypted files.
///
/// Attempt order:
///   1. Plain SQLite (no key) — covers dev builds and very early cops1 installs.
///   2. cops1 PBKDF2-v1 key — covers all production cops1 installs where the DB
///      was encrypted with PBKDF2-HMAC-SHA256(binding_secret, v1-salt, 100_000).
///
/// The PBKDF2 derivation takes ~200-300 ms and runs only here, only once ever.
fn open_cops1_db(src_path: &Path) -> anyhow::Result<rusqlite::Connection> {
    let flags = rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY;

    // ── Try 1: plain SQLite ───────────────────────────────────────────────────
    let conn = rusqlite::Connection::open_with_flags(src_path, flags)?;
    if conn
        .query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get::<_, i64>(0))
        .is_ok()
    {
        tracing::info!("cops1 DB is plain SQLite — proceeding with migration");
        return Ok(conn);
    }

    // ── Try 2: cops1 PBKDF2-v1 encrypted key ─────────────────────────────────
    tracing::info!("cops1 DB is encrypted — deriving PBKDF2-v1 key (100 000 iterations, one-time cost)…");
    let conn = rusqlite::Connection::open_with_flags(src_path, flags)?;
    conn.execute_batch(&crate::security::cops1_sqlcipher_pragma())
        .map_err(|e| anyhow::anyhow!("Failed to apply cops1 PBKDF2 key: {e}"))?;
    conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get::<_, i64>(0))
        .map_err(|e| anyhow::anyhow!(
            "cops1 DB could not be read with PBKDF2-v1 key — unknown encryption or corrupt file: {e}"
        ))?;

    tracing::info!("cops1 encrypted DB unlocked successfully with PBKDF2-v1 key");
    Ok(conn)
}

/// Migrate cops1's database into cops2, re-encrypting with cops2's SHA-256-v2 key.
///
/// Handles both plain and PBKDF2-encrypted source databases.
/// After this runs once, cops2 owns `cops.db` and this function is never called again.
fn migrate_cops1(src_path: &Path, dst_path: &Path) -> anyhow::Result<()> {
    use rusqlite::backup::Backup;
    use std::time::Duration;

    let src = open_cops1_db(src_path)?;

    // Open destination and apply cops2's key so every page is written encrypted.
    let mut dst = rusqlite::Connection::open(dst_path)?;
    dst.execute_batch(&crate::security::sqlcipher_pragma())
        .map_err(|e| anyhow::anyhow!("Failed to initialise cops2 DB key: {e}"))?;

    // Page-by-page copy via SQLCipher's backup API.
    // steps=5 pages per iteration, 50 ms sleep between — keeps the source DB
    // readable if anything else has it open during migration.
    let backup = Backup::new(&src, &mut dst)?;
    backup.run_to_completion(5, Duration::from_millis(50), None)?;

    tracing::info!(
        "cops1 → cops2 migration complete ({} bytes, re-encrypted with SHA-256-v2 key)",
        dst_path.metadata().map(|m| m.len()).unwrap_or(0)
    );
    Ok(())
}
use tower_http::cors::{Any, CorsLayer};
use tower_http::compression::CompressionLayer;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter("cops2=debug")
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            // ── Database ──────────────────────────────────────────────────────
            let app_data = app.path().app_data_dir()
                .expect("failed to get app data dir");
            std::fs::create_dir_all(&app_data)?;

            let db_path = resolve_db_path(&app_data);

            // ── Detect and encrypt existing plain-SQLite databases ─────────────
            // If cops2 has a plain (unencrypted) database from an earlier build
            // before SQLCipher was added, encrypt it in-place now.
            if db_path.exists() && security::is_plain_sqlite(&db_path) {
                tracing::info!("Detected plain-SQLite database — encrypting in-place…");
                security::encrypt_plain_db_inplace(&db_path)
                    .expect("failed to encrypt existing plain database");
            }

            let pool = db::create_pool(&db_path)
                .expect("failed to create database pool");
            db::run_migrations(&pool)
                .expect("failed to run migrations");

            let pool = Arc::new(pool);

            // ── Axum HTTP server embedded in Tauri process ────────────────────
            // Same REST API shape as cops1 — frontend changes are minimal.
            let pool_clone = Arc::clone(&pool);
            let cors = CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any);

            let router = api::build_router(pool_clone)
                .layer(cors)
                .layer(CompressionLayer::new());

            // Same port as cops1 so the existing frontend api.ts base URL works.
            let listener = std::net::TcpListener::bind("127.0.0.1:8000")
                .expect("port 8000 in use");

            tokio::spawn(async move {
                axum::serve(
                    tokio::net::TcpListener::from_std(listener).unwrap(),
                    router,
                )
                .await
                .expect("axum server crashed");
            });

            tracing::info!("COPS2 API → http://127.0.0.1:8000");

            // Show the window now that the backend is ready and the webview has
            // had a chance to paint its first frame.  Starting with visible=false
            // prevents the split-second where Windows DWM sees the native host
            // frame and the WebView2 compositor surface as two separate surfaces
            // (the "two screens" taskbar thumbnail flicker on Windows 11).
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.show();
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
