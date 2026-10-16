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
use serde::Deserialize;
use serde_json::json;

pub const COOKIE: &str = "grimtable_session";

/// How long a session lives without use. Each request within the last day
/// of a session's life renews it. Override with `GRIMTABLE_SESSION_DAYS`.
pub fn session_days() -> i64 {
    std::env::var("GRIMTABLE_SESSION_DAYS").ok().and_then(|v| v.parse().ok()).filter(|&d| d > 0).unwrap_or(30)
}

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
        let days = session_days();
        let user: Option<(i64, String)> = sqlx::query_as(concat!(
            "SELECT u.id, u.username FROM sessions s JOIN users u ON u.id = s.user_id WHERE s.token = $1 AND (s.expires_at IS NULL OR s.expires_at > ",
            crate::now_sql!(),
            ")"
        ))
        .bind(&token)
        .fetch_optional(&state.db)
        .await?;
        if user.is_some() {
            // Sliding expiry: renew once the session is into its last day.
            sqlx::query(concat!(
                "UPDATE sessions SET expires_at = ",
                crate::utc_text!("now() + make_interval(days => $1::int)"),
                " WHERE token = $2 AND (expires_at IS NULL OR expires_at < ",
                crate::utc_text!("now() + make_interval(days => $1::int - 1)"),
                ")"
            ))
            .bind(days)
            .bind(&token)
            .execute(&state.db)
            .await?;
        } else {
            sqlx::query("DELETE FROM sessions WHERE token = $1").bind(&token).execute(&state.db).await?;
        }
        user.map(|(id, username)| AuthUser { id, username }).ok_or_else(|| ApiError::unauthorized("session expired"))
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

/// `; Secure` when the server sits behind HTTPS (`GRIMTABLE_SECURE_COOKIES`),
/// so the cookie is never sent in the clear. Off for plain-HTTP local runs.
fn secure_attr(state: &AppState) -> &'static str {
    if state.secure_cookies {
        "; Secure"
    } else {
        ""
    }
}

fn session_cookie(state: &AppState, token: &str) -> HeaderValue {
    let max_age = session_days() * 24 * 60 * 60;
    HeaderValue::from_str(&format!("{COOKIE}={token}; HttpOnly; SameSite=Lax; Path=/; Max-Age={max_age}{}", secure_attr(state))).expect("cookie")
}

fn clear_cookie(state: &AppState) -> HeaderValue {
    HeaderValue::from_str(&format!("{COOKIE}=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0{}", secure_attr(state))).expect("cookie")
}

async fn start_session(state: &AppState, user_id: i64) -> ApiResult<String> {
    let token = new_token();
    // Housekeeping: drop sessions that have expired.
    sqlx::query(concat!("DELETE FROM sessions WHERE expires_at IS NOT NULL AND expires_at <= ", crate::now_sql!())).execute(&state.db).await?;
    sqlx::query(concat!("INSERT INTO sessions (token, user_id, expires_at) VALUES ($1, $2, ", crate::utc_text!("now() + make_interval(days => $3::int)"), ")"))
        .bind(&token)
        .bind(user_id)
        .bind(session_days())
        .execute(&state.db)
        .await?;
    Ok(token)
}

fn signed_in(state: &AppState, status: StatusCode, token: &str, id: i64, username: &str) -> Response {
    let mut resp = (status, Json(json!({ "id": id, "username": username }))).into_response();
    resp.headers_mut().insert(header::SET_COOKIE, session_cookie(state, token));
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
    let inserted: Result<i64, sqlx::Error> =
        sqlx::query_scalar("INSERT INTO users (username, password_hash) VALUES ($1, $2) RETURNING id").bind(&username).bind(&hash).fetch_one(&state.db).await;
    let id = match inserted {
        Ok(id) => id,
        Err(sqlx::Error::Database(e)) if e.is_unique_violation() => return Err(ApiError::conflict("username taken")),
        Err(e) => return Err(e.into()),
    };
    let token = start_session(&state, id).await?;
    tracing::info!(user = %username, "registered");
    Ok(signed_in(&state, StatusCode::CREATED, &token, id, &username))
}

pub async fn login(State(state): State<AppState>, Json(body): Json<Credentials>) -> ApiResult<Response> {
    let username = body.username.trim().to_string();
    let row: Option<(i64, String)> = sqlx::query_as("SELECT id, password_hash FROM users WHERE username = $1").bind(&username).fetch_optional(&state.db).await?;
    let Some((id, hash)) = row else {
        return Err(ApiError::unauthorized("bad username or password"));
    };
    let parsed = PasswordHash::new(&hash).map_err(|e| ApiError::internal(format!("stored hash: {e}")))?;
    if Argon2::default().verify_password(body.password.as_bytes(), &parsed).is_err() {
        return Err(ApiError::unauthorized("bad username or password"));
    }
    let token = start_session(&state, id).await?;
    Ok(signed_in(&state, StatusCode::OK, &token, id, &username))
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
        sqlx::query("DELETE FROM sessions WHERE token = $1").bind(&token).execute(&state.db).await?;
    }
    let mut resp = StatusCode::NO_CONTENT.into_response();
    resp.headers_mut().insert(header::SET_COOKIE, clear_cookie(&state));
    Ok(resp)
}

pub async fn me(user: AuthUser) -> Json<serde_json::Value> {
    Json(json!({ "id": user.id, "username": user.username }))
}
