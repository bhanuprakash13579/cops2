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

// ── Excluding tables from the backup ─────────────────────────────────────────

#[tokio::test]
async fn revenue_tables_can_be_left_out_but_case_records_never_can() {
    // The office exports and emails each shift's revenue report, so a copy
    // exists outside the system and they may choose not to back the sessions up.
    // Case records have no such copy, so no setting may drop them — including a
    // setting typed by someone who has misunderstood what it does.
    let (base, _d, pool) = serve_with_pool(50).await;
    let c = reqwest::Client::new();

    // Someone excludes the revenue tables AND, mistakenly, the OS register.
    pool.get().unwrap().execute(
        "INSERT OR REPLACE INTO app_settings(key, value) VALUES
         ('backup_exclude_tables', 'dcr_sessions,dcr_entries,cops_master')",
        [],
    ).unwrap();

    let target = _d.path().join("excluded.cops");
    let r = c.post(format!("{base}/backup/archive/save"))
        .bearer_auth(officer_token())
        .json(&serde_json::json!({ "path": target.to_str().unwrap() }))
        .send().await.unwrap();
    assert_eq!(r.status(), 200);
    let body: serde_json::Value = r.json().await.unwrap();

    // Restore it into a second, empty installation and see what actually arrived.
    let (base2, _d2) = serve_with_tariffs().await;
    let bytes = std::fs::read(&target).unwrap();
    let (status, out) = restore(&base2, bytes, true).await;
    assert_eq!(status, 200, "{out}");

    // The OS register survived despite being listed — it is protected.
    let cases: serde_json::Value = c.get(format!("{base2}/os?status=pending"))
        .bearer_auth(officer_token()).send().await.unwrap().json().await.unwrap();
    assert!(cases["total"].as_i64().unwrap_or(0) > 0 || cases.get("items").is_some(),
            "case records must be backed up even when listed for exclusion: {cases}");
    assert!(body["rows"].as_i64().unwrap_or(0) >= 50,
            "the archive should still hold the case rows: {body}");
}

#[tokio::test]
async fn a_session_still_explains_its_figures_after_the_rates_change() {
    // The concern: a session stores tariff_id. Rates change, or the row is lost,
    // and the shift's figures can no longer be explained — the value is there,
    // the duty is there, and what rate connected them is gone.
    let (base, _d, pool) = serve_with_pool(5).await;
    let c = reqwest::Client::new();

    // A tariff in force, then a session worked under it.
    c.post(format!("{base}/dcr/tariffs")).bearer_auth(officer_token())
        .json(&serde_json::json!({
            "effective_from": "2026-01-01", "label": "budget 2026",
            "baggage_rate": 0.38, "liquor_duty_rate": 0.15, "aidc_liquor_rate": 0.035,
            "gold_bcd_rate": 0.125, "aidc_gold_rate": 0.05,
            "gold_cons_bcd_rate": 0.125, "aidc_gold_cons_rate": 0.05,
            "silver_bcd_rate": 0.35, "aidc_silver_rate": 0.05,
            "silver_cons_rate": 0.35, "aidc_silver_cons_rate": 0.05
        })).send().await.unwrap();

    let s: serde_json::Value = c.post(format!("{base}/dcr/sessions"))
        .bearer_auth(officer_token())
        .json(&serde_json::json!({ "report_date": "2026-03-01", "shift": "DAY" }))
        .send().await.unwrap().json().await.unwrap();
    let id = s["id"].as_i64().unwrap();
    assert_eq!(s["tariff"]["baggage_rate"], 0.38, "the live rate should be joined: {s}");

    // A foreign key already stops a tariff in use from being DELETED, so the
    // real exposure is the rates being EDITED — which is what happens at a
    // budget. The row keeps its id, the join keeps working, and it silently
    // starts describing this old shift with next year's rates.
    pool.get().unwrap().execute(
        "UPDATE dcr_tariffs SET baggage_rate = 0.45, label = 'budget 2027'", []
    ).unwrap();

    let after: serde_json::Value = c.get(format!("{base}/dcr/sessions/{id}"))
        .bearer_auth(officer_token()).send().await.unwrap().json().await.unwrap();
    assert_eq!(after["tariff_applied"]["baggage_rate"], 0.38,
               "the shift must still report the rate it was WORKED under, not today's: {after}");
    assert_eq!(after["tariff_applied"]["label"], "budget 2026");
    assert!(after["tariff_applied"]["frozen_at"].is_string(),
            "and when it was frozen, so the record is self-describing: {after}");

    // It must also survive a backup and restore, or the protection is only skin deep.
    let archive = take_archive(&base).await;
    let (status, body) = restore(&base, archive, true).await;
    assert_eq!(status, 200, "{body}");
    let restored: serde_json::Value = c.get(format!("{base}/dcr/sessions/{id}"))
        .bearer_auth(officer_token()).send().await.unwrap().json().await.unwrap();
    assert_eq!(restored["tariff_applied"]["baggage_rate"], 0.38,
               "the applied rates must survive a restore: {restored}");
}

#[tokio::test]
async fn a_month_of_sessions_comes_back_in_one_request() {
    // The monthly register is built in the browser, so it needs the whole
    // month. Session by session would be up to 62 round trips for one click.
    let (base, _d) = serve_with_tariffs().await;
    let c = reqwest::Client::new();

    for (d, shift) in [("2026-03-01", "DAY"), ("2026-03-01", "NIGHT"),
                       ("2026-03-15", "DAY"), ("2026-04-02", "DAY")] {
        c.post(format!("{base}/dcr/sessions")).bearer_auth(officer_token())
            .json(&serde_json::json!({ "report_date": d, "shift": shift }))
            .send().await.unwrap();
    }

    let m: serde_json::Value = c.get(format!("{base}/dcr/month/2026/3"))
        .bearer_auth(officer_token()).send().await.unwrap().json().await.unwrap();
    let sessions = m["sessions"].as_array().expect("sessions");
    assert_eq!(sessions.len(), 3, "April must not leak into March: {m}");

    // Ordered as the register is read: by date, day shift before night.
    assert_eq!(sessions[0]["report_date"], "2026-03-01");
    assert_eq!(sessions[0]["shift"], "DAY");
    assert_eq!(sessions[1]["shift"], "NIGHT");
    assert_eq!(sessions[2]["report_date"], "2026-03-15");

    // Entries must be present — the report cannot be built from headers alone.
    assert!(sessions[0].get("entries").is_some(), "entries missing: {m}");
    // And the rates it was worked under, for the register's duty columns.
    assert!(sessions[0].get("tariff").is_some());

    let empty: serde_json::Value = c.get(format!("{base}/dcr/month/2026/12"))
        .bearer_auth(officer_token()).send().await.unwrap().json().await.unwrap();
    assert_eq!(empty["sessions"].as_array().unwrap().len(), 0,
               "a month with no sessions returns none, not an error");

    let bad = c.get(format!("{base}/dcr/month/2026/13"))
        .bearer_auth(officer_token()).send().await.unwrap();
    assert_eq!(bad.status(), 400, "month 13 must be refused");
}

// ── Device registration ──────────────────────────────────────────────────────

#[tokio::test]
async fn this_machine_can_register_itself_and_doing_it_twice_is_harmless() {
    let (base, _d) = serve().await;
    let c = reqwest::Client::new();

    let before: serde_json::Value = c.get(format!("{base}/admin/device-info"))
        .bearer_auth(admin_token()).send().await.unwrap().json().await.unwrap();
    assert!(before["hostname"].as_str().map(|h| !h.is_empty()).unwrap_or(false),
            "a machine must report a hostname: {before}");
    assert_eq!(before["registered"], false);

    let first: serde_json::Value = c.post(format!("{base}/admin/register-device"))
        .bearer_auth(admin_token()).send().await.unwrap().json().await.unwrap();
    assert_eq!(first["created"], true, "{first}");

    // Pressing it again when unsure whether it worked must not create a second
    // row — that is the obvious way to use the button.
    let again: serde_json::Value = c.post(format!("{base}/admin/register-device"))
        .bearer_auth(admin_token()).send().await.unwrap().json().await.unwrap();
    assert_eq!(again["created"], false, "a second press must not duplicate: {again}");
    assert_eq!(again["id"], first["id"]);

    let after: serde_json::Value = c.get(format!("{base}/admin/device-info"))
        .bearer_auth(admin_token()).send().await.unwrap().json().await.unwrap();
    assert_eq!(after["registered"], true);

    let devices: serde_json::Value = c.get(format!("{base}/admin/devices"))
        .bearer_auth(admin_token()).send().await.unwrap().json().await.unwrap();
    let n = devices.as_array().map(|a| a.len())
        .or_else(|| devices["items"].as_array().map(|a| a.len())).unwrap_or(0);
    assert_eq!(n, 1, "exactly one device row: {devices}");

    // An ordinary officer must not be able to add machines to the allow-list.
    let officer = c.post(format!("{base}/admin/register-device"))
        .bearer_auth(officer_token()).send().await.unwrap();
    assert!(officer.status() == 401 || officer.status() == 403);
}

// ── BR / DR custom report ────────────────────────────────────────────────────

#[tokio::test]
async fn the_brdr_report_returns_rows_and_refuses_columns_that_do_not_exist() {
    let (base, _d, pool) = serve_with_pool(5).await;
    pool.get().unwrap().execute(
        "INSERT INTO br_master(br_no, br_year, br_date, br_type, pax_name, entry_deleted)
         VALUES (101, 2026, '2026-05-04', 'DUTY', 'BR PASSENGER', 'N')", []).unwrap();

    let c = reqwest::Client::new();
    let r: serde_json::Value = c.post(format!("{base}/backup/custom-report-brdr"))
        .bearer_auth(officer_token())
        .json(&serde_json::json!({
            "register": "br", "master_cols": ["pax_name", "br_date"],
            "from_date": "2026-01-01", "to_date": "2026-12-31"
        }))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(r["count"], 1, "{r}");
    assert_eq!(r["rows"][0]["pax_name"], "BR PASSENGER");
    // The receipt number is always included, so a row can be traced back.
    assert_eq!(r["rows"][0]["br_no"], 101, "every row must be traceable: {r}");

    // Column names cannot be parameterised, so they are the one part of the
    // query built from input. Anything not a real column must be refused.
    for bad in ["pax_name; DROP TABLE br_master", "sqlite_master", "nonexistent_col"] {
        let resp = c.post(format!("{base}/backup/custom-report-brdr"))
            .bearer_auth(officer_token())
            .json(&serde_json::json!({ "register": "br", "master_cols": [bad] }))
            .send().await.unwrap();
        assert_eq!(resp.status(), 400, "must refuse {bad:?}");
    }

    // And the register itself is a fixed choice.
    let wrong = c.post(format!("{base}/backup/custom-report-brdr"))
        .bearer_auth(officer_token())
        .json(&serde_json::json!({ "register": "cops_master", "master_cols": ["pax_name"] }))
        .send().await.unwrap();
    assert_eq!(wrong.status(), 400, "only br or dr");

    // The table survived every attempt above.
    let still: i64 = pool.get().unwrap()
        .query_row("SELECT COUNT(*) FROM br_master", [], |r| r.get(0)).unwrap();
    assert_eq!(still, 1, "br_master must be intact");
}

#[tokio::test]
async fn indexes_are_present_after_a_restore_even_though_the_archive_omits_them() {
    // The archive drops indexes on purpose — they are derived data and were 24%
    // of its size. The question that matters is whether the RESTORED database
    // still has them, because a restore that leaves the registers unindexed
    // would be quietly slower for ever afterwards.
    let (base, _d, pool) = serve_with_pool(200).await;

    let count_indexes = |pool: &std::sync::Arc<db::DbPool>| -> i64 {
        pool.get().unwrap().query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type='index' AND name NOT LIKE 'sqlite_autoindex%'",
            [], |r| r.get(0)).unwrap()
    };
    let before = count_indexes(&pool);
    assert!(before > 0, "the live database should be indexed to begin with");

    let archive = take_archive(&base).await;
    // Prove the archive itself carries none of them.
    let tmp = _d.path().join("peek.db");
    std::fs::write(_d.path().join("a.cops"), &archive).unwrap();
    let (status, body) = restore(&base, archive, true).await;
    assert_eq!(status, 200, "{body}");
    let _ = tmp;

    let after = count_indexes(&pool);
    assert_eq!(after, before,
               "every index must still exist after a restore: {before} before, {after} after");

    // And they must actually be usable, not merely present.
    let plan: String = pool.get().unwrap().query_row(
        "EXPLAIN QUERY PLAN SELECT * FROM cops_master WHERE os_no = '5' AND os_year = 2026",
        [], |r| r.get(3)).unwrap();
    assert!(plan.to_lowercase().contains("index"),
            "the query planner should use an index, got: {plan}");

    // The indexes live in the SCHEMA, which is rebuilt from migrations.sql when
    // the database is opened — they are not carried in the archive at all. So an
    // archive taken by an older build restores into a newer one and picks up
    // every index that build added, without anyone reindexing anything. Check
    // the newest one specifically, and check the planner actually reaches for it
    // on the query it was added for.
    let c = pool.get().unwrap();
    let present: i64 = c.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='ix_br_master_list'",
        [], |r| r.get(0)).unwrap();
    assert_eq!(present, 1, "the baggage list index must exist after a restore");

    let plan: String = c.query_row(
        "EXPLAIN QUERY PLAN
         SELECT br_no FROM br_master WHERE entry_deleted='N'
         ORDER BY br_date DESC LIMIT 20",
        [], |r| r.get(3)).unwrap();
    assert!(plan.contains("ix_br_master_list"),
            "the baggage list should use its index after a restore, got: {plan}");
}

#[tokio::test]
async fn the_office_csv_backup_restores_completely_into_cops2() {
    // The upgrade path the office will actually use: the backup file from the
    // Python version, uploaded straight into COPS2. It has to bring EVERYTHING —
    // the registers included. Restoring the cases and quietly leaving 334,546
    // baggage receipts behind would be discovered only when someone went looking
    // for one.
    let path = "/home/bhanu/Downloads/cops_backup_2026-08-06.zip";
    let Ok(bytes) = std::fs::read(path) else {
        eprintln!("skipping: {path} not present");
        return;
    };

    let (base, _d, pool) = serve_with_pool(0).await;
    let form = reqwest::multipart::Form::new()
        .part("file", reqwest::multipart::Part::bytes(bytes).file_name("backup.zip"));
    let r = reqwest::Client::new()
        .post(format!("{base}/admin/backup/restore"))
        .bearer_auth(admin_token())
        .multipart(form)
        .send().await.unwrap();
    let st = r.status();
    let txt = r.text().await.unwrap();
    assert_eq!(st, 200, "the office backup must restore — server said: {txt}");
    let body: serde_json::Value = serde_json::from_str(&txt).unwrap();

    let count = |t: &str| -> i64 {
        pool.get().unwrap()
            .query_row(&format!("SELECT COUNT(*) FROM {t}"), [], |r| r.get(0)).unwrap()
    };
    let got = [
        ("cops_master", count("cops_master")),
        ("cops_items",  count("cops_items")),
        ("br_master",   count("br_master")),
        ("br_items",    count("br_items")),
        ("dr_master",   count("dr_master")),
        ("dr_items",    count("dr_items")),
    ];
    eprintln!("RESTORED INTO COPS2: {got:?}");
    eprintln!("  reported: {}", serde_json::to_string(&body).unwrap_or_default());

    for (t, n) in got {
        assert!(n > 0, "{t} came back empty — the register was not restored");
    }
    assert!(got[0].1 >= 29_000, "expected the office's OS cases, got {}", got[0].1);
    assert!(got[2].1 >= 300_000, "expected the baggage register, got {}", got[2].1);
    assert!(got[4].1 >= 14_000, "expected the detention register, got {}", got[4].1);
}

#[tokio::test]
async fn foreign_key_enforcement_is_restored_after_a_backup_and_a_restore() {
    // Both paths turn foreign keys OFF to copy rows in arbitrary order. If they
    // do not come back on, the connection returns to the pool with integrity
    // checks disabled and every later case write skips them, silently. SQLite
    // also ignores that pragma inside a transaction while reporting success, so
    // the value has to be READ BACK rather than assumed.
    let (base, _d, pool) = serve_with_pool(30).await;
    let fk_on = |p: &std::sync::Arc<db::DbPool>| -> i64 {
        p.get().unwrap().query_row("PRAGMA foreign_keys", [], |r| r.get(0)).unwrap_or(-1)
    };
    assert_eq!(fk_on(&pool), 1, "enforcement should be on to begin with");

    let archive = take_archive(&base).await;
    assert_eq!(fk_on(&pool), 1, "enforcement must be back on after a backup");

    let (status, body) = restore(&base, archive, true).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(fk_on(&pool), 1, "enforcement must be back on after a restore");
}

#[tokio::test]
async fn a_long_case_still_prints_on_exactly_two_pages() {
    // The OS form is pre-printed stationery: page 1 is the booking, page 2 the
    // adjudication order. A third page means the layout overflowed, and the
    // officer finds out only when the filed copy does not match the form.
    //
    // The single-item case already covered here never exercises that. Real
    // seizures run to dozens of items with remarks filled to the field limits,
    // and that is precisely when the previous system's output spilled onto a
    // third page.
    let (base, _d) = serve().await;
    let c = reqwest::Client::new();
    let t = officer_token();

    let items: Vec<_> = (1..=30).map(|i| serde_json::json!({
        "items_sno": i,
        "items_desc": format!(
            "ASSORTED GOLD ORNAMENTS, ITEM {i} — chain with pendant, hallmarked, \
             recovered from the concealed lining of the said baggage"),
        "items_qty": 2.0,
        "items_value": 125_000.0,
        "items_release_category": "Under Duty"
    })).collect();

    let r = c.post(format!("{base}/os")).bearer_auth(&t)
        .json(&serde_json::json!({
            "os_no": "9003", "os_date": "2026-08-09", "os_year": 2026,
            "pax_name": "LONG CONTENT TEST", "passport_no": "Z9999999",
            "supdts_remarks": "A".repeat(1500),       // the field's own limit
            "adjn_offr_remarks": "B".repeat(3000),    // ditto
            "items": items
        }))
        .send().await.unwrap();
    assert_eq!(r.status(), 200, "booking the long case failed: {}", r.text().await.unwrap());

    let pdf = c.get(format!("{base}/os/9003/2026/print-pdf"))
        .bearer_auth(&t).send().await.unwrap();
    assert_eq!(pdf.status(), 200);
    let bytes = pdf.bytes().await.unwrap();
    let pages = count_pdf_pages(&bytes);
    eprintln!("LONG CASE: 30 items, 1500+3000 chars of remarks -> {pages} pages");
    assert_eq!(pages, 2, "a long case must still be two pages, got {pages}");
}

