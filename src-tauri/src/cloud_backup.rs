//! The monthly copy that leaves the building, without anyone carrying it.
//!
//! Every automatic backup this program takes lands somewhere on the office
//! network. That covers a failed disk. It does not cover a fire, a theft, or
//! ransomware, which take every copy in the building at once — and the answer to
//! that was an officer remembering to plug in a pen drive once a month, which is
//! a plan that works until the month somebody is busy.
//!
//! So the archive goes to the office's own WorkDrive on a schedule, and the
//! reminder to carry one out by hand appears only when that has stopped working.
//!
//! ── Where the credentials live ───────────────────────────────────────────────
//!
//! Not in the database, and not in any file this program writes.
//!
//! The archive's own password is derived from a secret compiled into the binary,
//! which means anybody holding the installer can recover it. That is acceptable
//! for a file in a locked drawer. It is worthless for protecting a credential
//! that can reach the office's cloud storage, because the attacker who wants the
//! credential has the installer too.
//!
//! The refresh token therefore goes to the operating system's own credential
//! store — Windows Credential Manager, the login keyring on Linux — where it is
//! held against the signed-in account and cannot be read by copying the database
//! away. If the store is unavailable the token is not written anywhere else:
//! the feature reports itself as unconfigured rather than falling back to
//! something weaker without saying so.

use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use std::path::Path;

use crate::db::DbPool;

const KEYRING_SERVICE: &str = "cops2-cloud-backup";
const KEYRING_USER: &str = "zoho-refresh-token";

/// How often the copy goes out, unless the office says otherwise.
const DEFAULT_EVERY_DAYS: i64 = 30;

/// After this long without a successful upload, the officer is asked to carry a
/// copy out by hand. Longer than the interval, so one failed attempt on a bad
/// line does not start nagging immediately.
const OVERDUE_AFTER_DAYS: i64 = 45;

// ── Settings ─────────────────────────────────────────────────────────────────
//
// Everything here is ordinary configuration and lives in the database. The one
// secret does not.

fn setting(pool: &DbPool, key: &str) -> Option<String> {
    let conn = pool.get().ok()?;
    conn.query_row("SELECT value FROM app_settings WHERE key = ?1", [key], |r| {
        r.get::<_, String>(0)
    })
    .ok()
    .map(|s| s.trim().to_string())
    .filter(|s| !s.is_empty())
}

fn put_setting(pool: &DbPool, key: &str, value: &str) -> Result<()> {
    let conn = pool.get()?;
    conn.execute(
        "INSERT INTO app_settings(key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![key, value],
    )?;
    Ok(())
}

/// Where the office's WorkDrive actually lives.
///
/// Not a data-centre letter. The department's account is on the government cloud
/// — workplace.mgovcloud.in — which is a separate deployment from public Zoho
/// with its own sign-in and its own API host. A token issued by one is refused
/// by the other, with an error that does not say so plainly.
///
/// Both hosts are therefore settings rather than something worked out from a
/// suffix, so an office on the government cloud, on zoho.in, or on whatever
/// replaces either of them is a matter of typing two addresses.
fn accounts_base(pool: &DbPool) -> String {
    setting(pool, "cloud_backup_accounts_base")
        .unwrap_or_else(|| "https://accounts.mgovcloud.in".into())
        .trim_end_matches('/')
        .to_string()
}

/// The API root, up to and including the version — because the two deployments
/// do not agree on the path either:
///
/// ```text
/// government cloud   https://workdrive.mgovcloud.in/api/v1
/// public Zoho        https://www.zohoapis.in/workdrive/api/v1
/// ```
///
/// Confirmed against the live host: an unauthenticated call to
/// `…/api/v1/users/me` on the government cloud answers with WorkDrive's own
/// INVALID_TICKET, which is the API saying "no token", not a wrong address.
fn api_base(pool: &DbPool) -> String {
    setting(pool, "cloud_backup_api_base")
        .unwrap_or_else(|| "https://workdrive.mgovcloud.in/api/v1".into())
        .trim_end_matches('/')
        .to_string()
}

pub fn is_enabled(pool: &DbPool) -> bool {
    setting(pool, "cloud_backup_enabled").as_deref() == Some("true")
}

