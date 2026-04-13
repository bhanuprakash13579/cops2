use std::sync::Arc;
use axum::{extract::State, http::StatusCode, Json};
use serde_json::{json, Value};
use crate::{auth::AuthUser, db::DbPool};

type Db = State<Arc<DbPool>>;
type Err = (StatusCode, Json<Value>);

fn e500(m: &str) -> Err { (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "detail": m }))) }

pub async fn stats(State(pool): Db, _auth: AuthUser) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;

    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let year  = chrono::Local::now().format("%Y").to_string();
    let month_start = chrono::Local::now().format("%Y-%m-01").to_string();

    // OS counts
    let total_os: i64 = conn.query_row(
        "SELECT COUNT(*) FROM cops_master WHERE entry_deleted='N' AND is_draft='N'",
        [], |r| r.get(0)
    ).unwrap_or(0);

    let pending_os: i64 = conn.query_row(
        "SELECT COUNT(*) FROM cops_master WHERE entry_deleted='N' AND is_draft='N'
         AND adjudication_date IS NULL AND adj_offr_name IS NULL
         AND (is_offline_adjudication IS NULL OR is_offline_adjudication!='Y')
         AND (is_legacy IS NULL OR is_legacy!='Y')",
        [], |r| r.get(0)
    ).unwrap_or(0);

    let adjudicated_os: i64 = conn.query_row(
        "SELECT COUNT(*) FROM cops_master WHERE entry_deleted='N' AND is_draft='N'
         AND adjudication_date IS NOT NULL",
        [], |r| r.get(0)
    ).unwrap_or(0);

    let offline_pending: i64 = conn.query_row(
        "SELECT COUNT(*) FROM cops_master WHERE entry_deleted='N' AND is_draft='N'
         AND is_offline_adjudication='Y' AND adj_offr_name IS NULL",
        [], |r| r.get(0)
    ).unwrap_or(0);

    let draft_os: i64 = conn.query_row(
        "SELECT COUNT(*) FROM cops_master WHERE entry_deleted='N' AND is_draft='Y'",
        [], |r| r.get(0)
    ).unwrap_or(0);

    // Today's activity
    let today_os: i64 = conn.query_row(
        "SELECT COUNT(*) FROM cops_master WHERE os_date=? AND entry_deleted='N' AND is_draft='N'",
        rusqlite::params![today], |r| r.get(0)
    ).unwrap_or(0);

    let today_adj: i64 = conn.query_row(
        "SELECT COUNT(*) FROM cops_master WHERE adjudication_date=? AND entry_deleted='N'",
        rusqlite::params![today], |r| r.get(0)
    ).unwrap_or(0);

    // This month
    let month_os: i64 = conn.query_row(
        "SELECT COUNT(*) FROM cops_master WHERE os_date>=? AND entry_deleted='N' AND is_draft='N'",
        rusqlite::params![month_start], |r| r.get(0)
    ).unwrap_or(0);

    // This year
    let year_os: i64 = conn.query_row(
        "SELECT COUNT(*) FROM cops_master WHERE os_year=? AND entry_deleted='N' AND is_draft='N'",
        rusqlite::params![year.parse::<i64>().unwrap_or(0)], |r| r.get(0)
    ).unwrap_or(0);

    // Financial totals
    let total_duty: f64 = conn.query_row(
        "SELECT COALESCE(SUM(total_duty_amount),0) FROM cops_master WHERE entry_deleted='N' AND is_draft='N'",
        [], |r| r.get(0)
    ).unwrap_or(0.0);

    let total_payable: f64 = conn.query_row(
        "SELECT COALESCE(SUM(total_payable),0) FROM cops_master WHERE entry_deleted='N' AND is_draft='N' AND adjudication_date IS NOT NULL",
        [], |r| r.get(0)
    ).unwrap_or(0.0);

    // BR/DR counts
    let total_br: i64 = conn.query_row("SELECT COUNT(*) FROM br_master", [], |r| r.get(0)).unwrap_or(0);
    let total_dr: i64 = conn.query_row("SELECT COUNT(*) FROM dr_master", [], |r| r.get(0)).unwrap_or(0);

    // Top 5 item categories
    let mut cat_stmt = conn.prepare(
        "SELECT items_category, COUNT(*) as cnt FROM cops_items
         WHERE entry_deleted='N' AND items_category IS NOT NULL
         GROUP BY items_category ORDER BY cnt DESC LIMIT 5"
    ).map_err(|e| e500(&e.to_string()))?;

    let top_categories: Vec<Value> = cat_stmt.query_map([], |r| {
        Ok(json!({ "category": r.get::<_, Option<String>>(0)?, "count": r.get::<_, i64>(1)? }))
    }).map_err(|e| e500(&e.to_string()))?.filter_map(|r| r.ok()).collect();

    Ok(Json(json!({
        "os": {
            "total":           total_os,
            "pending":         pending_os,
            "adjudicated":     adjudicated_os,
            "offline_pending": offline_pending,
            "draft":           draft_os,
            "today":           today_os,
            "today_adj":       today_adj,
            "this_month":      month_os,
            "this_year":       year_os,
        },
        "financials": {
            "total_duty_amount": total_duty,
            "total_payable":     total_payable,
        },
        "br": { "total": total_br },
        "dr": { "total": total_dr },
        "top_categories": top_categories,
    })))
}