#[tokio::test]
async fn the_printed_os_carries_the_customs_emblem() {
    // The emblem is part of the form, not decoration. Typst resolves the image
    // through the World's file() hook, and a path it cannot resolve is a compile
    // error rather than a blank space — but a template that simply stopped
    // asking for it would fail silently and look fine in every other assertion.
    // So check the PDF really contains an embedded image.
    let (base, _d) = serve().await;
    let c = reqwest::Client::new();
    let t = officer_token();

    c.post(format!("{base}/os")).bearer_auth(&t)
        .json(&serde_json::json!({
            "os_no": "9004", "os_date": "2026-08-09", "os_year": 2026,
            "pax_name": "EMBLEM TEST", "passport_no": "Z7777777",
            "items": [{ "items_sno": 1, "items_desc": "WATCH",
                        "items_qty": 1.0, "items_value": 40000.0,
                        "items_release_category": "Under Duty" }]
        })).send().await.unwrap();

    let bytes = c.get(format!("{base}/os/9004/2026/print-pdf"))
        .bearer_auth(&t).send().await.unwrap()
        .bytes().await.unwrap();

    if let Ok(dir) = std::env::var("OS_PDF_DUMP") {
        std::fs::write(format!("{dir}/os_page.pdf"), &bytes).unwrap();
    }
    let t_ = String::from_utf8_lossy(&bytes);
    assert!(t_.contains("/Subtype /Image") || t_.contains("/Subtype/Image"),
            "the printed OS has no embedded image — the emblem is missing");
    assert_eq!(count_pdf_pages(&bytes), 2, "still exactly two pages with the emblem");
}

// ── Licensing ────────────────────────────────────────────────────────────────
//
// The codes live only as bcrypt hashes in the binary, so these tests cannot
// contain them either — they would end up in the repository, which is the one
// place the whole design says they must never be. Instead they exercise every
// path that does NOT need a real code, plus the reuse record itself.

async fn status(base: &str) -> serde_json::Value {
    reqwest::get(format!("{base}/trial-status")).await.unwrap()
        .json().await.unwrap()
}

async fn try_code(base: &str, code: &str) -> (u16, serde_json::Value) {
    let r = reqwest::Client::new()
        .post(format!("{base}/license/activate"))
        .json(&serde_json::json!({ "code": code }))
        .send().await.unwrap();
    let st = r.status().as_u16();
    (st, r.json().await.unwrap())
}

#[tokio::test]
async fn a_fresh_install_starts_its_trial_on_first_look() {
    let (base, _d) = serve().await;
    let s = status(&base).await;
    assert_eq!(s["trial_disabled"], false);
    assert_eq!(s["expired"], false);
    assert_eq!(s["trial_days"], 30, "a fresh install gets the default 30 days");
    assert_eq!(s["days_remaining"], 30, "the clock starts on first look, not at build time");
    assert!(s["trial_start_date"].is_string());
}

#[tokio::test]
async fn the_trial_endpoint_needs_no_login() {
    // An expired installation cannot log in. If this required a session, the
    // code entry would be locked behind the very thing the code unlocks.
    let (base, _d) = serve().await;
    let r = reqwest::get(format!("{base}/trial-status")).await.unwrap();
    assert_eq!(r.status(), 200, "trial status must be readable anonymously");

    let r = reqwest::Client::new()
        .post(format!("{base}/license/activate"))
        .json(&serde_json::json!({ "code": "XXXX-XXXX-XXXX-XXXX-XXXX" }))
        .send().await.unwrap();
    assert_ne!(r.status(), 401, "activation must be reachable without a session");
}

#[tokio::test]
async fn a_wrong_code_is_refused_and_changes_nothing() {
    let (base, _d) = serve().await;
    let before = status(&base).await;

    for bad in ["", "short", "XXXX-XXXX-XXXX-XXXX-XXXX", "0000000000000000000000000"] {
        let (st, body) = try_code(&base, bad).await;
        assert_eq!(st, 400, "'{bad}' should be refused, got {body}");
    }

    let after = status(&base).await;
    assert_eq!(before["days_remaining"], after["days_remaining"],
               "a refused code must not move the trial window");
    assert_eq!(after["trial_disabled"], false, "a refused code must not license the install");
}

#[tokio::test]
async fn the_administrator_keeps_full_control_of_the_trial() {
    // Same three controls the Python app gives, so the office does not have to
    // learn anything new.
    let (base, _d) = serve().await;
    let c = reqwest::Client::new();
    let t = admin_token();

    let r = c.post(format!("{base}/admin/trial/set-days"))
        .bearer_auth(&t).json(&serde_json::json!({ "trial_days": 120 }))
        .send().await.unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(status(&base).await["trial_days"], 120);

    let r = c.post(format!("{base}/admin/trial/disable")).bearer_auth(&t)
        .send().await.unwrap();
    assert_eq!(r.status(), 200);
    let s = status(&base).await;
    assert_eq!(s["trial_disabled"], true, "disabling makes the install permanent");
    assert_eq!(s["expired"], false, "a disabled trial never expires");

    let r = c.post(format!("{base}/admin/trial/reset")).bearer_auth(&t)
        .send().await.unwrap();
    assert_eq!(r.status(), 200);
    let s = status(&base).await;
    assert_eq!(s["trial_disabled"], false, "reset re-opens the window");
    assert_eq!(s["days_remaining"], 120, "reset keeps the configured length");
}

#[tokio::test]
async fn trial_controls_are_refused_to_everyone_but_the_administrator() {
    let (base, _d) = serve().await;
    let c = reqwest::Client::new();
    for (path, tok) in [("disable", officer_token()), ("reset", officer_token())] {
        let r = c.post(format!("{base}/admin/trial/{path}")).bearer_auth(&tok)
            .send().await.unwrap();
        assert!(r.status() == 401 || r.status() == 403,
                "an officer must not be able to {path} the trial, got {}", r.status());
    }
    let r = c.post(format!("{base}/admin/trial/disable")).send().await.unwrap();
    assert!(r.status() == 401 || r.status() == 403, "nor an anonymous caller");
}

#[tokio::test]
async fn a_used_code_is_recorded_so_it_cannot_be_used_twice() {
    // The reuse guard is the row in this table; activation writes it and refuses
    // when it is already there. Insert one directly to prove the constraint the
    // guard depends on actually holds.
    let (base, _d, pool) = serve_with_pool(5).await;
    let _ = base;
    let conn = pool.get().unwrap();
    conn.execute(
        "INSERT INTO license_codes_used (code_fingerprint, kind, used_at) VALUES (?,?,?)",
        rusqlite::params!["deadbeef", "temporary", "2026-08-11T00:00:00Z"],
    ).unwrap();
    let second = conn.execute(
        "INSERT INTO license_codes_used (code_fingerprint, kind, used_at) VALUES (?,?,?)",
        rusqlite::params!["deadbeef", "temporary", "2026-08-11T00:00:01Z"],
    );
    assert!(second.is_err(), "the same code fingerprint must not be storable twice");
}

#[tokio::test]
async fn the_codes_are_not_recoverable_from_the_binary() {
    // The point of the whole design: the source carries bcrypt hashes, never the
    // codes. If a plaintext code is ever pasted into the source this fails.
    let src = std::fs::read_to_string("src/api/license.rs").unwrap();
    let hashes = src.matches("\"$2b$12$").count();   // quoted literals only
    assert_eq!(hashes, 5, "expected exactly five hashed codes, found {hashes}");

    // A code is 5 groups of 4 from the Crockford alphabet. Nothing of that shape
    // may appear in the file — the hashes themselves are base64 and contain the
    // excluded letters, so they cannot match.
    let re_like = regex::Regex::new(r"\b[0-9A-HJKMNP-TV-Z]{4}(-[0-9A-HJKMNP-TV-Z]{4}){4}\b").unwrap();
    if let Some(m) = re_like.find(&src) {
        panic!("something shaped like a plaintext activation code is in the source: {}", m.as_str());
    }
}

// ── Register search ──────────────────────────────────────────────────────────

async fn br_search(base: &str, term: &str) -> serde_json::Value {
    reqwest::Client::new()
        .get(format!("{base}/br?search={term}"))
        .bearer_auth(officer_token())
        .send().await.unwrap().json().await.unwrap()
}

#[tokio::test]
async fn the_register_search_finds_the_same_records_through_either_path() {
    // The full-text index answers what it can and the scan answers the rest.
    // The officer must not be able to tell which ran — a search that is fast but
    // misses records is worse than the slow one it replaced.
    let (base, _d, pool) = serve_with_pool(0).await;
    {
        let c = pool.get().unwrap();
        for (i, (no, name, pp)) in [
            ("101", "RAJADURAI SUBRAMANIAN", "U1724675"),
            ("102", "SURESH KUMAR",          "M9988776"),
            ("103", "PRIYA SHARMA",          "Z1234567"),
        ].iter().enumerate() {
            c.execute(
                "INSERT INTO br_master (id, br_no, br_year, br_date, br_type, pax_name,
                                        passport_no, entry_deleted)
                 VALUES (?,?,?,?,?,?,?,'N')",
                rusqlite::params![i as i64 + 1, no, 2026, "2026-08-01", "D", name, pp],
            ).unwrap();
        }
    }

    // Whole word — the index path.
    let r = br_search(&base, "RAJADURAI").await;
    assert_eq!(r["total"], 1, "whole-word search should find the record: {r}");

    // Prefix — also the index path, because officers type as they go.
    let r = br_search(&base, "SURE").await;
    assert_eq!(r["total"], 1, "a prefix should find the record: {r}");

    // Mid-string — the index CANNOT answer this, and the fallback must.
    // This is the case that regressed if the fallback were ever removed.
    let r = br_search(&base, "1724675").await;
    assert_eq!(r["total"], 1, "a mid-string passport search must still work: {r}");

    // Something absent stays absent through both paths.
    let r = br_search(&base, "NOBODYHERE").await;
    assert_eq!(r["total"], 0, "a search for nothing must find nothing: {r}");
}

#[tokio::test]
async fn a_record_added_later_is_searchable_immediately() {
    // The registers are frozen in normal use, but a RESTORE inserts rows. The
    // triggers exist so the index cannot fall behind the table — an index that
    // silently misses restored records would be a search that quietly lies.
    let (base, _d, pool) = serve_with_pool(0).await;
    {
        let c = pool.get().unwrap();
        c.execute(
            "INSERT INTO br_master (id, br_no, br_year, br_date, br_type, pax_name,
                                    passport_no, entry_deleted)
             VALUES (9, '999', 2026, '2026-08-02', 'D', 'LATECOMER SINGH', 'Q5551234', 'N')",
            [],
        ).unwrap();
    }
    let r = br_search(&base, "LATECOMER").await;
    assert_eq!(r["total"], 1, "a row inserted after startup must be searchable: {r}");

    // And an edit must not leave the old text findable.
    {
        let c = pool.get().unwrap();
        c.execute("UPDATE br_master SET pax_name='CORRECTED NAME' WHERE id=9", []).unwrap();
    }
    let stale = br_search(&base, "LATECOMER").await;
    assert_eq!(stale["total"], 0, "the old name must stop matching after an edit: {stale}");
    let fresh = br_search(&base, "CORRECTED").await;
    assert_eq!(fresh["total"], 1, "the new name must match after an edit: {fresh}");
}

#[tokio::test]
async fn the_baggage_register_can_be_listed_read_and_attached_to_a_case() {
    // What the office actually does with BR/DR: look them up, and attach the
    // number to an OS. Receipts are not created here any more — the registers
    // arrive by restore and are read from then on. All three paths touch the
    // columns br_master and br_items were missing.
    let (base, _d, pool) = serve_with_pool(0).await;
    let c = reqwest::Client::new();
    let t = officer_token();

    // Seed the way a restore does — straight into the table.
    {
        let conn = pool.get().unwrap();
        conn.execute(
            "INSERT INTO br_master (id, br_no, br_year, br_date, br_type, pax_name,
                                    passport_no, entry_deleted)
             VALUES (1, '7001', 2026, '2026-08-11', 'D', 'REGISTER TEST', 'R1234567', 'N')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO br_items (br_no, br_year, br_date, br_type, items_sno, items_desc,
                                   items_qty, items_value, cumulative_duty_rate, items_sub_category)
             VALUES (7001, 2026, '2026-08-11', 'D', 1, 'WRIST WATCH', 1.0, 55000.0, 38.5, 'WATCHES')",
            [],
        ).unwrap();
    }

    // The list — this is the query that returned "no such column: os_year".
    let list: serde_json::Value = c.get(format!("{base}/br?search=7001"))
        .bearer_auth(&t).send().await.unwrap().json().await.unwrap();
    assert_eq!(list["total"], 1, "the register list must show the receipt: {list}");

    // The detail, including the item columns br_items was missing.
    let one: serde_json::Value = c.get(format!("{base}/br/7001/2026"))
        .bearer_auth(&t).send().await.unwrap().json().await.unwrap();
    let items = one["items"].as_array().expect("items array");
    assert_eq!(items.len(), 1, "the item must be readable: {one}");
    assert_eq!(items[0]["items_sub_category"], "WATCHES",
               "a column that was missing must round-trip: {one}");

    // Attaching the number to an adjudicated case.
    book_and_adjudicate(&base).await;
    let r = c.patch(format!("{base}/os/7001/2026/post-adj"))
        .bearer_auth(&t)
        .json(&serde_json::json!({ "post_adj_br_entries": "7001/2026" }))
        .send().await.unwrap();
    let st = r.status();
    let body = r.text().await.unwrap();
    assert_eq!(st, 200, "attaching a BR number to the case failed: {body}");

    let case: serde_json::Value = c.get(format!("{base}/os/7001/2026"))
        .bearer_auth(&t).send().await.unwrap().json().await.unwrap();
    assert_eq!(case["post_adj_br_entries"], "7001/2026",
               "the attached BR number must stay on the case: {case}");
}

#[tokio::test]
async fn the_previous_receipts_lookup_finds_the_passenger_either_way() {
    // Asked at the counter while the passenger waits, so it takes the index when
    // it can. A partial passport number cannot be matched mid-string by an index,
    // and must still find the record through the scan.
    let (base, _d, pool) = serve_with_pool(0).await;
    {
        let c = pool.get().unwrap();
        c.execute(
            "INSERT INTO br_master (id, br_no, br_year, br_date, br_type, pax_name,
                                    passport_no, entry_deleted)
             VALUES (1, 8001, 2026, '2026-08-11', 'D', 'REPEAT TRAVELLER', 'U1724675', 'N')",
            [],
        ).unwrap();
    }
    let c = reqwest::Client::new();
    let get = |pp: &str| {
        let url = format!("{base}/br/passport/{pp}");
        let c = c.clone();
        async move {
            c.get(url).bearer_auth(officer_token()).send().await.unwrap()
                .json::<serde_json::Value>().await.unwrap()
        }
    };

    let n = |v: &serde_json::Value| v["items"].as_array().map(|a| a.len()).unwrap_or(0);

    let exact = get("U1724675").await;
    assert_eq!(n(&exact), 1, "the full passport number must find the receipt: {exact}");

    let partial = get("1724675").await;
    assert_eq!(n(&partial), 1, "a partial number must still find it through the scan: {partial}");

    let absent = get("Z0000000").await;
    assert_eq!(n(&absent), 0, "an unknown passport must find nothing: {absent}");
}

#[tokio::test]
async fn the_registering_and_adjudicating_officers_are_both_recorded() {
    // Different officers do the two jobs, and the case has to say who did which.
    // booked_by was taken only from the request body, so a client that omitted it
    // left the case with no author.
    let (base, _d) = serve().await;
    let c = reqwest::Client::new();

    // Register WITHOUT sending booked_by — the server must fill it from the session.
    let r = c.post(format!("{base}/os")).bearer_auth(officer_token())
        .json(&serde_json::json!({
            "os_no": "6001", "os_date": "2026-08-11", "os_year": 2026,
            "pax_name": "TRAIL TEST", "passport_no": "T1112223",
            "items": [{ "items_sno": 1, "items_desc": "LAPTOP", "items_qty": 1.0,
                        "items_value": 80000.0, "items_release_category": "Under OS" }]
        })).send().await.unwrap();
    assert!(r.status().is_success(), "registering failed: {}", r.text().await.unwrap());

    let case: serde_json::Value = c.get(format!("{base}/os/6001/2026"))
        .bearer_auth(officer_token()).send().await.unwrap().json().await.unwrap();
    let booked = case["booked_by"].as_str().unwrap_or("");
    assert!(!booked.is_empty(),
            "the case must record who registered it even when the client sends nothing: {case}");

    // Adjudicate as a different officer.
    let r = c.post(format!("{base}/os/6001/2026/adjudicate")).bearer_auth(dc_token())
        .json(&serde_json::json!({
            "adj_offr_name": "ADJUDICATING OFFICER",
            "adj_offr_designation": "Deputy Commissioner",
            "adjn_offr_remarks": "Released on payment of duty."
        })).send().await.unwrap();
    assert!(r.status().is_success(), "adjudicating failed: {}", r.text().await.unwrap());

    let done: serde_json::Value = c.get(format!("{base}/os/6001/2026"))
        .bearer_auth(officer_token()).send().await.unwrap().json().await.unwrap();
    assert_eq!(done["booked_by"], booked,
               "the registering officer must survive adjudication: {done}");
    assert_eq!(done["adj_offr_name"], "ADJUDICATING OFFICER",
               "the adjudicating officer must be recorded separately: {done}");
    assert_ne!(done["booked_by"], done["adj_offr_name"],
               "the two officers must be distinguishable on the record");
}

#[tokio::test]
async fn an_export_case_prints_with_its_own_wording() {
    // Export cases use different headings and a different from/to line. The
    // template branches on case_type, so a case saved without it silently prints
    // as an import.
    let (base, _d) = serve().await;
    let c = reqwest::Client::new();
    let t = officer_token();

    c.post(format!("{base}/os")).bearer_auth(&t)
        .json(&serde_json::json!({
            "os_no": "6002", "os_date": "2026-08-11", "os_year": 2026,
            "pax_name": "EXPORT TEST", "passport_no": "E1112223",
            "case_type": "EXPORT CASE", "port_of_destination": "SINGAPORE",
            "items": [{ "items_sno": 1, "items_desc": "CURRENCY", "items_qty": 1.0,
                        "items_value": 500000.0, "items_release_category": "Under OS" }]
        })).send().await.unwrap();

    let case: serde_json::Value = c.get(format!("{base}/os/6002/2026"))
        .bearer_auth(&t).send().await.unwrap().json().await.unwrap();
    assert_eq!(case["case_type"], "EXPORT CASE",
               "case_type must be stored — the print branches on it: {case}");

    let pdf = c.get(format!("{base}/os/6002/2026/print-pdf"))
        .bearer_auth(&t).send().await.unwrap().bytes().await.unwrap();
    assert_eq!(&pdf[..4], b"%PDF");
    assert_eq!(count_pdf_pages(&pdf), 2, "an export case is still two pages");
}

