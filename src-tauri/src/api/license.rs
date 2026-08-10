//! Trial window and licence activation.
//!
//! The trial model is the Python app's, so the administrator has the same
//! controls they already know: reset the window, set its length, or disable it
//! outright to make the installation permanent. State lives in `feature_flags`
//! under three keys — `trial_start_date`, `trial_days`, `trial_disabled`.
//!
//! On top of that sit five activation codes. Four grant three months each; the
//! fifth is permanent and is handed over when the client pays in full.
//!
//! ## Why only hashes appear below
//!
//! The codes themselves are NOT in this file, this binary, or this repository.
//! What is stored is a bcrypt hash of each, at cost 12. Running `strings` on the
//! executable — or handing it to someone, or something, that reads code — yields
//! `$2b$12$…` and nothing else, because bcrypt cannot be run backwards. Each code
//! carries 100 bits of entropy (20 characters of a 32-symbol alphabet), so
//! guessing is not a route either, and cost 12 puts roughly a quarter-second
//! between attempts by construction.
//!
//! What this does NOT defend against, stated plainly so nobody is surprised
//! later: someone able to modify the binary can remove the check rather than
//! defeat it. Protecting the codes and protecting the enforcement are different
//! problems, and the second one has no honest offline answer. For an office
//! installation the first is what matters.
//!
//! The codes were generated once and given to the operator. They cannot be
//! recovered from here — if they are lost, new ones must be generated and this
//! table replaced.

use std::sync::Arc;
use axum::{extract::State, http::StatusCode, Json};
use chrono::{Duration, NaiveDate, Utc};
use serde_json::{json, Value};
use crate::{auth::AdminUser, db::DbPool};

type Db  = State<Arc<DbPool>>;
type Err = (StatusCode, Json<Value>);

fn e400(m: &str) -> Err { (StatusCode::BAD_REQUEST,           Json(json!({ "detail": m }))) }
fn e500(m: &str) -> Err { (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "detail": m }))) }

/// How long one temporary code is worth.
const TEMPORARY_DAYS: i64 = 90;
/// Trial length for a fresh installation, matching the Python app's default.
const DEFAULT_TRIAL_DAYS: i64 = 30;

#[derive(Clone, Copy, PartialEq)]
pub enum CodeKind { Temporary, Permanent }

/// bcrypt hashes of the five activation codes. Order is not meaningful and
/// carries no information about the codes themselves.
const CODE_HASHES: &[(CodeKind, &str)] = &[
    (CodeKind::Temporary, "$2b$12$K3o94ZlLLrCSplthdCAq0eTBfMjpfUlmk3nQbCVHQwtTMXnlDyqrO"),
    (CodeKind::Temporary, "$2b$12$BUdkwYnFQHllGywRnmXCgOpG0VsKcpP8Uv/WXgPG4gInO79Ph0ZEC"),
    (CodeKind::Temporary, "$2b$12$Rkau7BTnZIso5jgJiHwlCuDR0goZQUr8QE8dCdgy28E00e7LfDjxa"),
    (CodeKind::Temporary, "$2b$12$kvl5FZCVNxamhb6pH7XOf.Jhynt6Hp7qUEFcVB8Rr5fLIw.xfifCm"),
    (CodeKind::Permanent, "$2b$12$11CxgX2eWGjbdC6khvygK.zXGub8e04Ni6iR50GjaZM4GCrvCSu3C"),
];

// ── feature_flags helpers ─────────────────────────────────────────────────────

fn flag(conn: &rusqlite::Connection, key: &str) -> Option<String> {
    conn.query_row(
        "SELECT config_value FROM feature_flags WHERE config_key = ?",
        [key], |r| r.get(0),
    ).ok()
}

fn set_flag(conn: &rusqlite::Connection, key: &str, val: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO feature_flags (config_key, config_value) VALUES (?, ?)
           ON CONFLICT(config_key) DO UPDATE SET config_value = excluded.config_value",
        rusqlite::params![key, val],
    )?;
    Ok(())
}

