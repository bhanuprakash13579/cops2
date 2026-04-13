use anyhow::Result;
use axum::{
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use axum_extra::{
    headers::{authorization::Bearer, Authorization},
    TypedHeader,
};
use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use serde_json::json;

use once_cell::sync::Lazy;
use crate::config::{JWT_EXPIRY_HOURS, JWT_SECRET};

// ── Admin credentials (read from env at startup) ──────────────────────────────

pub static ADMIN_USERNAME: &str = "sysadmin";

/// Bcrypt hash of admin password.  Resolved once at process startup.
///
/// Priority:
///   1. **Compile-time baked hash** (`ADMIN_PWD_HASH_BAKED` set by `build.rs`)
///      — this is the production path.  The hash is embedded in the binary
///      so no environment variable is needed on the user's machine.
///   2. **Runtime `ADMIN_PASSWORD` env var** (local dev convenience only)
///      — plaintext, hashed on first access.
///   3. `None` — admin login is disabled.
///
/// To change the production admin password:
///   1. Update the `ADMIN_PASSWORD` GitHub secret.
///   2. Push a new tag to trigger CI.  `build.rs` will bake the new hash.
pub static ADMIN_PWD_HASH: Lazy<Option<String>> = Lazy::new(|| {
    // 1. Compile-time baked hash (production builds via CI)
    if let Some(h) = option_env!("ADMIN_PWD_HASH_BAKED") {
        if !h.is_empty() {
            return Some(h.to_string());
        }
    }
    // 2. Runtime env var (local development only)
    if let Ok(p) = std::env::var("ADMIN_PASSWORD") {
        if !p.is_empty() {
            if let Ok(h) = bcrypt::hash(&p, bcrypt::DEFAULT_COST) {
                return Some(h);
            }
        }
    }
    None
});

// ── JWT Claims ────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: String,        // user_id
    pub role: String,       // SDO | DC | AC | TEMP
    pub name: String,
    pub desig: Option<String>,
    pub status: String,     // ACTIVE | TEMP | CLOSED
    pub exp: i64,
}

impl Claims {
    pub fn is_sdo(&self) -> bool   { self.role == "SDO" }
    pub fn is_adjn(&self) -> bool  { self.role == "DC" || self.role == "AC" }
    pub fn is_active(&self) -> bool { self.status != "CLOSED" }
}

// ── Token generation ──────────────────────────────────────────────────────────

pub fn create_token(
    user_id: &str,
    role: &str,
    name: &str,
    desig: Option<&str>,
    status: &str,
) -> Result<String> {
    let exp = (Utc::now() + Duration::hours(JWT_EXPIRY_HOURS)).timestamp();
    let claims = Claims {
        sub: user_id.to_string(),
        role: role.to_string(),
        name: name.to_string(),
        desig: desig.map(|s| s.to_string()),
        status: status.to_string(),
        exp,
    };
    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(JWT_SECRET.as_bytes()),
    )?;
    Ok(token)
}

pub fn verify_token(token: &str) -> Result<Claims> {
    let data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(JWT_SECRET.as_bytes()),
        &Validation::default(),
    )?;
    Ok(data.claims)
}

// ── Axum extractors ───────────────────────────────────────────────────────────

/// Extracts and validates the Bearer token; returns 401 on failure.
pub struct AuthUser(pub Claims);

#[axum::async_trait]
impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let TypedHeader(Authorization(bearer)) =
            TypedHeader::<Authorization<Bearer>>::from_request_parts(parts, state)
                .await
                .map_err(|_| AuthError::Missing)?;

        let claims = verify_token(bearer.token()).map_err(|_| AuthError::Invalid)?;

        if !claims.is_active() {
            return Err(AuthError::Forbidden("Account is closed.".into()));
        }

        Ok(AuthUser(claims))
    }
}

/// SDO-only extractor
pub struct SdoUser(pub Claims);

#[axum::async_trait]
impl<S> FromRequestParts<S> for SdoUser
where
    S: Send + Sync,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let AuthUser(claims) = AuthUser::from_request_parts(parts, state).await?;
        if !claims.is_sdo() {
            return Err(AuthError::Forbidden("SDO role required.".into()));
        }
        Ok(SdoUser(claims))
    }
}

/// DC/AC-only extractor
pub struct AdjnUser(pub Claims);

#[axum::async_trait]
impl<S> FromRequestParts<S> for AdjnUser
where
    S: Send + Sync,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let AuthUser(claims) = AuthUser::from_request_parts(parts, state).await?;
        if !claims.is_adjn() {
            return Err(AuthError::Forbidden("DC/AC role required.".into()));
        }
        Ok(AdjnUser(claims))
    }
}

/// Admin-only extractor — requires role == "system_admin"
pub struct AdminUser(pub Claims);

#[axum::async_trait]
impl<S> FromRequestParts<S> for AdminUser
where
    S: Send + Sync,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let TypedHeader(Authorization(bearer)) =
            TypedHeader::<Authorization<Bearer>>::from_request_parts(parts, state)
                .await
                .map_err(|_| AuthError::Missing)?;

        let claims = verify_token(bearer.token()).map_err(|_| AuthError::Invalid)?;

        if claims.role != "system_admin" {
            return Err(AuthError::Forbidden("Admin access required.".into()));
        }
        Ok(AdminUser(claims))
    }
}

/// Create an admin-specific JWT (role = "system_admin", 8h expiry).
pub fn create_admin_token() -> Result<String> {
    let exp = (Utc::now() + Duration::hours(8)).timestamp();
    let claims = Claims {
        sub: "__sysadmin__".to_string(),
        role: "system_admin".to_string(),
        name: "System Admin".to_string(),
        desig: None,
        status: "ACTIVE".to_string(),
        exp,
    };
    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(JWT_SECRET.as_bytes()),
    )?;
    Ok(token)
}

// ── Error type ────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum AuthError {
    Missing,
    Invalid,
    Forbidden(String),
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let (status, msg) = match self {
            AuthError::Missing  => (StatusCode::UNAUTHORIZED, "Authorization header missing".into()),
            AuthError::Invalid  => (StatusCode::UNAUTHORIZED, "Token invalid or expired".into()),
            AuthError::Forbidden(m) => (StatusCode::FORBIDDEN, m),
        };
        (status, Json(json!({ "detail": msg }))).into_response()
    }
}