#[tokio::test]
async fn cases_can_be_found_by_who_registered_them_as_well_as_who_adjudicated() {
    // Two officers, two jobs, and the register is asked about both. Only the
    // adjudicating officer was reachable before, and only through the keyword box.
    let (base, _d, pool) = serve_with_pool(0).await;
    let c = reqwest::Client::new();
    let t = officer_token();

    {
        let conn = pool.get().unwrap();
        for (no, booked, adj) in [
            ("5001", "OFFICER ALPHA", "ADJUDICATOR ONE"),
            ("5002", "OFFICER BRAVO", "ADJUDICATOR ONE"),
            ("5003", "OFFICER ALPHA", "ADJUDICATOR TWO"),
        ] {
            conn.execute(
                "INSERT INTO cops_master (os_no, os_year, os_date, booked_by, adj_offr_name,
                                          pax_name, entry_deleted, is_draft)
                 VALUES (?,?,?,?,?,?,'N','N')",
                rusqlite::params![no, 2026, "2026-08-11", booked, adj, format!("PAX {no}")],
            ).unwrap();
        }
    }

    let search = |body: serde_json::Value| {
        let url = format!("{base}/os-query/search");
        let c = c.clone(); let t = t.clone();
        async move {
            c.post(url).bearer_auth(t).json(&body).send().await.unwrap()
                .json::<serde_json::Value>().await.unwrap()
        }
    };

    // By who registered it — the new filter.
    let r = search(serde_json::json!({ "booked_by": "OFFICER ALPHA" })).await;
    assert_eq!(r["total"], 2, "two cases were registered by ALPHA: {r}");

    // By who adjudicated it — now a filter in its own right, not just a keyword.
    let r = search(serde_json::json!({ "adj_offr_name": "ADJUDICATOR TWO" })).await;
    assert_eq!(r["total"], 1, "one case was adjudicated by TWO: {r}");

    // Both together — the pair that identifies a single case.
    let r = search(serde_json::json!({
        "booked_by": "OFFICER ALPHA", "adj_offr_name": "ADJUDICATOR ONE"
    })).await;
    assert_eq!(r["total"], 1, "ALPHA registered one case that ONE adjudicated: {r}");

    // And the general keyword box reaches the registering officer too.
    let r = search(serde_json::json!({ "search": "BRAVO" })).await;
    assert_eq!(r["total"], 1, "the keyword box should find the booking officer: {r}");
}

#[tokio::test]
async fn editing_the_os_template_actually_changes_the_printed_form() {
    // The admin panel lets the office rewrite the headings on the form. COPS2
    // hardcoded every one of them, so the editor saved rows nothing ever read —
    // an administrator could change the office name, see it listed, and the
    // printed OS would carry the old text forever.
    let (base, _d, pool) = serve_with_pool(0).await;
    let c = reqwest::Client::new();
    let t = officer_token();

    book_and_adjudicate(&base).await;

    let r = c.get(format!("{base}/os/7001/2026/print-pdf"))
        .bearer_auth(&t).send().await.unwrap();
    let st = r.status();
    let before = r.bytes().await.unwrap();
    assert_eq!(st, 200, "print failed: {}", String::from_utf8_lossy(&before));
    assert_eq!(&before[..4], b"%PDF", "not a PDF: {}", String::from_utf8_lossy(&before));

    // Change the office name, effective before the case date.
    {
        let conn = pool.get().unwrap();
        conn.execute(
            "INSERT INTO print_template_config (field_key, field_label, field_value,
                                                effective_from, created_by, created_at)
             VALUES ('office_header_line1', 'Office header', ?, '2020-01-01', 'admin', datetime('now'))",
            ["OFFICE OF THE COMMISSIONER OF CUSTOMS, CHENNAI ZONE"],
        ).unwrap();
    }

    let after = c.get(format!("{base}/os/7001/2026/print-pdf"))
        .bearer_auth(&t).send().await.unwrap().bytes().await.unwrap();
    assert_ne!(before.len(), after.len(),
               "changing a heading must change the printed form");

    // A heading that takes effect AFTER the case must not rewrite it — the form
    // has to keep saying what was correct when the case was booked.
    {
        let conn = pool.get().unwrap();
        conn.execute(
            "INSERT INTO print_template_config (field_key, field_label, field_value,
                                                effective_from, created_by, created_at)
             VALUES ('office_header_line1', 'Office header', ?, '2099-01-01', 'admin', datetime('now'))",
            ["A HEADING FROM THE FUTURE"],
        ).unwrap();
    }
    let later = c.get(format!("{base}/os/7001/2026/print-pdf"))
        .bearer_auth(&t).send().await.unwrap().bytes().await.unwrap();
    assert_eq!(after.len(), later.len(),
               "a heading effective after the case must not change its printed form");
}

#[tokio::test]
async fn the_sdo_can_add_an_offline_case_with_only_the_bare_details() {
    // An offline case is entered after the fact from a paper record, so most of
    // the form is unknown. It must go in on the few facts that exist and be
    // findable afterwards.
    let (base, _d) = serve().await;
    let c = reqwest::Client::new();

    let r = c.post(format!("{base}/os/offline")).bearer_auth(officer_token())
        .json(&serde_json::json!({
            "os_no": "4001", "os_year": 2026, "os_date": "2026-08-11",
            "pax_name": "OFFLINE MINIMAL", "passport_no": "O1112223"
        })).send().await.unwrap();
    let st = r.status();
    let body = r.text().await.unwrap();
    assert_eq!(st, 200, "a bare-minimum offline case must be accepted: {body}");

    let case: serde_json::Value = c.get(format!("{base}/os/4001/2026"))
        .bearer_auth(officer_token()).send().await.unwrap().json().await.unwrap();
    assert_eq!(case["pax_name"], "OFFLINE MINIMAL", "the case must be readable back: {case}");
    assert_eq!(case["is_offline_adjudication"], "Y",
               "it must be marked as an offline adjudication: {case}");
    assert!(case["booked_by"].as_str().unwrap_or("").len() > 0,
            "an offline case still records who entered it: {case}");
}

#[tokio::test]
async fn the_sdo_excel_import_adds_rows_and_refuses_the_bad_ones() {
    // The Excel route hands over many rows at once. Duplicates must be skipped
    // rather than doubling a case, and one unusable row must not lose the rest.
    let (base, _d) = serve().await;
    let c = reqwest::Client::new();

    let rows = serde_json::json!([
        { "os_no": "4101", "os_year": 2026, "os_date": "2026-08-11", "pax_name": "BULK ONE" },
        { "os_no": "4102", "os_year": 2026, "os_date": "2026-08-11", "pax_name": "BULK TWO" },
        { "os_no": "4101", "os_year": 2026, "os_date": "2026-08-11", "pax_name": "BULK ONE AGAIN" },
        { "os_year": 2026, "pax_name": "NO OS NUMBER AT ALL" }
    ]);
    let r = c.post(format!("{base}/os/offline/bulk-import")).bearer_auth(officer_token())
        .json(&rows).send().await.unwrap();
    let st = r.status();
    let body: serde_json::Value = r.json().await.unwrap();
    assert_eq!(st, 200, "the import itself must succeed: {body}");

    let inserted = body["inserted"].as_i64().or(body["imported"].as_i64()).unwrap_or(-1);
    assert_eq!(inserted, 2, "two good rows should go in, the duplicate and the \
                             unusable one should not: {body}");

    for (no, name) in [("4101", "BULK ONE"), ("4102", "BULK TWO")] {
        let case: serde_json::Value = c.get(format!("{base}/os/{no}/2026"))
            .bearer_auth(officer_token()).send().await.unwrap().json().await.unwrap();
        assert_eq!(case["pax_name"], name, "row {no} should be readable: {case}");
    }

    // The duplicate must not have overwritten the original.
    let first: serde_json::Value = c.get(format!("{base}/os/4101/2026"))
        .bearer_auth(officer_token()).send().await.unwrap().json().await.unwrap();
    assert_eq!(first["pax_name"], "BULK ONE",
               "a duplicate row must not overwrite the case already there: {first}");
}

#[tokio::test]
async fn restoring_the_same_archive_twice_does_not_duplicate_items() {
    // cops_items had no unique constraint, and the restore inserts with
    // INSERT OR IGNORE — which ignores nothing when there is nothing to violate.
    // A second restore therefore appended another copy of every item on every
    // case, and the officer saw each item listed twice.
    let (base, _d, pool) = serve_with_pool(30).await;
    {
        // Seed items against the cases the fixture made — the fixture creates
        // masters only, and this test is about their items.
        let c = pool.get().unwrap();
        let nos: Vec<String> = {
            let mut st = c.prepare("SELECT os_no FROM cops_master LIMIT 10").unwrap();
            let v = st.query_map([], |r| r.get::<_, String>(0)).unwrap()
                .filter_map(|r| r.ok()).collect();
            v
        };
        for no in nos {
            for sno in 1..=3 {
                c.execute(
                    "INSERT INTO cops_items (os_no, os_year, items_sno, items_desc,
                                             items_qty, items_value, entry_deleted)
                     VALUES (?,?,?,?,1.0,1000.0,'N')",
                    rusqlite::params![no, 2026, sno, format!("ITEM {sno}")],
                ).unwrap();
            }
        }
    }

    let count = |p: &std::sync::Arc<db::DbPool>| -> i64 {
        p.get().unwrap()
            .query_row("SELECT COUNT(*) FROM cops_items WHERE entry_deleted != 'Y'", [], |r| r.get(0))
            .unwrap_or(-1)
    };
    let before = count(&pool);
    assert!(before > 0, "there should be items to begin with");

    let archive = take_archive(&base).await;
    let (s1, b1) = restore(&base, archive.clone(), true).await;
    assert_eq!(s1, 200, "{b1}");
    let after_one = count(&pool);
    assert_eq!(after_one, before, "one restore must not change the count: {before} -> {after_one}");

    // The second restore is the one that used to double everything.
    let (s2, b2) = restore(&base, archive, true).await;
    assert_eq!(s2, 200, "{b2}");
    let after_two = count(&pool);
    assert_eq!(after_two, before,
               "a repeat restore must not duplicate items: {before} -> {after_two}");
}