fn truthy(v: Option<String>) -> bool {
    matches!(v.as_deref(), Some("1") | Some("true") | Some("True") | Some("yes"))
}

/// Normalise what the officer typed: case, spacing and the grouping dashes are
/// all cosmetic. Someone reading a code off a slip should not fail because they
/// typed it in lower case or left the dashes out.
fn canonical(input: &str) -> String {
    input.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

/// The stored hashes were made from the dashed, upper-case form, so rebuild it.
fn dashed(canon: &str) -> String {
    canon.as_bytes()
        .chunks(4)
        .map(|c| String::from_utf8_lossy(c).to_string())
        .collect::<Vec<_>>()
        .join("-")
}

/// A code's identity for the "used once" record. This is a SHA-256 of the code,
/// not the code — the used-codes table must not become the leak that the hashed
/// constants above were chosen to avoid.
fn fingerprint(canon: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(b"cops2-license-v1:");     // domain separation
    h.update(canon.as_bytes());
    format!("{:x}", h.finalize())
}

// ── Status ────────────────────────────────────────────────────────────────────

/// Public — the banner needs it before anyone has logged in.
///
/// Shape matches the Python app's `/api/trial-status` so the existing hook and
/// banner work unchanged.
pub async fn trial_status(State(pool): Db) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;

    if truthy(flag(&conn, "trial_disabled")) {
        return Ok(Json(json!({
            "trial_disabled": true,
            "days_remaining": Value::Null,
            "expired":        false,
            "trial_days":     trial_days(&conn),
            "licensed":       true,
            "license_kind":   flag(&conn, "license_kind").unwrap_or_else(|| "permanent".into()),
        })));
    }

    // First run: start the clock. Doing it here rather than at install means a
    // machine that is set up weeks before it is used does not burn its trial
    // sitting in a cupboard.
    let start = match flag(&conn, "trial_start_date") {
        Some(s) if NaiveDate::parse_from_str(&s, "%Y-%m-%d").is_ok() => s,
        _ => {
            let today = Utc::now().date_naive().to_string();
            let _ = set_flag(&conn, "trial_start_date", &today);
            today
        }
    };
    let start_date = NaiveDate::parse_from_str(&start, "%Y-%m-%d")
        .map_err(|_| e500("trial start date is not a date"))?;

    let days = trial_days(&conn);
    let elapsed = (Utc::now().date_naive() - start_date).num_days();
    let remaining = (days - elapsed).max(0);

    Ok(Json(json!({
        "trial_disabled":   false,
        "trial_start_date": start,
        "trial_days":       days,
        "days_elapsed":     elapsed,
        "days_remaining":   remaining,
        "expired":          remaining <= 0,
        "licensed":         flag(&conn, "license_kind").is_some(),
        "license_kind":     flag(&conn, "license_kind"),
    })))
}

fn trial_days(conn: &rusqlite::Connection) -> i64 {
    flag(conn, "trial_days")
        .and_then(|v| v.parse::<i64>().ok())
        .filter(|d| *d > 0)
        .unwrap_or(DEFAULT_TRIAL_DAYS)
}

// ── Activation ────────────────────────────────────────────────────────────────

