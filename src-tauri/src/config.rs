use once_cell::sync::Lazy;

pub static JWT_SECRET: Lazy<String> = Lazy::new(|| {
    std::env::var("COPS_JWT_SECRET")
        .unwrap_or_else(|_| "cops-super-secret-jwt-key-change-in-production".to_string())
});

pub static APP_VERSION: &str = "3.0.5";
pub static APP_NAME: &str = "COPS";
pub const JWT_EXPIRY_HOURS: i64 = 12;

// ── Business rule limits (matches legacy VB6 MaxLength values) ────────────────
/// Maximum characters for adjudicating officer remarks (txtDCRem MaxLength in old module)
pub const ADJN_REMARKS_MAX_CHARS: usize = 3000;
/// Maximum characters for superintendent remarks (txtSupRem MaxLength in sdo_2023.exe)
pub const SUPDT_REMARKS_MAX_CHARS: usize = 1500;