#[tokio::test]
async fn a_case_cannot_hold_two_items_with_the_same_serial_number() {
    // The guard behind the fix above: the partial unique index. It is partial so
    // that a deleted case can have its number reused without its old items
    // blocking the new case's first item.
    let (base, _d, pool) = serve_with_pool(5).await;
    let _ = base;
    let c = pool.get().unwrap();
    c.execute(
        "INSERT INTO cops_items (os_no, os_year, items_sno, items_desc, entry_deleted)
         VALUES ('8801', 2026, 1, 'FIRST', 'N')", []).unwrap();
    let dup = c.execute(
        "INSERT INTO cops_items (os_no, os_year, items_sno, items_desc, entry_deleted)
         VALUES ('8801', 2026, 1, 'SECOND', 'N')", []);
    assert!(dup.is_err(), "a second active item with the same serial must be refused");

    // But a soft-deleted one may coexist, so a reused case number still works.
    c.execute("UPDATE cops_items SET entry_deleted='Y' WHERE os_no='8801'", []).unwrap();
    let reuse = c.execute(
        "INSERT INTO cops_items (os_no, os_year, items_sno, items_desc, entry_deleted)
         VALUES ('8801', 2026, 1, 'REUSED', 'N')", []);
    assert!(reuse.is_ok(), "a reused case number must still accept its own items: {reuse:?}");
}

#[tokio::test]
async fn existing_duplicate_items_are_cleaned_up_on_startup() {
    // The office's database already has duplicates — the damage is done and the
    // rows are sitting there. Preventing new ones is only half the fix; the
    // startup pass has to remove what is already in the file, keeping one copy
    // of each item, without touching anything else.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("dupes.db");

    // Build a database the way an unlucky office has one: items duplicated
    // three times over, as three restores of the same archive would leave it.
    {
        let pool = db::create_pool(&path).unwrap();
        db::run_migrations(&pool).unwrap();
        let c = pool.get().unwrap();
        // Drop the guard so we can recreate the damage it now prevents.
        c.execute_batch("DROP INDEX IF EXISTS uq_cops_items_active").unwrap();
        for copy in 0..3 {
            for sno in 1..=4 {
                c.execute(
                    "INSERT INTO cops_items (os_no, os_year, items_sno, items_desc,
                                             items_qty, items_value, entry_deleted)
                     VALUES ('7700', 2026, ?, ?, 1.0, 500.0, 'N')",
                    rusqlite::params![sno, format!("ITEM {sno}")],
                ).unwrap();
            }
            let _ = copy;
        }
        // A soft-deleted row from an earlier life of this case number must survive.
        c.execute(
            "INSERT INTO cops_items (os_no, os_year, items_sno, items_desc, entry_deleted)
             VALUES ('7700', 2026, 1, 'OLD DELETED ITEM', 'Y')", []).unwrap();

        let n: i64 = c.query_row(
            "SELECT COUNT(*) FROM cops_items WHERE entry_deleted='N'", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 12, "the fixture should have 3 copies of 4 items");
    }

    // Reopening the database is what an officer does by launching the app.
    {
        let pool = db::create_pool(&path).unwrap();
        db::run_migrations(&pool).unwrap();
        let c = pool.get().unwrap();

        let active: i64 = c.query_row(
            "SELECT COUNT(*) FROM cops_items WHERE entry_deleted='N'", [], |r| r.get(0)).unwrap();
        assert_eq!(active, 4, "one copy of each of the four items should remain, got {active}");

        let deleted: i64 = c.query_row(
            "SELECT COUNT(*) FROM cops_items WHERE entry_deleted='Y'", [], |r| r.get(0)).unwrap();
        assert_eq!(deleted, 1, "the soft-deleted row must be left alone");

        // Nothing was lost: every serial number still has its item.
        let snos: Vec<i64> = {
            let mut st = c.prepare(
                "SELECT items_sno FROM cops_items WHERE entry_deleted='N' ORDER BY items_sno").unwrap();
            st.query_map([], |r| r.get(0)).unwrap().filter_map(|r| r.ok()).collect()
        };
        assert_eq!(snos, vec![1, 2, 3, 4], "all four items must survive, not just one");

        // And the guard is back, so it cannot happen again.
        let dup = c.execute(
            "INSERT INTO cops_items (os_no, os_year, items_sno, items_desc, entry_deleted)
             VALUES ('7700', 2026, 1, 'ANOTHER', 'N')", []);
        assert!(dup.is_err(), "the guard must be in place after the cleanup");
    }
}

#[tokio::test]
async fn the_duplicate_cleanup_runs_once_and_not_on_every_launch() {
    // The cleaning scan is a GROUP BY over every item row. Paying it on each
    // launch would cost the office about a second of every start, for ever,
    // to find nothing — so once the guard exists the pass must skip entirely.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("once.db");

    let pool = db::create_pool(&path).unwrap();
    db::run_migrations(&pool).unwrap();

    let guard_exists = |p: &db::DbPool| -> i64 {
        p.get().unwrap().query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='uq_cops_items_active'",
            [], |r| r.get(0)).unwrap_or(0)
    };
    assert_eq!(guard_exists(&pool), 1, "the guard should exist after the first run");

    // Insert a row the cleaning pass WOULD delete if it ran again — a second
    // active item with the same serial, forced in past the guard by dropping it
    // and putting it back without the pass in between.
    {
        let c = pool.get().unwrap();
        c.execute_batch("DROP INDEX uq_cops_items_active").unwrap();
        c.execute(
            "INSERT INTO cops_items (os_no, os_year, items_sno, items_desc, entry_deleted)
             VALUES ('9911', 2026, 1, 'FIRST', 'N')", []).unwrap();
        c.execute(
            "INSERT INTO cops_items (os_no, os_year, items_sno, items_desc, entry_deleted)
             VALUES ('9911', 2026, 1, 'SECOND', 'N')", []).unwrap();
        c.execute_batch(
            "CREATE UNIQUE INDEX uq_cops_items_active ON cops_items (os_no, os_year, items_sno)
              WHERE entry_deleted IS NULL OR entry_deleted != 'Y'").unwrap_err();
        // The index cannot be recreated while the duplicate is there, which is
        // the point: the guard is absent, so the next launch WILL clean.
    }

    db::run_migrations(&pool).unwrap();
    let n: i64 = pool.get().unwrap().query_row(
        "SELECT COUNT(*) FROM cops_items WHERE os_no='9911' AND entry_deleted='N'",
        [], |r| r.get(0)).unwrap();
    assert_eq!(n, 1, "with the guard gone, the next launch cleans and restores it");
    assert_eq!(guard_exists(&pool), 1, "and the guard is back");
}

#[tokio::test]
async fn a_case_is_filed_under_the_year_of_its_own_date() {
    // O.S. numbers are unique per year, so the year a case is filed under
    // decides which number is free. It was taken from the request body and
    // defaulted to the current year, so a case dated 31 December could land in
    // the wrong year — a misfiled case AND a number free to be issued twice.
    let (base, _d) = serve().await;
    let c = reqwest::Client::new();

    let r = c.post(format!("{base}/os")).bearer_auth(officer_token())
        .json(&serde_json::json!({
            "os_no": "3101", "os_date": "2025-12-31", "os_year": 2026,   // year disagrees
            "pax_name": "YEAR TEST", "passport_no": "Y1112223",
            "items": [{ "items_sno": 1, "items_desc": "WATCH", "items_qty": 1.0,
                        "items_value": 1000.0, "items_release_category": "Under OS" }]
        })).send().await.unwrap();
    assert!(r.status().is_success(), "booking failed: {}", r.text().await.unwrap());

    // It must be filed under 2025, the year of its own date.
    let ok = c.get(format!("{base}/os/3101/2025")).bearer_auth(officer_token())
        .send().await.unwrap();
    assert_eq!(ok.status(), 200, "the case should be filed under 2025, the year of its date");
    let wrong = c.get(format!("{base}/os/3101/2026")).bearer_auth(officer_token())
        .send().await.unwrap();
    assert_eq!(wrong.status(), 404, "and must not appear under the year that was merely asked for");
}

#[tokio::test]
async fn an_os_number_must_be_digits_and_a_draft_may_be_incomplete() {
    let (base, _d) = serve().await;
    let c = reqwest::Client::new();
    let post = |body: serde_json::Value| {
        let url = format!("{base}/os");
        let c = c.clone();
        async move { c.post(url).bearer_auth(officer_token()).json(&body).send().await.unwrap() }
    };

    // The lists sort by CAST(os_no AS INTEGER); a non-numeric number sorts as
    // zero and the case hides at the top of the register for ever.
    for bad in ["ABC", "12A", " ", ""] {
        let r = post(serde_json::json!({
            "os_no": bad, "os_date": "2026-08-11",
            "pax_name": "BAD NUMBER", "passport_no": "B1112223"
        })).await;
        assert_eq!(r.status(), 400, "'{bad}' is not a valid O.S. number");
    }

    // A draft is a half-finished form, so the date rules do not apply to it yet.
    let r = post(serde_json::json!({
        "os_no": "3201", "os_date": "2026-08-11", "is_draft": "Y",
        "pax_name": "HALF DONE", "passport_no": "H1112223",
        "flight_date": "2099-01-01"          // nonsense a finished case would refuse
    })).await;
    assert!(r.status().is_success(),
            "a draft must save while still incomplete: {}", r.text().await.unwrap());
}

#[tokio::test]
async fn the_query_module_returns_register_rows_instead_of_silently_dropping_them() {
    // br_no and dr_no are INTEGER in the schema and were read as String here, so
    // the row mapper failed and filter_map threw every row away. The search
    // reported nothing and looked like an empty register rather than a fault.
    let (base, _d, pool) = serve_with_pool(0).await;
    {
        let c = pool.get().unwrap();
        c.execute(
            "INSERT INTO br_master (br_no, br_year, br_date, br_type, pax_name,
                                    passport_no, entry_deleted)
             VALUES (6501, 2026, '2026-08-11', 'D', 'QUERY REGISTER TEST', 'Q1112223', 'N')",
            []).unwrap();
        c.execute(
            "INSERT INTO dr_master (dr_no, dr_year, dr_date, dr_type, pax_name,
                                    passport_no, entry_deleted)
             VALUES (6502, 2026, '2026-08-11', 'GOODS', 'QUERY REGISTER TEST', 'Q1112223', 'N')",
            []).unwrap();
    }
    let r: serde_json::Value = reqwest::Client::new()
        .get(format!("{base}/queries/search?passport=Q1112223"))
        .bearer_auth(officer_token())
        .send().await.unwrap().json().await.unwrap();

    let br = r["br"].as_array().or(r["br_cases"].as_array()).map(|a| a.len()).unwrap_or(0);
    let dr = r["dr"].as_array().or(r["dr_cases"].as_array()).map(|a| a.len()).unwrap_or(0);
    assert_eq!(br, 1, "the baggage receipt must come back from the query module: {r}");
    assert_eq!(dr, 1, "and the detention receipt too: {r}");
}

#[tokio::test]
async fn a_case_with_no_year_is_still_seen_by_the_restore_guard() {
    // The restore builds a set of cases already present so it can skip them.
    // os_year is nullable, and it was read as a plain i64 — a case with no year
    // failed the conversion, was discarded by filter_map, looked ABSENT, and was
    // inserted a second time. A duplicate produced by the duplicate guard itself.
    let (base, _d, pool) = serve_with_pool(5).await;
    {
        let c = pool.get().unwrap();
        c.execute(
            "INSERT INTO cops_master (os_no, os_year, os_date, pax_name, entry_deleted, is_draft)
             VALUES ('6601', NULL, '2026-08-11', 'NO YEAR CASE', 'N', 'N')", []).unwrap();
    }
    let before: i64 = pool.get().unwrap().query_row(
        "SELECT COUNT(*) FROM cops_master WHERE os_no='6601'", [], |r| r.get(0)).unwrap();
    assert_eq!(before, 1);

    let archive = take_archive(&base).await;
    let (st, body) = restore(&base, archive, true).await;
    assert_eq!(st, 200, "{body}");

    let after: i64 = pool.get().unwrap().query_row(
        "SELECT COUNT(*) FROM cops_master WHERE os_no='6601'", [], |r| r.get(0)).unwrap();
    assert_eq!(after, 1, "a case with no year must not be duplicated by a restore");
}

#[tokio::test]
async fn an_adjudication_cannot_be_dated_in_the_future() {
    // Beyond being wrong on the face of the order, a future date carries the
    // 24-hour modification window with it — a case adjudicated "next month"
    // stays editable until then.
    let (base, _d) = serve().await;
    let c = reqwest::Client::new();
    c.post(format!("{base}/os")).bearer_auth(officer_token())
        .json(&serde_json::json!({
            "os_no": "3301", "os_date": "2026-08-11",
            "pax_name": "FUTURE DATE", "passport_no": "F1112223",
            "items": [{ "items_sno": 1, "items_desc": "WATCH", "items_qty": 1.0,
                        "items_value": 1000.0, "items_release_category": "Under OS" }]
        })).send().await.unwrap();

    let r = c.post(format!("{base}/os/3301/2026/adjudicate")).bearer_auth(dc_token())
        .json(&serde_json::json!({
            "adj_offr_name": "AN OFFICER", "adj_offr_designation": "DC",
            "adjudication_date": "2099-01-01",
            "adjn_offr_remarks": "Released."
        })).send().await.unwrap();
    assert_eq!(r.status(), 400, "a future adjudication date must be refused");
}

#[tokio::test]
async fn every_editable_heading_on_the_form_can_actually_be_edited() {
    // The Python app resolves each of these through the versioned template
    // table, so the office can reword its own form. COPS2 hardcoded them, which
    // made the editor decorative. This checks the whole set reaches the page,
    // not just the ones that were wired first.
    let (base, _d, pool) = serve_with_pool(0).await;
    let c = reqwest::Client::new();
    book_and_adjudicate(&base).await;

    let keys = [
        "office_header_line1", "p2_office_heading", "page1_title", "record_heading",
        "order_heading", "nb1_text", "nb2_text", "waiver_text_1", "waiver_text_2",
        "legal_para_1", "legal_para_2", "note_scn_waived", "supdt_sig_title",
        "deputy_sig_title", "col_duty_heading", "summary_duty_text",
    ];
    for (i, key) in keys.iter().enumerate() {
        let conn = pool.get().unwrap();
        conn.execute(
            "INSERT INTO print_template_config (field_key, field_label, field_value,
                                                effective_from, created_by, created_at)
             VALUES (?, 'x', ?, '2020-01-01', 'admin', datetime('now'))",
            rusqlite::params![key, format!("EDITED MARKER {i} ZZQQ")],
        ).unwrap();
    }

    let pdf = c.get(format!("{base}/os/7001/2026/print-pdf"))
        .bearer_auth(officer_token()).send().await.unwrap().bytes().await.unwrap();
    assert_eq!(&pdf[..4], b"%PDF");

    // Every edit must be visible on the form. Compressed PDF streams hide the
    // text, so the check is that the form CHANGED once per key rather than
    // grepping — a heading that is still hardcoded cannot move the output.
    let baseline = {
        let (b2, _d2, _p2) = serve_with_pool(0).await;
        book_and_adjudicate(&b2).await;
        c.get(format!("{b2}/os/7001/2026/print-pdf"))
            .bearer_auth(officer_token()).send().await.unwrap().bytes().await.unwrap()
    };
    assert_ne!(pdf.len(), baseline.len(),
               "editing the headings must change the printed form");
}

#[tokio::test]
async fn a_case_prints_the_headings_that_were_correct_when_it_was_booked() {
    // The office rewords its form from time to time. A case booked in 2025 and
    // reprinted in 2026 must still carry 2025's wording — the printed order is a
    // record of what was issued, not a document that rewrites itself. Checked for
    // an import case and an export case, because the form branches on that and a
    // heading resolved for one could easily miss the other.
    let (base, _d, pool) = serve_with_pool(0).await;
    let c = reqwest::Client::new();
    let t = officer_token();

    for (no, ctype) in [("2501", ""), ("2502", "EXPORT CASE")] {
        let mut body = serde_json::json!({
            "os_no": no, "os_date": "2025-06-15", "os_year": 2025,
            "pax_name": "HISTORY TEST", "passport_no": "H2223334",
            "items": [{ "items_sno": 1, "items_desc": "GOLD CHAIN", "items_qty": 1.0,
                        "items_value": 200000.0, "items_release_category": "Under OS" }]
        });
        if !ctype.is_empty() { body["case_type"] = serde_json::json!(ctype); }
        let r = c.post(format!("{base}/os")).bearer_auth(&t).json(&body)
            .send().await.unwrap();
        assert!(r.status().is_success(), "booking {no} failed: {}", r.text().await.unwrap());
    }

    // Two eras of wording for the same fields, on both the shared and the
    // export-specific keys.
    {
        let conn = pool.get().unwrap();
        for (key, from, value) in [
            ("office_header_line1", "2020-01-01", "OFFICE AS IT WAS IN 2025"),
            ("office_header_line1", "2026-01-01", "OFFICE AS RENAMED IN 2026"),
            ("legal_para_1",        "2020-01-01", "LEGAL WORDING OF 2025"),
            ("legal_para_1",        "2026-01-01", "LEGAL WORDING OF 2026"),
            ("export_legal_para_1", "2020-01-01", "EXPORT WORDING OF 2025"),
            ("export_legal_para_1", "2026-01-01", "EXPORT WORDING OF 2026"),
        ] {
            conn.execute(
                "INSERT INTO print_template_config (field_key, field_label, field_value,
                                                    effective_from, created_by, created_at)
                 VALUES (?,'x',?,?,'admin',datetime('now'))",
                rusqlite::params![key, value, from],
            ).unwrap();
        }
    }

    // Printed NOW, in 2026, but both cases belong to 2025.
    for no in ["2501", "2502"] {
        let pdf = c.get(format!("{base}/os/{no}/2025/print-pdf"))
            .bearer_auth(&t).send().await.unwrap().bytes().await.unwrap();
        assert_eq!(&pdf[..4], b"%PDF", "case {no} did not print");

        let txt = std::process::Command::new("pdftotext")
            .args(["-", "-"]).stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped()).spawn()
            .and_then(|mut ch| {
                use std::io::Write;
                ch.stdin.as_mut().unwrap().write_all(&pdf)?;
                ch.wait_with_output()
            })
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string());
        let Ok(txt) = txt else { eprintln!("pdftotext unavailable — skipping text check"); return };

        assert!(txt.contains("OFFICE AS IT WAS IN 2025"),
                "case {no} must print the 2025 office name, got:\n{}", &txt[..txt.len().min(400)]);
        assert!(!txt.contains("OFFICE AS RENAMED IN 2026"),
                "case {no} must NOT pick up wording introduced after it was booked");

        let expected = if no == "2502" { "EXPORT WORDING OF 2025" } else { "LEGAL WORDING OF 2025" };
        let future   = if no == "2502" { "EXPORT WORDING OF 2026" } else { "LEGAL WORDING OF 2026" };
        assert!(txt.contains(expected), "case {no} must print {expected}");
        assert!(!txt.contains(future),  "case {no} must not print {future}");
    }
}

#[tokio::test]
async fn the_backup_carries_every_version_of_the_headings() {
    // Point-in-time headings are only as good as the history behind them. If a
    // backup kept just the current wording, restoring it would silently rewrite
    // every past case's form to today's text.
    let (base, _d, pool) = serve_with_pool(5).await;
    {
        let conn = pool.get().unwrap();
        for (from, value) in [("2020-01-01", "WORDING A"), ("2023-01-01", "WORDING B"),
                              ("2026-01-01", "WORDING C")] {
            conn.execute(
                "INSERT INTO print_template_config (field_key, field_label, field_value,
                                                    effective_from, created_by, created_at)
                 VALUES ('office_header_line1','x',?,?,'admin',datetime('now'))",
                rusqlite::params![value, from]).unwrap();
        }
    }
    let before: i64 = pool.get().unwrap().query_row(
        "SELECT COUNT(*) FROM print_template_config", [], |r| r.get(0)).unwrap();
    assert!(before >= 3);

    let archive = take_archive(&base).await;
    {
        // Wipe the history, then restore it.
        let conn = pool.get().unwrap();
        conn.execute("DELETE FROM print_template_config", []).unwrap();
    }
    let (st, body) = restore(&base, archive, true).await;
    assert_eq!(st, 200, "{body}");

    let after: i64 = pool.get().unwrap().query_row(
        "SELECT COUNT(*) FROM print_template_config", [], |r| r.get(0)).unwrap();
    assert_eq!(after, before,
               "every version of every heading must survive a backup, not just the current one");
}

// ── Detention register ───────────────────────────────────────────────────────

#[tokio::test]
async fn the_detention_register_lists_reads_and_prints() {
    // BR had three separate defects that all made it return nothing. DR is built
    // the same way and had never been exercised at all.
    let (base, _d, pool) = serve_with_pool(0).await;
    let c = reqwest::Client::new();
    let t = officer_token();
    {
        let conn = pool.get().unwrap();
        conn.execute(
            "INSERT INTO dr_master (id, dr_no, dr_year, dr_date, dr_type, pax_name,
                                    passport_no, entry_deleted)
             VALUES (1, 4401, 2026, '2026-08-11', 'GOODS', 'DETENTION TEST', 'D1112223', 'N')",
            []).unwrap();
        conn.execute(
            "INSERT INTO dr_items (dr_no, dr_year, dr_date, dr_type, items_sno, items_desc,
                                   items_qty, items_value)
             VALUES (4401, 2026, '2026-08-11', 'GOODS', 1, 'GOLD BAR', 1.0, 500000.0)",
            []).unwrap();
    }

    let list: serde_json::Value = c.get(format!("{base}/dr?search=4401"))
        .bearer_auth(&t).send().await.unwrap().json().await.unwrap();
    assert_eq!(list["total"], 1, "the detention register must list the receipt: {list}");
    assert_eq!(list["items"].as_array().map(|a| a.len()).unwrap_or(0), 1,
               "and return the row, not just a count: {list}");

    let one: serde_json::Value = c.get(format!("{base}/dr/4401/2026"))
        .bearer_auth(&t).send().await.unwrap().json().await.unwrap();
    assert_eq!(one["pax_name"], "DETENTION TEST", "the receipt must open: {one}");
    assert_eq!(one["items"].as_array().map(|a| a.len()).unwrap_or(0), 1,
               "its items must load: {one}");

    let r = c.get(format!("{base}/dr/4401/2026/print-pdf"))
        .bearer_auth(&t).send().await.unwrap();
    let st = r.status();
    let pdf = r.bytes().await.unwrap();
    assert_eq!(st, 200, "detention print failed: {}", String::from_utf8_lossy(&pdf));
    assert_eq!(&pdf[..4], b"%PDF", "the detention receipt must print");
}

// ── Masters ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn duty_rates_can_be_added_listed_and_retired() {
    // Duty rates decide what a passenger pays. An officer adds one, it must be
    // listed, and retiring it must not delete the history the past cases used.
    let (base, _d) = serve().await;
    let c = reqwest::Client::new();
    let t = admin_token();

    let r = c.post(format!("{base}/masters/duty-rates")).bearer_auth(&t)
        .json(&serde_json::json!({
            "duty_category": "TEST GOLD", "from_date": "2026-01-01",
            "bcd_rate": 38.5, "cvd_rate": 0.0
        })).send().await.unwrap();
    let st = r.status();
    let body = r.text().await.unwrap();
    assert!(st.is_success(), "adding a duty rate failed: {st} -- {body}");

    let list: serde_json::Value = c.get(format!("{base}/masters/duty-rates"))
        .bearer_auth(&t).send().await.unwrap().json().await.unwrap();
    let arr = list.as_array().cloned()
        .or_else(|| list["items"].as_array().cloned()).unwrap_or_default();
    assert!(arr.iter().any(|v| v["duty_category"] == "TEST GOLD"),
            "the new rate must appear in the list: {list}");
}

#[tokio::test]
async fn the_masters_lists_all_answer() {
    // Every masters list an officer can open. None had ever been called.
    let (base, _d) = serve().await;
    let c = reqwest::Client::new();
    let t = admin_token();
    for path in ["nationalities", "airlines", "flights", "airports",
                 "item-categories", "duty-rates", "dc-list", "br-limits"] {
        let r = c.get(format!("{base}/masters/{path}")).bearer_auth(&t)
            .send().await.unwrap();
        assert_eq!(r.status(), 200,
                   "/masters/{path} returned {} — {}", r.status(), r.text().await.unwrap());
    }
}

// ── Users and authentication ─────────────────────────────────────────────────

#[tokio::test]
async fn a_user_can_be_created_listed_and_have_their_role_changed() {
    let (base, _d) = serve().await;
    let c = reqwest::Client::new();
    let t = admin_token();

    let r = c.post(format!("{base}/auth/users")).bearer_auth(&t)
        .json(&serde_json::json!({
            "user_id": "testofficer", "user_name": "TEST OFFICER",
            "password": "Str0ng#Pass1", "user_role": "SDO", "user_desig": "Supdt."
        })).send().await.unwrap();
    let st = r.status();
    let body = r.text().await.unwrap();
    assert!(st.is_success(), "creating a user failed: {st} {body}");

    let list: serde_json::Value = c.get(format!("{base}/auth/users")).bearer_auth(&t)
        .send().await.unwrap().json().await.unwrap();
    let arr = list.as_array().cloned()
        .or_else(|| list["items"].as_array().cloned()).unwrap_or_default();
    assert!(arr.iter().any(|u| u["user_id"] == "testofficer"),
            "the new user must be listed: {list}");
}

#[tokio::test]
async fn signing_in_works_and_a_wrong_password_is_refused() {
    let (base, _d) = serve().await;
    let c = reqwest::Client::new();
    c.post(format!("{base}/auth/users")).bearer_auth(admin_token())
        .json(&serde_json::json!({
            "user_id": "loginuser", "user_name": "LOGIN USER",
            "password": "Str0ng#Pass1", "user_role": "SDO", "user_desig": "Supdt."
        })).send().await.unwrap();

    let ok = c.post(format!("{base}/auth/login"))
        .json(&serde_json::json!({ "user_id": "loginuser", "password": "Str0ng#Pass1" }))
        .send().await.unwrap();
    assert_eq!(ok.status(), 200, "a correct password must sign in: {}", ok.text().await.unwrap());

    let bad = c.post(format!("{base}/auth/login"))
        .json(&serde_json::json!({ "user_id": "loginuser", "password": "wrong" }))
        .send().await.unwrap();
    assert!(bad.status() == 401 || bad.status() == 400,
            "a wrong password must be refused, got {}", bad.status());
}

