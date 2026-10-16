use std::{env, future::IntoFuture};

use ferret_server::{AppState, ConfigKey, SessionKey, StartupError, WorkerSecret};
use sqlx::PgPool;
use tokio::{net::TcpListener, signal, sync::watch};
use tracing_subscriber::{EnvFilter, filter::LevelFilter};

const PUBLIC_ADDR: &str = "0.0.0.0:8080";
/// The plugin workers' API. Not exposed by the Ingress, a NetworkPolicy limits it to the workers.
const INTERNAL_ADDR: &str = "0.0.0.0:8081";

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
    let worker_secret = WorkerSecret::new(required_env("FERRET_WORKER_SECRET")?)?;
    let config_key = ConfigKey::new(&required_env("FERRET_CONFIG_KEY")?)?;
    // Only needed while no enabled admin exists, so its absence is judged by the bootstrap.
    let bootstrap_password = env::var("FERRET_BOOTSTRAP_PASSWORD").ok();

    let pool = PgPool::connect(&database_url).await?;
    ferret_server::migrate(&pool).await?;
    ferret_server::bootstrap_admin(&pool, bootstrap_password.as_deref()).await?;

    let state = AppState::new(pool, session_key, worker_secret, config_key);
    let public = TcpListener::bind(PUBLIC_ADDR).await?;
    let internal = TcpListener::bind(INTERNAL_ADDR).await?;
    tracing::info!(public = PUBLIC_ADDR, internal = INTERNAL_ADDR, "listening");

    let wakeups = tokio::spawn({
        let state = state.clone();
        async move {
            // Degrades, doesn't break: claims then only wake at their next event or timeout.
            if let Err(error) = ferret_server::listen_for_due_runs(state).await {
                tracing::error!(%error, "can't listen for due runs");
            }
        }
    });
    let (stop, stopped) = watch::channel(false);
    tokio::spawn(async move {
        shutdown_requested().await;
        stop.send_replace(true);
    });
    let until_stopped = || {
        let mut stopped = stopped.clone();
        async move {
            let _ = stopped.wait_for(|stopped| *stopped).await;
        }
    };
    tokio::try_join!(
        axum::serve(public, ferret_server::router(state.clone()))
            .with_graceful_shutdown(until_stopped())
            .into_future(),
        axum::serve(internal, ferret_server::internal_router(state))
            .with_graceful_shutdown(until_stopped())
            .into_future(),
    )?;
    wakeups.abort();
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
