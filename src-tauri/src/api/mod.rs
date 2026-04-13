//! REST API router for the embedded Axum HTTP server.
//!
//! ## Architecture
//!
//! All routes defined here are mounted under [`API_PREFIX`] (`/api`) by
//! [`build_app`].  When adding a new route, define it in `build_routes()`
//! using a path like `/foo` — it will be served at `/api/foo`.
//!
//! The frontend (`src/lib/api.ts`) uses `baseURL = http://127.0.0.1:{SERVER_PORT}/api`,
//! so every `api.get('/foo')` call hits `http://127.0.0.1:8000/api/foo`.
//!
//! ## Adding a new route
//!
//! 1. Create a handler in the appropriate submodule (e.g. `offence.rs`).
//! 2. Add `.route("/your-path", get(module::handler))` inside `build_routes()`.
//! 3. The `/api` prefix is applied automatically — do NOT add it to the route path.

mod auth;
mod offence;
mod masters;
mod backup;
mod baggage;
mod detention;
mod statutes;
mod admin;
mod dashboard;
mod apis;
mod queries;
mod reports;

use std::sync::Arc;
use axum::{Router, routing::{get, post, put, delete, patch}};
use crate::db::DbPool;

/// The URL prefix for all API routes.  The frontend's `api.ts` must use the
/// same prefix in its `baseURL`.  Change this in ONE place and both sides
/// stay in sync (the frontend reads `http://127.0.0.1:{SERVER_PORT}{API_PREFIX}`).
pub const API_PREFIX: &str = "/api";

/// The port the embedded HTTP server listens on.  Must match the port in
/// `src/lib/api.ts` (`_resolveApiUrl`).
pub const SERVER_PORT: u16 = 8000;

/// Build the full application router with the `/api` prefix applied.
///
/// This is the entry point called from `lib.rs`.  It wraps all routes under
/// [`API_PREFIX`] so callers don't need to remember to nest manually.
pub fn build_app(pool: Arc<DbPool>) -> Router {
    Router::new().nest(API_PREFIX, build_routes(pool))
}