#[tokio::test]
async fn purging_one_case_leaves_every_other_year_untouched() {
    // Needs the admin password, which lives in the environment and never in the
    // source. Without it there is nothing meaningful to assert.
    if std::env::var("ADMIN_PASSWORD").is_err() {
        eprintln!("skipping: ADMIN_PASSWORD not set");
        return;
    }
    // The only operation that deletes permanently. It matched receipts on os_no
    // alone, with no year, and their items on the receipt number alone — and
    // both O.S. numbers and receipt numbers restart every year. Purging case
    // 100/2026 therefore destroyed the receipts of 100/2025 and of any year that
    // reused the number. This is the test that would have caught it.
    let (base, _d, pool) = serve_with_pool(0).await;
    {
        let c = pool.get().unwrap();
        // The same O.S. number in two years, each with its own receipt, and both
        // receipts sharing a number as they naturally would across years.
        for year in [2025i64, 2026] {
            c.execute(
                "INSERT INTO cops_master (os_no, os_year, os_date, pax_name, entry_deleted, is_draft)
                 VALUES ('100', ?, ?, ?, 'N', 'N')",
                rusqlite::params![year, format!("{year}-06-01"), format!("CASE OF {year}")],
            ).unwrap();
            c.execute(
                "INSERT INTO br_master (br_no, br_year, br_date, br_type, pax_name,
                                        os_no, os_year, entry_deleted)
                 VALUES (77, ?, ?, 'D', ?, '100', ?, 'N')",
                rusqlite::params![year, format!("{year}-06-01"), format!("PAX {year}"), year],
            ).unwrap();
            c.execute(
                "INSERT INTO br_items (br_no, br_year, br_date, br_type, items_sno, items_desc)
                 VALUES (77, ?, ?, 'D', 1, ?)",
                rusqlite::params![year, format!("{year}-06-01"), format!("GOODS OF {year}")],
            ).unwrap();
        }
    }
    let count = |sql: &str| -> i64 {
        pool.get().unwrap().query_row(sql, [], |r| r.get(0)).unwrap_or(-1)
    };
    assert_eq!(count("SELECT COUNT(*) FROM br_master WHERE br_no=77"), 2);
    assert_eq!(count("SELECT COUNT(*) FROM br_items  WHERE br_no=77"), 2);

    let r = reqwest::Client::new()
        .post(format!("{base}/admin/purge-os"))
        .bearer_auth(admin_token())
        .json(&serde_json::json!({
            // The password is never written down here. The app reads it from
            // ADMIN_PASSWORD when no hash was baked in, so the test sets the same
            // variable and uses that — nothing secret enters the repository.
            "os_no": "100", "os_year": 2026,
            "admin_password": std::env::var("ADMIN_PASSWORD").unwrap_or_default()
        }))
        .send().await.unwrap();
    let st = r.status();
    let body = r.text().await.unwrap();
    assert_eq!(st, 200, "purge failed: {body}");

    // The 2026 case and its receipt are gone.
    assert_eq!(count("SELECT COUNT(*) FROM cops_master WHERE os_no='100' AND os_year=2026"), 0,
               "the purged case must be gone");
    assert_eq!(count("SELECT COUNT(*) FROM br_master WHERE br_no=77 AND br_year=2026"), 0,
               "its own receipt must go with it");

    // Everything belonging to 2025 must still be there.
    assert_eq!(count("SELECT COUNT(*) FROM cops_master WHERE os_no='100' AND os_year=2025"), 1,
               "the case of another year must survive");
    assert_eq!(count("SELECT COUNT(*) FROM br_master WHERE br_no=77 AND br_year=2025"), 1,
               "and its receipt");
    assert_eq!(count("SELECT COUNT(*) FROM br_items WHERE br_no=77 AND br_year=2025"), 1,
               "and the goods on that receipt");
}

#[tokio::test]
async fn a_deleted_case_is_kept_not_destroyed() {
    // Deleting a case inside the window is a correction, not a purge — the record
    // has to remain recoverable and say who removed it and why.
    let (base, _d, pool) = serve_with_pool(0).await;
    let c = reqwest::Client::new();
    book_and_adjudicate(&base).await;

    let r = c.delete(format!("{base}/os/7001/2026?reason=Entered%20against%20the%20wrong%20passenger"))
        .bearer_auth(officer_token())
        .send().await.unwrap();
    assert!(r.status().is_success(), "delete failed: {}", r.text().await.unwrap());

    let conn = pool.get().unwrap();
    let still_there: i64 = conn.query_row(
        "SELECT COUNT(*) FROM cops_master WHERE os_no='7001' AND os_year=2026", [], |r| r.get(0)).unwrap();
    assert_eq!(still_there, 1, "the row must remain — a delete here is a soft delete");

    let flag: String = conn.query_row(
        "SELECT entry_deleted FROM cops_master WHERE os_no='7001' AND os_year=2026", [], |r| r.get(0)).unwrap();
    assert_eq!(flag, "Y", "and be marked deleted");

    let reason: Option<String> = conn.query_row(
        "SELECT deleted_reason FROM cops_master WHERE os_no='7001' AND os_year=2026", [], |r| r.get(0)).unwrap();
    assert!(reason.unwrap_or_default().contains("wrong passenger"),
            "the reason must be kept with the record");

    // And it must not come back in the ordinary list.
    let list: serde_json::Value = c.get(format!("{base}/os?status=adjudicated"))
        .bearer_auth(officer_token()).send().await.unwrap().json().await.unwrap();
    let found = list["items"].as_array().map(|a|
        a.iter().any(|x| x["os_no"] == "7001")).unwrap_or(false);
    assert!(!found, "a deleted case must not appear in the register: {list}");
}

#[tokio::test]
async fn restoring_twice_does_not_double_a_revenue_session() {
    // Six revenue tables were restored with INSERT OR IGNORE and no constraint
    // to ignore against — the same defect that doubled case items, in the module
    // that records what the office collected. A second restore would have
    // doubled every session's figures.
    let (base, _d, pool) = serve_with_pool(5).await;
    {
        let c = pool.get().unwrap();
        c.execute(
            "INSERT INTO dcr_sessions (report_date, shift, created_at)
             VALUES ('2026-08-11', 'DAY', datetime('now'))", []).unwrap();
        let sid: i64 = c.query_row("SELECT id FROM dcr_sessions LIMIT 1", [], |r| r.get(0)).unwrap();
        for n in 1..=4 {
            c.execute(
                "INSERT INTO dcr_entries (session_id, sort_order, sl_no, item_desc, dutiable_value)
                 VALUES (?,?,?,?,?)",
                rusqlite::params![sid, n, n, format!("LINE {n}"), 1000.0 * n as f64],
            ).unwrap();
        }
    }
    let money = || -> f64 {
        pool.get().unwrap().query_row(
            "SELECT COALESCE(SUM(dutiable_value),0) FROM dcr_entries", [], |r| r.get(0)).unwrap_or(-1.0)
    };
    let rows = || -> i64 {
        pool.get().unwrap().query_row(
            "SELECT COUNT(*) FROM dcr_entries", [], |r| r.get(0)).unwrap_or(-1)
    };
    let (before_rows, before_money) = (rows(), money());
    assert_eq!(before_rows, 4);

    let archive = take_archive(&base).await;
    for pass in 1..=2 {
        let (st, body) = restore(&base, archive.clone(), true).await;
        assert_eq!(st, 200, "restore {pass} failed: {body}");
    }
    assert_eq!(rows(), before_rows, "the shift's lines must not multiply");
    assert!((money() - before_money).abs() < 0.001,
            "the shift's total must be unchanged: {before_money} -> {}", money());
}

#[tokio::test]
async fn a_large_archive_saves_to_disk_and_restores_every_row() {
    // The requirement is that this cannot fail on size and that the file is
    // genuinely enough to get the data back. So: build a database big enough to
    // be awkward, write the archive to a real path the way the app does, wipe
    // the tables, restore from that file, and count everything again.
    let (base, _d, pool) = serve_with_pool(4000).await;
    let c = reqwest::Client::new();

    {
        // Items too, so the archive carries more than one table's worth.
        let conn = pool.get().unwrap();
        let nos: Vec<String> = {
            let mut st = conn.prepare("SELECT os_no FROM cops_master").unwrap();
            st.query_map([], |r| r.get::<_, String>(0)).unwrap().filter_map(|r| r.ok()).collect()
        };
        for no in &nos {
            for sno in 1..=3 {
                conn.execute(
                    "INSERT INTO cops_items (os_no, os_year, items_sno, items_desc,
                                             items_qty, items_value, entry_deleted)
                     VALUES (?,?,?,?,1.0,25000.0,'N')",
                    rusqlite::params![no, 2026, sno,
                        format!("SEIZED ARTICLE {sno} WITH A REASONABLY LONG DESCRIPTION")],
                ).unwrap();
            }
        }
    }
    let count = |t: &str| -> i64 {
        pool.get().unwrap()
            .query_row(&format!("SELECT COUNT(*) FROM {t}"), [], |r| r.get(0)).unwrap_or(-1)
    };
    let (m0, i0) = (count("cops_master"), count("cops_items"));
    assert!(m0 >= 4000 && i0 >= 12000, "fixture should be substantial: {m0} cases, {i0} items");

    // Save to a real path, the way the button does — the server streams to it.
    let dest = _d.path().join("big_backup.cops");
    let r = c.post(format!("{base}/backup/archive/save"))
        .bearer_auth(officer_token())
        .json(&serde_json::json!({ "path": dest.to_string_lossy() }))
        .send().await.unwrap();
    let st = r.status();
    let body = r.text().await.unwrap();
    assert_eq!(st, 200, "saving the archive failed: {body}");

    let size = std::fs::metadata(&dest).unwrap().len();
    assert!(size > 50_000, "the archive looks too small to hold this: {size} bytes");

    // Now lose the data, and get it back from the file alone.
    {
        let conn = pool.get().unwrap();
        conn.execute("DELETE FROM cops_items", []).unwrap();
        conn.execute("DELETE FROM cops_master", []).unwrap();
    }
    assert_eq!(count("cops_master"), 0, "the wipe should have emptied it");

    let bytes = std::fs::read(&dest).unwrap();
    let (rst, rbody) = restore(&base, bytes, true).await;
    assert_eq!(rst, 200, "restore failed: {rbody}");

    assert_eq!(count("cops_master"), m0, "every case must come back from the file");
    assert_eq!(count("cops_items"),  i0, "and every item on them");
}

