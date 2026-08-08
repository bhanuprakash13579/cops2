pub mod api;
pub mod auth;
mod backup_export;
mod backup_service;
mod config;
pub mod db;
mod models;
mod pdf;
mod security;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::Manager;

/// Resolve which SQLite file cops2 should open.
///
/// Priority:
///   1. `cops.db` — cops2's own database, UNLESS a cops1 database beside it
///      holds more case records, in which case that one wins and `cops.db` is
///      renamed aside rather than deleted. See the comment on that branch: the
///      failure it prevents is silent and severe.
///   2. `cops_br_database.db` — cops1's database in the same app-data directory.
///      If it exists, it is migrated into `cops.db` (handling both plain and
///      encrypted cops1 formats).  After a VERIFIED migration the old cops1
///      database is securely wiped so sensitive data never sits unencrypted.
///   3. Fresh install — `cops.db` will be created empty by `create_pool`.
fn resolve_db_path(app_data: &Path) -> PathBuf {
    let cops2_db       = app_data.join("cops.db");
    let cops1_db       = app_data.join("cops_br_database.db");
    let migration_lock = app_data.join(".migration_lock");

    // ── Interrupted-migration recovery ────────────────────────────────────────
    // If cops.db exists but the sentinel lock file is also present, the last
    // migration run was interrupted before it could finish (e.g. force-quit,
    // power loss).  The partially-written cops.db is unusable — delete it and
    // its WAL/SHM companions so we fall through to a clean re-migration below.
    if cops2_db.exists() && migration_lock.exists() {
        tracing::warn!(
            "Detected interrupted migration (lock file present). \
             Removing partial cops.db and retrying migration…"
        );
        for name in &["cops.db", "cops.db-wal", "cops.db-shm"] {
            let _ = std::fs::remove_file(app_data.join(name));
        }
        // Leave the lock file in place — it will be removed after a successful
        // migration completes further below.
    }

    // Already have a fully-migrated cops2 database — use it directly.
    //
    // Unless a cops1 database is sitting beside it holding more. That should not
    // happen: a successful migration wipes the cops1 file. But it DOES happen —
    // a migration that failed after creating cops.db, a test run on an officer's
    // machine, a cops1 database restored afterwards — and the consequence is the
    // worst kind. The app opens a nearly-empty database, reports no error, and
    // the office finds their records missing while the file holding all of them
    // sits untouched in the same folder. The sibling project hit exactly this,
    // where a leftover holding one case was chosen over one holding 28,896.
    //
    // So when both exist, the one with more case records wins, loudly.
    if cops2_db.exists() {
        if cops1_db.exists() {
            let mine = case_count(&cops2_db, false);
            let theirs = case_count(&cops1_db, true);
            if theirs > mine {
                tracing::error!(
                    "REFUSING TO USE THE SMALLER DATABASE. {:?} holds {} OS cases but the \
                     cops1 database beside it holds {}. Using the cops1 database and \
                     migrating it. The smaller file is left untouched.",
                    cops2_db, mine, theirs
                );
                let stale = app_data.join(format!(
                    "cops.db.ignored-{}",
                    chrono::Local::now().format("%Y-%m-%d_%H%M%S")
                ));
                // Renamed, never deleted — it may hold cases booked since the
                // cops1 file was last written, and that is not ours to discard.
                let _ = std::fs::rename(&cops2_db, &stale);
                for suffix in ["-wal", "-shm"] {
                    let _ = std::fs::remove_file(
                        app_data.join(format!("cops.db{suffix}")),
                    );
                }
            } else {
                tracing::info!("Using existing cops2 database: {:?} ({mine} OS cases)", cops2_db);
                return cops2_db;
            }
        } else {
            tracing::info!("Using existing cops2 database: {:?}", cops2_db);
            return cops2_db;
        }
    }

    // Check for cops1 database (handles both plain and encrypted).
    if cops1_db.exists() {
        tracing::info!("Found cops1 database at {:?} — attempting migration…", cops1_db);

        // Write the sentinel BEFORE touching cops.db so that any interruption
        // (force-quit, power loss) between now and the final cleanup is
        // detected on the next startup and the partial DB is discarded.
        let _ = std::fs::write(&migration_lock, b"migration in progress");

        match migrate_cops1(&cops1_db, &cops2_db) {
            Ok(()) => {
                tracing::info!("cops1 → cops2 migration complete. Using {:?}", cops2_db);

                // ── SECURITY: Wipe old cops1 database ─────────────────────
                // The old DB may be plain-text (unencrypted) — a security
                // threat for sensitive customs data.  Overwrite with zeros
                // then delete so it can't be recovered.
                secure_delete_cops1_files(app_data);

                // Remove the sentinel only after everything succeeded.
                let _ = std::fs::remove_file(&migration_lock);

                return cops2_db;
            }
            Err(e) => {
                tracing::warn!(
                    "cops1 migration skipped ({}). \
                     Use the admin panel to restore from a cops1 backup.",
                    e
                );
                // Clean up the partial cops.db (if any) and the sentinel so
                // we don't loop on the recovery path next time.
                for name in &["cops.db", "cops.db-wal", "cops.db-shm"] {
                    let _ = std::fs::remove_file(app_data.join(name));
                }
                let _ = std::fs::remove_file(&migration_lock);
                // Fall through: create_pool will create a fresh cops.db.
            }
        }
    }

    cops2_db
}