/// Internal: all API routes WITHOUT the `/api` prefix.
/// The prefix is applied by [`build_app`] above.
fn build_routes(pool: Arc<DbPool>) -> Router<()> {
    Router::new()
        // ── Auth (regular users) ──────────────────────────────────────────────
        .route("/auth/login",                    post(auth::login))
        .route("/auth/users",                    get(auth::list_users).post(auth::create_user))
        .route("/auth/users/{id}",               put(auth::update_user).delete(auth::delete_user))
        .route("/auth/users/{user_id}/role",     patch(auth::upgrade_role))
        .route("/auth/change-password",          post(auth::change_password))
        .route("/auth/bootstrap/{module_type}",  get(auth::bootstrap))
        .route("/auth/me",                       get(auth::me))

        // ── Admin auth + user management ──────────────────────────────────────
        .route("/admin/login",                   post(auth::admin_login))
        .route("/admin/users",                   get(auth::admin_list_users).post(auth::admin_create_user))
        .route("/admin/users/{id}",              patch(auth::admin_update_user).delete(auth::admin_soft_delete_user))
        .route("/admin/users/{id}/hard",         delete(auth::admin_hard_delete_user))

        // ── OS Cases ──────────────────────────────────────────────────────────
        .route("/os",                                  get(offence::list_os).post(offence::create_os))
        .route("/os/sidebar-counts",                   get(offence::sidebar_counts))
        .route("/os/item-descriptions",                get(offence::item_descriptions))
        .route("/os/classify-item",                    get(offence::classify_item))
        .route("/os/check-os-no",                      get(offence::check_os_no))
        .route("/os/offline",                          post(offence::create_offline))
        .route("/os/{os_no}/{os_year}",
               get(offence::get_os)
               .put(offence::update_os)
               .delete(offence::delete_os))
        .route("/os/{os_no}/{os_year}/adjudicate",             post(offence::adjudicate))
        .route("/os/{os_no}/{os_year}/complete-offline-adj",   patch(offence::complete_offline))
        .route("/os/{os_no}/{os_year}/quash",                  post(offence::quash_os))
        .route("/os/{os_no}/{os_year}/post-adj",               patch(offence::post_adj))
        .route("/os/{os_no}/{os_year}/print-pdf",              get(offence::print_pdf))
        .route("/os/{os_no}/{os_year}/mark-printed",           post(offence::mark_printed))

        // ── Passport search ───────────────────────────────────────────────────
        .route("/passports/search",     post(offence::passport_search))
        .route("/passports/lookup",     post(offence::passport_lookup_by_pp))

        // ── Masters ───────────────────────────────────────────────────────────
        .route("/masters/nationalities",    get(masters::nationalities).post(masters::create_nationality))
        .route("/masters/airlines",         get(masters::airlines).post(masters::create_airline))
        .route("/masters/flights",          get(masters::flights).post(masters::create_flight))
        .route("/masters/airports",         get(masters::airports))
        .route("/masters/airports/close-all", post(masters::close_all_airports))
        .route("/masters/item-categories",  get(masters::item_categories).post(masters::create_item_category))
        .route("/masters/item-categories/{id}", put(masters::deactivate_item_category))
        .route("/masters/duty-rates",       get(masters::duty_rates).post(masters::create_duty_rate))
        .route("/masters/duty-rates/{id}",  put(masters::deactivate_duty_rate))
        .route("/masters/dc-list",          get(masters::dc_list).post(masters::create_dc))
        .route("/masters/br-limits",        get(masters::br_limits).post(masters::create_br_limit))

        // ── Baggage Register (BR) ─────────────────────────────────────────────
        .route("/br",                               get(baggage::list_brs).post(baggage::create_br))
        .route("/br/passport/{passport_no}",        get(baggage::get_brs_by_passport))
        .route("/br/{br_no}/{br_year}",             get(baggage::get_br))
        .route("/br/{br_no}/{br_year}/mark-printed",post(baggage::mark_br_printed))
        .route("/br/{br_no}/{br_year}/print-pdf",   get(baggage::print_br_pdf))

        // ── Detention Register (DR) ───────────────────────────────────────────
        .route("/dr",                               get(detention::list_drs).post(detention::create_dr))
        .route("/dr/{dr_no}/{dr_year}",             get(detention::get_dr))
        .route("/dr/{dr_no}/{dr_year}/mark-printed",post(detention::mark_dr_printed))
        .route("/dr/{dr_no}/{dr_year}/print-pdf",   get(detention::print_dr_pdf))

        // ── Legal Statutes ────────────────────────────────────────────────────
        .route("/statutes",       get(statutes::list_statutes).post(statutes::create_statute))
        .route("/statutes/{id}",  put(statutes::update_statute).delete(statutes::delete_statute))

        // ── Cross-reference search ────────────────────────────────────────────
        .route("/queries/search",  get(queries::cross_reference))

        // ── CSV Register Reports (r4=BR, r5=OS, r6=DR) ───────────────────────
        .route("/reports/generate",  get(reports::generate))

        // ── Dashboard ─────────────────────────────────────────────────────────
        .route("/dashboard/stats",  get(dashboard::stats))

        // ── APIS (passenger manifest matching) ───────────────────────────────
        .route("/apis/match",   post(apis::match_manifest))
        .route("/apis/export",  post(apis::export_manifest))

        // ── Admin — mode, features, config ───────────────────────────────────
        .route("/admin/mode",                       get(admin::get_mode).put(admin::set_mode))
        .route("/admin/features",                   get(admin::get_features).put(admin::set_features))
        .route("/admin/config/print-template",      get(admin::get_print_template).post(admin::upsert_print_template))
        .route("/admin/config/print-template/{id}", put(admin::update_print_template_row).delete(admin::delete_print_template_row))
        .route("/admin/config/baggage-rules",       get(admin::get_baggage_rules).post(admin::upsert_baggage_rules))
        .route("/admin/config/baggage-rules/{id}",  put(admin::update_baggage_rules_row).delete(admin::delete_baggage_rules_row))
        .route("/admin/config/special-allowances",  get(admin::get_special_allowances).post(admin::create_special_allowance))
        .route("/admin/config/special-allowances/{id}", put(admin::update_special_allowance).delete(admin::delete_special_allowance))
        .route("/admin/config/pit",                 get(admin::get_pit_config))
        .route("/admin/config/os",                  get(admin::get_os_config))
        .route("/admin/config/remarks-templates",       get(admin::get_remarks_templates))
        .route("/admin/config/remarks-templates/{key}", put(admin::upsert_remarks_template))
        .route("/admin/devices",                    get(admin::list_devices).post(admin::create_device))
        .route("/admin/devices/{id}",               put(admin::update_device).delete(admin::delete_device))
        .route("/admin/purge-os",                   post(admin::purge_os))

        // ── Backup / Restore ─────────────────────────────────────────────────
        .route("/backup/export/csv",                get(backup::export_csv))
        .route("/backup/export/db",                 get(backup::export_db))
        .route("/backup/upload/new",                post(backup::upload_new))
        .route("/backup/upload/legacy",             post(backup::upload_legacy))
        .route("/backup/custom-report",             post(backup::custom_report))
        .route("/backup/adjudication-summary-pdf",  post(backup::adjudication_summary_pdf))

        // ── Admin Backup / Restore ─────────────────────────────────────────────
        .route("/admin/backup/export",              get(backup::admin_export_csv))
        .route("/admin/backup/export-fulldb",       get(backup::admin_export_db))
        .route("/admin/backup/restore-fulldb",      post(backup::admin_restore_fulldb))
        .route("/admin/backup/upload-legacy",       post(backup::admin_upload_legacy))
        .route("/admin/backup/upload-legacy-items", post(backup::admin_upload_legacy_items))
        .route("/admin/backup/import-mdb",          post(backup::admin_import_mdb))
        .route("/admin/backup/restore",             post(backup::admin_restore))
        .route("/admin/backup/db-cipher-key",       get(admin::get_db_cipher_key))

        // ── OS Query (shared across modules) ─────────────────────────────────
        .route("/os-query/search",          post(offence::query_search))
        .route("/os-query/monthly-report",  get(offence::monthly_report))
        .route("/os-query/br/search",       get(baggage::list_brs))
        .route("/os-query/br/{br_no}/{br_year}", get(baggage::get_br))
        .route("/os-query/dr/search",       get(detention::list_drs))
        .route("/os-query/dr/{dr_no}/{dr_year}", get(detention::get_dr))

        // ── App mode / feature flags (public — splash screen / route guard) ──
        .route("/mode",     get(admin::get_mode))
        .route("/features", get(admin::get_features))

        // ── Health ────────────────────────────────────────────────────────────
        .route("/health", get(|| async { "ok" }))

        .with_state(pool)
}
