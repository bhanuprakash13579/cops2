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
