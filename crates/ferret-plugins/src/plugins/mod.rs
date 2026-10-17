//! The plugins this worker can run, and what a run gets.

use std::sync::{Arc, Mutex};

use ferret_api::{
    Client,
    internal::{PluginRegistration, RunStats},
};
use serde_json::Value;

pub mod carapace;
pub mod git;
pub mod repo;

pub type PluginError = Box<dyn std::error::Error + Send + Sync>;

/// Every plugin this binary knows, as the server learns them at registration.
pub fn registrations() -> Vec<PluginRegistration> {
    vec![
        PluginRegistration {
            name: carapace::NAME.to_owned(),
            config_schema: serde_json::to_value(schemars::schema_for!(carapace::CarapaceConfig))
                .expect("a schema serializes"),
        },
        PluginRegistration {
            name: git::NAME.to_owned(),
            config_schema: serde_json::to_value(schemars::schema_for!(git::GitConfig))
                .expect("a schema serializes"),
        },
    ]
}

/// What a plugin gets for one run.
pub struct Run {
    /// The public API, authenticated with the run's token: `ingest:write` below `source`.
    pub api: Client,
    /// The connection's source root.
    pub source: String,
    pub config: Value,
    /// The cursor the last successful run left, `{}` at first.
    pub state: Value,
    pub log: Log,
    pub options: PluginOptions,
}

/// How the worker lets plugins reach out, fixed per deployment rather than per connection.
#[derive(Clone)]
pub struct PluginOptions {
    /// The transports git may use, as `GIT_ALLOW_PROTOCOL` takes them (colon-separated).
    pub git_protocols: String,
}

impl Default for PluginOptions {
    fn default() -> Self {
        Self {
            git_protocols: "https".to_owned(),
        }
    }
}

/// What a successful run hands back.
pub struct Outcome {
    pub state: Value,
    pub stats: RunStats,
}

/// Log lines of a run, shipped to the server with the next heartbeat.
#[derive(Clone, Default)]
pub struct Log(Arc<Mutex<Vec<String>>>);

impl Log {
    pub fn line(&self, line: impl Into<String>) {
        let line = line.into();
        tracing::info!(%line, "run log");
        self.0
            .lock()
            .expect("the log lock isn't poisoned")
            .push(line);
    }

    /// The lines since the last call.
    pub fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.0.lock().expect("the log lock isn't poisoned"))
    }

    /// Returns lines that `take` handed out but couldn't be delivered, ahead of newer ones.
    pub fn put_back(&self, lines: Vec<String>) {
        let mut log = self.0.lock().expect("the log lock isn't poisoned");
        log.splice(0..0, lines);
    }
}

pub async fn run(plugin: &str, run: &Run) -> Result<Outcome, PluginError> {
    match plugin {
        carapace::NAME => carapace::run(run).await,
        git::NAME => git::run(run).await,
        other => Err(format!("this worker doesn't know the plugin {other:?}").into()),
    }
}
