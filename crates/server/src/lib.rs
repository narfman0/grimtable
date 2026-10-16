//! Authoritative game server. Thin: accounts, armies, lobbies and one
//! WebSocket per connected client; the engine decides everything else.
//! The HTTP and WebSocket contract is `docs/multiplayer.md`.

pub mod armies;
pub mod auth;
pub mod catalogue;
pub mod db;
pub mod error;
pub mod games;

use axum::http::{header, HeaderValue, Method};
use axum::routing::{get, post};
use axum::Router;
use sqlx::PgPool;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

/// Shared state behind every handler.
#[derive(Clone)]
pub struct AppState(pub Arc<Inner>);

pub struct Inner {
    pub db: PgPool,
    pub root: PathBuf,
    pub cache: catalogue::Cache,
    pub rooms: games::Rooms,
    /// Mark the session cookie `Secure` (the server is behind HTTPS).
    pub secure_cookies: bool,
}

impl AppState {
    pub fn new(db: PgPool, root: PathBuf, secure_cookies: bool) -> AppState {
        let cache = catalogue::Cache::new(root.clone());
        AppState(Arc::new(Inner { db, root, cache, rooms: games::Rooms::default(), secure_cookies }))
    }
}

impl std::ops::Deref for AppState {
    type Target = Inner;
    fn deref(&self) -> &Inner {
        &self.0
    }
}

/// Walk up from `start` until a directory holding `data/40kdc-data` is found.
pub fn find_root(start: &Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        if dir.join("data/40kdc-data").is_dir() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// The full router: `/api/*`, then static files from `client/dist` when it exists.
pub fn router(state: AppState) -> Router {
    let api = Router::new()
        .route("/register", post(auth::register))
        .route("/login", post(auth::login))
        .route("/logout", post(auth::logout))
        .route("/me", get(auth::me))
        .route("/catalogue", get(catalogue::list))
        .route("/catalogue/{faction_id}", get(catalogue::get_one))
        .route("/armies", get(armies::list).post(armies::create))
        .route("/armies/import", post(armies::import))
        .route("/armies/{id}", get(armies::get_one).put(armies::update).delete(armies::delete))
        .route("/armies/{id}/pack", get(armies::pack))
        .route("/games", post(games::create))
        .route("/games/open", get(games::open))
        .route("/games/mine", get(games::mine))
        .route("/games/join", post(games::join_by_code))
        .route("/games/{id}", get(games::get_one))
        .route("/games/{id}/join", post(games::join_by_id))
        .route("/games/{id}/start", post(games::start))
        .route("/games/{id}/leave", post(games::leave))
        .route("/games/{id}/ws", get(games::ws))
        .fallback(|| async { error::ApiError::not_found("no such endpoint") });

    let cors = CorsLayer::new()
        .allow_origin("http://localhost:5173".parse::<HeaderValue>().expect("origin"))
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE, Method::OPTIONS])
        .allow_headers([header::CONTENT_TYPE])
        .allow_credentials(true);

    let mut app = Router::new().route("/health", get(|| async { "ok" })).nest("/api", api);
    let dist = state.root.join("client/dist");
    if dist.is_dir() {
        let index = ServeFile::new(dist.join("index.html"));
        app = app.fallback_service(ServeDir::new(&dist).fallback(index));
        tracing::info!("serving static files from {}", dist.display());
    }
    app.layer(cors).layer(TraceLayer::new_for_http()).with_state(state)
}