/// Public — an expired installation cannot log in, so requiring a session here
/// would lock the code entry behind the very thing the code unlocks.
pub async fn activate(State(pool): Db, Json(req): Json<Value>) -> Result<Json<Value>, Err> {
    let raw = req.get("code").and_then(|c| c.as_str()).unwrap_or("");
    let canon = canonical(raw);
    if canon.len() != 20 {
        return Err(e400("That is not a valid activation code."));
    }
    let candidate = dashed(&canon);

    // Every hash is checked even after a match, so the time taken says nothing
    // about which code was entered or whether it was close to a real one.
    let mut matched: Option<CodeKind> = None;
    for (kind, hash) in CODE_HASHES {
        if bcrypt::verify(&candidate, hash).unwrap_or(false) {
            matched = Some(*kind);
        }
    }
    let Some(kind) = matched else {
        return Err(e400("That code was not recognised."));
    };

    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let fp = fingerprint(&canon);

    let already: i64 = conn.query_row(
        "SELECT COUNT(*) FROM license_codes_used WHERE code_fingerprint = ?",
        [&fp], |r| r.get(0),
    ).unwrap_or(0);
    if already > 0 {
        return Err(e400("That code has already been used on this installation."));
    }

    let today = Utc::now().date_naive();
    let (kind_str, expires) = match kind {
        CodeKind::Permanent => {
            // Same switch the administrator's "disable trial" flips. A permanent
            // licence and a permanent installation are the same state.
            set_flag(&conn, "trial_disabled", "1").map_err(|e| e500(&e.to_string()))?;
            set_flag(&conn, "license_kind", "permanent").map_err(|e| e500(&e.to_string()))?;
            ("permanent", None)
        }
        CodeKind::Temporary => {
            // Extend from whatever is left rather than from today, so entering a
            // code early is never a reason to lose the days already paid for.
            let remaining = current_remaining(&conn);
            let total = remaining.max(0) + TEMPORARY_DAYS;
            set_flag(&conn, "trial_start_date", &today.to_string())
                .map_err(|e| e500(&e.to_string()))?;
            set_flag(&conn, "trial_days", &total.to_string())
                .map_err(|e| e500(&e.to_string()))?;
            set_flag(&conn, "trial_disabled", "0").map_err(|e| e500(&e.to_string()))?;
            set_flag(&conn, "license_kind", "temporary").map_err(|e| e500(&e.to_string()))?;
            ("temporary", Some((today + Duration::days(total)).to_string()))
        }
    };

    conn.execute(
        "INSERT INTO license_codes_used (code_fingerprint, kind, used_at) VALUES (?, ?, ?)",
        rusqlite::params![fp, kind_str, Utc::now().to_rfc3339()],
    ).map_err(|e| e500(&e.to_string()))?;

    tracing::info!("licence activated: {kind_str}");
    Ok(Json(json!({
        "ok": true,
        "kind": kind_str,
        "expires_on": expires,
        "message": match kind {
            CodeKind::Permanent => "Activated permanently. This installation will not expire.",
            CodeKind::Temporary => "Activated for three months.",
        },
    })))
}

fn current_remaining(conn: &rusqlite::Connection) -> i64 {
    let Some(start) = flag(conn, "trial_start_date") else { return 0 };
    let Ok(start) = NaiveDate::parse_from_str(&start, "%Y-%m-%d") else { return 0 };
    let elapsed = (Utc::now().date_naive() - start).num_days();
    (trial_days(conn) - elapsed).max(0)
}

// ── Administrator controls (the Python app's, unchanged) ──────────────────────

pub async fn trial_reset(State(pool): Db, _admin: AdminUser) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let today = Utc::now().date_naive().to_string();
    set_flag(&conn, "trial_start_date", &today).map_err(|e| e500(&e.to_string()))?;
    set_flag(&conn, "trial_disabled", "0").map_err(|e| e500(&e.to_string()))?;
    Ok(Json(json!({
        "trial_start_date": today,
        "trial_disabled":   false,
        "trial_days":       trial_days(&conn),
    })))
}

pub async fn trial_disable(State(pool): Db, _admin: AdminUser) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    set_flag(&conn, "trial_disabled", "1").map_err(|e| e500(&e.to_string()))?;
    Ok(Json(json!({ "trial_disabled": true })))
}

pub async fn trial_set_days(
    State(pool): Db, _admin: AdminUser, Json(req): Json<Value>,
) -> Result<Json<Value>, Err> {
    let days = req.get("trial_days").and_then(|d| d.as_i64())
        .ok_or_else(|| e400("trial_days is required"))?;
    if !(1..=3650).contains(&days) {
        return Err(e400("trial_days must be between 1 and 3650"));
    }
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    set_flag(&conn, "trial_days", &days.to_string()).map_err(|e| e500(&e.to_string()))?;
    Ok(Json(json!({ "trial_days": days })))
}