fn every_days(pool: &DbPool) -> i64 {
    setting(pool, "cloud_backup_every_days")
        .and_then(|s| s.parse::<i64>().ok())
        .map(|d| d.clamp(1, 365))
        .unwrap_or(DEFAULT_EVERY_DAYS)
}

// ── The credential ───────────────────────────────────────────────────────────

fn entry() -> Result<keyring::Entry> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
        .map_err(|e| anyhow!("the credential store is not available: {e}"))
}

/// Hand the refresh token to the operating system. Nothing else keeps a copy.
pub fn store_refresh_token(token: &str) -> Result<()> {
    let token = token.trim();
    if token.is_empty() {
        return Err(anyhow!("no refresh token was given"));
    }
    entry()?
        .set_password(token)
        .map_err(|e| anyhow!("the credential store refused the token: {e}"))
}

pub fn forget_refresh_token() -> Result<()> {
    match entry()?.delete_credential() {
        Ok(()) => Ok(()),
        // Already gone is the state we wanted.
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(anyhow!("could not remove the token: {e}")),
    }
}

fn refresh_token() -> Option<String> {
    entry().ok()?.get_password().ok().filter(|t| !t.trim().is_empty())
}

pub fn has_credentials(pool: &DbPool) -> bool {
    refresh_token().is_some()
        && setting(pool, "cloud_backup_client_id").is_some()
        && setting(pool, "cloud_backup_client_secret").is_some()
        && setting(pool, "cloud_backup_folder_id").is_some()
}

// ── Talking to WorkDrive ─────────────────────────────────────────────────────

/// Trade the long-lived refresh token for an access token good for an hour.
///
/// The refresh token never leaves the credential store except to make this one
/// call, and the access token is never written down at all.
async fn access_token(pool: &DbPool) -> Result<String> {
    let token = refresh_token().ok_or_else(|| anyhow!("no refresh token is stored"))?;
    let client_id = setting(pool, "cloud_backup_client_id")
        .ok_or_else(|| anyhow!("the client id has not been set"))?;
    let client_secret = setting(pool, "cloud_backup_client_secret")
        .ok_or_else(|| anyhow!("the client secret has not been set"))?;

    // Built by hand rather than through a query helper, so the exact bytes sent
    // are the ones written here: a secret mangled by an encoder that treats some
    // character specially fails with "invalid client", which says nothing useful.
    let enc = |v: &str| -> String {
        v.bytes()
            .map(|b| match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' =>
                    (b as char).to_string(),
                _ => format!("%{b:02X}"),
            })
            .collect()
    };
    let url = format!(
        "{}/oauth/v2/token\
         ?refresh_token={}&client_id={}&client_secret={}&grant_type=refresh_token",
        accounts_base(pool), enc(&token), enc(&client_id), enc(&client_secret),
    );
    let res = reqwest::Client::new()
        .post(&url)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .context("could not reach Zoho to refresh the token")?;

    let status = res.status();
    let body: serde_json::Value = res.json().await.unwrap_or(serde_json::Value::Null);
    if let Some(t) = body.get("access_token").and_then(|v| v.as_str()) {
        return Ok(t.to_string());
    }
    // Zoho answers 200 with an "error" field for a revoked token, so the status
    // alone does not tell us whether this worked.
    let detail = body
        .get("error")
        .and_then(|v| v.as_str())
        .unwrap_or("no access token was returned");
    Err(anyhow!("Zoho refused the refresh token ({status}): {detail}"))
}