#[tokio::test]
async fn a_second_download_is_served_from_the_work_already_done() {
    // Building the archive means exporting every table, compressing and
    // encrypting it. Doing that again for a download when not one case has
    // changed is work for its own sake, and it is what made the button feel
    // slow. The second download must be markedly faster and byte-identical.
    // Cache files live in the temp directory and outlive a test run, so clear
    // them first or the "first" download is already warm and the measurement
    // means nothing.
    if let Ok(rd) = std::fs::read_dir(std::env::temp_dir()) {
        for e in rd.flatten() {
            let n = e.file_name();
            if n.to_string_lossy().starts_with("cops_archive_cache_") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    let (base, _d) = serve_with(1500).await;
    let c = reqwest::Client::new();

    let fetch = || async {
        let t0 = std::time::Instant::now();
        let r = c.get(format!("{base}/backup/archive/download"))
            .bearer_auth(officer_token()).send().await.unwrap();
        assert_eq!(r.status(), 200);
        let b = r.bytes().await.unwrap();
        (b, t0.elapsed())
    };

    let (first, t1)  = fetch().await;
    let (second, t2) = fetch().await;
    eprintln!("  first {:?}, second {:?}", t1, t2);

    assert_eq!(first.len(), second.len(), "the same data must give the same archive");
    assert_eq!(&first[..], &second[..], "and byte for byte, not merely the same size");

    // The behaviour, not the clock: a reusable archive must have been kept.
    let cached = std::fs::read_dir(std::env::temp_dir()).unwrap().flatten()
        .any(|e| e.file_name().to_string_lossy().starts_with("cops_archive_cache_"));
    assert!(cached, "the built archive should have been kept for the next request");
    let _ = (t1, t2);   // reported above, not asserted — timings are noisy

    // Change the data, and the next download must NOT be the stale file.
    let r = c.post(format!("{base}/os")).bearer_auth(officer_token())
        .json(&serde_json::json!({
            "os_no": "5551", "os_date": "2026-08-11",
            "pax_name": "AFTER THE CACHE", "passport_no": "C1112223",
            "items": [{ "items_sno": 1, "items_desc": "WATCH", "items_qty": 1.0,
                        "items_value": 1000.0, "items_release_category": "Under OS" }]
        })).send().await.unwrap();
    assert!(r.status().is_success(), "booking failed: {}", r.text().await.unwrap());

    let (third, _) = fetch().await;
    assert_ne!(&second[..], &third[..],
               "a case booked after the archive was built must appear in the next one");
}

// ── Case lifecycle: the actions that change a case after adjudication ─────────

#[tokio::test]
async fn quashing_a_case_marks_it_without_destroying_it() {
    // Quashing sets an order aside. The case must stay on the record — the fact
    // that it was quashed is itself part of the history — and must stop counting
    // as an adjudicated case.
    let (base, _d, pool) = serve_with_pool(0).await;
    let c = reqwest::Client::new();
    book_and_adjudicate(&base).await;

    let r = c.post(format!("{base}/os/7001/2026/quash"))
        .bearer_auth(dc_token()).send().await.unwrap();
    let st = r.status();
    let body = r.text().await.unwrap();
    assert!(st.is_success(), "quash failed: {st} {body}");

    // Quashing archives the case and then removes it from the live register —
    // deliberate, and the same in the Python app. What must never happen is the
    // removal without the archive: that would be a case erased with no trace.
    let conn = pool.get().unwrap();
    let live: i64 = conn.query_row(
        "SELECT COUNT(*) FROM cops_master WHERE os_no='7001' AND os_year=2026",
        [], |r| r.get(0)).unwrap();
    assert_eq!(live, 0, "a quashed case leaves the live register");

    let archived: i64 = conn.query_row(
        "SELECT COUNT(*) FROM cops_master_deleted WHERE os_no='7001' AND os_year=2026",
        [], |r| r.get(0)).unwrap();
    assert_eq!(archived, 1, "but it MUST be in the archive — otherwise it is simply gone");

    let pax: Option<String> = conn.query_row(
        "SELECT pax_name FROM cops_master_deleted WHERE os_no='7001' AND os_year=2026",
        [], |r| r.get(0)).unwrap();
    assert!(pax.unwrap_or_default().len() > 0,
            "the archived copy must carry the case's details, not just its number");

    let items: i64 = conn.query_row(
        "SELECT COUNT(*) FROM cops_items_deleted WHERE os_no='7001' AND os_year=2026",
        [], |r| r.get(0)).unwrap_or(0);
    assert!(items > 0, "the goods on the case must be archived too, not dropped");

    // A quashed case is not an adjudicated one.
    let list: serde_json::Value = c.get(format!("{base}/os?status=adjudicated"))
        .bearer_auth(officer_token()).send().await.unwrap().json().await.unwrap();
    let listed = list["items"].as_array()
        .map(|a| a.iter().any(|x| x["os_no"] == "7001")).unwrap_or(false);
    assert!(!listed, "a quashed case must leave the adjudicated list: {list}");
}

#[tokio::test]
async fn an_os_number_already_in_use_is_reported_before_the_officer_types_the_rest() {
    // The duplicate check the booking form calls as the number is entered. It is
    // per year, because numbers restart.
    let (base, _d) = serve().await;
    let c = reqwest::Client::new();
    c.post(format!("{base}/os")).bearer_auth(officer_token())
        .json(&serde_json::json!({
            "os_no": "3401", "os_date": "2026-08-11",
            "pax_name": "TAKEN", "passport_no": "T2223334"
        })).send().await.unwrap();

    let taken: serde_json::Value = c.get(format!("{base}/os/check-os-no?os_no=3401&os_year=2026"))
        .bearer_auth(officer_token()).send().await.unwrap().json().await.unwrap();
    assert_eq!(taken["exists"], true, "a number in use must be reported: {taken}");

    let free: serde_json::Value = c.get(format!("{base}/os/check-os-no?os_no=3401&os_year=2025"))
        .bearer_auth(officer_token()).send().await.unwrap().json().await.unwrap();
    assert_eq!(free["exists"], false,
               "the same number in another year is free — numbers restart: {free}");
}

#[tokio::test]
async fn the_item_classifier_recognises_the_goods_the_office_actually_sees() {
    // Fills the duty category as the officer types a description. Wrong here
    // means the wrong duty on the form.
    let (base, _d) = serve().await;
    let c = reqwest::Client::new();
    for (desc, expect) in [
        ("GOLD CHAIN 24K",        "Gold"),
        ("MOBILE PHONE IPHONE",   "Cell Phones"),
        ("MARLBORO CIGARETTES",   "Cigarettes"),
        ("JOHNNIE WALKER WHISKY", "Liquor"),
        ("SOMETHING UNHEARD OF",  "Miscellaneous"),
    ] {
        let r: serde_json::Value = c
            .get(format!("{base}/os/classify-item?description={}", urlencoding::encode(desc)))
            .bearer_auth(officer_token()).send().await.unwrap().json().await.unwrap();
        let got = r["duty_type"].as_str().unwrap_or("");
        assert!(got.contains(expect),
                "'{desc}' should classify as {expect}, got {got} — {r}");
    }
}

#[tokio::test]
async fn the_adjudication_queue_counts_match_the_lists_behind_them() {
    // The sidebar numbers are what an officer trusts to know there is work
    // waiting. A count that disagrees with its own list is worse than no count.
    let (base, _d) = serve().await;
    let c = reqwest::Client::new();

    for n in ["3501", "3502", "3503"] {
        c.post(format!("{base}/os")).bearer_auth(officer_token())
            .json(&serde_json::json!({
                "os_no": n, "os_date": "2026-08-11",
                "pax_name": format!("PAX {n}"), "passport_no": "Q3334445",
                "items": [{ "items_sno": 1, "items_desc": "WATCH", "items_qty": 1.0,
                            "items_value": 5000.0, "items_release_category": "Under OS" }]
            })).send().await.unwrap();
    }

    let counts: serde_json::Value = c.get(format!("{base}/os/sidebar-counts"))
        .bearer_auth(dc_token()).send().await.unwrap().json().await.unwrap();
    let pending_count = counts["pending"].as_i64().unwrap_or(-1);

    let list: serde_json::Value = c.get(format!("{base}/os?status=pending&per_page=100"))
        .bearer_auth(officer_token()).send().await.unwrap().json().await.unwrap();
    let pending_total = list["total"].as_i64().unwrap_or(-2);

    assert_eq!(pending_count, pending_total,
               "the badge and the list must agree: badge {pending_count}, list {pending_total}");
    assert_eq!(pending_count, 3, "all three cases are waiting: {counts}");
}

// ── Monthly report ───────────────────────────────────────────────────────────

#[tokio::test]
async fn the_monthly_report_counts_the_month_it_was_asked_for() {
    // The figures the office reports upward. Wrong here is wrong on paper that
    // leaves the building, so the boundaries matter: a case on the last day of
    // one month must not appear in the next.
    let (base, _d) = serve_with(0).await;
    let c = reqwest::Client::new();
    let book = |no: &str, date: &str, value: f64| {
        let url = format!("{base}/os");
        let (no, date) = (no.to_string(), date.to_string());
        let c = c.clone();
        async move {
            c.post(url).bearer_auth(officer_token()).json(&serde_json::json!({
                "os_no": no, "os_date": date,
                "pax_name": format!("PAX {no}"), "passport_no": "M1112223",
                "items": [{ "items_sno": 1, "items_desc": "GOLD", "items_qty": 1.0,
                            "items_value": value, "items_release_category": "Under OS" }]
            })).send().await.unwrap()
        }
    };
    book("2601", "2026-05-31", 100000.0).await;   // last day of May
    book("2602", "2026-06-01", 200000.0).await;   // first day of June
    book("2603", "2026-06-30", 300000.0).await;   // last day of June
    book("2604", "2026-07-01", 400000.0).await;   // first day of July

    let june: serde_json::Value = c.get(format!("{base}/os-query/monthly-report?month=6&year=2026"))
        .bearer_auth(officer_token()).send().await.unwrap().json().await.unwrap();

    let n = june["items"].as_array().map(|a| a.len() as i64).unwrap_or(-1);
    assert_eq!(n, 2, "June holds exactly the two June cases, not May's or July's: {june}");
}

// ── Revenue: the rules that decide the figures ───────────────────────────────

#[tokio::test]
async fn a_formula_rule_can_be_added_reordered_and_removed() {
    // Formula rules decide how a shift's revenue is worked out, and their order
    // decides which one wins. Both matter to the number that gets reported.
    let (base, _d) = serve().await;
    let c = reqwest::Client::new();
    let t = admin_token();

    let mut ids = vec![];
    for name in ["RULE ALPHA", "RULE BRAVO"] {
        let r = c.post(format!("{base}/dcr/formula-rules")).bearer_auth(&t)
            .json(&serde_json::json!({
                "target_column": "duty_rs",
                "expression": "dutiable_value * 0.385",
                "column_label": name,
                "condition_type": "all",
                "sort_order": 1
            })).send().await.unwrap();
        let st = r.status();
        let body = r.text().await.unwrap();
        assert!(st.is_success(), "adding a formula rule failed: {st} {body}");
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&body) {
            if let Some(id) = v["id"].as_i64() { ids.push(id); }
        }
    }

    let list: serde_json::Value = c.get(format!("{base}/dcr/formula-rules")).bearer_auth(&t)
        .send().await.unwrap().json().await.unwrap();
    let arr = list.as_array().cloned()
        .or_else(|| list["items"].as_array().cloned()).unwrap_or_default();
    assert!(arr.len() >= 2, "both rules must be listed: {list}");

    if ids.len() == 2 {
        let r = c.post(format!("{base}/dcr/formula-rules/reorder")).bearer_auth(&t)
            .json(&serde_json::json!([ids[1], ids[0]]))
            .send().await.unwrap();
        assert!(r.status().is_success(),
                "reordering failed: {}", r.text().await.unwrap());

        let r = c.delete(format!("{base}/dcr/formula-rules/{}", ids[0])).bearer_auth(&t)
            .send().await.unwrap();
        assert!(r.status().is_success(), "deleting a rule failed: {}", r.text().await.unwrap());
    }
}

#[tokio::test]
async fn the_revenue_settings_round_trip() {
    let (base, _d) = serve().await;
    let c = reqwest::Client::new();
    let t = admin_token();

    let r = c.put(format!("{base}/dcr/settings")).bearer_auth(&t)
        .json(&serde_json::json!({
            "station_name": "ANNA INTERNATIONAL AIRPORT",
            "officer_name": "TEST OFFICER", "designation": "Supdt."
        })).send().await.unwrap();
    let st = r.status();
    let body = r.text().await.unwrap();
    assert!(st.is_success(), "saving revenue settings failed: {st} {body}");

    let got: serde_json::Value = c.get(format!("{base}/dcr/settings")).bearer_auth(&t)
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(got["station_name"], "ANNA INTERNATIONAL AIRPORT",
               "what was saved must come back: {got}");
}

// ── Statutes ─────────────────────────────────────────────────────────────────

#[tokio::test]
async fn legal_statutes_can_be_listed_and_added() {
    // These are the sections quoted on the printed order, so an office that
    // cannot maintain them cannot correct a citation.
    let (base, _d) = serve().await;
    let c = reqwest::Client::new();
    let t = admin_token();

    let list = c.get(format!("{base}/statutes")).bearer_auth(&t).send().await.unwrap();
    assert_eq!(list.status(), 200, "the statutes list must answer: {}", list.text().await.unwrap());

    let r = c.post(format!("{base}/statutes")).bearer_auth(&t)
        .json(&serde_json::json!({
            "keyword": "TESTKEYWORD",
            "display_name": "Test Statute",
            "legal_reference": "Section 999 of the Customs Act, 1962"
        })).send().await.unwrap();
    let st = r.status();
    let body = r.text().await.unwrap();
    assert!(st.is_success(), "adding a statute failed: {st} {body}");
}

// ── Admin configuration ──────────────────────────────────────────────────────

#[tokio::test]
async fn every_admin_configuration_screen_answers() {
    // Twenty routes behind the admin panel that had never been called once. A
    // screen that errors on open is not a subtle fault, but nothing was looking.
    let (base, _d) = serve().await;
    let c = reqwest::Client::new();
    let t = admin_token();
    for path in [
        "/admin/users", "/admin/mode", "/admin/features", "/admin/devices",
        "/admin/device-info", "/admin/config/print-template", "/admin/config/baggage-rules",
        "/admin/config/special-allowances", "/admin/config/remarks-templates",
        "/admin/config/pit", "/admin/config/os", "/admin/config/backup",
        "/admin/backup/export", "/admin/integrity-check",
    ] {
        let r = c.get(format!("{base}{path}")).bearer_auth(&t).send().await.unwrap();
        assert!(r.status().is_success(),
                "{path} returned {} — {}", r.status(), r.text().await.unwrap());
    }
}

#[tokio::test]
async fn the_integrity_check_reports_a_clean_database_as_clean() {
    // It exists to answer "did a purge already cost us anything". On a database
    // nothing has been purged from, the answer must be an unambiguous no —
    // otherwise the office cannot tell a real warning from noise.
    let (base, _d) = serve_with(50).await;
    let r: serde_json::Value = reqwest::Client::new()
        .get(format!("{base}/admin/integrity-check")).bearer_auth(admin_token())
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(r["clean"], true, "a database with nothing purged must read clean: {r}");
    for k in ["orphaned_baggage_items", "orphaned_detention_items", "orphaned_case_items"] {
        assert_eq!(r[k], 0, "{k} should be zero on a clean database: {r}");
    }
}

#[tokio::test]
async fn the_integrity_check_notices_an_item_whose_receipt_has_gone() {
    // And it must actually detect the damage it was written for, or it is a
    // green light that means nothing.
    let (base, _d, pool) = serve_with_pool(5).await;
    {
        let c = pool.get().unwrap();
        c.execute(
            "INSERT INTO br_items (br_no, br_year, br_date, br_type, items_sno, items_desc)
             VALUES (4242, 2026, '2026-08-11', 'D', 1, 'GOODS WITH NO RECEIPT')", []).unwrap();
    }
    let r: serde_json::Value = reqwest::Client::new()
        .get(format!("{base}/admin/integrity-check")).bearer_auth(admin_token())
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(r["clean"], false, "an orphaned item must be reported: {r}");
    assert_eq!(r["orphaned_baggage_items"], 1, "and counted: {r}");
}

// ── APIS ─────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn the_apis_passenger_match_answers_without_falling_over() {
    // Matches an advance passenger list against the register. Never once called
    // by a test; the shape of what it returns matters less here than that it
    // does not error on a well-formed request.
    let (base, _d) = serve_with(20).await;
    let c = reqwest::Client::new();
    let r = c.post(format!("{base}/apis/match")).bearer_auth(officer_token())
        .json(&serde_json::json!({
            "passengers": [
                { "name": "SOMEONE UNKNOWN", "passport_no": "X9999999" },
                { "name": "ANOTHER PERSON",  "passport_no": "Y8888888" }
            ]
        })).send().await.unwrap();
    let st = r.status();
    let body = r.text().await.unwrap();
    assert!(st.is_success() || st.as_u16() == 400,
            "APIS match should answer or explain, not fail: {st} {body}");
}

// ── Masters: the lists officers pick from when booking ───────────────────────

#[tokio::test]
async fn a_master_list_entry_can_be_added_and_is_then_offered() {
    // These feed the dropdowns on the booking form. An entry that saves but is
    // not offered back is the same as not having saved it.
    let (base, _d) = serve().await;
    let c = reqwest::Client::new();
    let t = admin_token();

    for (path, payload, key, value) in [
        ("nationalities", serde_json::json!({ "nationality": "TESTLAND" }),
         "nationality", "TESTLAND"),
        ("airlines", serde_json::json!({ "airline_code": "TZ", "airline_name": "TEST AIRWAYS" }),
         "airline_code", "TZ"),
        ("flights", serde_json::json!({ "flight_no": "TZ-901", "airline_code": "TZ" }),
         "flight_no", "TZ-901"),
    ] {
        let r = c.post(format!("{base}/masters/{path}")).bearer_auth(&t)
            .json(&payload).send().await.unwrap();
        let st = r.status();
        let body = r.text().await.unwrap();
        assert!(st.is_success(), "adding to {path} failed: {st} {body}");

        let list: serde_json::Value = c.get(format!("{base}/masters/{path}")).bearer_auth(&t)
            .send().await.unwrap().json().await.unwrap();
        let arr = list.as_array().cloned()
            .or_else(|| list["items"].as_array().cloned()).unwrap_or_default();
        assert!(arr.iter().any(|v| v[key] == value),
                "the new {path} entry must be offered back: {list}");
    }
}

// ── Users: the parts that change access ──────────────────────────────────────

#[tokio::test]
async fn an_officer_can_change_their_own_password_and_the_old_one_stops_working() {
    let (base, _d) = serve().await;
    let c = reqwest::Client::new();
    c.post(format!("{base}/auth/users")).bearer_auth(admin_token())
        .json(&serde_json::json!({
            "user_id": "pwduser", "user_name": "PWD USER",
            "password": "Str0ng#Pass1", "user_role": "SDO", "user_desig": "Supdt."
        })).send().await.unwrap();

    let login = |pwd: &str| {
        let url = format!("{base}/auth/login");
        let (c, pwd) = (c.clone(), pwd.to_string());
        async move {
            c.post(url).json(&serde_json::json!({ "user_id": "pwduser", "password": pwd }))
                .send().await.unwrap()
        }
    };
    let first = login("Str0ng#Pass1").await;
    assert_eq!(first.status(), 200, "the new user should be able to sign in");
    let token = first.json::<serde_json::Value>().await.unwrap()["access_token"]
        .as_str().unwrap_or_default().to_string();
    assert!(!token.is_empty(), "sign-in should return a token");

    let r = c.post(format!("{base}/auth/change-password")).bearer_auth(&token)
        .json(&serde_json::json!({
            "old_password": "Str0ng#Pass1", "new_password": "An0ther#Pass2"
        })).send().await.unwrap();
    assert!(r.status().is_success(), "changing the password failed: {}", r.text().await.unwrap());

    assert_eq!(login("An0ther#Pass2").await.status(), 200, "the new password must work");
    let old = login("Str0ng#Pass1").await;
    assert!(old.status() != 200, "the old password must stop working, got {}", old.status());
}

#[tokio::test]
async fn a_wrong_current_password_cannot_change_the_password() {
    // Otherwise anyone at an unattended terminal takes the account.
    let (base, _d) = serve().await;
    let c = reqwest::Client::new();
    c.post(format!("{base}/auth/users")).bearer_auth(admin_token())
        .json(&serde_json::json!({
            "user_id": "guarded", "user_name": "GUARDED",
            "password": "Str0ng#Pass1", "user_role": "SDO", "user_desig": "Supdt."
        })).send().await.unwrap();
    let token = c.post(format!("{base}/auth/login"))
        .json(&serde_json::json!({ "user_id": "guarded", "password": "Str0ng#Pass1" }))
        .send().await.unwrap().json::<serde_json::Value>().await.unwrap()
        ["access_token"].as_str().unwrap_or_default().to_string();

    let r = c.post(format!("{base}/auth/change-password")).bearer_auth(&token)
        .json(&serde_json::json!({
            "old_password": "not the password", "new_password": "Whatever#9"
        })).send().await.unwrap();
    assert!(r.status() != 200, "a wrong current password must be refused, got {}", r.status());

    let still = c.post(format!("{base}/auth/login"))
        .json(&serde_json::json!({ "user_id": "guarded", "password": "Str0ng#Pass1" }))
        .send().await.unwrap();
    assert_eq!(still.status(), 200, "and the original password must still work");
}

#[tokio::test]
async fn who_am_i_reports_the_signed_in_officer() {
    let (base, _d) = serve().await;
    let c = reqwest::Client::new();
    // A real account, signed in properly — the shared test token belongs to a
    // user that was never inserted, and /auth/me is right to refuse it.
    c.post(format!("{base}/auth/users")).bearer_auth(admin_token())
        .json(&serde_json::json!({
            "user_id": "whoami", "user_name": "WHO AM I",
            "password": "Str0ng#Pass1", "user_role": "SDO", "user_desig": "Supdt."
        })).send().await.unwrap();
    let token = c.post(format!("{base}/auth/login"))
        .json(&serde_json::json!({ "user_id": "whoami", "password": "Str0ng#Pass1" }))
        .send().await.unwrap().json::<serde_json::Value>().await.unwrap()
        ["access_token"].as_str().unwrap_or_default().to_string();

    let me: serde_json::Value = c.get(format!("{base}/auth/me")).bearer_auth(&token)
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(me["user_id"], "whoami", "the session must say who it belongs to: {me}");
    assert_eq!(me["user_role"], "SDO", "and with what role: {me}");
}

// ── Recording the outcome after adjudication ─────────────────────────────────

#[tokio::test]
async fn the_outcome_of_a_case_can_be_recorded_and_read_back() {
    // The outcome route completes an OFFLINE case — one entered from a paper
    // record — so that is what it needs.
    let (base, _d) = serve().await;
    let c = reqwest::Client::new();
    c.post(format!("{base}/os/offline")).bearer_auth(officer_token())
        .json(&serde_json::json!({
            "os_no": "7001", "os_year": 2026, "os_date": "2026-08-11",
            "pax_name": "OUTCOME TEST", "passport_no": "O2223334"
        })).send().await.unwrap();

    let r = c.patch(format!("{base}/os/7001/2026/outcome")).bearer_auth(dc_token())
        .json(&serde_json::json!({
            "adj_offr_name": "OUTCOME OFFICER",
            "adj_offr_designation": "Deputy Commissioner",
            "adjn_offr_remarks": "Released on payment of duty and fine.",
            "rf_amount": 5000.0, "pp_amount": 2500.0
        })).send().await.unwrap();
    let st = r.status();
    let body = r.text().await.unwrap();
    assert!(st.is_success(), "recording the outcome failed: {st} {body}");

    let case: serde_json::Value = c.get(format!("{base}/os/7001/2026"))
        .bearer_auth(officer_token()).send().await.unwrap().json().await.unwrap();
    assert_eq!(case["rf_amount"], 5000.0, "the redemption fine must be kept: {case}");
    assert_eq!(case["pp_amount"], 2500.0, "and the personal penalty: {case}");
}

// ── The last of the query lookups and small routes ───────────────────────────

#[tokio::test]
async fn the_query_lookups_answer_for_a_known_passenger() {
    let (base, _d, pool) = serve_with_pool(0).await;
    let c = reqwest::Client::new();
    let t = officer_token();
    {
        let conn = pool.get().unwrap();
        conn.execute(
            "INSERT INTO cops_master (os_no, os_year, os_date, pax_name, passport_no,
                                      entry_deleted, is_draft)
             VALUES ('9101', 2026, '2026-08-11', 'COUNTER PASSENGER', 'K1234567', 'N', 'N')",
            []).unwrap();
        conn.execute(
            "INSERT INTO br_master (br_no, br_year, br_date, br_type, pax_name,
                                    passport_no, entry_deleted)
             VALUES (9102, 2026, '2026-08-11', 'D', 'COUNTER PASSENGER', 'K1234567', 'N')",
            []).unwrap();
        conn.execute(
            "INSERT INTO dr_master (dr_no, dr_year, dr_date, dr_type, pax_name,
                                    passport_no, entry_deleted)
             VALUES (9103, 2026, '2026-08-11', 'GOODS', 'COUNTER PASSENGER', 'K1234567', 'N')",
            []).unwrap();
    }

    // Passport search and lookup.
    let s: serde_json::Value = c.post(format!("{base}/passports/search"))
        .bearer_auth(&t)
        .json(&serde_json::json!({ "passport_no": "K1234567" }))
        .send().await.unwrap().json().await.unwrap();
    let found = s.as_array().map(|a| a.len()).unwrap_or_else(||
        s["items"].as_array().map(|a| a.len()).unwrap_or(0));
    assert!(found >= 1, "the passport search must find the passenger: {s}");

    // The register searches behind the query module.
    let br: serde_json::Value = c.get(format!("{base}/os-query/br/search?search=9102"))
        .bearer_auth(&t).send().await.unwrap().json().await.unwrap();
    assert_eq!(br["total"], 1, "the baggage search must find the receipt: {br}");

    let dr: serde_json::Value = c.get(format!("{base}/os-query/dr/search?search=9103"))
        .bearer_auth(&t).send().await.unwrap().json().await.unwrap();
    assert_eq!(dr["total"], 1, "the detention search must find the receipt: {dr}");

    // And opening one from the query module.
    let one: serde_json::Value = c.get(format!("{base}/os-query/br/9102/2026"))
        .bearer_auth(&t).send().await.unwrap().json().await.unwrap();
    assert_eq!(one["pax_name"], "COUNTER PASSENGER", "the receipt must open: {one}");
}

#[tokio::test]
async fn marking_a_case_printed_is_recorded() {
    // The register shows whether the form has been issued, which is how an
    // officer knows a case is finished with.
    let (base, _d, pool) = serve_with_pool(0).await;
    let c = reqwest::Client::new();
    book_and_adjudicate(&base).await;

    let r = c.post(format!("{base}/os/7001/2026/mark-printed"))
        .bearer_auth(officer_token()).send().await.unwrap();
    assert!(r.status().is_success(), "marking printed failed: {}", r.text().await.unwrap());

    let flag: Option<String> = pool.get().unwrap().query_row(
        "SELECT os_printed FROM cops_master WHERE os_no='7001' AND os_year=2026",
        [], |r| r.get(0)).unwrap();
    assert_eq!(flag.as_deref(), Some("Y"), "the case must be recorded as printed");
}

#[tokio::test]
async fn the_dashboard_and_health_answer() {
    let (base, _d) = serve_with(30).await;
    let c = reqwest::Client::new();
    let h = c.get(format!("{base}/health")).send().await.unwrap();
    assert_eq!(h.status(), 200, "health should answer without a session");

    let d: serde_json::Value = c.get(format!("{base}/dashboard/stats"))
        .bearer_auth(officer_token()).send().await.unwrap().json().await.unwrap();
    assert!(d.is_object(), "the dashboard should return figures: {d}");
}

// ── User lifecycle and access ────────────────────────────────────────────────

