/// Build script for COPS2.
///
/// ## Admin password baking
///
/// At build time, if the `ADMIN_PASSWORD` environment variable is set (e.g. by
/// GitHub Actions from a repository secret), this script:
///   1. Hashes it with bcrypt (cost 12)
///   2. Passes the hash to the compiler via `ADMIN_PWD_HASH_BAKED` env var
///
/// The auth module (`auth/mod.rs`) reads this at *compile time* via
/// `option_env!("ADMIN_PWD_HASH_BAKED")`.  At runtime on the user's machine,
/// no environment variable is needed — the hash is baked into the binary.
///
/// This mirrors the cops1 approach (`bake_hash.py` + PyInstaller) exactly.
///
/// For local development, set `ADMIN_PASSWORD=yourpass` before `cargo build`
/// or `npm run tauri dev`.
fn main() {
    // ── Bake admin password hash ──────────────────────────────────────────────
    // Only runs when ADMIN_PASSWORD is set (CI builds or dev with env var).
    // In dev without it, admin login is simply disabled — same as cops1.
    if let Ok(password) = std::env::var("ADMIN_PASSWORD") {
        if !password.is_empty() {
            let hash = bcrypt::hash(&password, 12)
                .expect("build.rs: failed to hash ADMIN_PASSWORD with bcrypt");
            println!("cargo:rustc-env=ADMIN_PWD_HASH_BAKED={hash}");
            println!("cargo:warning=Admin password hash baked into binary.");
        }
    }
    // Re-run this script if ADMIN_PASSWORD changes
    println!("cargo:rerun-if-env-changed=ADMIN_PASSWORD");

    tauri_build::build()
}