/// Put one file into the office's WorkDrive folder.
async fn upload(pool: &DbPool, path: &Path, filename: &str) -> Result<Uploaded> {
    let token = access_token(pool).await?;
    let folder = setting(pool, "cloud_backup_folder_id")
        .ok_or_else(|| anyhow!("the WorkDrive folder has not been set"))?;

    let bytes = tokio::fs::read(path)
        .await
        .with_context(|| format!("could not read the archive at {}", path.display()))?;
    let size = bytes.len();

    let part = reqwest::multipart::Part::bytes(bytes)
        .file_name(filename.to_string())
        .mime_str("application/octet-stream")?;
    let form = reqwest::multipart::Form::new()
        .text("parent_id", folder)
        .text("override-name-exist", "true")
        .part("content", part);

    let url = format!("{}/upload", api_base(pool));
    let res = reqwest::Client::new()
        .post(&url)
        .header("Authorization", format!("Zoho-oauthtoken {token}"))
        .multipart(form)
        // A 45 MB archive over an office line takes as long as it takes; the
        // schedule can wait, and a half-finished upload helps nobody.
        .timeout(std::time::Duration::from_secs(30 * 60))
        .send()
        .await
        .context("the upload could not be sent")?;

    let status = res.status();
    let text = res.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(anyhow!("WorkDrive refused the file ({status}): {}",
                           text.chars().take(300).collect::<String>()));
    }

    // The id of what was just stored, so last month's copy can be removed once
    // this one is safely up — and only then.
    let file_id = serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|v| find_resource_id(&v));

    tracing::info!("cloud backup: {filename} ({size} bytes) uploaded to WorkDrive");
    Ok(Uploaded { bytes: size as u64, file_id })
}

/// What was uploaded, and where it landed.
pub struct Uploaded {
    pub bytes: u64,
    pub file_id: Option<String>,
}

/// WorkDrive answers with the file nested inside `data`, and the shape has
/// changed between versions of the API. Rather than depend on one of them, the
/// id is looked for wherever it is: a wrong guess here would leave last month's
/// copy undeletable and the folder growing quietly.
fn find_resource_id(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::Object(map) => {
            for key in ["resource_id", "id"] {
                if let Some(s) = map.get(key).and_then(|x| x.as_str()) {
                    if !s.is_empty() { return Some(s.to_string()); }
                }
            }
            map.values().find_map(find_resource_id)
        }
        serde_json::Value::Array(items) => items.iter().find_map(find_resource_id),
        _ => None,
    }
}

/// Move a file to the WorkDrive trash.
///
/// Only ever called once the replacement is already stored. If it fails the
/// office is left with two copies, which costs a little space and loses nothing
/// — the opposite order would risk a month with no copy at all.
async fn remove(pool: &DbPool, file_id: &str) -> Result<()> {
    let token = access_token(pool).await?;
    let url = format!("{}/files/{}", api_base(pool), file_id);
    let res = reqwest::Client::new()
        .delete(&url)
        .header("Authorization", format!("Zoho-oauthtoken {token}"))
        .timeout(std::time::Duration::from_secs(60))
        .send()
        .await
        .context("could not reach WorkDrive to remove the previous copy")?;
    if !res.status().is_success() {
        return Err(anyhow!("WorkDrive would not remove the previous copy ({})", res.status()));
    }
    Ok(())
}

// ── The scheduled copy ───────────────────────────────────────────────────────

#[derive(Serialize, Default, Clone)]
pub struct CloudStatus {
    pub enabled: bool,
    pub configured: bool,
    /// Whether the credential store itself is usable on this machine.
    pub credential_store: bool,
    pub every_days: i64,
    pub last_success: Option<String>,
    pub last_attempt: Option<String>,
    pub last_error: Option<String>,
    pub days_since_success: Option<i64>,
    /// True when the office should be asked to carry a copy out by hand.
    pub needs_manual_backup: bool,
}

pub fn status(pool: &DbPool) -> CloudStatus {
    let last_success = setting(pool, "cloud_backup_last_success");
    let days = last_success.as_deref().and_then(|s| {
        chrono::DateTime::parse_from_rfc3339(s)
            .ok()
            .map(|t| (chrono::Local::now() - t.with_timezone(&chrono::Local)).num_days())
    });
    let enabled = is_enabled(pool);
    let configured = has_credentials(pool);

    // The hand-carried copy is asked for when the automatic one is not working:
    // switched off, never set up, or silent for longer than it should be. That
    // is the whole point of the arrangement — the officer is troubled only when
    // the thing that was meant to spare them has stopped.
    let needs_manual_backup = match (enabled && configured, days) {
        (false, _) => true,
        (true, None) => true,
        (true, Some(d)) => d >= OVERDUE_AFTER_DAYS,
    };

    CloudStatus {
        enabled,
        configured,
        credential_store: entry().is_ok(),
        every_days: every_days(pool),
        last_success,
        last_attempt: setting(pool, "cloud_backup_last_attempt"),
        last_error: setting(pool, "cloud_backup_last_error"),
        days_since_success: days,
        needs_manual_backup,
    }
}