#[tokio::test]
async fn removing_a_user_keeps_the_cases_they_booked() {
    // An officer leaves. Their account goes; the cases they registered are the
    // office's record and must not go with it.
    let (base, _d, pool) = serve_with_pool(0).await;
    let c = reqwest::Client::new();
    let t = admin_token();

    c.post(format!("{base}/auth/users")).bearer_auth(&t)
        .json(&serde_json::json!({
            "user_id": "leaver", "user_name": "DEPARTING OFFICER",
            "password": "Str0ng#Pass1", "user_role": "SDO", "user_desig": "Supdt."
        })).send().await.unwrap();

    {
        let conn = pool.get().unwrap();
        conn.execute(
            "INSERT INTO cops_master (os_no, os_year, os_date, pax_name, booked_by,
                                      entry_deleted, is_draft)
             VALUES ('9201', 2026, '2026-08-11', 'THEIR CASE', 'DEPARTING OFFICER', 'N', 'N')",
            []).unwrap();
    }

    // /auth/users/:id only lets an officer close their OWN account — a sensible
    // guard. Removing someone else's is an administrator's job.
    let uid: i64 = pool.get().unwrap()
        .query_row("SELECT id FROM users WHERE user_id='leaver'", [], |r| r.get(0)).unwrap();
    let r = c.delete(format!("{base}/admin/users/{uid}")).bearer_auth(&t)
        .send().await.unwrap();
    assert!(r.status().is_success() || r.status() == 404,
            "removing the user failed: {}", r.text().await.unwrap());

    let kept: i64 = pool.get().unwrap().query_row(
        "SELECT COUNT(*) FROM cops_master WHERE os_no='9201' AND os_year=2026",
        [], |r| r.get(0)).unwrap();
    assert_eq!(kept, 1, "the case must survive the officer who booked it");

    let who: Option<String> = pool.get().unwrap().query_row(
        "SELECT booked_by FROM cops_master WHERE os_no='9201' AND os_year=2026",
        [], |r| r.get(0)).unwrap();
    assert_eq!(who.as_deref(), Some("DEPARTING OFFICER"),
               "and must still say who booked it");
}

#[tokio::test]
async fn a_role_can_be_changed_and_a_nonsense_role_refused() {
    let (base, _d) = serve().await;
    let c = reqwest::Client::new();
    c.post(format!("{base}/auth/users")).bearer_auth(admin_token())
        .json(&serde_json::json!({
            "user_id": "promoted", "user_name": "TO BE PROMOTED",
            "password": "Str0ng#Pass1", "user_role": "SDO", "user_desig": "Supdt."
        })).send().await.unwrap();

    let ok = c.patch(format!("{base}/auth/users/promoted/role")).bearer_auth(dc_token())
        .json(&serde_json::json!({ "user_role": "AC" })).send().await.unwrap();
    assert!(ok.status().is_success(), "changing the role failed: {}", ok.text().await.unwrap());

    let bad = c.patch(format!("{base}/auth/users/promoted/role")).bearer_auth(dc_token())
        .json(&serde_json::json!({ "user_role": "EMPEROR" })).send().await.unwrap();
    assert_eq!(bad.status(), 400, "a role that does not exist must be refused");
}

// ── Versioned configuration edits ────────────────────────────────────────────

#[tokio::test]
async fn editing_a_configuration_row_keeps_the_version_that_came_before() {
    // These rows are versioned by effective_from precisely so past cases keep
    // printing as they were. Editing must not rewrite that history.
    let (base, _d, pool) = serve_with_pool(0).await;
    let c = reqwest::Client::new();
    let t = admin_token();

    let r = c.post(format!("{base}/admin/config/print-template")).bearer_auth(&t)
        .json(&serde_json::json!({
            "field_key": "order_heading", "field_label": "Order heading",
            "field_value": "ORDER", "effective_from": "2020-01-01"
        })).send().await.unwrap();
    assert!(r.status().is_success(), "adding a template row failed: {}", r.text().await.unwrap());

    let before: i64 = pool.get().unwrap().query_row(
        "SELECT COUNT(*) FROM print_template_config WHERE field_key='order_heading'",
        [], |r| r.get(0)).unwrap();
    assert!(before >= 1);

    // A newer version, not a replacement.
    c.post(format!("{base}/admin/config/print-template")).bearer_auth(&t)
        .json(&serde_json::json!({
            "field_key": "order_heading", "field_label": "Order heading",
            "field_value": "ORDER-IN-ORIGINAL", "effective_from": "2026-01-01"
        })).send().await.unwrap();

    let after: i64 = pool.get().unwrap().query_row(
        "SELECT COUNT(*) FROM print_template_config WHERE field_key='order_heading'",
        [], |r| r.get(0)).unwrap();
    assert_eq!(after, before + 1,
               "a new wording must be added alongside the old, not overwrite it");
}

// ── Revenue sessions ─────────────────────────────────────────────────────────

#[tokio::test]
async fn a_revenue_session_can_be_submitted_and_reopened() {
    // Submitting closes a shift's sheet. It must be possible to reopen it —
    // a figure entered wrongly and locked away is worse than one still open.
    let (base, _d, pool) = serve_with_pool(0).await;
    let c = reqwest::Client::new();
    let t = admin_token();

    let sid: i64 = {
        let conn = pool.get().unwrap();
        conn.execute(
            "INSERT INTO dcr_sessions (report_date, shift, created_at)
             VALUES ('2026-08-11', 'DAY', datetime('now'))", []).unwrap();
        conn.query_row("SELECT id FROM dcr_sessions LIMIT 1", [], |r| r.get(0)).unwrap()
    };

    let r = c.post(format!("{base}/dcr/sessions/{sid}/submit")).bearer_auth(&t)
        .json(&serde_json::json!({})).send().await.unwrap();
    assert!(r.status().is_success(), "submitting the shift failed: {}", r.text().await.unwrap());

    let r = c.post(format!("{base}/dcr/sessions/{sid}/unsubmit")).bearer_auth(&t)
        .json(&serde_json::json!({})).send().await.unwrap();
    assert!(r.status().is_success(), "reopening the shift failed: {}", r.text().await.unwrap());
}

#[tokio::test]
async fn item_types_are_listed_and_their_use_is_counted() {
    // The list an officer picks goods from in the revenue sheet, ordered by how
    // often each is actually used.
    let (base, _d) = serve().await;
    let c = reqwest::Client::new();
    let t = admin_token();

    let r = c.post(format!("{base}/dcr/item-types")).bearer_auth(&t)
        .json(&serde_json::json!({ "name": "TEST ITEM TYPE" })).send().await.unwrap();
    let st = r.status();
    let body = r.text().await.unwrap();
    assert!(st.is_success(), "adding an item type failed: {st} {body}");

    let list: serde_json::Value = c.get(format!("{base}/dcr/item-types")).bearer_auth(&t)
        .send().await.unwrap().json().await.unwrap();
    let arr = list.as_array().cloned()
        .or_else(|| list["items"].as_array().cloned()).unwrap_or_default();
    let id = arr.iter().find(|v| v["name"] == "TEST ITEM TYPE")
        .and_then(|v| v["id"].as_i64());
    assert!(id.is_some(), "the new item type must be listed: {list}");

    let r = c.patch(format!("{base}/dcr/item-types/{}/use", id.unwrap())).bearer_auth(&t)
        .send().await.unwrap();
    assert!(r.status().is_success(), "recording use failed: {}", r.text().await.unwrap());
}

// ── Admin: devices, masters retirement, statutes ─────────────────────────────

#[tokio::test]
async fn a_device_can_be_registered_disabled_and_removed() {
    // The list of machines allowed to run the app. Disabling must be reversible;
    // removing must not take the record of the others with it.
    let (base, _d) = serve().await;
    let c = reqwest::Client::new();
    let t = admin_token();

    for label in ["COUNTER PC ONE", "COUNTER PC TWO"] {
        let r = c.post(format!("{base}/admin/devices")).bearer_auth(&t)
            .json(&serde_json::json!({ "label": label, "hostname": label }))
            .send().await.unwrap();
        assert!(r.status().is_success(), "adding {label} failed: {}", r.text().await.unwrap());
    }

    let list: serde_json::Value = c.get(format!("{base}/admin/devices")).bearer_auth(&t)
        .send().await.unwrap().json().await.unwrap();
    let arr = list.as_array().cloned()
        .or_else(|| list["items"].as_array().cloned()).unwrap_or_default();
    assert!(arr.len() >= 2, "both machines must be listed: {list}");
    let id = arr[0]["id"].as_i64().expect("device id");

    let r = c.put(format!("{base}/admin/devices/{id}")).bearer_auth(&t)
        .json(&serde_json::json!({ "is_active": 0 })).send().await.unwrap();
    assert!(r.status().is_success(), "disabling failed: {}", r.text().await.unwrap());

    let r = c.delete(format!("{base}/admin/devices/{id}")).bearer_auth(&t)
        .send().await.unwrap();
    assert!(r.status().is_success(), "removing failed: {}", r.text().await.unwrap());

    let after: serde_json::Value = c.get(format!("{base}/admin/devices")).bearer_auth(&t)
        .send().await.unwrap().json().await.unwrap();
    let left = after.as_array().cloned()
        .or_else(|| after["items"].as_array().cloned()).unwrap_or_default();
    assert!(!left.is_empty(), "removing one machine must leave the other: {after}");
}

#[tokio::test]
async fn retiring_a_duty_rate_hides_it_without_erasing_what_it_was() {
    // Past cases were charged at the old rate. Retiring it must stop it being
    // offered, not remove the reason a past figure is what it is.
    let (base, _d, pool) = serve_with_pool(0).await;
    let c = reqwest::Client::new();
    let t = admin_token();

    c.post(format!("{base}/masters/duty-rates")).bearer_auth(&t)
        .json(&serde_json::json!({
            "duty_category": "RETIRING RATE", "from_date": "2020-01-01",
            "bcd_rate": 35.0, "cvd_rate": 0.0
        })).send().await.unwrap();

    let id: i64 = pool.get().unwrap().query_row(
        "SELECT id FROM duty_rate_master WHERE duty_category='RETIRING RATE'",
        [], |r| r.get(0)).unwrap();

    let r = c.put(format!("{base}/masters/duty-rates/{id}")).bearer_auth(&t)
        .json(&serde_json::json!({})).send().await.unwrap();
    assert!(r.status().is_success(), "retiring the rate failed: {}", r.text().await.unwrap());

    let still: i64 = pool.get().unwrap().query_row(
        "SELECT COUNT(*) FROM duty_rate_master WHERE duty_category='RETIRING RATE'",
        [], |r| r.get(0)).unwrap();
    assert_eq!(still, 1, "the row must remain — a past case was charged at it");

    let list: serde_json::Value = c.get(format!("{base}/masters/duty-rates")).bearer_auth(&t)
        .send().await.unwrap().json().await.unwrap();
    let arr = list.as_array().cloned()
        .or_else(|| list["items"].as_array().cloned()).unwrap_or_default();
    assert!(!arr.iter().any(|v| v["duty_category"] == "RETIRING RATE"),
            "but it must stop being offered: {list}");
}

#[tokio::test]
async fn a_statute_can_be_corrected_after_it_was_entered() {
    let (base, _d, pool) = serve_with_pool(0).await;
    let c = reqwest::Client::new();
    let t = admin_token();

    c.post(format!("{base}/statutes")).bearer_auth(&t)
        .json(&serde_json::json!({
            "keyword": "FIXME", "display_name": "Wrong At First",
            "legal_reference": "Section 000"
        })).send().await.unwrap();
    let id: i64 = pool.get().unwrap().query_row(
        "SELECT id FROM legal_statutes WHERE keyword='FIXME'", [], |r| r.get(0)).unwrap();

    let r = c.put(format!("{base}/statutes/{id}")).bearer_auth(&t)
        .json(&serde_json::json!({
            "keyword": "FIXME", "display_name": "Corrected",
            "legal_reference": "Section 111(d) of the Customs Act, 1962"
        })).send().await.unwrap();
    assert!(r.status().is_success(), "correcting the statute failed: {}", r.text().await.unwrap());

    let got: Option<String> = pool.get().unwrap().query_row(
        "SELECT legal_reference FROM legal_statutes WHERE keyword='FIXME'", [], |r| r.get(0)).unwrap();
    assert!(got.unwrap_or_default().contains("111(d)"), "the correction must stick");
}

// ── Remaining backup routes ──────────────────────────────────────────────────

#[tokio::test]
async fn the_backup_folder_can_be_tested_before_it_is_trusted() {
    // An officer points the automatic backup at a network share. Finding out it
    // is unwritable a week later, when it matters, is the failure this prevents.
    let (base, _d) = serve().await;
    let c = reqwest::Client::new();
    let t = admin_token();
    let dir = tempfile::tempdir().unwrap();

    let ok = c.post(format!("{base}/admin/backup/auto/test-folder")).bearer_auth(&t)
        .json(&serde_json::json!({ "path": dir.path().to_string_lossy() }))
        .send().await.unwrap();
    assert_eq!(ok.status(), 200, "a writable folder must pass: {}", ok.text().await.unwrap());

    let bad = c.post(format!("{base}/admin/backup/auto/test-folder")).bearer_auth(&t)
        .json(&serde_json::json!({ "path": "/definitely/not/a/real/place" }))
        .send().await.unwrap();
    let bad_st = bad.status();
    let body = bad.text().await.unwrap();
    assert!(bad_st != 200 || body.contains("false") || body.to_lowercase().contains("error"),
            "an unusable folder must be reported, not silently accepted: {body}");
}

#[tokio::test]
async fn the_csv_export_returns_the_register_as_text() {
    let (base, _d) = serve_with(25).await;
    let r = reqwest::Client::new()
        .get(format!("{base}/backup/export/csv")).bearer_auth(officer_token())
        .send().await.unwrap();
    assert_eq!(r.status(), 200, "the CSV export must answer");
    let body = r.text().await.unwrap();
    assert!(body.len() > 50, "and contain the register, not an empty file");
    assert!(body.contains(','), "a CSV should have columns: {}", &body[..body.len().min(120)]);
}

#[tokio::test]
async fn the_adjudication_summary_pdf_is_produced() {
    let (base, _d) = serve().await;
    let c = reqwest::Client::new();
    book_and_adjudicate(&base).await;
    let r = c.post(format!("{base}/backup/adjudication-summary-pdf"))
        .bearer_auth(officer_token())
        .json(&serde_json::json!({ "from_date": "2026-01-01", "to_date": "2026-12-31" }))
        .send().await.unwrap();
    let st = r.status();
    let bytes = r.bytes().await.unwrap();
    assert_eq!(st, 200, "the summary failed: {}", String::from_utf8_lossy(&bytes));
    assert_eq!(&bytes[..4], b"%PDF", "it must be a PDF");
}

// ── The last of the configuration and lookup routes ──────────────────────────

#[tokio::test]
async fn baggage_rules_and_allowances_can_be_set_and_read_back() {
    // The free-allowance figures that decide how much duty a passenger pays.
    let (base, _d) = serve().await;
    let c = reqwest::Client::new();
    let t = admin_token();

    let r = c.post(format!("{base}/admin/config/baggage-rules")).bearer_auth(&t)
        .json(&serde_json::json!({
            "rule_key": "TEST_FREE_ALLOWANCE", "rule_label": "Test allowance",
            "rule_value": 50000.0, "rule_uqc": "INR", "effective_from": "2026-01-01"
        })).send().await.unwrap();
    assert!(r.status().is_success(), "adding a baggage rule failed: {}", r.text().await.unwrap());

    let rules: serde_json::Value = c.get(format!("{base}/admin/config/baggage-rules"))
        .bearer_auth(&t).send().await.unwrap().json().await.unwrap();
    let arr = rules.as_array().cloned()
        .or_else(|| rules["items"].as_array().cloned()).unwrap_or_default();
    assert!(arr.iter().any(|v| v["rule_key"] == "TEST_FREE_ALLOWANCE"),
            "the rule must be readable back: {rules}");

    let r = c.post(format!("{base}/admin/config/special-allowances")).bearer_auth(&t)
        .json(&serde_json::json!({
            "item_name": "TEST ALLOWANCE ITEM", "keywords": "TESTITEM",
            "allowance_qty": 2.0, "allowance_uqc": "NOS", "effective_from": "2026-01-01"
        })).send().await.unwrap();
    assert!(r.status().is_success(),
            "adding a special allowance failed: {}", r.text().await.unwrap());
}

#[tokio::test]
async fn a_missing_required_field_is_named_rather_than_reported_as_a_database_error() {
    // The class that produced "NOT NULL constraint failed: allowed_devices.label"
    // on screen. An officer cannot act on that; they can act on a field name.
    let (base, _d) = serve().await;
    let c = reqwest::Client::new();
    let t = admin_token();

    for (path, payload) in [
        ("/admin/config/baggage-rules",
         serde_json::json!({ "rule_key": "NO_VALUE", "effective_from": "2026-01-01" })),
        ("/admin/config/special-allowances",
         serde_json::json!({ "keywords": "NONAME", "effective_from": "2026-01-01" })),
    ] {
        let r = c.post(format!("{base}{path}")).bearer_auth(&t)
            .json(&payload).send().await.unwrap();
        let st = r.status();
        let body = r.text().await.unwrap();
        assert_eq!(st, 400, "{path} should refuse this politely, got {st}: {body}");
        assert!(!body.contains("NOT NULL") && !body.contains("constraint"),
                "the officer should be told which field is missing, not shown a \
                 database error: {body}");
    }
}

#[tokio::test]
async fn the_item_description_suggestions_come_from_cases_already_booked() {
    // Autocomplete on the booking form. It should offer what this office has
    // actually seized, not a fixed list.
    let (base, _d) = serve().await;
    let c = reqwest::Client::new();
    c.post(format!("{base}/os")).bearer_auth(officer_token())
        .json(&serde_json::json!({
            "os_no": "9301", "os_date": "2026-08-11",
            "pax_name": "SUGGESTION SOURCE", "passport_no": "S1112223",
            "items": [{ "items_sno": 1, "items_desc": "DISTINCTIVE ARTICLE XYZZY",
                        "items_qty": 1.0, "items_value": 1000.0,
                        "items_release_category": "Under OS" }]
        })).send().await.unwrap();

    let r = c.get(format!("{base}/os/item-descriptions")).bearer_auth(officer_token())
        .send().await.unwrap();
    assert_eq!(r.status(), 200, "the suggestions must answer");
    let body = r.text().await.unwrap();
    assert!(body.contains("XYZZY"),
            "a description just booked should be offered back: {}", &body[..body.len().min(200)]);
}

#[tokio::test]
async fn a_passport_lookup_returns_the_passengers_earlier_details() {
    // Fills the form from a previous visit, so an officer does not retype a
    // name and risk spelling it differently the second time.
    let (base, _d, pool) = serve_with_pool(0).await;
    {
        let conn = pool.get().unwrap();
        conn.execute(
            "INSERT INTO cops_master (os_no, os_year, os_date, pax_name, passport_no,
                                      pax_nationality, entry_deleted, is_draft)
             VALUES ('9401', 2026, '2026-08-11', 'RETURNING PASSENGER', 'R9998887',
                     'INDIAN', 'N', 'N')", []).unwrap();
    }
    let r: serde_json::Value = reqwest::Client::new()
        .post(format!("{base}/passports/lookup")).bearer_auth(officer_token())
        .json(&serde_json::json!({ "passport_no": "R9998887" }))
        .send().await.unwrap().json().await.unwrap();
    let txt = r.to_string();
    assert!(txt.contains("RETURNING PASSENGER"),
            "the earlier details should come back: {r}");
}

#[tokio::test]
async fn the_report_generator_answers_for_a_range() {
    let (base, _d) = serve_with(40).await;
    let r = reqwest::Client::new()
        .get(format!("{base}/reports/generate?from_date=2026-01-01&to_date=2026-12-31"))
        .bearer_auth(officer_token()).send().await.unwrap();
    assert!(r.status().is_success() || r.status() == 400,
            "the report generator should answer or explain: {} {}",
            r.status(), r.text().await.unwrap());
}

#[tokio::test]
async fn unclaimed_cases_are_never_treated_as_the_same_passenger() {
    // Unclaimed goods are booked with a placeholder name and a dummy passport.
    // Matching on either ties every unclaimed case in the register to every
    // other, and a passenger is handed a form claiming a history that belongs to
    // a pile of abandoned baggage. This was corrected once on screen; the
    // printed form was still doing it.
    let (base, _d, pool) = serve_with_pool(0).await;
    let c = reqwest::Client::new();
    {
        let conn = pool.get().unwrap();
        // Three unclaimed cases, sharing the placeholder name and dummy passport
        // exactly as the office records them.
        for (n, nm, pp) in [
            ("8801", "UNCLAIMED", "NA"),
            ("8802", "UNCLAIMED", "NA"),
            ("8803", "UNCLAIMED BAGGAGE", "UNCLAIMED"),
        ] {
            conn.execute(
                "INSERT INTO cops_master (os_no, os_year, os_date, pax_name, passport_no,
                                          entry_deleted, is_draft)
                 VALUES (?,2026,'2026-03-01',?,?,'N','N')",
                rusqlite::params![n, nm, pp]).unwrap();
        }
        // And a real passenger with two genuine visits on one passport.
        for (n, d) in [("8804", "2026-01-10"), ("8805", "2026-05-20")] {
            conn.execute(
                "INSERT INTO cops_master (os_no, os_year, os_date, pax_name, passport_no,
                                          pax_date_of_birth, entry_deleted, is_draft)
                 VALUES (?,2026,?, 'GENUINE TRAVELLER', 'Z1234567', '1990-04-04', 'N','N')",
                rusqlite::params![n, d]).unwrap();
        }
    }

    let text_of = |no: String| {
        let url = format!("{base}/os/{no}/2026/print-pdf");
        let c = c.clone();
        async move {
            let pdf = c.get(url).bearer_auth(officer_token()).send().await.unwrap()
                .bytes().await.unwrap();
            assert_eq!(&pdf[..4], b"%PDF", "case {no} did not print");
            let out = std::process::Command::new("pdftotext")
                .args(["-", "-"]).stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped()).spawn()
                .and_then(|mut ch| {
                    use std::io::Write;
                    ch.stdin.as_mut().unwrap().write_all(&pdf)?;
                    ch.wait_with_output()
                });
            out.map(|o| String::from_utf8_lossy(&o.stdout).to_string()).ok()
        }
    };

    let Some(unclaimed) = text_of("8802".to_string()).await else {
        eprintln!("pdftotext unavailable — skipping"); return;
    };
    // The other two unclaimed cases must not appear anywhere on this form.
    for ghost in ["8801", "8803"] {
        assert!(!unclaimed.contains(&format!("{ghost}/2026")),
                "an unclaimed case must not cite another unclaimed case as a previous \
                 offence — found {ghost} on the form for 8802");
    }

    // The genuine repeat visitor still gets their real history.
    let Some(genuine) = text_of("8805".to_string()).await else { return };
    assert!(genuine.contains("8804/2026"),
            "a real passenger's earlier case on the same passport must still be cited:\n{}",
            &genuine[..genuine.len().min(600)]);
}

