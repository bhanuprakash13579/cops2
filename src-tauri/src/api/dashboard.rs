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
    let pending_os: i64 = conn.query_row(
        "SELECT COUNT(*) FROM cops_master WHERE entry_deleted='N' AND is_draft='N'
         AND adjudication_date IS NULL AND adj_offr_name IS NULL
         AND (adjn_offr_remarks IS NULL OR adjn_offr_remarks='')
         AND (quashed IS NULL OR quashed!='Y') AND (rejected IS NULL OR rejected!='Y')
         AND (is_offline_adjudication IS NULL OR is_offline_adjudication!='Y')
         AND (is_legacy IS NULL OR is_legacy!='Y')
         AND EXISTS (
             SELECT 1 FROM cops_items ci
             WHERE ci.os_no = cops_master.os_no
               AND ci.os_year = cops_master.os_year
               AND (ci.entry_deleted IS NULL OR ci.entry_deleted != 'Y')
         )",
        [], |r| r.get(0)
    ).unwrap_or(0);

    // One pass over the register instead of eleven.
    //
    // These were eleven separate COUNT(*) and SUM() statements, each reading the
    // whole of cops_master to answer one question about it — the slowest thing
    // on the dashboard, and it runs the moment an officer signs in. They ask
    // different questions of the same rows, so they are asked together.
    //
    // Every figure is the same figure as before; the conditions are copied
    // across unchanged, and a test compares the two ways of counting.
    let (total_os, adjudicated_os, offline_pending, draft_os,
         today_os, today_adj, month_os, year_os, total_duty, total_payable):
        (i64, i64, i64, i64, i64, i64, i64, i64, f64, f64) = conn.query_row(
        "SELECT
           COUNT(*) FILTER (WHERE is_draft='N'),
           COUNT(*) FILTER (WHERE is_draft='N'
                              AND (adjudication_date IS NOT NULL OR adj_offr_name IS NOT NULL)),
           COUNT(*) FILTER (WHERE is_draft='N'
                              AND is_offline_adjudication='Y' AND adj_offr_name IS NULL),
           COUNT(*) FILTER (WHERE is_draft='Y'),
           COUNT(*) FILTER (WHERE is_draft='N' AND os_date = ?1),
           COUNT(*) FILTER (WHERE adjudication_date = ?1),
           COUNT(*) FILTER (WHERE is_draft='N' AND os_date >= ?2),
           COUNT(*) FILTER (WHERE is_draft='N' AND os_year = ?3),
           COALESCE(SUM(total_duty_amount) FILTER (WHERE is_draft='N'), 0),
           COALESCE(SUM(total_payable)     FILTER (WHERE is_draft='N'
                                                     AND adjudication_date IS NOT NULL), 0)
         FROM cops_master WHERE entry_deleted='N'",
        rusqlite::params![today, month_start, year.parse::<i64>().unwrap_or(0)],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?,
                r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?)),
    ).unwrap_or((0, 0, 0, 0, 0, 0, 0, 0, 0.0, 0.0));

    let total_br: i64 = conn.query_row("SELECT COUNT(*) FROM br_master WHERE entry_deleted='N'", [], |r| r.get(0)).unwrap_or(0);
    let total_dr: i64 = conn.query_row("SELECT COUNT(*) FROM dr_master WHERE entry_deleted='N'", [], |r| r.get(0)).unwrap_or(0);

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
