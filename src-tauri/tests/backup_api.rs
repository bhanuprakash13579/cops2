//! The automatic-backup endpoints, exercised over real HTTP.
//!
//! These go through the actual axum router on a real TCP socket rather than
//! calling the handlers directly, because most of what can be wrong here is not
//! in the handler bodies: a route registered at the wrong path, an auth guard on
//! the wrong side of it, a response the browser cannot use. A test that calls
//! the function proves none of that.

use std::net::SocketAddr;
use std::sync::Arc;

use cops2_lib::api;
use cops2_lib::auth;
use cops2_lib::db;

/// Start the real router on an ephemeral port; returns its base URL.
async fn serve() -> (String, tempfile::TempDir) { serve_with(200).await }

/// A database with almost nothing in it, for testing the shrink guard.
async fn serve_tiny() -> (String, tempfile::TempDir) { serve_with(1).await }

async fn serve_with(rows: i64) -> (String, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let pool = db::create_pool(&dir.path().join("live.db")).unwrap();
    // The REAL schema, not three hand-written tables. Hand-written fixtures
    // drift from the migrations and then tests pass against a database shape
    // that does not exist in the office.
    db::run_migrations(&pool).unwrap();
    {
        let c = pool.get().unwrap();
        // app_settings only — everything else comes from the migrations above.
        c.execute_batch(
            "CREATE TABLE IF NOT EXISTS app_settings(key TEXT PRIMARY KEY, value TEXT);"
        )
        .unwrap();
        // Migrations seed a starter tariff dated today. Tests that care about
        // point-in-time selection supply their own rows, so clear it here.
        let _ = c.execute("DELETE FROM dcr_tariffs", []);
        for i in 1..=rows {
            c.execute(
                "INSERT INTO cops_master(os_no, os_date, os_year) VALUES (?1, ?2, ?3)",
                rusqlite::params![format!("OS/{i}/2026"), "2026-08-09", 2026],
            )
            .unwrap();
        }
        c.execute(
            "INSERT INTO print_template_config(field_key, field_value, effective_from)
             VALUES ('os_heading', 'CHENNAI', '2020-01-01')",
            [],
        )
        .unwrap();
    }
    let app = api::build_app(Arc::new(pool));
    let listener = tokio::net::TcpListener::bind::<SocketAddr>("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("http://{addr}{}", api::API_PREFIX), dir)
}

fn officer_token() -> String {
    auth::create_token("1", "SDO", "Test Officer", Some("Supdt"), "ACTIVE").unwrap()
}

#[tokio::test]
async fn an_officer_can_read_status_and_an_anonymous_caller_cannot() {
    let (base, _d) = serve().await;
    let c = reqwest::Client::new();

    // 401, not 404 — proving the route exists AND is guarded. A missing route
    // also refuses the request, which is why the distinction is the assertion.
    let anon = c.get(format!("{base}/backup/auto/status")).send().await.unwrap();
    assert_eq!(anon.status(), 401, "status must require a signed-in officer");

    let ok = c
        .get(format!("{base}/backup/auto/status"))
        .bearer_auth(officer_token())
        .send()
        .await
        .unwrap();
    assert_eq!(ok.status(), 200);
    let body: serde_json::Value = ok.json().await.unwrap();
    assert!(body.get("destinations").is_some(), "body was {body}");
    assert!(body.get("any_off_machine").is_some());
    assert!(body.get("interval_minutes").is_some());
}

#[tokio::test]
async fn changing_where_backups_go_needs_the_administrator() {
    let (base, _d) = serve().await;
    let c = reqwest::Client::new();
    let r = c
        .post(format!("{base}/admin/backup/auto/settings"))
        .bearer_auth(officer_token())
        .json(&serde_json::json!({ "dirs": "/tmp/anywhere" }))
        .send()
        .await
        .unwrap();
    assert!(
        r.status() == 401 || r.status() == 403,
        "an ordinary officer must not be able to redirect the backups, got {}",
        r.status()
    );
}

#[tokio::test]
async fn any_officer_can_take_the_archive_and_it_is_a_real_encrypted_zip() {
    // Deliberately not admin-only. This is the copy that leaves the building,
    // and requiring the administrator to be present to take one is how a month
    // goes by without anybody taking one.
    let (base, _d) = serve().await;
    let c = reqwest::Client::new();

    let r = c
        .get(format!("{base}/backup/archive/download"))
        .bearer_auth(officer_token())
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200, "an officer must be able to take an archive");

    let disp = r
        .headers()
        .get("content-disposition")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    assert!(disp.contains("attachment"), "must download, not render: {disp}");
    assert!(disp.contains(".cops"), "wrong extension: {disp}");

    let bytes = r.bytes().await.unwrap();
    assert!(bytes.len() > 100, "archive is empty");
    assert_eq!(&bytes[..2], b"PK", "not a zip");
    // The case data must not be readable in the downloaded bytes.
    assert!(
        !bytes.windows(11).any(|w| w == b"OS/1/2026\0\0".get(..11).unwrap_or(w)),
        "unencrypted case data in the archive"
    );
    assert!(
        !bytes.windows(7).any(|w| w == b"CHENNAI"),
        "unencrypted template text in the archive"
    );
}