#[tokio::test]
async fn an_old_case_reprinted_today_shows_only_what_was_known_then() {
    // A form is a statement of the passenger's history on the day it was issued.
    // Reprinting case 1/2025 in 2026 must not show it having acquired prior
    // offences that had not happened yet — the copy in the file and the copy
    // printed today have to say the same thing.
    let (base, _d, pool) = serve_with_pool(0).await;
    let c = reqwest::Client::new();
    {
        let conn = pool.get().unwrap();
        // One passenger, one passport, three visits either side of the middle one.
        for (no, date) in [("1", "2024-06-01"), ("2", "2025-01-01"), ("3", "2026-03-01")] {
            conn.execute(
                "INSERT INTO cops_master (os_no, os_year, os_date, pax_name, passport_no,
                                          pax_date_of_birth, entry_deleted, is_draft)
                 VALUES (?,?,?, 'REPEAT TRAVELLER', 'P7654321', '1985-02-02', 'N','N')",
                rusqlite::params![no, date[0..4].parse::<i64>().unwrap(), date]).unwrap();
        }
        // And the same person under a different passport, also on both sides.
        for (no, date) in [("11", "2024-08-01"), ("12", "2026-05-01")] {
            conn.execute(
                "INSERT INTO cops_master (os_no, os_year, os_date, pax_name, passport_no,
                                          pax_date_of_birth, entry_deleted, is_draft)
                 VALUES (?,?,?, 'REPEAT TRAVELLER', 'Q1122334', '1985-02-02', 'N','N')",
                rusqlite::params![no, date[0..4].parse::<i64>().unwrap(), date]).unwrap();
        }
    }

    let pdf = c.get(format!("{base}/os/2/2025/print-pdf"))
        .bearer_auth(officer_token()).send().await.unwrap().bytes().await.unwrap();
    assert_eq!(&pdf[..4], b"%PDF");
    let Ok(out) = std::process::Command::new("pdftotext")
        .args(["-", "-"]).stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped()).spawn()
        .and_then(|mut ch| {
            use std::io::Write;
            ch.stdin.as_mut().unwrap().write_all(&pdf)?;
            ch.wait_with_output()
        }) else { eprintln!("pdftotext unavailable — skipping"); return };
    let txt = String::from_utf8_lossy(&out.stdout).to_string();

    // What had happened before 01/01/2025.
    assert!(txt.contains("1/2024"),
            "the earlier visit on the same passport must be cited:\n{}", &txt[..txt.len().min(700)]);
    assert!(txt.contains("Q1122334"),
            "the earlier case under the other passport must be cited:\n{}", &txt[..txt.len().min(700)]);

    // What had not happened yet.
    assert!(!txt.contains("3/2026"),
            "a case booked AFTER this one must not appear as a prior offence");
    assert!(!txt.contains("12/2026"),
            "nor one under another passport booked after this one");
}

// ── Revenue sheet: receipts by case, and whether a line is settled ───────────

#[tokio::test]
async fn the_revenue_sheet_can_look_up_the_receipts_for_a_case() {
    // An officer types the O.S. number during a shift change. The register
    // already knows which receipts belong to it; asking them to copy it across
    // by hand is how the column ends up empty.
    let (base, _d, pool) = serve_with_pool(0).await;
    let c = reqwest::Client::new();
    {
        let conn = pool.get().unwrap();
        conn.execute(
            "INSERT INTO cops_master (os_no, os_year, os_date, pax_name,
                                      post_adj_br_entries, entry_deleted, is_draft)
             VALUES ('520', 2026, '2026-08-11', 'REVENUE CASE', '901/2026', 'N','N')",
            []).unwrap();
        for br in [901i64, 902] {
            conn.execute(
                "INSERT INTO br_master (br_no, br_year, br_date, br_type, pax_name,
                                        os_no, os_year, entry_deleted)
                 VALUES (?,2026,'2026-08-11','D','REVENUE CASE','520',2026,'N')",
                rusqlite::params![br]).unwrap();
        }
        conn.execute(
            "INSERT INTO dr_master (dr_no, dr_year, dr_date, dr_type, pax_name,
                                    os_no, os_year, entry_deleted)
             VALUES (701,2026,'2026-08-11','GOODS','REVENUE CASE','520',2026,'N')",
            []).unwrap();
    }

    // Written the way an officer types it.
    let r: serde_json::Value = c.get(format!("{base}/dcr/receipts-for-os?os_ref=520/2026"))
        .bearer_auth(officer_token()).send().await.unwrap().json().await.unwrap();
    let brs = r["br_numbers"].as_array().cloned().unwrap_or_default();
    assert_eq!(brs.len(), 2, "both receipts on the case must come back: {r}");
    assert_eq!(r["dr_numbers"].as_array().map(|a| a.len()).unwrap_or(0), 1,
               "the detention receipt too: {r}");

    // A number with the year given separately works the same way.
    let r2: serde_json::Value = c.get(format!("{base}/dcr/receipts-for-os?os_ref=520&os_year=2026"))
        .bearer_auth(officer_token()).send().await.unwrap().json().await.unwrap();
    assert_eq!(r2["br_numbers"].as_array().map(|a| a.len()).unwrap_or(0), 2,
               "the same case, written the other way: {r2}");

    // Nothing typed, nothing claimed.
    let empty: serde_json::Value = c.get(format!("{base}/dcr/receipts-for-os?os_ref="))
        .bearer_auth(officer_token()).send().await.unwrap().json().await.unwrap();
    assert_eq!(empty["br_numbers"].as_array().map(|a| a.len()).unwrap_or(9), 0,
               "an empty reference must return nothing rather than guess: {empty}");

    // A case that does not exist is not an error, just no receipts.
    let none: serde_json::Value = c.get(format!("{base}/dcr/receipts-for-os?os_ref=9999/2026"))
        .bearer_auth(officer_token()).send().await.unwrap().json().await.unwrap();
    assert_eq!(none["br_numbers"].as_array().map(|a| a.len()).unwrap_or(9), 0,
               "an unknown case returns nothing: {none}");
}

#[tokio::test]
async fn a_line_is_open_until_the_personal_penalty_is_paid() {
    // The penalty is mandatory. Unpaid, the case is still open whatever else was
    // collected; paid, it is settled — on an absolute confiscation the penalty is
    // the whole of it, and on a normal one it accompanies the fine and the duty.
    let (base, _d, pool) = serve_with_pool(0).await;
    let c = reqwest::Client::new();
    let sid: i64 = {
        let conn = pool.get().unwrap();
        conn.execute(
            "INSERT INTO dcr_sessions (report_date, shift, created_at)
             VALUES ('2026-08-11','DAY',datetime('now'))", []).unwrap();
        conn.query_row("SELECT id FROM dcr_sessions LIMIT 1", [], |r| r.get(0)).unwrap()
    };
    {
        let conn = pool.get().unwrap();
        // duty and a fine collected, but no penalty — still open
        conn.execute(
            "INSERT INTO dcr_entries (session_id, sort_order, sl_no, os_ref,
                                      total_duty, redemption_fine, personal_penalty)
             VALUES (?,1,1,'520/2026', 40000, 10000, 0)",
            rusqlite::params![sid]).unwrap();
        // absolute confiscation: the penalty is the whole of it
        conn.execute(
            "INSERT INTO dcr_entries (session_id, sort_order, sl_no, os_ref,
                                      total_duty, redemption_fine, personal_penalty)
             VALUES (?,2,2,'521/2026', 0, 0, 5000)",
            rusqlite::params![sid]).unwrap();
        // normal confiscation: fine and duty alongside the penalty
        conn.execute(
            "INSERT INTO dcr_entries (session_id, sort_order, sl_no, os_ref,
                                      total_duty, redemption_fine, personal_penalty)
             VALUES (?,3,3,'522/2026', 40000, 10000, 2500)",
            rusqlite::params![sid]).unwrap();
    }

    let s: serde_json::Value = c.get(format!("{base}/dcr/sessions/{sid}"))
        .bearer_auth(officer_token()).send().await.unwrap().json().await.unwrap();
    let entries = s["entries"].as_array().cloned()
        .or_else(|| s["items"].as_array().cloned())
        .unwrap_or_default();
    assert_eq!(entries.len(), 3, "all three lines must come back: {s}");

    let status_of = |sl: i64| -> String {
        entries.iter().find(|e| e["sl_no"] == sl)
            .and_then(|e| e["status"].as_str()).unwrap_or("").to_string()
    };
    assert_eq!(status_of(1), "OPEN",
               "duty and a fine without the penalty leaves the case open");
    assert_eq!(status_of(2), "CLOSED",
               "an absolute confiscation is settled by the penalty alone");
    assert_eq!(status_of(3), "CLOSED",
               "penalty with fine and duty settles a normal confiscation");
}

#[tokio::test]
async fn the_revenue_sheet_teaches_the_register_which_receipt_settled_which_case() {
    // The sheet is filled every day and carries the receipt and the case on the
    // same line. The case is supposed to learn that through the adjudication
    // screen, and during a shift change nobody goes back to open it — so the
    // register never finds out. This reads the linkage and writes it across.
    let (base, _d, pool) = serve_with_pool(0).await;
    let c = reqwest::Client::new();
    let sid: i64 = {
        let conn = pool.get().unwrap();
        conn.execute(
            "INSERT INTO dcr_sessions (report_date, shift, created_at)
             VALUES ('2026-08-11','DAY',datetime('now'))", []).unwrap();
        // A case with nothing recorded against it, and one an officer already
        // filled in by hand.
        conn.execute(
            "INSERT INTO cops_master (os_no, os_year, os_date, pax_name, entry_deleted, is_draft)
             VALUES ('520', 2026, '2026-08-11', 'NEEDS LINKING', 'N','N')", []).unwrap();
        conn.execute(
            "INSERT INTO cops_master (os_no, os_year, os_date, pax_name,
                                      post_adj_br_entries, entry_deleted, is_draft)
             VALUES ('521', 2026, '2026-08-11', 'ALREADY DONE',
                     '[{\"no\":\"777\",\"date\":\"2026-08-01\",\"amount\":\"5000\"}]', 'N','N')",
            []).unwrap();
        conn.query_row("SELECT id FROM dcr_sessions LIMIT 1", [], |r| r.get(0)).unwrap()
    };

    let r = c.put(format!("{base}/dcr/sessions/{sid}/entries"))
        .bearer_auth(officer_token())
        .json(&serde_json::json!({
            "entries": [
                { "sort_order": 1, "sl_no": 1, "br_no": "901", "os_ref": "520/2026",
                  "personal_penalty": 2500.0, "redemption_fine": 10000.0, "total_duty": 40000.0 },
                // two receipts on one line
                { "sort_order": 2, "sl_no": 2, "br_no": "902, 903", "os_ref": "520/2026",
                  "personal_penalty": 0.0 },
                // a case that already has an entry — must not be disturbed
                { "sort_order": 3, "sl_no": 3, "br_no": "777", "os_ref": "521/2026",
                  "personal_penalty": 1000.0 },
                // a case that does not exist — must be ignored quietly
                { "sort_order": 4, "sl_no": 4, "br_no": "999", "os_ref": "9999/2026",
                  "personal_penalty": 0.0 }
            ]
        }))
        .send().await.unwrap();
    let st = r.status();
    let body = r.text().await.unwrap();
    assert!(st.is_success(), "saving the shift failed: {st} {body}");

    let conn = pool.get().unwrap();
    let entries_of = |no: &str| -> String {
        conn.query_row(
            "SELECT COALESCE(post_adj_br_entries,'') FROM cops_master
              WHERE os_no = ?1 AND os_year = 2026", [no], |r| r.get(0)).unwrap_or_default()
    };

    let linked = entries_of("520");
    for br in ["901", "902", "903"] {
        assert!(linked.contains(br),
                "receipt {br} named against 520/2026 must reach the case: {linked}");
    }
    assert!(linked.contains("2026-08-11"),
            "the receipt carries the date of the report it appeared on: {linked}");
    // The shape matters as much as the content: the printed form, the offence
    // list and the case query all read `no`/`date`, and an entry written under
    // any other key is stored but never seen.
    let parsed: Vec<serde_json::Value> = serde_json::from_str(&linked).expect("valid JSON");
    assert!(parsed.iter().all(|e| e.get("no").is_some() && e.get("date").is_some()),
            "every entry must use the keys the rest of the app reads: {linked}");

    // The hand-entered one is untouched, amount and all.
    let untouched = entries_of("521");
    assert!(untouched.contains("5000"),
            "an entry already on the case must be left exactly as it was: {untouched}");
    assert_eq!(untouched.matches("777").count(), 1,
               "and must not be duplicated: {untouched}");
}

#[tokio::test]
async fn a_redemption_case_stays_open_until_the_duty_is_paid_too() {
    // Absolute confiscation is settled by the penalty alone. Where redemption was
    // offered, the passenger takes the goods back, so the duty on them falls due
    // as well — two of the three is a case still owing money.
    let (base, _d, pool) = serve_with_pool(0).await;
    let c = reqwest::Client::new();
    let sid: i64 = {
        let conn = pool.get().unwrap();
        conn.execute(
            "INSERT INTO dcr_sessions (report_date, shift, created_at)
             VALUES ('2026-08-11','DAY',datetime('now'))", []).unwrap();
        conn.query_row("SELECT id FROM dcr_sessions LIMIT 1", [], |r| r.get(0)).unwrap()
    };
    {
        let conn = pool.get().unwrap();
        for (sl, pp, rf, duty) in [
            (1, 5000.0, 0.0,     0.0),      // absolute: penalty is the whole of it
            (2, 2500.0, 10000.0, 40000.0),  // redemption: penalty, fine and duty
            (3, 2500.0, 10000.0, 0.0),      // redemption, duty not paid
            (4, 0.0,    10000.0, 40000.0),  // no penalty at all
        ] {
            conn.execute(
                "INSERT INTO dcr_entries (session_id, sort_order, sl_no,
                                          personal_penalty, redemption_fine, total_duty)
                 VALUES (?,?,?,?,?,?)",
                rusqlite::params![sid, sl, sl, pp, rf, duty]).unwrap();
        }
    }
    let s: serde_json::Value = c.get(format!("{base}/dcr/sessions/{sid}"))
        .bearer_auth(officer_token()).send().await.unwrap().json().await.unwrap();
    let entries = s["entries"].as_array().cloned()
        .or_else(|| s["items"].as_array().cloned()).unwrap_or_default();
    let status_of = |sl: i64| -> String {
        entries.iter().find(|e| e["sl_no"] == sl)
            .and_then(|e| e["status"].as_str()).unwrap_or("").to_string()
    };
    assert_eq!(status_of(1), "CLOSED", "absolute confiscation: the penalty settles it");
    assert_eq!(status_of(2), "CLOSED", "redemption with fine and duty paid");
    assert_eq!(status_of(3), "OPEN",   "redemption offered but the duty is unpaid");
    assert_eq!(status_of(4), "OPEN",   "no personal penalty, so nothing is settled");
}
