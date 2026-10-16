//! The worker loop: register, claim, run, heartbeat, finish.

use std::time::Duration;

use ferret_api::{
    Client,
    internal::{Heartbeat, InternalClient, RunFinish, RunResult, RunStats, WorkerRegistration},
};
use reqwest::StatusCode;
use uuid::Uuid;

use crate::plugins::{self, Log, PluginError, Run};

/// How long a claim waits on the server for a due run.
const CLAIM_WAIT: Duration = Duration::from_secs(30);
/// Well inside the server's two-minute lease.
const HEARTBEAT_EVERY: Duration = Duration::from_secs(30);
/// After a failed request to the server, before trying again.
const RETRY_AFTER: Duration = Duration::from_secs(5);

pub struct Worker {
    pub internal: InternalClient,
    /// The public API's origin, where runs ingest.
    pub public_url: String,
    pub name: String,
}

impl Worker {
    /// Claims and runs forever, re-registering when the server forgot the worker.
    pub async fn work(&self) -> ! {
        let mut worker_id = self.register().await;
        loop {
            match self.work_once(worker_id, CLAIM_WAIT).await {
                Ok(_) => {}
                Err(error) if error.status() == Some(StatusCode::NOT_FOUND) => {
                    worker_id = self.register().await;
                }
                Err(error) => {
                    tracing::warn!(%error, "claiming a run failed, retrying");
                    tokio::time::sleep(RETRY_AFTER).await;
                }
            }
        }
    }

    /// Registers, retrying until the server answers: workers may start before it.
    pub async fn register(&self) -> Uuid {
        let registration = WorkerRegistration {
            name: self.name.clone(),
            plugins: plugins::registrations(),
        };
        loop {
            match self.internal.register(&registration).await {
                Ok(registered) => return registered.worker_id,
                Err(error) => {
                    tracing::warn!(%error, "registering with the server failed, retrying");
                    tokio::time::sleep(RETRY_AFTER).await;
                }
            }
        }
    }

    /// Waits up to `wait` for a run and executes it. True if there was one.
    pub async fn work_once(&self, worker_id: Uuid, wait: Duration) -> Result<bool, reqwest::Error> {
        let Some(claimed) = self.internal.claim(worker_id, wait).await? else {
            return Ok(false);
        };
        tracing::info!(run = %claimed.run_id, plugin = %claimed.plugin, source = %claimed.source, "run started");
        let run = Run {
            api: Client::new(self.public_url.clone()).with_api_key(claimed.token),
            source: claimed.source,
            config: claimed.config,
            state: claimed.state.clone(),
            log: Log::default(),
        };
        let result = tokio::select! {
            result = plugins::run(&claimed.plugin, &run) => result,
            stopped = self.beat(claimed.run_id, &run.log) => Err(stopped),
        };
        let finish = match result {
            Ok(outcome) => RunFinish {
                result: RunResult::Succeeded,
                state: outcome.state,
                stats: outcome.stats,
                error: None,
                log_lines: run.log.take(),
            },
            Err(error) => {
                run.log.line(format!("failed: {error}"));
                RunFinish {
                    result: RunResult::Failed,
                    state: claimed.state,
                    stats: RunStats::default(),
                    error: Some(error.to_string()),
                    log_lines: run.log.take(),
                }
            }
        };
        tracing::info!(run = %claimed.run_id, result = ?finish.result, "run finished");
        match self.internal.finish(claimed.run_id, &finish).await {
            // The run was lost or its connection deleted meanwhile: nothing left to report to.
            Err(error) if error.status() == Some(StatusCode::CONFLICT) => Ok(true),
            other => other.map(|()| true),
        }
    }

    /// Sends heartbeats with the new log lines until the server says the run is over (lost, or
    /// its connection deleted), which ends the run.
    async fn beat(&self, run_id: Uuid, log: &Log) -> PluginError {
        loop {
            tokio::time::sleep(HEARTBEAT_EVERY).await;
            let heartbeat = Heartbeat {
                log_lines: log.take(),
            };
            match self.internal.heartbeat(run_id, &heartbeat).await {
                Ok(()) => {}
                Err(error) if error.status() == Some(StatusCode::CONFLICT) => {
                    return "the server ended the run (lost, or its connection was deleted)".into();
                }
                // A blip: the lease outlasts a few missed beats.
                Err(error) => tracing::warn!(%error, %run_id, "heartbeat failed"),
            }
        }
    }
}