/// How many OS cases does this database hold? 0 if it cannot be read.
///
/// Used only to decide which of two databases is the real one, so a file that
/// cannot be opened counts as empty — it is not a candidate either way.
fn case_count(path: &Path, cops1: bool) -> i64 {
    let open = || -> anyhow::Result<i64> {
        let conn = rusqlite::Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;
        // A plain database needs no key; try that before paying for PBKDF2.
        if conn.query_row("SELECT COUNT(*) FROM sqlite_master", [], |r| r.get::<_, i64>(0)).is_err() {
            conn.execute_batch(&if cops1 {
                crate::security::cops1_sqlcipher_pragma()
            } else {
                crate::security::sqlcipher_pragma()
            })?;
        }
        Ok(conn.query_row("SELECT COUNT(*) FROM cops_master", [], |r| r.get(0))?)
    };
    open().unwrap_or(0)
}

/// Securely wipe old cops1 database files after successful migration.
///
/// Steps:
///   1. Overwrite the file with zeros (prevents casual undelete recovery).
///   2. Delete the zeroed file.
///   3. Remove associated WAL/SHM journal files.
///   4. Remove the `.enc.bak` backup copy if present.
fn secure_delete_cops1_files(app_data: &Path) {
    let files_to_wipe = [
        "cops_br_database.db",
        "cops_br_database.db-wal",
        "cops_br_database.db-shm",
        "cops_br_database.db.enc.bak",
    ];

    for name in &files_to_wipe {
        let path = app_data.join(name);
        if !path.exists() { continue; }

        // Overwrite with zeros for security
        if let Ok(meta) = std::fs::metadata(&path) {
            let size = meta.len();
            if size > 0 {
                if let Ok(mut f) = std::fs::OpenOptions::new().write(true).open(&path) {
                    use std::io::Write;
                    let zeros = vec![0u8; 65536]; // 64 KB chunks
                    let mut remaining = size;
                    while remaining > 0 {
                        let chunk = remaining.min(zeros.len() as u64) as usize;
                        if f.write_all(&zeros[..chunk]).is_err() { break; }
                        remaining -= chunk as u64;
                    }
                    let _ = f.flush();
                    let _ = f.sync_all();
                }
            }
        }

        // Delete the zeroed file
        match std::fs::remove_file(&path) {
            Ok(()) => tracing::info!("Securely wiped old cops1 file: {name}"),
            Err(e) => tracing::warn!("Could not delete {name}: {e}"),
        }
    }
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
    //
    // 1000 pages per step with a 5 ms pause, NOT 5 pages per 50 ms. The pause is
    // taken between every step, so the step size sets the total time: a 242 MB
    // database is roughly 62,000 pages, which at 5 pages per 50 ms is about ten
    // minutes of what looks to the officer like a hung first launch. At this
    // rate the same copy is a few seconds. The pause still exists so the source
    // stays readable if anything else has it open.
    let backup = Backup::new(&src, &mut dst)?;
    backup.run_to_completion(1000, Duration::from_millis(5), None)?;
    drop(backup);

    // Prove the copy is complete BEFORE the caller is told it succeeded.
    //
    // This matters more here than anywhere else in the program: on success the
    // caller securely wipes the cops1 database — zeroed and deleted, with no
    // recovery. Returning Ok on the strength of "the copy did not error" would
    // mean a subtly incomplete migration destroys the only original. The byte
    // size that used to be logged here proves nothing at all about content.
    verify_migration(&src, &dst)?;

    tracing::info!(
        "cops1 → cops2 migration complete and verified ({} bytes, re-encrypted with SHA-256-v2 key)",
        dst_path.metadata().map(|m| m.len()).unwrap_or(0)
    );
    Ok(())
}