#[tokio::test]
async fn taking_an_archive_clears_the_reminder() {
    let (base, _d) = serve().await;
    let c = reqwest::Client::new();

    let before: serde_json::Value = c
        .get(format!("{base}/backup/archive/status"))
        .bearer_auth(officer_token())
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(before["state"], "never", "a fresh install has never archived");

    c.get(format!("{base}/backup/archive/download"))
        .bearer_auth(officer_token())
        .send()
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();

    let after: serde_json::Value = c
        .get(format!("{base}/backup/archive/status"))
        .bearer_auth(officer_token())
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(after["state"], "ok", "the reminder must clear: {after}");
}

// ── Restore ──────────────────────────────────────────────────────────────────
// Restore is the most destructive operation in the program: it overwrites every
// case record the office has. These test the guards, not the happy path.

async fn take_archive(base: &str) -> Vec<u8> {
    reqwest::Client::new()
        .get(format!("{base}/backup/archive/download"))
        .bearer_auth(officer_token())
        .send().await.unwrap()
        .bytes().await.unwrap().to_vec()
}

fn admin_token() -> String {
    // The real admin token, not a hand-rolled claim set. AdminUser requires
    // role == "system_admin"; inventing a plausible-looking role instead is how
    // a test ends up asserting against a 403 it caused itself.
    auth::create_admin_token().unwrap()
}

async fn restore(base: &str, bytes: Vec<u8>, confirm: bool) -> (u16, serde_json::Value) {
    let mut form = reqwest::multipart::Form::new().part(
        "file",
        reqwest::multipart::Part::bytes(bytes).file_name("b.cops"),
    );
    if confirm {
        form = form.text("confirm_data_loss", "yes");
    }
    let r = reqwest::Client::new()
        .post(format!("{base}/admin/backup/restore-archive"))
        .bearer_auth(admin_token())
        .multipart(form)
        .send().await.unwrap();
    let s = r.status().as_u16();
    (s, r.json().await.unwrap_or(serde_json::Value::Null))
}

#[tokio::test]
async fn a_backup_taken_today_can_actually_be_restored() {
    // The whole point. A backup format with no working restore is not a backup.
    let (base, _d) = serve().await;
    let archive = take_archive(&base).await;
    let (status, body) = restore(&base, archive, false).await;
    assert_eq!(status, 200, "restore must succeed: {body}");
    assert_eq!(body["os_cases"], 200, "every case must come back: {body}");
    assert!(
        body["previous_data_saved_to"].as_str().unwrap_or("").ends_with(".cops"),
        "the replaced data must be archived first: {body}"
    );
}

#[tokio::test]
async fn a_backup_holding_less_data_is_refused_unless_confirmed() {
    // The old restore checked only that a cops_master TABLE existed, so a valid
    // COPS database holding two rows passed and destroyed everything.
    let (base, _d) = serve().await;
    let full = take_archive(&base).await;

    // An archive taken when the database held far less.
    let (small_base, _d2) = serve().await;
    {
        let c = reqwest::Client::new();
        let _ = c.get(format!("{small_base}/backup/auto/status"))
            .bearer_auth(officer_token()).send().await;
    }
    // Restoring the FULL archive into the full database is fine; the guard is
    // about restoring something smaller, so shrink the live side instead by
    // restoring a small archive built from a nearly-empty database.
    let small = {
        let (b2, _d3) = serve_tiny().await;
        take_archive(&b2).await
    };

    let (status, body) = restore(&base, small.clone(), false).await;
    assert_eq!(status, 400, "a shrinking restore must be refused: {body}");
    let detail = body["detail"].as_str().unwrap_or("");
    assert!(detail.contains("FEWER"), "must say what is wrong: {detail}");
    assert!(detail.contains("cops_master"), "must name the table: {detail}");

    // Still possible deliberately.
    let (status2, body2) = restore(&base, small, true).await;
    assert_eq!(status2, 200, "an explicit confirmation must work: {body2}");

    let _ = full;
}

#[tokio::test]
async fn a_file_that_is_not_a_backup_changes_nothing() {
    let (base, _d) = serve().await;
    let (status, body) = restore(&base, b"this is not an archive".to_vec(), false).await;
    assert_eq!(status, 400);
    let detail = body["detail"].as_str().unwrap_or("");
    assert!(
        detail.contains("Nothing has been changed"),
        "the officer must be told the database is untouched: {detail}"
    );

    // And it really is untouched.
    let after: serde_json::Value = reqwest::Client::new()
        .get(format!("{base}/backup/auto/status"))
        .bearer_auth(officer_token())
        .send().await.unwrap().json().await.unwrap();
    assert!(after.get("destinations").is_some());
}

