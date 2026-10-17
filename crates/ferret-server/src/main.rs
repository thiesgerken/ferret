use std::{env, path::Path, sync::Arc};

use ferret_server::{AppState, SessionKey, StartupError, config::Config, embeddings};
use sqlx::PgPool;
use tokio::{net::TcpListener, signal};
use tracing_subscriber::{EnvFilter, filter::LevelFilter};

const PUBLIC_ADDR: &str = "0.0.0.0:8080";

#[tokio::main]
async fn main() -> Result<(), StartupError> {
    match env::args().nth(1).as_deref() {
        None => serve().await,
        Some("schema") => {
            print!("{}", ferret_server::graphql::sdl());
            Ok(())
        }
        Some(command) => {
            Err(format!("unknown command {command:?}, run without one or with `schema`").into())
        }
    }
}

async fn serve() -> Result<(), StartupError> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        .init();

    let database_url = required_env("FERRET_DATABASE_URL")?;
    let session_key = SessionKey::new(&required_env("FERRET_SESSION_SECRET")?)?;
    let config = Config::load(Path::new(&required_env("FERRET_CONFIG_FILE")?))?;
    // Only needed while no enabled admin exists, so its absence is judged by the bootstrap.
    let bootstrap_password = env::var("FERRET_BOOTSTRAP_PASSWORD").ok();

    let pool = PgPool::connect(&database_url).await?;
    ferret_server::migrate(&pool).await?;
    ferret_server::bootstrap_admin(&pool, bootstrap_password.as_deref()).await?;
    let embedding = Arc::new(embeddings::prepare(&pool, config.embedding).await?);
    tokio::spawn(embeddings::run_pump(pool.clone(), embedding.clone()));

    let listener = TcpListener::bind(PUBLIC_ADDR).await?;
    tracing::info!(addr = PUBLIC_ADDR, "listening");
    axum::serve(
        listener,
        ferret_server::router(AppState::new(pool, session_key, embedding)),
    )
    .with_graceful_shutdown(shutdown_requested())
    .await?;
    Ok(())
}

fn required_env(name: &str) -> Result<String, String> {
    env::var(name).map_err(|_| format!("{name} is not set"))
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
