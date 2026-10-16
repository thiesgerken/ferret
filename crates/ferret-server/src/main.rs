use std::env;

use sqlx::PgPool;
use tokio::{net::TcpListener, signal};
use tracing_subscriber::{EnvFilter, filter::LevelFilter};

const PUBLIC_ADDR: &str = "0.0.0.0:8080";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        .init();

    let database_url =
        env::var("FERRET_DATABASE_URL").map_err(|_| "FERRET_DATABASE_URL is not set")?;
    let pool = PgPool::connect(&database_url).await?;
    let app = ferret_server::app(pool).await?;

    let listener = TcpListener::bind(PUBLIC_ADDR).await?;
    tracing::info!(addr = PUBLIC_ADDR, "listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_requested())
        .await?;
    Ok(())
}

/// Ctrl-C locally, SIGTERM from Docker and Kubernetes.
async fn shutdown_requested() {
    let mut terminate = signal::unix::signal(signal::unix::SignalKind::terminate())
        .expect("SIGTERM handler can be installed");
    tokio::select! {
        _ = signal::ctrl_c() => {},
        _ = terminate.recv() => {},
    }
}
