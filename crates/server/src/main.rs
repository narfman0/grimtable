//! Authoritative game server. Only a health endpoint so far; rooms and the
//! WebSocket protocol arrive once the engine can play a turn.

use axum::{routing::get, Router};

#[tokio::main]
async fn main() {
    let app = Router::new().route("/health", get(|| async { "ok" }));
    let addr = "127.0.0.1:8080";
    let listener = tokio::net::TcpListener::bind(addr).await.expect("bind");
    println!("grimtable server listening on http://{addr}");
    axum::serve(listener, app).await.expect("serve");
}
