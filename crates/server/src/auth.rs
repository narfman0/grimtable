//! Accounts and sessions. Argon2 password hashes; 32 random bytes per
//! session token, kept in an `HttpOnly` cookie and in the `sessions` table.

use crate::error::{ApiError, ApiResult};
use crate::AppState;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use axum::extract::{FromRequestParts, State};
use axum::http::{header, request::Parts, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use rand::RngCore;
use rusqlite::{params, OptionalExtension};
use serde::Deserialize;
use serde_json::json;

pub const COOKIE: &str = "grimtable_session";

/// The signed-in user, resolved from the session cookie.
#[derive(Clone, Debug)]
pub struct AuthUser {
    pub id: i64,
    pub username: String,
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        let token = parts
            .headers
            .get_all(header::COOKIE)
            .iter()
            .filter_map(|h| h.to_str().ok())
            .flat_map(|h| h.split(';'))
            .filter_map(|c| c.trim().strip_prefix(COOKIE).and_then(|r| r.strip_prefix('=')))
            .next()
            .map(str::to_string)
            .ok_or_else(|| ApiError::unauthorized("not signed in"))?;
        let user = state.db.with(|c| {
            c.query_row(
                "SELECT u.id, u.username FROM sessions s JOIN users u ON u.id = s.user_id WHERE s.token = ?1",
                params![token],
                |r| Ok(AuthUser { id: r.get(0)?, username: r.get(1)? }),
            )
            .optional()
        })?;
        user.ok_or_else(|| ApiError::unauthorized("session expired"))
    }
}

#[derive(Deserialize)]
pub struct Credentials {
    pub username: String,
    pub password: String,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn new_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    hex(&bytes)
}

fn session_cookie(token: &str) -> HeaderValue {
    HeaderValue::from_str(&format!("{COOKIE}={token}; HttpOnly; SameSite=Lax; Path=/")).expect("cookie")
}

fn clear_cookie() -> HeaderValue {
    HeaderValue::from_static("grimtable_session=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0")
}

fn start_session(state: &AppState, user_id: i64) -> ApiResult<String> {
    let token = new_token();
    state.db.with(|c| c.execute("INSERT INTO sessions (token, user_id) VALUES (?1, ?2)", params![token, user_id]))?;
    Ok(token)
}

fn signed_in(status: StatusCode, token: &str, id: i64, username: &str) -> Response {
    let mut resp = (status, Json(json!({ "id": id, "username": username }))).into_response();
    resp.headers_mut().insert(header::SET_COOKIE, session_cookie(token));
    resp
}

pub async fn register(State(state): State<AppState>, Json(body): Json<Credentials>) -> ApiResult<Response> {
    let username = body.username.trim().to_string();
    if username.is_empty() || username.chars().count() > 32 {
        return Err(ApiError::bad_request("username must be 1–32 characters"));
    }
    if body.password.is_empty() {
        return Err(ApiError::bad_request("password must not be empty"));
    }
    let mut salt_bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut salt_bytes);
    let salt = SaltString::encode_b64(&salt_bytes).map_err(|e| ApiError::internal(format!("salt: {e}")))?;
    let hash = Argon2::default().hash_password(body.password.as_bytes(), &salt).map_err(|e| ApiError::internal(format!("hashing: {e}")))?.to_string();
    let inserted = state.db.with(|c| {
        match c.execute("INSERT INTO users (username, password_hash) VALUES (?1, ?2)", params![username, hash]) {
            Ok(_) => Ok(Some(c.last_insert_rowid())),
            Err(rusqlite::Error::SqliteFailure(e, _)) if e.code == rusqlite::ErrorCode::ConstraintViolation => Ok(None),
            Err(e) => Err(e),
        }
    })?;
    let id = inserted.ok_or_else(|| ApiError::conflict("username taken"))?;
    let token = start_session(&state, id)?;
    tracing::info!(user = %username, "registered");
    Ok(signed_in(StatusCode::CREATED, &token, id, &username))
}

pub async fn login(State(state): State<AppState>, Json(body): Json<Credentials>) -> ApiResult<Response> {
    let username = body.username.trim().to_string();
    let row = state.db.with(|c| {
        c.query_row("SELECT id, password_hash FROM users WHERE username = ?1", params![username], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))).optional()
    })?;
    let Some((id, hash)) = row else {
        return Err(ApiError::unauthorized("bad username or password"));
    };
    let parsed = PasswordHash::new(&hash).map_err(|e| ApiError::internal(format!("stored hash: {e}")))?;
    if Argon2::default().verify_password(body.password.as_bytes(), &parsed).is_err() {
        return Err(ApiError::unauthorized("bad username or password"));
    }
    let token = start_session(&state, id)?;
    Ok(signed_in(StatusCode::OK, &token, id, &username))
}

pub async fn logout(State(state): State<AppState>, parts: axum::http::request::Parts) -> ApiResult<Response> {
    // Delete the session named by the cookie, if any; always clear the cookie.
    let token = parts
        .headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|h| h.to_str().ok())
        .flat_map(|h| h.split(';'))
        .filter_map(|c| c.trim().strip_prefix(COOKIE).and_then(|r| r.strip_prefix('=')))
        .next()
        .map(str::to_string);
    if let Some(token) = token {
        state.db.with(|c| c.execute("DELETE FROM sessions WHERE token = ?1", params![token]))?;
    }
    let mut resp = StatusCode::NO_CONTENT.into_response();
    resp.headers_mut().insert(header::SET_COOKIE, clear_cookie());
    Ok(resp)
}

pub async fn me(user: AuthUser) -> Json<serde_json::Value> {
    Json(json!({ "id": user.id, "username": user.username }))
}