/// Compare every table in the source against the destination, row for row.
///
/// Deliberately driven from the SOURCE's table list rather than a list written
/// here, so a table this code has never heard of still has to survive the
/// migration. A hardcoded list would silently ignore exactly the tables a
/// future version adds.
fn verify_migration(src: &rusqlite::Connection, dst: &rusqlite::Connection) -> anyhow::Result<()> {
    let mut stmt = src.prepare(
        "SELECT name FROM sqlite_master
         WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
    )?;
    let tables: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .filter_map(|r| r.ok())
        .collect();
    if tables.is_empty() {
        anyhow::bail!("source database has no tables — refusing to treat this as a migration");
    }

    let mut checked = 0usize;
    let mut total = 0i64;
    for t in &tables {
        let want: i64 = src
            .query_row(&format!("SELECT COUNT(*) FROM \"{t}\""), [], |r| r.get(0))
            .map_err(|e| anyhow::anyhow!("cannot count {t} in the cops1 database: {e}"))?;
        let got: i64 = dst
            .query_row(&format!("SELECT COUNT(*) FROM \"{t}\""), [], |r| r.get(0))
            .map_err(|e| anyhow::anyhow!(
                "table {t} is missing from the migrated database ({e}) — \
                 the cops1 database has NOT been touched"
            ))?;
        if got != want {
            anyhow::bail!(
                "{t}: migrated database has {got} rows, cops1 has {want} — \
                 migration incomplete, the cops1 database has NOT been touched"
            );
        }
        checked += 1;
        total += want;
    }

    let check: String = dst.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
    if check != "ok" {
        anyhow::bail!("migrated database failed its integrity check: {check}");
    }

    tracing::info!("migration verified: {checked} tables, {total} rows, all counts match");
    Ok(())
}
use tower_http::cors::{Any, CorsLayer};
use tower_http::compression::CompressionLayer;
use tauri::Emitter;

// ── Windows-only: raw Win32 FFI (no extra crate — user32.dll is always present)
#[cfg(target_os = "windows")]
mod win32 {
    use std::ffi::c_void;
    pub type HWND   = *mut c_void;
    pub type WPARAM = usize;
    pub type LPARAM = isize;
    pub type BOOL   = i32;
    pub const GWL_EXSTYLE:      i32 = -20;
    pub const WS_EX_TOOLWINDOW: i32 = 0x0000_0080_u32 as i32;
    pub const WS_EX_APPWINDOW:  i32 = 0x0004_0000_u32 as i32;
    /// Show without activating — foreground app keeps focus.
    pub const SW_SHOWNOACTIVATE: i32 = 4;
    pub const TRUE: BOOL = 1;
    /// WM_SYSCOMMAND + SC_MAXIMIZE maximizes without activating the window.
    pub const WM_SYSCOMMAND: u32 = 0x0112;
    pub const SC_MAXIMIZE: WPARAM = 0xF030;
    #[link(name = "user32")]
    extern "system" {
        pub fn ShowWindow(hwnd: HWND, n_cmd_show: i32) -> BOOL;
        pub fn GetWindowLongW(hwnd: HWND, n_index: i32) -> i32;
        pub fn SetWindowLongW(hwnd: HWND, n_index: i32, dw_new_long: i32) -> i32;
        pub fn GetClassNameW(hwnd: HWND, lp_class_name: *mut u16, n_max_count: i32) -> i32;
        pub fn EnumChildWindows(
            hwnd_parent:  HWND,
            lp_enum_func: Option<unsafe extern "system" fn(HWND, LPARAM) -> BOOL>,
            l_param:      LPARAM,
        ) -> BOOL;
        pub fn PostMessageW(hwnd: HWND, msg: u32, w_param: WPARAM, l_param: LPARAM) -> BOOL;
    }
}

