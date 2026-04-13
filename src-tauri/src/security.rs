//! Database & backup encryption — mirrors cops1's `security/device.py` design.
//!
//! The binding secret is XOR-obfuscated so the plaintext never appears in
//! the compiled binary (`strings`, hex dumps, or Ghidra all see garbage).
//! A CI script (`bake_binding_secret.py`) replaces `_ES` at build time with
//! the XOR-encoded production secret — the decode key `_XK` is never the
//! password itself.
//!
//! Key derivation
//! ──────────────
//!   binding_secret()          XOR-decode → plaintext (= ZIP password)
//!   db_key()                  SHA-256(binding_secret + fixed_salt) → 64-char hex
//!   sqlcipher_pragma()        PRAGMA key = "x'<64-char-hex>'"  (raw key, zero KDF)
//!
//! Design rationale (same as cops1)
//! ──────────────────────────────────
//! • The DB key is constant for all builds from the same source.
//! • Deliberately NOT machine-specific — copy cops.db to any new machine and
//!   it opens immediately (disaster recovery without a key file).
//! • An attacker who only has the .db file cannot read it; they also need the
//!   binary to reverse the XOR and reproduce the SHA-256 derivation.
//! • The developer always has the source → always has the secret → can always
//!   open any database.
//!
//! Recovery (DB Browser for SQLite + SQLCipher extension)
//! ──────────────────────────────────────────────────────
//!   1. Run `cargo run --bin print-db-key`  (or check /admin/backup/db-cipher-key)
//!   2. Open cops.db → Raw key / Hex key → paste the 64-char hex → OK

use anyhow::{Context, Result};
use once_cell::sync::OnceCell;
use sha2::{Digest, Sha256};
use std::path::Path;

// ── XOR-obfuscated binding secret ─────────────────────────────────────────────
// `bake_binding_secret.py` replaces `_ES` at CI time with XOR(_BINDING_SECRET, _XK).
// The real secret is NEVER stored as plain bytes in this file.
const _XK: &[u8] = b"\xde\xad\xbe\xef\xca\xfe\xba\xbe\xde\xad\xbe\xef\xca\xfe";
const _ES: &[u8] = &[0x9d, 0xc2, 0xce, 0x9c, 0x8a, 0xcc, 0x8a, 0x8c, 0xe8, 0x8e]; // BAKE_TARGET

fn _xdec(enc: &[u8], key: &[u8]) -> Vec<u8> {
    enc.iter().enumerate().map(|(i, &e)| e ^ key[i % key.len()]).collect()
}

// ── Cached decoded values ─────────────────────────────────────────────────────

static BINDING_SECRET: OnceCell<String> = OnceCell::new();
static DB_KEY:         OnceCell<String> = OnceCell::new();

/// The decoded binding secret — also used as the ZIP backup password.
/// Users type this in 7-Zip / WinRAR to open an exported backup.
pub fn zip_password() -> &'static str {
    BINDING_SECRET.get_or_init(|| {
        String::from_utf8(_xdec(_ES, _XK))
            .expect("binding secret must be valid UTF-8")
    })
}

/// The 64-char hex AES-256 database key.
///
/// Derived as SHA-256(binding_secret ‖ fixed_salt) so that:
///   • The DB key is different from the ZIP password.
///   • Knowing the ZIP password alone is not enough to open the database.
fn db_key() -> &'static str {
    DB_KEY.get_or_init(|| {
        const SALT: &[u8] = b"cops-db-cipher-v2-2026-chennai-customs";
        let mut h = Sha256::new();
        h.update(zip_password().as_bytes());
        h.update(SALT);
        h.finalize().iter().map(|b| format!("{b:02x}")).collect()
    })
}

/// The PRAGMA key statement for SQLCipher (raw-key format).
///
/// `x'<64 hex chars>'` tells SQLCipher to use the bytes directly as the AES-256
/// key, skipping PBKDF2 entirely → connection open overhead is <0.1ms.
pub fn sqlcipher_pragma() -> String {
    format!("PRAGMA key = \"x'{}'\";", db_key())
}

// ── Admin helper ──────────────────────────────────────────────────────────────

/// Returns the derived 64-char hex DB key for admin display / disaster recovery.
pub fn get_db_key_hex() -> &'static str {
    db_key()
}

// ── cops1 (OS_module_upgrade) compatibility ───────────────────────────────────

/// Derive the SQLCipher PRAGMA for cops1's (OS_module_upgrade) encrypted database.
///
/// cops1 uses PBKDF2-HMAC-SHA256 with 100,000 iterations and a v1 salt — a
/// completely different derivation from cops2's single-pass SHA-256 with v2 salt.
/// Same binding secret, different output key.
///
/// This function is called exactly ONCE: during first-boot migration when COPS2
/// finds an existing encrypted cops1 database.  After migration the result is
/// never used again — cops2 re-encrypts everything with its own key.
pub fn cops1_sqlcipher_pragma() -> String {
    use pbkdf2::pbkdf2_hmac;
    use sha2::Sha256;

    const SALT: &[u8] = b"cops-db-cipher-v1-2024-chennai-customs";
    let mut key = [0u8; 32]; // 256-bit output
    pbkdf2_hmac::<Sha256>(zip_password().as_bytes(), SALT, 100_000, &mut key);
    let hex: String = key.iter().map(|b| format!("{b:02x}")).collect();
    format!("PRAGMA key = \"x'{}'\";", hex)
}

// ── Plain-SQLite detection ─────────────────────────────────────────────────────

/// Returns true if the file at `path` is a plain (unencrypted) SQLite database.
///
/// Plain SQLite files always start with the 16-byte magic string
/// `"SQLite format 3\0"`. SQLCipher-encrypted files start with random bytes.
pub fn is_plain_sqlite(path: &Path) -> bool {
    use std::io::Read;
    if let Ok(mut f) = std::fs::File::open(path) {
        let mut buf = [0u8; 16];
        if f.read_exact(&mut buf).is_ok() {
            return &buf == b"SQLite format 3\0";
        }
    }
    false
}

/// Encrypt an existing plain-SQLite database in-place using `sqlcipher_export`.
///
/// Opens the plain DB without a key, ATTACHes a new encrypted copy using raw-key
/// format, exports all data, then atomically replaces the original file.
///
/// This is safe: if anything fails the original plain file is untouched.
pub fn encrypt_plain_db_inplace(db_path: &Path) -> Result<()> {
    let key = db_key();
    let enc_path = db_path.with_extension("db.encrypting");

    if enc_path.exists() { std::fs::remove_file(&enc_path).ok(); }

    tracing::info!("Encrypting existing plain-SQLite database in-place…");

    let conn = rusqlite::Connection::open(db_path)
        .context("Cannot open plain DB for in-place encryption")?;

    conn.execute_batch(&format!(
        "ATTACH DATABASE '{enc}' AS encrypted KEY \"x'{key}'\";
         SELECT sqlcipher_export('encrypted');
         DETACH DATABASE encrypted;",
        enc = enc_path.display(),
        key = key,
    )).context("sqlcipher_export failed")?;

    drop(conn);

    std::fs::rename(&enc_path, db_path)
        .context("Cannot replace original DB with encrypted copy")?;

    tracing::info!("Database encrypted in-place successfully.");
    Ok(())
}
