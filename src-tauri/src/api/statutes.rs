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
        "SELECT id, statute_code, act_name, section_no, description, penalty_desc,
                is_active, created_on
         FROM legal_statutes ORDER BY act_name, section_no"
    ).map_err(|e| e500(&e.to_string()))?;

    let rows: Vec<Value> = stmt.query_map([], |r| {
        Ok(json!({
            "id":           r.get::<_, i64>(0)?,
            "statute_code": r.get::<_, Option<String>>(1)?,
            "act_name":     r.get::<_, Option<String>>(2)?,
            "section_no":   r.get::<_, Option<String>>(3)?,
            "description":  r.get::<_, Option<String>>(4)?,
            "penalty_desc": r.get::<_, Option<String>>(5)?,
            "is_active":    r.get::<_, Option<String>>(6)?,
            "created_on":   r.get::<_, Option<String>>(7)?,
        }))
    }).map_err(|e| e500(&e.to_string()))?.filter_map(|r| r.ok()).collect();

    Ok(Json(json!(rows)))
}

pub async fn create_statute(State(pool): Db, _auth: AuthUser, Json(req): Json<serde_json::Value>) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let act_name = req.get("act_name").and_then(|v| v.as_str())
        .ok_or_else(|| e400("act_name is required"))?;

    conn.execute(
        "INSERT INTO legal_statutes (statute_code, act_name, section_no, description, penalty_desc, is_active, created_on)
         VALUES (?,?,?,?,?,?,?)",
        rusqlite::params![
            req.get("statute_code").and_then(|v| v.as_str()),
            act_name,
            req.get("section_no").and_then(|v| v.as_str()),
            req.get("description").and_then(|v| v.as_str()),
            req.get("penalty_desc").and_then(|v| v.as_str()),
            "Y",
            today,
        ],
    ).map_err(|e| e400(&e.to_string()))?;

    Ok(Json(json!({ "message": "Statute created." })))
}

pub async fn update_statute(State(pool): Db, _auth: AuthUser, Path(id): Path<i64>, Json(req): Json<serde_json::Value>) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;

    let affected = conn.execute(
        "UPDATE legal_statutes SET statute_code=COALESCE(?,statute_code),
         act_name=COALESCE(?,act_name), section_no=COALESCE(?,section_no),
         description=COALESCE(?,description), penalty_desc=COALESCE(?,penalty_desc),
         is_active=COALESCE(?,is_active)
         WHERE id=?",
        rusqlite::params![
            req.get("statute_code").and_then(|v| v.as_str()),
            req.get("act_name").and_then(|v| v.as_str()),
            req.get("section_no").and_then(|v| v.as_str()),
            req.get("description").and_then(|v| v.as_str()),
            req.get("penalty_desc").and_then(|v| v.as_str()),
            req.get("is_active").and_then(|v| v.as_str()),
            id,
        ],
    ).map_err(|e| e500(&e.to_string()))?;

    if affected == 0 { return Err(e404("Statute not found.")); }
    Ok(Json(json!({ "message": "Statute updated." })))
}

pub async fn delete_statute(State(pool): Db, _auth: AuthUser, Path(id): Path<i64>) -> Result<Json<Value>, Err> {
    let conn = pool.get().map_err(|e| e500(&e.to_string()))?;
    let affected = conn.execute(
        "UPDATE legal_statutes SET is_active='N' WHERE id=?",
        rusqlite::params![id],
    ).map_err(|e| e500(&e.to_string()))?;

    if affected == 0 { return Err(e404("Statute not found.")); }
    Ok(Json(json!({ "message": "Statute deactivated." })))
}