// ── Tauri command: show window without stealing focus on Windows ───────────────
// Called by main.tsx once the webview has painted its first frame.
// On Windows uses SW_SHOWNOACTIVATE so COPS appears without yanking focus from
// Chrome or other apps.  On Linux/macOS falls back to normal show().
#[tauri::command]
fn show_main_window(app: tauri::AppHandle) {
    let Some(window) = app.get_webview_window("main") else { return };
    #[cfg(target_os = "windows")]
    {
        if let Ok(hwnd) = window.hwnd() {
            unsafe {
                win32::ShowWindow(hwnd.0, win32::SW_SHOWNOACTIVATE);
                // Maximize without stealing focus: post WM_SYSCOMMAND + SC_MAXIMIZE.
                win32::PostMessageW(hwnd.0, win32::WM_SYSCOMMAND, win32::SC_MAXIMIZE, 0);
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = window.show();
        // GTK/WebKit2GTK may not honour maximized:true for initially-hidden windows.
        let _ = window.maximize();
    }
}

// ── Windows helper: hide WebView2 child window from taskbar grouping ──────────
// WebView2 creates Chrome_WidgetWin_1 under the Tauri HWND. Windows 11 DWM
// shows it as a second thumbnail (the "double tab" effect). Setting
// WS_EX_TOOLWINDOW on it removes it from the taskbar group.
#[cfg(target_os = "windows")]
unsafe extern "system" fn hide_webview2_thumbnail(
    hwnd: win32::HWND, _: win32::LPARAM,
) -> win32::BOOL {
    let mut buf = [0u16; 256];
    let len = win32::GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
    if len > 0 {
        let class = String::from_utf16_lossy(&buf[..len as usize]);
        if class.starts_with("Chrome_WidgetWin") {
            let ex = win32::GetWindowLongW(hwnd, win32::GWL_EXSTYLE);
            win32::SetWindowLongW(hwnd, win32::GWL_EXSTYLE,
                (ex | win32::WS_EX_TOOLWINDOW) & !win32::WS_EX_APPWINDOW);
        }
    }
    win32::TRUE
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // ── Linux/Wayland compatibility ───────────────────────────────────────────
    // Force X11 backend and disable DMA-BUF renderer so WebKit2GTK works on
    // both X11 and Wayland sessions (including GNOME on Ubuntu 22.04/24.04).
    // Without these, the app silently fails to open on many Wayland desktops.
    #[cfg(target_os = "linux")]
    {
        std::env::set_var("GDK_BACKEND", "x11");
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }

    tracing_subscriber::fmt()
        .with_env_filter("cops2=debug")
        .init();

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![show_main_window])
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            // ── Database ──────────────────────────────────────────────────────
            // Use app_local_data_dir (AppData\Local on Windows) to match cops1's
            // storage location exactly — cops1 stores cops_br_database.db there.
            // On Linux/macOS app_local_data_dir == app_data_dir.
            let app_data = match app.path().app_local_data_dir() {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("[cops2] FATAL: cannot resolve app data dir: {e}");
                    let _ = app.handle().emit("sidecar-startup-failed",
                        format!("Cannot determine app data directory: {e}. Try reinstalling COPS."));
                    return Ok(());
                }
            };

            if let Err(e) = std::fs::create_dir_all(&app_data) {
                eprintln!("[cops2] FATAL: cannot create app data dir: {e}");
                let _ = app.handle().emit("sidecar-startup-failed",
                    format!("Cannot create app data directory: {e}. Check folder permissions."));
                return Ok(());
            }

            let db_path = resolve_db_path(&app_data);

            // ── Detect and encrypt existing plain-SQLite databases ─────────────
            if db_path.exists() && security::is_plain_sqlite(&db_path) {
                tracing::info!("Detected plain-SQLite database — encrypting in-place…");
                if let Err(e) = security::encrypt_plain_db_inplace(&db_path) {
                    eprintln!("[cops2] FATAL: in-place DB encryption failed: {e}");
                    let _ = app.handle().emit("sidecar-startup-failed",
                        format!("Database encryption failed: {e}. Contact support."));
                    return Ok(());
                }
            }

            let pool = match db::create_pool(&db_path) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("[cops2] FATAL: cannot open database: {e}");
                    let _ = app.handle().emit("sidecar-startup-failed",
                        format!("Cannot open database: {e}. \
                            The database file may be corrupt. Try restoring from a backup."));
                    return Ok(());
                }
            };

            if let Err(e) = db::run_migrations(&pool) {
                eprintln!("[cops2] FATAL: migrations failed: {e}");
                let _ = app.handle().emit("sidecar-startup-failed",
                    format!("Database migration failed: {e}. Try reinstalling COPS."));
                return Ok(());
            }

            let pool = Arc::new(pool);

            // Automatic backups. Starts a timer and returns immediately — the
            // first run is delayed so it does not compete with startup, and the
            // orphan sweep happens on that first tick rather than here, because
            // it walks the destination folders and a switched-off machine would
            // otherwise delay the application appearing.
            //
            // Does nothing at all until a destination folder is configured.
            backup_service::start(pool.clone());

            // ── Windows: fix WebView2 double-taskbar thumbnail ────────────────
            // After 800 ms (WebView2 init time), enumerate child windows and set
            // WS_EX_TOOLWINDOW on Chrome_WidgetWin_* to hide them from the
            // taskbar group so only one thumbnail appears on hover.
            #[cfg(target_os = "windows")]
            {
                if let Some(main_win) = app.get_webview_window("main") {
                    if let Ok(main_hwnd) = main_win.hwnd() {
                        // *mut c_void is not Send — convert to usize before crossing thread
                        // boundary, then cast back inside the async block.
                        let hwnd_raw = main_hwnd.0 as usize;
                        tauri::async_runtime::spawn(async move {
                            tokio::time::sleep(std::time::Duration::from_millis(800)).await;
                            unsafe {
                                win32::EnumChildWindows(
                                    hwnd_raw as win32::HWND,
                                    Some(hide_webview2_thumbnail),
                                    0,
                                );
                            }
                        });
                    }
                }
            }

            // ── Axum HTTP server embedded in Tauri process ────────────────────
            let pool_clone = Arc::clone(&pool);
            let cors = CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any);

            let router = api::build_app(pool_clone)
                .layer(cors)
                .layer(CompressionLayer::new());

            // ── Port binding with SO_REUSEADDR ────────────────────────────────
            // Using socket2 to set SO_REUSEADDR before bind.  Without this,
            // Windows keeps the port in TIME_WAIT for ~60 s after an abrupt
            // crash, causing "port already in use" on an immediate restart even
            // when no other instance is actually running.
            let bind_addr = format!("127.0.0.1:{}", api::SERVER_PORT);
            let bind_sock_addr: std::net::SocketAddr = match bind_addr.parse() {
                Ok(a) => a,
                Err(e) => return Err(format!("Invalid bind address {bind_addr}: {e}").into()),
            };
            let socket = match socket2::Socket::new(
                socket2::Domain::IPV4,
                socket2::Type::STREAM,
                Some(socket2::Protocol::TCP),
            ) {
                Ok(s) => s,
                Err(e) => return Err(format!("Failed to create TCP socket: {e}").into()),
            };
            if let Err(e) = socket.set_reuse_address(true) {
                tracing::warn!("Could not set SO_REUSEADDR (non-fatal): {e}");
            }
            if let Err(e) = socket.bind(&bind_sock_addr.into()) {
                return Err(format!(
                    "Port {} is already in use ({e}).\n\n\
                     Another instance of COPS may already be running.\n\
                     Please close it and try again.",
                    api::SERVER_PORT
                ).into());
            }
            if let Err(e) = socket.listen(128) {
                return Err(format!("Failed to listen on port {}: {e}", api::SERVER_PORT).into());
            }
            let listener: std::net::TcpListener = socket.into();

            let app_handle_for_axum = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let tcp = match tokio::net::TcpListener::from_std(listener) {
                    Ok(l) => l,
                    Err(e) => {
                        eprintln!("[cops2] FATAL: TcpListener conversion failed: {e}");
                        let _ = app_handle_for_axum.emit("sidecar-startup-failed",
                            format!("Internal server error: {e}. Please restart COPS."));
                        return;
                    }
                };
                if let Err(e) = axum::serve(tcp, router).await {
                    eprintln!("[cops2] Axum server stopped: {e}");
                    let _ = app_handle_for_axum.emit("sidecar-startup-failed",
                        format!("The internal API server stopped unexpectedly: {e}. Please restart COPS."));
                }
            });

            tracing::info!("COPS2 API → http://127.0.0.1:{}{}", api::SERVER_PORT, api::API_PREFIX);

            // ── Window show is handled from JS (main.tsx → show_main_window) ──
            // DO NOT call win.show() here. setup() runs before the webview
            // renders its first frame, so showing here causes a white-flash DWM
            // flicker on Windows (visible: false provides no benefit if you show
            // before WebView2 paints).  The JS-side call in main.tsx fires after
            // React renders and uses SW_SHOWNOACTIVATE on Windows so COPS appears
            // without stealing focus from other apps.

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod migration_tests {
    use super::*;

    /// Build a database in cops1's shape: encrypted with the PBKDF2-v1 key.
    fn make_cops1(path: &Path, rows: i64) {
        let c = rusqlite::Connection::open(path).unwrap();
        c.execute_batch(&crate::security::cops1_sqlcipher_pragma()).unwrap();
        c.execute_batch(
            "CREATE TABLE cops_master(id INTEGER PRIMARY KEY, os_no TEXT, amt REAL);
             CREATE TABLE print_template_config(k TEXT PRIMARY KEY, body TEXT);",
        ).unwrap();
        for i in 1..=rows {
            c.execute("INSERT INTO cops_master(os_no, amt) VALUES (?1, ?2)",
                      rusqlite::params![format!("OS/{i}/2026"), i as f64]).unwrap();
        }
        c.execute("INSERT INTO print_template_config VALUES ('h','CHENNAI')", []).unwrap();
    }

    fn tmp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("cops_mig_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn an_encrypted_cops1_database_migrates_with_every_row_intact() {
        let d = tmp("ok");
        let src = d.join("cops_br_database.db");
        let dst = d.join("cops.db");
        make_cops1(&src, 2000);

        migrate_cops1(&src, &dst).expect("migration must succeed");

        let c = rusqlite::Connection::open(&dst).unwrap();
        c.execute_batch(&crate::security::sqlcipher_pragma()).unwrap();
        let n: i64 = c.query_row("SELECT COUNT(*) FROM cops_master", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 2000);
        let t: String = c.query_row(
            "SELECT body FROM print_template_config WHERE k='h'", [], |r| r.get(0)).unwrap();
        assert_eq!(t, "CHENNAI", "admin templates must survive the upgrade");
    }

    #[test]
    fn a_truncated_migration_is_refused_so_the_original_is_never_wiped() {
        // The caller securely wipes the cops1 database when this returns Ok.
        // Verification is the only thing standing between an incomplete copy
        // and the original being destroyed, so prove it actually rejects one.
        let d = tmp("bad");
        let src = d.join("cops_br_database.db");
        make_cops1(&src, 500);
        let s = rusqlite::Connection::open(&src).unwrap();
        s.execute_batch(&crate::security::cops1_sqlcipher_pragma()).unwrap();

        // A destination that is a plausible but incomplete copy.
        let dst = d.join("cops.db");
        let t = rusqlite::Connection::open(&dst).unwrap();
        t.execute_batch(&crate::security::sqlcipher_pragma()).unwrap();
        t.execute_batch(
            "CREATE TABLE cops_master(id INTEGER PRIMARY KEY, os_no TEXT, amt REAL);
             CREATE TABLE print_template_config(k TEXT PRIMARY KEY, body TEXT);").unwrap();
        t.execute("INSERT INTO cops_master(os_no, amt) VALUES ('OS/1/2026', 1.0)", []).unwrap();

        let e = verify_migration(&s, &t).expect_err("an incomplete copy must be refused");
        assert!(e.to_string().contains("NOT been touched"),
                "the message must tell the officer the original is safe: {e}");
    }

    #[test]
    fn a_leftover_cops2_database_never_hides_a_fuller_cops1_one() {
        // The exact situation found on a real machine: a 315 KB cops.db sitting
        // beside a 254 MB cops_br_database.db. Choosing the first that exists
        // opens the near-empty one, reports no error, and the office finds their
        // records gone while the file holding all of them sits in the same folder.
        let d = tmp("shadow");
        make_cops1(&d.join("cops_br_database.db"), 5000);

        // A small cops2 database, as an abandoned migration or a test run leaves.
        let leftover = d.join("cops.db");
        {
            let c = rusqlite::Connection::open(&leftover).unwrap();
            c.execute_batch(&crate::security::sqlcipher_pragma()).unwrap();
            c.execute_batch("CREATE TABLE cops_master(id INTEGER PRIMARY KEY, os_no TEXT);").unwrap();
            c.execute("INSERT INTO cops_master(os_no) VALUES ('OS/1/2026')", []).unwrap();
        }

        let chosen = resolve_db_path(&d);
        let n: i64 = {
            let c = rusqlite::Connection::open(&chosen).unwrap();
            c.execute_batch(&crate::security::sqlcipher_pragma()).unwrap();
            c.query_row("SELECT COUNT(*) FROM cops_master", [], |r| r.get(0)).unwrap()
        };
        assert_eq!(n, 5000, "the fuller database must win, not the one found first");

        // And the one set aside must still exist — it may hold cases booked
        // after the cops1 file was last written, which is not ours to discard.
        let kept = std::fs::read_dir(&d).unwrap().flatten()
            .any(|e| e.file_name().to_string_lossy().starts_with("cops.db.ignored-"));
        assert!(kept, "the smaller database must be kept, not deleted");
    }

    #[test]
    fn an_ordinary_cops2_database_is_used_without_interference() {
        // The common case must not pay for the rule above.
        let d = tmp("normal");
        let only = d.join("cops.db");
        {
            let c = rusqlite::Connection::open(&only).unwrap();
            c.execute_batch(&crate::security::sqlcipher_pragma()).unwrap();
            c.execute_batch("CREATE TABLE cops_master(id INTEGER PRIMARY KEY);").unwrap();
        }
        assert_eq!(resolve_db_path(&d), only);
    }

    /// The complete cycle on real data: upgrade from cops1, take a backup,
    /// wipe the live database, restore it, and prove the case records came back.
    ///
    /// Every stage of this has been tested in isolation. This is the only test
    /// that proves they compose — which is where backup schemes actually fail.
    #[test]
    fn real_data_survives_upgrade_backup_wipe_and_restore() {
        let Ok(real) = std::env::var("COPS1_TEST_DB") else {
            eprintln!("skipping: set COPS1_TEST_DB to a COPY of a real cops1 database");
            return;
        };
        let d = tmp("cycle");
        let src = d.join("cops_br_database.db");
        std::fs::copy(&real, &src).unwrap();
        let live = d.join("cops.db");

        migrate_cops1(&src, &live).expect("upgrade");
        let pool = crate::db::create_pool(&live).unwrap();

        let before: Vec<(String, i64)> = ["cops_master", "cops_items", "br_master",
                                         "br_items", "dr_master", "dr_items",
                                         "print_template_config"]
            .iter()
            .map(|t| {
                let n: i64 = pool.get().unwrap()
                    .query_row(&format!("SELECT COUNT(*) FROM {t}"), [], |r| r.get(0)).unwrap();
                (t.to_string(), n)
            })
            .collect();
        let total: i64 = before.iter().map(|(_, n)| *n).sum();

        let archive = d.join("backup.cops");
        let t0 = std::time::Instant::now();
        let rep = crate::backup_export::write_archive(&pool, &archive).expect("backup");
        let backup_secs = t0.elapsed().as_secs_f64();

        // Destroy the live data, exactly as a failed disk or a bad import would.
        {
            let c = pool.get().unwrap();
            for (t, _) in &before {
                c.execute(&format!("DELETE FROM {t}"), []).unwrap();
            }
        }
        let wiped: i64 = pool.get().unwrap()
            .query_row("SELECT COUNT(*) FROM cops_master", [], |r| r.get(0)).unwrap();
        assert_eq!(wiped, 0, "the wipe must actually have happened");

        let unpacked = d.join("restore.db");
        crate::backup_export::extract(&archive, &unpacked).expect("extract");
        let t1 = std::time::Instant::now();
        crate::backup_export::restore_into(&pool, &unpacked).expect("restore");
        let restore_secs = t1.elapsed().as_secs_f64();

        for (t, want) in &before {
            let got: i64 = pool.get().unwrap()
                .query_row(&format!("SELECT COUNT(*) FROM {t}"), [], |r| r.get(0)).unwrap();
            assert_eq!(got, *want, "{t} did not come back intact");
        }

        eprintln!(
            "REAL CYCLE: {total} rows | backup {:.1} MB in {backup_secs:.1}s ({:.1}x) \
             | restore {restore_secs:.1}s | every table matched",
            rep.archive_bytes as f64 / 1_048_576.0, rep.ratio()
        );
    }

    /// Runs against a real cops1 database when COPS1_TEST_DB points at a copy.
    /// Skipped otherwise, so this suite still passes on a machine without one.
    #[test]
    fn migrates_a_real_cops1_database_when_one_is_provided() {
        let Ok(real) = std::env::var("COPS1_TEST_DB") else {
            eprintln!("skipping: set COPS1_TEST_DB to a COPY of a real cops1 database");
            return;
        };
        let d = tmp("real");
        let src = d.join("cops_br_database.db");
        std::fs::copy(&real, &src).expect("copy the source, never migrate the original");
        let dst = d.join("cops.db");

        let t0 = std::time::Instant::now();
        migrate_cops1(&src, &dst).expect("real migration must succeed and verify");
        let secs = t0.elapsed().as_secs_f64();

        let c = rusqlite::Connection::open(&dst).unwrap();
        c.execute_batch(&crate::security::sqlcipher_pragma()).unwrap();
        let cases: i64 = c.query_row("SELECT COUNT(*) FROM cops_master", [], |r| r.get(0)).unwrap();
        let br: i64 = c.query_row("SELECT COUNT(*) FROM br_master", [], |r| r.get(0)).unwrap();
        eprintln!("REAL MIGRATION: {cases} OS cases, {br} BRs, {secs:.1}s, \
                   {:.1} MB", std::fs::metadata(&dst).unwrap().len() as f64 / 1_048_576.0);
        assert!(cases > 0 && br > 0);
        assert!(secs < 120.0, "migration took {secs:.0}s — too slow for a first launch");
    }
}
