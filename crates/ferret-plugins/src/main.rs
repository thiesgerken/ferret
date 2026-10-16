use std::env;

use ferret_api::internal::InternalClient;
use ferret_plugins::worker::Worker;
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

    let worker = Worker {
        internal: InternalClient::new(
            required_env("FERRET_INTERNAL_URL")?,
            required_env("FERRET_WORKER_SECRET")?,
        ),
        public_url: required_env("FERRET_SERVER_URL")?,
        // Informational, so the pod's name is good enough.
        name: env::var("HOSTNAME").unwrap_or_else(|_| "ferret-plugins".to_owned()),
    };
    worker.work().await
}

fn required_env(name: &str) -> Result<String, String> {
    env::var(name).map_err(|_| format!("{name} is not set"))
}
