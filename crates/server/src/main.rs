//! `server` binary. Configuration from the environment:
//! `DATABASE_URL` (required: `postgres://user:pass@host/db`),
//! `GRIMTABLE_ROOT` (default: walk up from the current directory to the
//! checkout holding `data/40kdc-data`), `GRIMTABLE_ADDR` (default
//! `127.0.0.1:8080`), `GRIMTABLE_SECURE_COOKIES` (`1` behind HTTPS),
//! `RUST_LOG` for tracing.

use std::path::PathBuf;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,tower_http=info".into())).init();

    let db_url = std::env::var("DATABASE_URL").map_err(|_| anyhow::anyhow!("DATABASE_URL is not set (postgres://user:pass@host/db)"))?;
    let root = match std::env::var("GRIMTABLE_ROOT") {
        Ok(r) => PathBuf::from(r),
        Err(_) => server::find_root(&std::env::current_dir()?).ok_or_else(|| anyhow::anyhow!("data/40kdc-data not found; set GRIMTABLE_ROOT or run from the checkout (git submodule update --init)"))?,
    };
    if !root.join("data/40kdc-data").is_dir() {
        anyhow::bail!("{} has no data/40kdc-data", root.display());
    }
    let addr = std::env::var("GRIMTABLE_ADDR").unwrap_or_else(|_| "127.0.0.1:8080".into());
    let secure_cookies = matches!(std::env::var("GRIMTABLE_SECURE_COOKIES").as_deref(), Ok("1") | Ok("true"));

    let db = server::db::connect(&db_url).await?;
    let state = server::AppState::new(db, root.clone(), secure_cookies);
    let app = server::router(state);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("grimtable server on http://{addr} (root {})", root.display());
    axum::serve(listener, app).with_graceful_shutdown(shutdown()).await?;
    Ok(())
}

/// SIGINT or SIGTERM. As PID 1 in a container the process gets no default
/// handler for either, so without this a stop waits out the kill timeout.
async fn shutdown() {
    let int = tokio::signal::ctrl_c();
    #[cfg(unix)]
    let term = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! {
        _ = int => {}
        _ = term => {}
    }
    tracing::info!("shutting down");
}
