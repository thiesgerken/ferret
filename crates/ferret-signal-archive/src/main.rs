use std::{env, fs};

use ferret_signal_archive::{ImportSummary, Store, import, receive, router};
use tokio::signal::unix::{SignalKind, signal};
use tracing::info;
use tracing_subscriber::{EnvFilter, filter::LevelFilter};

type StartupError = Box<dyn std::error::Error + Send + Sync>;

#[tokio::main]
async fn main() -> Result<(), StartupError> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        .init();
    let store = Store::open(required_env("SIGNAL_ARCHIVE_DB")?).await?;
    match env::args().skip(1).collect::<Vec<_>>().as_slice() {
        [] => serve(store).await,
        [command, path] if command == "import" => {
            let ImportSummary {
                inserted,
                present,
                skipped,
                unkeyed,
            } = import(&store, path.as_ref()).await?;
            info!(
                inserted,
                present, skipped, unkeyed, "imported the chat export"
            );
            Ok(())
        }
        _ => Err("usage: ferret-signal-archive [import <main.jsonl>]".into()),
    }
}

async fn serve(store: Store) -> Result<(), StartupError> {
    let token = fs::read_to_string(required_env("SIGNAL_ARCHIVE_TOKEN_FILE")?)
        .map_err(|_| "cannot read archive token file")?;
    let app = router(store.clone(), &token)?;
    let signal_cli = required_env("SIGNAL_CLI_ADDRESS")?;
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    let mut terminate = signal(SignalKind::terminate())?;
    tokio::select! {
        served = axum::serve(listener, app).into_future() => served?,
        received = receive(store, signal_cli) => {
            received?;
        }
        _ = terminate.recv() => {}
        _ = tokio::signal::ctrl_c() => {}
    }
    Ok(())
}

fn required_env(name: &str) -> Result<String, String> {
    env::var(name).map_err(|_| format!("{name} is required"))
}
