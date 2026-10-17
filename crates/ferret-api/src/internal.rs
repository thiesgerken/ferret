//! The internal API between the server and plugin workers, on its own port, authenticated with
//! the worker PSK (`FERRET_WORKER_SECRET`).

use std::time::Duration;

use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// `POST /internal/workers/register`: a worker starts and announces its plugins.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerRegistration {
    /// Informational, e.g. the pod name.
    pub name: String,
    pub plugins: Vec<PluginRegistration>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginRegistration {
    pub name: String,
    /// JSON Schema of the plugin's config. The UI renders the connection form from it, the server
    /// validates configs against it.
    pub config_schema: Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Registered {
    pub worker_id: Uuid,
}

/// `POST /internal/runs/claim?wait=<seconds>`
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimRequest {
    pub worker_id: Uuid,
}

/// A run a worker claimed: everything the plugin needs.
#[derive(Debug, Serialize, Deserialize)]
pub struct ClaimedRun {
    pub run_id: Uuid,
    pub connection_id: Uuid,
    pub plugin: String,
    /// The connection's source root. The run writes to it and below it.
    pub source: String,
    pub config: Value,
    /// The cursor the last successful run left, `{}` at first.
    pub state: Value,
    /// Bearer token for the public REST API: `ingest:write` below `source`, valid while the run
    /// is live.
    pub token: String,
}

/// `POST /internal/runs/{id}/heartbeat`: keeps the run alive, carries new log lines.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Heartbeat {
    pub log_lines: Vec<String>,
}

/// `POST /internal/runs/{id}/finish`, e.g. `{"result": "failed", "error": "…", "log_lines": []}`.
// No `deny_unknown_fields`: serde doesn't support it next to `flatten`.
#[derive(Debug, Serialize, Deserialize)]
pub struct RunFinish {
    #[serde(flatten)]
    pub result: RunResult,
    /// Log lines since the last heartbeat.
    pub log_lines: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "lowercase")]
pub enum RunResult {
    /// `state` is the new cursor.
    Succeeded {
        state: Value,
        stats: RunStats,
    },
    Failed {
        error: String,
    },
}

/// What a run did to the index.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunStats {
    pub put: u64,
    pub unchanged: u64,
    pub deleted: u64,
}

/// Requests other than a claim answer right away, so this only trips on a server or connection
/// that stopped answering.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// A client for the internal API.
#[derive(Clone)]
pub struct InternalClient {
    http: reqwest::Client,
    base_url: String,
    secret: String,
}

impl InternalClient {
    /// `base_url` is the internal API's origin, e.g. `http://ferret-server:8081`.
    pub fn new(base_url: impl Into<String>, secret: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(10))
                .timeout(REQUEST_TIMEOUT)
                .build()
                .expect("the HTTP client builds"),
            base_url: base_url.into(),
            secret: secret.into(),
        }
    }

    pub async fn register(
        &self,
        registration: &WorkerRegistration,
    ) -> Result<Registered, reqwest::Error> {
        self.post("/internal/workers/register")
            .json(registration)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
    }

    /// Waits up to `wait` for a due run of one of the worker's plugins, `None` if there was none.
    pub async fn claim(
        &self,
        worker_id: Uuid,
        wait: Duration,
    ) -> Result<Option<ClaimedRun>, reqwest::Error> {
        let response = self
            .post(&format!("/internal/runs/claim?wait={}", wait.as_secs()))
            .timeout(wait + REQUEST_TIMEOUT)
            .json(&ClaimRequest { worker_id })
            .send()
            .await?
            .error_for_status()?;
        if response.status() == StatusCode::NO_CONTENT {
            return Ok(None);
        }
        response.json().await.map(Some)
    }

    /// Fails with 409 once the run is no longer live (lost, or its connection deleted).
    pub async fn heartbeat(
        &self,
        run_id: Uuid,
        heartbeat: &Heartbeat,
    ) -> Result<(), reqwest::Error> {
        self.post(&format!("/internal/runs/{run_id}/heartbeat"))
            .json(heartbeat)
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    pub async fn finish(&self, run_id: Uuid, finish: &RunFinish) -> Result<(), reqwest::Error> {
        self.post(&format!("/internal/runs/{run_id}/finish"))
            .json(finish)
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    fn post(&self, path: &str) -> reqwest::RequestBuilder {
        self.http
            .post(format!("{}{path}", self.base_url))
            .bearer_auth(&self.secret)
    }
}
