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
///      If it exists, it is migrated into `cops.db` (handling both plain and
///      encrypted cops1 formats).  After successful migration the old cops1
///      database is securely wiped so sensitive data never sits unencrypted.
///   3. Fresh install — `cops.db` will be created empty by `create_pool`.
fn resolve_db_path(app_data: &Path) -> PathBuf {
    let cops2_db  = app_data.join("cops.db");
    let cops1_db  = app_data.join("cops_br_database.db");

    // Already have a cops2 database — use it directly.
    if cops2_db.exists() {
        tracing::info!("Using existing cops2 database: {:?}", cops2_db);
        return cops2_db;
    }

    // Check for cops1 database (handles both plain and encrypted).
    if cops1_db.exists() {
        tracing::info!("Found cops1 database at {:?} — attempting migration…", cops1_db);
        match migrate_cops1(&cops1_db, &cops2_db) {
            Ok(()) => {
                tracing::info!("cops1 → cops2 migration complete. Using {:?}", cops2_db);

                // ── SECURITY: Wipe old cops1 database ─────────────────────
                // The old DB may be plain-text (unencrypted) — a security
                // threat for sensitive customs data.  Overwrite with zeros
                // then delete so it can't be recovered.
                secure_delete_cops1_files(app_data);

                return cops2_db;
            }
            Err(e) => {
                tracing::warn!(
                    "cops1 migration skipped ({}). \
                     Use the admin panel to restore from a cops1 backup.",
                    e
                );
                // Fall through: cops2_db doesn't exist yet, create_pool will create it.
            }
        }
    }

    cops2_db
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
use tauri::Emitter;

// ── Windows-only: raw Win32 FFI (no extra crate — user32.dll is always present)
#[cfg(target_os = "windows")]
mod win32 {
    use std::ffi::c_void;
    pub type HWND   = *mut c_void;
    pub type LPARAM = isize;
    pub type BOOL   = i32;
    pub const GWL_EXSTYLE:      i32 = -20;
    pub const WS_EX_TOOLWINDOW: i32 = 0x0000_0080_u32 as i32;
    pub const WS_EX_APPWINDOW:  i32 = 0x0004_0000_u32 as i32;
    /// Show without activating — foreground app keeps focus.
    pub const SW_SHOWNOACTIVATE: i32 = 4;
    pub const TRUE: BOOL = 1;
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
                win32::ShowWindow(hwnd as usize as win32::HWND, win32::SW_SHOWNOACTIVATE);
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    { let _ = window.show(); }
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

            // ── Windows: fix WebView2 double-taskbar thumbnail ────────────────
            // After 800 ms (WebView2 init time), enumerate child windows and set
            // WS_EX_TOOLWINDOW on Chrome_WidgetWin_* to hide them from the
            // taskbar group so only one thumbnail appears on hover.
            #[cfg(target_os = "windows")]
            {
                if let Some(main_win) = app.get_webview_window("main") {
                    if let Ok(main_hwnd) = main_win.hwnd() {
                        tauri::async_runtime::spawn(async move {
                            tokio::time::sleep(std::time::Duration::from_millis(800)).await;
                            unsafe {
                                win32::EnumChildWindows(
                                    main_hwnd as usize as win32::HWND,
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

            // If port is already bound (another instance running), show the
            // window so the user can see the error dialog, then bail.
            let bind_addr = format!("127.0.0.1:{}", api::SERVER_PORT);
            let listener = match std::net::TcpListener::bind(&bind_addr) {
                Ok(l) => l,
                Err(e) => {
                    return Err(format!(
                        "Port {} is already in use ({e}).\n\n\
                         Another instance of COPS may already be running.\n\
                         Please close it and try again.",
                        api::SERVER_PORT
                    ).into());
                }
            };

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