/// Build the archive and send it. Records what happened either way.
pub async fn run_once(pool: &DbPool) -> Result<String> {
    let now = chrono::Local::now().to_rfc3339();
    let _ = put_setting(pool, "cloud_backup_last_attempt", &now);

    let tmp = std::env::temp_dir()
        .join(format!("cops_cloud_{}.cops", uuid::Uuid::new_v4()));
    let p = pool.clone();
    let t = tmp.clone();
    let built = tokio::task::spawn_blocking(move || crate::backup_export::write_archive(&p, &t))
        .await
        .map_err(|e| anyhow!("{e}"))?;

    let result = match built {
        Err(e) => Err(anyhow!("the archive could not be built: {e}")),
        Ok(_report) => {
            let name = format!(
                "cops_backup_{}_{}.cops",
                crate::backup_service::machine_name(),
                chrono::Local::now().format("%Y-%m-%d")
            );
            upload(pool, &tmp, &name).await
        }
    };

    // The temp file goes whatever happened; the periodic sweep would take it
    // eventually, but there is no reason to leave 45 MB lying about.
    let _ = std::fs::remove_file(&tmp);

    match result {
        Ok(up) => {
            let _ = put_setting(pool, "cloud_backup_last_success", &now);
            let _ = put_setting(pool, "cloud_backup_last_error", "");

            // Last month's copy goes only now, with this month's already stored.
            //
            // One copy is kept on purpose: the folder must not grow by 45 MB a
            // month for years. But the order matters more than the count — the
            // old copy is the only one that exists until the new one is up, so
            // it is removed after, never before, and a failure to remove it is
            // written down rather than treated as a failed backup.
            let previous = setting(pool, "cloud_backup_last_file_id");
            if let Some(id) = up.file_id.as_deref() {
                let _ = put_setting(pool, "cloud_backup_last_file_id", id);
            }
            if let Some(old_id) = previous {
                if Some(old_id.as_str()) != up.file_id.as_deref() {
                    match remove(pool, &old_id).await {
                        Ok(()) => tracing::info!("cloud backup: previous copy removed"),
                        Err(e) => {
                            tracing::warn!("cloud backup: kept the previous copy — {e}");
                            let _ = put_setting(pool, "cloud_backup_last_error",
                                &format!("The new copy is stored. The previous one could \
                                          not be removed and is still there: {e}"));
                        }
                    }
                }
            }
            Ok(up.bytes.to_string())
        }
        Err(e) => {
            let msg = e.to_string();
            let _ = put_setting(pool, "cloud_backup_last_error", &msg);
            tracing::warn!("cloud backup failed: {msg}");
            Err(e)
        }
    }
}

/// Whether enough days have passed since the last successful copy.
fn due(pool: &DbPool) -> bool {
    if !is_enabled(pool) || !has_credentials(pool) {
        return false;
    }
    match setting(pool, "cloud_backup_last_success") {
        None => true,
        Some(s) => chrono::DateTime::parse_from_rfc3339(&s)
            .map(|t| {
                (chrono::Local::now() - t.with_timezone(&chrono::Local)).num_days()
                    >= every_days(pool)
            })
            .unwrap_or(true),
    }
}

/// Check every few hours whether the copy is due.
///
/// Deliberately not a timer that fires on the first of the month: an office
/// machine is not switched on at midnight, and a schedule that can be missed
/// entirely by being asleep is not a schedule. It asks "has it been thirty days"
/// instead, so a machine that was off simply sends it when it is next on.
pub fn spawn(pool: std::sync::Arc<DbPool>) {
    std::thread::spawn(move || {
        // Let the app finish starting before touching the network.
        std::thread::sleep(std::time::Duration::from_secs(120));
        let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
            Ok(rt) => rt,
            Err(e) => {
                tracing::error!("cloud backup: no runtime ({e}); the copy will not be sent");
                return;
            }
        };
        loop {
            if due(&pool) {
                if let Err(e) = rt.block_on(run_once(&pool)) {
                    tracing::warn!("cloud backup: {e}");
                }
            }
            std::thread::sleep(std::time::Duration::from_secs(4 * 60 * 60));
        }
    });
}
