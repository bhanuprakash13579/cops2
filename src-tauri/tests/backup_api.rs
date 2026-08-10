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

/// Like `serve_with`, but also hands back the pool so a test can age a record.
/// Time-based rules cannot be tested by waiting a day.
async fn serve_with_pool(rows: i64) -> (String, tempfile::TempDir, Arc<db::DbPool>) {
    let (base, dir, pool) = serve_inner(rows).await;
    (base, dir, pool)
}

async fn serve_with(rows: i64) -> (String, tempfile::TempDir) {
    let (base, dir, _pool) = serve_inner(rows).await;
    (base, dir)
}

async fn serve_inner(rows: i64) -> (String, tempfile::TempDir, Arc<db::DbPool>) {
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
    let pool = Arc::new(pool);
    let app = api::build_app(pool.clone());
    let listener = tokio::net::TcpListener::bind::<SocketAddr>("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("http://{addr}{}", api::API_PREFIX), dir, pool)
}

fn officer_token() -> String {
    auth::create_token("1", "SDO", "Test Officer", Some("Supdt"), "ACTIVE").unwrap()
}

/// An adjudicating officer. Booking and adjudicating are deliberately different
/// roles — the officer who seizes goods must not also decide the penalty.
fn dc_token() -> String {
    auth::create_token("2", "DC", "Test DC", Some("Deputy Commissioner"), "ACTIVE").unwrap()
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

#[tokio::test]
async fn the_bank_challan_number_can_be_recorded_corrected_and_survives_a_backup() {
    // COPS2 had nowhere to put this: no column and no route, so a figure the
    // office is accountable for had no home at all.
    let (base, _d) = serve_with_tariffs().await;
    let c = reqwest::Client::new();

    let s: serde_json::Value = c.post(format!("{base}/dcr/sessions"))
        .bearer_auth(officer_token())
        .json(&serde_json::json!({ "report_date": "2026-08-09", "shift": "DAY" }))
        .send().await.unwrap().json().await.unwrap();
    let id = s["id"].as_i64().expect("session id");
    assert!(s["challan_no"].is_null(), "a new session has no challan yet: {s}");

    let set: serde_json::Value = c.patch(format!("{base}/dcr/sessions/{id}/challan"))
        .bearer_auth(officer_token())
        .json(&serde_json::json!({ "challan_no": "SBI/2026/00417" }))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(set["challan_no"], "SBI/2026/00417", "{set}");

    // The number arrives from the bank after the shift is written up, so a
    // mistyped challan must be correctable.
    let fixed: serde_json::Value = c.patch(format!("{base}/dcr/sessions/{id}/challan"))
        .bearer_auth(officer_token())
        .json(&serde_json::json!({ "challan_no": "SBI/2026/00418" }))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(fixed["challan_no"], "SBI/2026/00418");

    // Blank is refused rather than silently wiping an audit reference.
    let blank = c.patch(format!("{base}/dcr/sessions/{id}/challan"))
        .bearer_auth(officer_token())
        .json(&serde_json::json!({ "challan_no": "   " }))
        .send().await.unwrap();
    assert_eq!(blank.status(), 400);

    // And it must come back after a restore — an audit trail that does not
    // survive a restore is not an audit trail.
    let archive = take_archive(&base).await;
    let (status, body) = restore(&base, archive, true).await;
    assert_eq!(status, 200, "{body}");
    let after: serde_json::Value = c.get(format!("{base}/dcr/sessions/{id}"))
        .bearer_auth(officer_token()).send().await.unwrap().json().await.unwrap();
    assert_eq!(after["challan_no"], "SBI/2026/00418",
               "the challan must survive backup and restore: {after}");
}

#[tokio::test]
async fn path_parameter_routes_actually_match() {
    // axum 0.7 uses :id for path parameters; {id} is a LITERAL segment. If the
    // codebase uses {id} against axum 0.7, every detail route in the program
    // 404s and only the parameterless ones work.
    let (base, _d) = serve_with_tariffs().await;
    let c = reqwest::Client::new();

    let s: serde_json::Value = c.post(format!("{base}/dcr/sessions"))
        .bearer_auth(officer_token())
        .json(&serde_json::json!({ "report_date": "2026-08-09", "shift": "DAY" }))
        .send().await.unwrap().json().await.unwrap();
    let id = s["id"].as_i64().unwrap();

    let r = c.get(format!("{base}/dcr/sessions/{id}"))
        .bearer_auth(officer_token()).send().await.unwrap();
    assert_eq!(r.status(), 200,
               "GET /dcr/sessions/{id} returned {} — path parameters are not matching",
               r.status());

    let lit = c.get(format!("{base}/dcr/sessions/%7Bid%7D"))
        .bearer_auth(officer_token()).send().await.unwrap();
    assert_ne!(lit.status(), 200,
               "the literal text {{id}} matched — {{}} is not being read as a parameter");
}

// ── The OS case lifecycle ────────────────────────────────────────────────────
//
// These routes were unreachable until the path-parameter fix, so nothing here
// had ever run. Booking a case, opening it, editing it and adjudicating it is
// what the program is FOR — worth proving over HTTP rather than assuming the
// handlers are right because they compile.

#[tokio::test]
async fn a_case_can_be_booked_opened_edited_and_adjudicated() {
    let (base, _d) = serve().await;
    let c = reqwest::Client::new();
    let t = officer_token();

    // Book it.
    let created = c.post(format!("{base}/os"))
        .bearer_auth(&t)
        .json(&serde_json::json!({
            "os_no": "9001", "os_date": "2026-08-09", "os_year": 2026,
            "pax_name": "TEST PASSENGER", "passport_no": "Z9999999",
            "flight_no": "AI-101", "booked_by": "Test Officer",
            // A real case carries seized items. A case with none never reaches
            // the pending list at all — that list requires an item still Under
            // OS or Under Duty, which is the whole definition of pending.
            "items": [{
                "items_sno": 1, "items_desc": "GOLD CHAIN 24K",
                "items_qty": 1.0, "items_uqc": "PCS",
                "items_value": 250000.0, "items_duty": 96250.0,
                "items_release_category": "Under OS"
            }]
        }))
        .send().await.unwrap();
    assert!(created.status().is_success(),
            "booking a case failed: HTTP {}", created.status());

    // Open it — the route that used to 404.
    let got = c.get(format!("{base}/os/9001/2026")).bearer_auth(&t).send().await.unwrap();
    assert_eq!(got.status(), 200, "opening the case returned {}", got.status());
    let case: serde_json::Value = got.json().await.unwrap();
    assert_eq!(case["pax_name"], "TEST PASSENGER", "wrong case came back: {case}");

    // Edit it.
    let edited = c.put(format!("{base}/os/9001/2026"))
        .bearer_auth(&t)
        .json(&serde_json::json!({
            "os_no": "9001", "os_date": "2026-08-09", "os_year": 2026,
            "pax_name": "CORRECTED NAME", "passport_no": "Z9999999",
            "items": [{
                "items_sno": 1, "items_desc": "GOLD CHAIN 24K",
                "items_qty": 1.0, "items_value": 250000.0,
                "items_release_category": "Under OS"
            }]
        }))
        .send().await.unwrap();
    assert!(edited.status().is_success(), "editing failed: HTTP {}", edited.status());

    let after: serde_json::Value = c.get(format!("{base}/os/9001/2026"))
        .bearer_auth(&t).send().await.unwrap().json().await.unwrap();
    assert_eq!(after["pax_name"], "CORRECTED NAME", "the edit did not persist: {after}");

    // And it appears in the list the officers actually look at.
    let list: serde_json::Value = c.get(format!("{base}/os?status=pending"))
        .bearer_auth(&t).send().await.unwrap().json().await.unwrap();
    let items = list["items"].as_array().expect("items array");
    assert!(items.iter().any(|i| i["os_no"] == "9001"),
            "the booked case is missing from the pending list");

    // An SDO must NOT be able to adjudicate their own seizure. Asserted rather
    // than assumed — it is the separation the whole process rests on.
    let refused = c.post(format!("{base}/os/9001/2026/adjudicate"))
        .bearer_auth(&t)
        .json(&serde_json::json!({
            "adj_offr_name": "SDO TRYING IT", "adj_offr_designation": "Supdt"
        }))
        .send().await.unwrap();
    assert_eq!(refused.status(), 403,
               "an SDO must not be able to adjudicate, got {}", refused.status());

    // Adjudicate it properly — the adjudicating officer's decision, and the
    // step the whole case exists to reach.
    let adj = c.post(format!("{base}/os/9001/2026/adjudicate"))
        .bearer_auth(dc_token())
        .json(&serde_json::json!({
            "adj_offr_name": "TEST DC",
            "adj_offr_designation": "Deputy Commissioner",
            "adjudication_date": "2026-08-09",
            "adjn_offr_remarks": "Redeemed on payment of fine.",
            "rf_amount": 50000.0, "pp_amount": 10000.0
        }))
        .send().await.unwrap();
    assert!(adj.status().is_success(), "adjudication failed: HTTP {}", adj.status());

    let done: serde_json::Value = c.get(format!("{base}/os/9001/2026"))
        .bearer_auth(&t).send().await.unwrap().json().await.unwrap();
    assert_eq!(done["adj_offr_name"], "TEST DC", "the decision did not persist: {done}");

    // It must move OUT of pending and INTO adjudicated. A case that stays in
    // pending after adjudication is booked twice by the next officer.
    let pending: serde_json::Value = c.get(format!("{base}/os?status=pending"))
        .bearer_auth(&t).send().await.unwrap().json().await.unwrap();
    assert!(!pending["items"].as_array().unwrap().iter().any(|i| i["os_no"] == "9001"),
            "an adjudicated case is still showing as pending");
    let adjudicated: serde_json::Value = c.get(format!("{base}/os?status=adjudicated"))
        .bearer_auth(&t).send().await.unwrap().json().await.unwrap();
    assert!(adjudicated["items"].as_array().unwrap().iter().any(|i| i["os_no"] == "9001"),
            "the adjudicated case is missing from the adjudicated list");
}

#[tokio::test]
async fn a_booked_case_can_be_printed() {
    // Printing is the point of booking. It was unreachable too.
    let (base, _d) = serve().await;
    let c = reqwest::Client::new();
    let t = officer_token();

    c.post(format!("{base}/os")).bearer_auth(&t)
        .json(&serde_json::json!({
            "os_no": "9002", "os_date": "2026-08-09", "os_year": 2026,
            "pax_name": "PRINT TEST", "passport_no": "Z8888888",
            "items": [{
                "items_sno": 1, "items_desc": "LAPTOP",
                "items_qty": 1.0, "items_value": 90000.0,
                "items_release_category": "Under Duty"
            }]
        }))
        .send().await.unwrap();

    let pdf = c.get(format!("{base}/os/9002/2026/print-pdf"))
        .bearer_auth(&t).send().await.unwrap();
    assert_eq!(pdf.status(), 200, "print returned {}", pdf.status());
    let bytes = pdf.bytes().await.unwrap();
    assert!(bytes.len() > 500, "PDF is suspiciously small: {} bytes", bytes.len());
    assert_eq!(&bytes[..4], b"%PDF", "not a PDF");

    // TWO pages, always. The OS form is the booking on page 1 and the
    // adjudication order on page 2; a one-page output means the second half was
    // dropped, which nobody notices until the printed copy is filed and the
    // order is missing from it.
    let pages = count_pdf_pages(&bytes);
    assert_eq!(pages, 2, "the OS print must be exactly two pages, got {pages}");

    // Legal size — 8.5in x 14in = 612 x 1008 points. The forms are pre-printed
    // stationery; A4 output does not line up with them.
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("1008") || text.contains("1008.0"),
            "expected legal-size pages (612 x 1008 pt); MediaBox looks wrong");
}

/// Count pages by counting page objects. `/Type /Pages` is the tree root, so it
/// must not be counted as a page.
fn count_pdf_pages(bytes: &[u8]) -> usize {
    let t = String::from_utf8_lossy(bytes);
    t.matches("/Type /Page").count() + t.matches("/Type/Page").count()
        - t.matches("/Type /Pages").count() - t.matches("/Type/Pages").count()
}

// ── The edit / delete window ─────────────────────────────────────────────────
//
// A case may be corrected for 24 hours after adjudication and not afterwards.
// That is a legal control, not a convenience: after the window the record is
// what was decided. cops-web carries a note about getting this wrong once —
// adjudication_time was left NULL, the check saw None and returned true, and
// the window never closed at all.

/// Book, then adjudicate, a case. Returns nothing; the case is OS 7001/2026.
async fn book_and_adjudicate(base: &str) {
    let c = reqwest::Client::new();
    c.post(format!("{base}/os")).bearer_auth(officer_token())
        .json(&serde_json::json!({
            "os_no": "7001", "os_date": "2026-08-09", "os_year": 2026,
            "pax_name": "WINDOW TEST", "passport_no": "Z7777777",
            "items": [{ "items_sno": 1, "items_desc": "WATCH",
                        "items_value": 120000.0, "items_release_category": "Under OS" }]
        }))
        .send().await.unwrap();
    let a = c.post(format!("{base}/os/7001/2026/adjudicate")).bearer_auth(dc_token())
        .json(&serde_json::json!({
            "adj_offr_name": "TEST DC", "adj_offr_designation": "Deputy Commissioner",
            "adjudication_date": "2026-08-09", "rf_amount": 25000.0
        }))
        .send().await.unwrap();
    assert!(a.status().is_success(), "adjudication failed: {}", a.status());
}

/// Push a case's adjudication_time into the past so the window has closed.
fn age_adjudication(pool: &db::DbPool, hours: i64) {
    let c = pool.get().unwrap();
    let when = (chrono::Local::now() - chrono::Duration::hours(hours))
        .format("%Y-%m-%d %H:%M:%S").to_string();
    let n = c.execute(
        "UPDATE cops_master SET adjudication_time = ?1 WHERE os_no='7001' AND os_year=2026",
        rusqlite::params![when],
    ).unwrap();
    assert_eq!(n, 1, "the test case should exist");
}

#[tokio::test]
async fn an_adjudicated_case_can_be_corrected_inside_the_window() {
    let (base, _d, pool) = serve_with_pool(5).await;
    book_and_adjudicate(&base).await;
    age_adjudication(&pool, 2); // two hours ago — still open

    let r = reqwest::Client::new()
        .put(format!("{base}/os/7001/2026")).bearer_auth(dc_token())
        .json(&serde_json::json!({
            "os_no": "7001", "os_date": "2026-08-09", "os_year": 2026,
            "pax_name": "CORRECTED INSIDE WINDOW", "passport_no": "Z7777777"
        }))
        .send().await.unwrap();
    assert!(r.status().is_success(),
            "a correction two hours after adjudication must be allowed, got {}", r.status());
}

#[tokio::test]
async fn the_window_actually_closes_for_both_edit_and_delete() {
    // The failure this guards against is silent: if adjudication_time is never
    // stamped, or the comparison is wrong, the window never closes and an
    // adjudicated case stays editable for ever.
    let (base, _d, pool) = serve_with_pool(5).await;
    book_and_adjudicate(&base).await;
    age_adjudication(&pool, 25); // just past 24 hours

    let c = reqwest::Client::new();
    let edit = c.put(format!("{base}/os/7001/2026")).bearer_auth(dc_token())
        .json(&serde_json::json!({
            "os_no": "7001", "os_date": "2026-08-09", "os_year": 2026,
            "pax_name": "TOO LATE", "passport_no": "Z7777777"
        }))
        .send().await.unwrap();
    assert_eq!(edit.status(), 400,
               "editing 25 hours after adjudication must be refused, got {}", edit.status());

    let del = c.delete(format!("{base}/os/7001/2026")).bearer_auth(dc_token())
        .send().await.unwrap();
    assert_eq!(del.status(), 400,
               "deleting 25 hours after adjudication must be refused, got {}", del.status());

    // And the record still says what was decided.
    let still: serde_json::Value = c.get(format!("{base}/os/7001/2026"))
        .bearer_auth(dc_token()).send().await.unwrap().json().await.unwrap();
    assert_eq!(still["pax_name"], "WINDOW TEST",
               "the refused edit must not have been applied anyway: {still}");
}

// ── Querying ─────────────────────────────────────────────────────────────────
//
// The query module is how an officer answers "has this passenger been caught
// before?" — a search that silently misses a case is worse than one that
// errors, because the answer looks authoritative.

#[tokio::test]
async fn a_booked_case_is_findable_by_passport_name_and_number() {
    let (base, _d) = serve().await;
    let c = reqwest::Client::new();
    let t = officer_token();

    c.post(format!("{base}/os")).bearer_auth(&t)
        .json(&serde_json::json!({
            "os_no": "6001", "os_date": "2026-08-09", "os_year": 2026,
            "pax_name": "RAMESH KUMAR", "passport_no": "M1234567",
            "flight_no": "EK-544",
            "items": [{ "items_sno": 1, "items_desc": "GOLD BAR",
                        "items_value": 500000.0, "items_release_category": "Under OS" }]
        }))
        .send().await.unwrap();

    // Cross-reference search — by passport.
    let by_pp: serde_json::Value = c
        .get(format!("{base}/queries/search?passport=M1234567"))
        .bearer_auth(&t).send().await.unwrap().json().await.unwrap();
    let found = serde_json::to_string(&by_pp).unwrap();
    assert!(found.contains("6001"), "passport search did not find the case: {found}");

    // By name, and case-insensitively — officers do not type in capitals.
    let by_name: serde_json::Value = c
        .get(format!("{base}/queries/search?name=ramesh"))
        .bearer_auth(&t).send().await.unwrap().json().await.unwrap();
    assert!(serde_json::to_string(&by_name).unwrap().contains("6001"),
            "lowercase name search must still match: {by_name}");

    // A passport that was never booked must return nothing — a search that
    // matches everything is as useless as one that matches nothing.
    let none: serde_json::Value = c
        .get(format!("{base}/queries/search?passport=ZZ0000000"))
        .bearer_auth(&t).send().await.unwrap().json().await.unwrap();
    assert!(!serde_json::to_string(&none).unwrap().contains("6001"),
            "an unrelated passport must not match: {none}");

    // The OS query module's own search.
    let osq: serde_json::Value = c.post(format!("{base}/os-query/search"))
        .bearer_auth(&t)
        .json(&serde_json::json!({ "passport_no": "M1234567" }))
        .send().await.unwrap().json().await.unwrap();
    assert!(serde_json::to_string(&osq).unwrap().contains("6001"),
            "the OS query search did not find the case: {osq}");
}

// ── Admin config backup / restore ────────────────────────────────────────────
//
// Settings without case data — how a new office machine is seeded so it prints
// and calculates exactly like the one already in service.

#[tokio::test]
async fn config_backup_round_trips_and_never_overwrites_local_settings() {
    let (base, _d) = serve().await;
    let c = reqwest::Client::new();

    let exported: serde_json::Value = c.get(format!("{base}/admin/config/backup"))
        .bearer_auth(admin_token()).send().await.unwrap().json().await.unwrap();
    assert_eq!(exported["format_version"], 1);
    assert_eq!(exported["kind"], "admin_config_only");
    assert!(exported["tables"]["print_template_config"].as_array().unwrap().len() >= 1,
            "the seeded template should be exported: {exported}");

    // It must carry settings and NOT case data — this file is meant to be
    // carried between machines and emailed.
    let text = serde_json::to_string(&exported).unwrap();
    assert!(!text.contains("cops_master"), "case data must never be in a config file");

    // Restoring into a machine that already has these settings must change
    // nothing — an import that replaced a live print template with an older one
    // would be discovered at the counter.
    let again: serde_json::Value = c.post(format!("{base}/admin/config/restore"))
        .bearer_auth(admin_token()).json(&exported).send().await.unwrap()
        .json().await.unwrap();
    let added: i64 = again["added"].as_object().unwrap()
        .values().filter_map(|v| v.as_i64()).sum();
    assert_eq!(added, 0, "re-importing the same file must add nothing: {again}");
}

#[tokio::test]
async fn a_config_file_from_the_python_version_restores() {
    // The actual file exported from the office, if it is present. This is the
    // whole point of matching the format: the settings already in service are
    // the ones worth keeping.
    let path = "/home/bhanu/Downloads/cops_config_backup_2026-08-06.json";
    let Ok(raw) = std::fs::read_to_string(path) else {
        eprintln!("skipping: {path} not present");
        return;
    };
    let payload: serde_json::Value = serde_json::from_str(&raw).unwrap();

    let (base, _d) = serve().await;
    let c = reqwest::Client::new();
    let r = c.post(format!("{base}/admin/config/restore"))
        .bearer_auth(admin_token()).json(&payload).send().await.unwrap();
    assert_eq!(r.status(), 200, "a real cops-web config file must restore");
    let body: serde_json::Value = r.json().await.unwrap();

    let templates: i64 = body["added"]["print_template_config"].as_i64().unwrap_or(0);
    let users: i64 = body["added"]["users"].as_i64().unwrap_or(0);
    assert!(templates >= 30, "expected the office's print templates: {body}");
    assert!(users >= 1, "expected the office's user accounts: {body}");

    // feature_flags is the schema that diverged — cops-web keeps one wide row,
    // COPS2 keeps key/value pairs. It must be translated, not dropped.
    let flags: i64 = body["added"]["feature_flags"].as_i64().unwrap_or(0);
    assert!(flags >= 1, "feature flags must be translated across the schemas: {body}");

    // And the settings must actually be queryable afterwards.
    let back: serde_json::Value = c.get(format!("{base}/admin/config/backup"))
        .bearer_auth(admin_token()).send().await.unwrap().json().await.unwrap();
    assert!(back["tables"]["print_template_config"].as_array().unwrap().len() >= 30,
            "restored templates must be readable back out");
    eprintln!("RESTORED FROM THE OFFICE FILE: {templates} templates, {users} users, {flags} flags");
}

#[tokio::test]
async fn a_session_can_be_found_by_its_date_and_shift() {
    // An officer opening the duty register thinks in "the night shift on the
    // 9th", not in row ids.
    let (base, _d) = serve_with_tariffs().await;
    let c = reqwest::Client::new();

    let made: serde_json::Value = c.post(format!("{base}/dcr/sessions"))
        .bearer_auth(officer_token())
        .json(&serde_json::json!({ "report_date": "2026-08-09", "shift": "NIGHT" }))
        .send().await.unwrap().json().await.unwrap();
    let id = made["id"].as_i64().unwrap();

    let found: serde_json::Value = c.get(format!("{base}/dcr/sessions/by-date/2026-08-09/NIGHT"))
        .bearer_auth(officer_token()).send().await.unwrap().json().await.unwrap();
    assert_eq!(found["id"], id, "wrong session came back: {found}");

    // A link carrying a lower-case shift must still find it — the column holds
    // DAY/NIGHT, and a case-sensitive match would look like a missing session.
    let lower: serde_json::Value = c.get(format!("{base}/dcr/sessions/by-date/2026-08-09/night"))
        .bearer_auth(officer_token()).send().await.unwrap().json().await.unwrap();
    assert_eq!(lower["id"], id, "lower-case shift must match: {lower}");

    // A date with no session says so, rather than returning someone else's.
    let missing = c.get(format!("{base}/dcr/sessions/by-date/2026-01-01/DAY"))
        .bearer_auth(officer_token()).send().await.unwrap();
    assert_eq!(missing.status(), 404);

    // And it must not collide with /dcr/sessions/:id — different shapes, both live.
    let by_id = c.get(format!("{base}/dcr/sessions/{id}"))
        .bearer_auth(officer_token()).send().await.unwrap();
    assert_eq!(by_id.status(), 200, "the by-id route must still work");
}

#[tokio::test]
async fn cess_on_cigarettes_is_recorded_and_survives() {
    // COPS2 had no column, no field and no place in the duty total, so every
    // cigarette case produced a SMALLER total than the same case in cops-web —
    // a wrong revenue figure, silently.
    let (base, _d) = serve_with_tariffs().await;
    let c = reqwest::Client::new();

    let s: serde_json::Value = c.post(format!("{base}/dcr/sessions"))
        .bearer_auth(officer_token())
        .json(&serde_json::json!({ "report_date": "2026-08-09", "shift": "DAY" }))
        .send().await.unwrap().json().await.unwrap();
    let id = s["id"].as_i64().unwrap();

    let saved = c.put(format!("{base}/dcr/sessions/{id}/entries"))
        .bearer_auth(officer_token())
        .json(&serde_json::json!({
            "entries": [{
                "sort_order": 1, "sl_no": 1, "br_no": "BR/1/2026",
                "item_desc": "CIGARETTES", "dutiable_value": 40000.0,
                "cigarette_duty": 10000.0,
                "cess_on_cig": 1500.0,
                "total_duty": 11500.0,
                "is_offline_br": true
            }],
            "dr_entries": [], "os_entries": []
        }))
        .send().await.unwrap();
    assert!(saved.status().is_success(), "saving entries failed: {}", saved.status());

    let back: serde_json::Value = c.get(format!("{base}/dcr/sessions/{id}"))
        .bearer_auth(officer_token()).send().await.unwrap().json().await.unwrap();
    let entry = &back["entries"][0];
    assert_eq!(entry["cess_on_cig"], 1500.0,
               "the cess must come back from the database: {entry}");

    // And it must survive a backup and restore — a duty component that vanishes
    // on restore understates the month after a recovery.
    let archive = take_archive(&base).await;
    let (status, body) = restore(&base, archive, true).await;
    assert_eq!(status, 200, "{body}");
    let after: serde_json::Value = c.get(format!("{base}/dcr/sessions/{id}"))
        .bearer_auth(officer_token()).send().await.unwrap().json().await.unwrap();
    assert_eq!(after["entries"][0]["cess_on_cig"], 1500.0,
               "the cess must survive a restore: {after}");
}
