//! `server` binary. Configuration from the environment:
//! `GRIMTABLE_DB` (default `grimtable.db`), `GRIMTABLE_ROOT` (default: walk
//! up from the current directory to the checkout holding `data/40kdc-data`),
//! `GRIMTABLE_ADDR` (default `127.0.0.1:8080`), `RUST_LOG` for tracing.

use std::path::PathBuf;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,tower_http=info".into())).init();

    let db_path = PathBuf::from(std::env::var("GRIMTABLE_DB").unwrap_or_else(|_| "grimtable.db".into()));
    let root = match std::env::var("GRIMTABLE_ROOT") {
        Ok(r) => PathBuf::from(r),
        Err(_) => server::find_root(&std::env::current_dir()?).ok_or_else(|| anyhow::anyhow!("data/40kdc-data not found; set GRIMTABLE_ROOT or run from the checkout (git submodule update --init)"))?,
    };
    if !root.join("data/40kdc-data").is_dir() {
        anyhow::bail!("{} has no data/40kdc-data", root.display());
    }
    let addr = std::env::var("GRIMTABLE_ADDR").unwrap_or_else(|_| "127.0.0.1:8080".into());

    let state = server::AppState::new(&db_path, root.clone())?;
    let app = server::router(state);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("grimtable server on http://{addr} (db {}, root {})", db_path.display(), root.display());
    axum::serve(listener, app).await?;
    Ok(())
}
