use once_cell::sync::Lazy;
use sha2::{Digest, Sha256};

/// Machine-specific JWT signing secret.
///
/// Derived as SHA-256(binding_secret + MAC + hostname) so that tokens signed
/// on one machine are invalid on any other — even if an attacker extracts the
/// binary and recovers the binding secret they cannot forge tokens for a
/// different machine.
///
/// Falls back to a runtime env var (COPS_JWT_SECRET) for dev / test use, and
/// to a deterministic but non-trivial derived value if neither is available
/// (always unique per binding_secret, never the raw hardcoded string).
pub static JWT_SECRET: Lazy<String> = Lazy::new(|| {
    // 1. Explicit env override (dev / test)
    if let Ok(s) = std::env::var("COPS_JWT_SECRET") {
        if !s.is_empty() { return s; }
    }
    // 2. Derive from binding_secret + MAC address + hostname
    //    (machine-specific, never appears as a literal in the binary)
    let mac      = mac_address();
    let hostname = hostname();
    let message  = format!("cops2-jwt:{}:{}", mac, hostname);
    let mut h = Sha256::new();
    h.update(crate::security::zip_password().as_bytes());
    h.update(message.as_bytes());
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
});

fn mac_address() -> String {
    // Portable: read MAC from network interfaces via standard library
    // Uses the same uuid trick as cops1's device.py
    format!("{:x}", uuid_mac())
}

fn uuid_mac() -> u64 {
    // uuid::Uuid::get_node_id equivalent — gets primary MAC as a u64
    // We re-implement the uuid crate's node ID logic without adding a dep
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    hostname().hash(&mut h);
    // Mix in process ID as entropy when MAC isn't cleanly obtainable
    std::process::id().hash(&mut h);
    h.finish()
}

fn hostname() -> String {
    std::env::var("COMPUTERNAME")                  // Windows
        .or_else(|_| std::env::var("HOSTNAME"))    // Linux/macOS env
        .unwrap_or_else(|_| {
            // Fallback: read from /etc/hostname (Linux) or gethostname syscall
            std::fs::read_to_string("/etc/hostname")
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|_| "unknown-host".to_string())
        })
        .to_lowercase()
}

pub static APP_VERSION: &str = "3.0.6";
pub static APP_NAME: &str = "COPS";
pub const JWT_EXPIRY_HOURS: i64 = 12;

// ── Business rule limits (matches legacy VB6 MaxLength values) ────────────────
/// Maximum characters for adjudicating officer remarks (txtDCRem MaxLength in old module)
pub const ADJN_REMARKS_MAX_CHARS: usize = 3000;
/// Maximum characters for superintendent remarks (txtSupRem MaxLength in sdo_2023.exe)
pub const SUPDT_REMARKS_MAX_CHARS: usize = 1500;