#[tokio::test]
async fn the_backend_writes_the_backup_straight_to_a_chosen_path() {
    // The route the desktop app actually uses. Nothing crosses the HTTP body,
    // so the archive is never held in the page — which is what broke the old
    // full-database download once the data grew.
    let (base, dir) = serve().await;
    let target = dir.path().join("chosen").join("my_backup.cops");

    let r = reqwest::Client::new()
        .post(format!("{base}/backup/archive/save"))
        .bearer_auth(officer_token())
        .json(&serde_json::json!({ "path": target.to_str().unwrap() }))
        .send().await.unwrap();
    assert_eq!(r.status(), 200);
    let body: serde_json::Value = r.json().await.unwrap();

    assert!(target.exists(), "the file must be written where the officer asked: {body}");
    let bytes = std::fs::read(&target).unwrap();
    assert_eq!(&bytes[..2], b"PK", "not a zip");
    assert!(body["rows"].as_i64().unwrap_or(0) >= 200, "should report what it saved: {body}");
    assert!(body["message"].as_str().unwrap_or("").contains("records"), "{body}");
}

#[tokio::test]
async fn saving_to_a_folder_or_to_nowhere_is_refused_clearly() {
    let (base, dir) = serve().await;
    let c = reqwest::Client::new();

    for (payload, what) in [
        (serde_json::json!({ "path": "" }), "an empty path"),
        (serde_json::json!({}), "no path at all"),
        (serde_json::json!({ "path": dir.path().to_str().unwrap() }), "a folder"),
    ] {
        let r = c.post(format!("{base}/backup/archive/save"))
            .bearer_auth(officer_token())
            .json(&payload)
            .send().await.unwrap();
        assert_eq!(r.status(), 400, "{what} must be refused, not written");
    }
}

// ── DCR tariffs ──────────────────────────────────────────────────────────────

/// Seed tariff rows on a server and return its base URL.
async fn serve_with_tariffs() -> (String, tempfile::TempDir) {
    let (base, dir) = serve().await;
    // The pool used by the router is separate, so seed over HTTP.
    let c = reqwest::Client::new();
    for (eff, label, rate) in [
        ("2020-01-01", "old",     0.35_f64),
        ("2024-07-01", "budget",  0.38),
        ("2099-01-01", "future",  0.99),
    ] {
        let r = c.post(format!("{base}/dcr/tariffs"))
            .bearer_auth(officer_token())
            .json(&serde_json::json!({
                "effective_from": eff, "label": label,
                "baggage_rate": rate, "liquor_duty_rate": 0.15, "aidc_liquor_rate": 0.035,
                "gold_bcd_rate": 0.125, "aidc_gold_rate": 0.05,
                "gold_cons_bcd_rate": 0.125, "aidc_gold_cons_rate": 0.05,
                "silver_bcd_rate": 0.35, "aidc_silver_rate": 0.05,
                "silver_cons_rate": 0.35, "aidc_silver_cons_rate": 0.05
            }))
            .send().await.unwrap();
        assert!(r.status().is_success(), "seeding tariff {label} failed: {}", r.status());
    }
    (base, dir)
}

#[tokio::test]
async fn the_current_tariff_route_exists_and_is_point_in_time() {
    // The screen was already calling this route; it did not exist and returned
    // 404, so the formula page could not show which rates were in force.
    let (base, _d) = serve_with_tariffs().await;
    let c = reqwest::Client::new();

    let r = c.get(format!("{base}/dcr/tariffs/current"))
        .bearer_auth(officer_token()).send().await.unwrap();
    assert_eq!(r.status(), 200, "the route must exist");
    let now: serde_json::Value = r.json().await.unwrap();
    assert_eq!(now["label"], "budget",
               "today must use the newest tariff already in force, not the future one: {now}");

    // The point of the whole thing: an older session is valued at the rates that
    // applied THEN. Taking the newest row regardless of date would silently
    // revalue historical collections — wrong in a way nobody notices until an
    // audit asks why last year's figures moved.
    let then: serde_json::Value = c
        .get(format!("{base}/dcr/tariffs/current?as_of=2021-06-30"))
        .bearer_auth(officer_token()).send().await.unwrap().json().await.unwrap();
    assert_eq!(then["label"], "old", "a 2021 date must use the 2020 rates: {then}");
    assert!((then["baggage_rate"].as_f64().unwrap() - 0.35).abs() < 1e-9);
}

#[tokio::test]
async fn a_date_before_every_tariff_falls_back_rather_than_failing() {
    // Matches the original: a session dated before the first tariff row is
    // better valued at the oldest known rates than refused outright.
    let (base, _d) = serve_with_tariffs().await;
    let r = reqwest::Client::new()
        .get(format!("{base}/dcr/tariffs/current?as_of=1999-01-01"))
        .bearer_auth(officer_token()).send().await.unwrap();
    assert_eq!(r.status(), 200);
    let v: serde_json::Value = r.json().await.unwrap();
    assert_eq!(v["label"], "old");
}
