use std::env::{self, VarError};

use ferret_api::internal::InternalClient;
use ferret_plugins::{plugins::PluginOptions, worker::Worker};
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
        options: plugin_options()?,
    };
    worker.work().await
}

fn plugin_options() -> Result<PluginOptions, String> {
    match env::var("FERRET_GIT_PROTOCOLS") {
        Err(VarError::NotPresent) => Ok(PluginOptions::default()),
        Ok(git_protocols) if !git_protocols.is_empty() => Ok(PluginOptions { git_protocols }),
        // Empty would let git use no transport at all, and every run would fail far from here.
        _ => Err("FERRET_GIT_PROTOCOLS is empty or not Unicode".to_owned()),
    }
}

fn required_env(name: &str) -> Result<String, String> {
    env::var(name).map_err(|_| format!("{name} is not set"))
}
