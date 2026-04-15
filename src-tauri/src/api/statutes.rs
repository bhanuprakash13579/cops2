use std::sync::Arc;
use axum::{extract::{Path, State}, http::StatusCode, Json};
use serde_json::{json, Value};
use crate::{auth::AuthUser, db::DbPool};

type Db = State<Arc<DbPool>>;
type Err = (StatusCode, Json<Value>);

fn e400(m: &str) -> Err { (StatusCode::BAD_REQUEST,          Json(json!({ "detail": m }))) }
fn e404(m: &str) -> Err { (StatusCode::NOT_FOUND,            Json(json!({ "detail": m }))) }
fn e500(m: &str) -> Err { (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "detail": m }))) }

pub async fn list_statutes(State(pool): Db, _auth: AuthUser) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let mut stmt = conn.prepare(
        "SELECT id, keyword, display_name, is_prohibited,
                supdt_goods_clause, adjn_goods_clause, legal_reference
         FROM legal_statutes ORDER BY display_name"
    ).map_err(|e| e500(&e.to_string()))?;

    let rows: Vec<Value> = stmt.query_map([], |r| {
        Ok(json!({
            "id":                r.get::<_, i64>(0)?,
            "keyword":           r.get::<_, String>(1)?,
            "display_name":      r.get::<_, String>(2)?,
            "is_prohibited":     r.get::<_, i64>(3)? != 0,
            "supdt_goods_clause": r.get::<_, String>(4)?,
            "adjn_goods_clause": r.get::<_, String>(5)?,
            "legal_reference":   r.get::<_, String>(6)?,
        }))
    }).map_err(|e| e500(&e.to_string()))?.filter_map(|r| r.ok()).collect();

    Ok(Json(json!(rows)))
}

pub async fn create_statute(State(pool): Db, _auth: AuthUser, Json(req): Json<serde_json::Value>) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let keyword = req.get("keyword").and_then(|v| v.as_str())
        .ok_or_else(|| e400("keyword is required"))?;
    let display_name = req.get("display_name").and_then(|v| v.as_str())
        .ok_or_else(|| e400("display_name is required"))?;
    let is_prohibited: i64 = req.get("is_prohibited").and_then(|v| v.as_bool())
        .map(|b| if b { 1 } else { 0 }).unwrap_or(0);

    conn.execute(
        "INSERT INTO legal_statutes (keyword, display_name, is_prohibited,
                supdt_goods_clause, adjn_goods_clause, legal_reference)
         VALUES (?,?,?,?,?,?)",
        rusqlite::params![
            keyword,
            display_name,
            is_prohibited,
            req.get("supdt_goods_clause").and_then(|v| v.as_str()).unwrap_or(""),
            req.get("adjn_goods_clause").and_then(|v| v.as_str()).unwrap_or(""),
            req.get("legal_reference").and_then(|v| v.as_str()).unwrap_or(""),
        ],
    ).map_err(|e| e400(&e.to_string()))?;

    Ok(Json(json!({ "message": "Statute created." })))
}

pub async fn update_statute(State(pool): Db, _auth: AuthUser, Path(id): Path<i64>, Json(req): Json<serde_json::Value>) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;

    let is_prohibited: Option<i64> = req.get("is_prohibited").and_then(|v| v.as_bool())
        .map(|b| if b { 1 } else { 0 });

    let affected = conn.execute(
        "UPDATE legal_statutes
         SET display_name=COALESCE(?,display_name),
             is_prohibited=COALESCE(?,is_prohibited),
             supdt_goods_clause=COALESCE(?,supdt_goods_clause),
             adjn_goods_clause=COALESCE(?,adjn_goods_clause),
             legal_reference=COALESCE(?,legal_reference)
         WHERE id=?",
        rusqlite::params![
            req.get("display_name").and_then(|v| v.as_str()),
            is_prohibited,
            req.get("supdt_goods_clause").and_then(|v| v.as_str()),
            req.get("adjn_goods_clause").and_then(|v| v.as_str()),
            req.get("legal_reference").and_then(|v| v.as_str()),
            id,
        ],
    ).map_err(|e| e500(&e.to_string()))?;

    if affected == 0 { return Err(e404("Statute not found.")); }
    Ok(Json(json!({ "message": "Statute updated." })))
}

pub async fn delete_statute(State(pool): Db, _auth: AuthUser, Path(id): Path<i64>) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let affected = conn.execute(
        "DELETE FROM legal_statutes WHERE id=?",
        rusqlite::params![id],
    ).map_err(|e| e500(&e.to_string()))?;

    if affected == 0 { return Err(e404("Statute not found.")); }
    Ok(Json(json!({ "message": "Statute deleted." })))
}
