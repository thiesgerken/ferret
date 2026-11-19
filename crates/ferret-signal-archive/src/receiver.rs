//! The collector side: one JSON-RPC connection to `signal-cli daemon --receive-mode manual`.
//! In manual mode signal-cli leaves envelopes queued on the Signal server until a client asks
//! for them, so nothing is lost while this process or the connection is down.

use std::{convert::Infallible, time::Duration};

use serde::Deserialize;
use serde_json::json;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines},
    net::{
        TcpStream,
        tcp::{OwnedReadHalf, OwnedWriteHalf},
    },
};
use tracing::{error, info, warn};

use crate::{Store, signal_cli::ReceiveResult};

const RECONNECT_DELAY: Duration = Duration::from_secs(5);

/// Receives from the daemon at `address` (`host:port` of `daemon --tcp`) forever, reconnecting
/// whenever the connection drops. Returns only on a fatal error: the chat archive cannot be
/// written, or the daemon serves a different account than the chat archive holds.
pub async fn receive(store: Store, address: String) -> Result<Infallible, Fatal> {
    loop {
        let Err(error) = session(&store, &address).await;
        match error {
            SessionError::Retry(error) => warn!(%error, "signal-cli session failed"),
            SessionError::Fatal(error) => return Err(error),
        }
        tokio::time::sleep(RECONNECT_DELAY).await;
    }
}

pub type Fatal = Box<dyn std::error::Error + Send + Sync>;

enum SessionError {
    Retry(std::io::Error),
    Fatal(Fatal),
}

impl From<std::io::Error> for SessionError {
    fn from(error: std::io::Error) -> Self {
        Self::Retry(error)
    }
}

impl From<sqlx::Error> for SessionError {
    fn from(error: sqlx::Error) -> Self {
        Self::Fatal(error.into())
    }
}

impl From<Fatal> for SessionError {
    fn from(error: Fatal) -> Self {
        Self::Fatal(error)
    }
}

fn retry(reason: &str) -> SessionError {
    SessionError::Retry(std::io::Error::other(reason))
}

async fn session(store: &Store, address: &str) -> Result<Infallible, SessionError> {
    let mut daemon = Daemon::connect(address).await?;
    let accounts: Vec<Account> =
        serde_json::from_value(daemon.call("listAccounts", json!({})).await?)
            .map_err(|_| retry("unexpected listAccounts result"))?;
    let aci = match accounts.as_slice() {
        [] => return Err(retry("signal-cli has no linked account yet")),
        [Account { aci: Some(aci) }] => aci.clone(),
        [_] => return Err(retry("signal-cli's account has no ACI yet")),
        _ => {
            return Err(SessionError::Fatal(
                "signal-cli serves several accounts; the chat archive holds one".into(),
            ));
        }
    };
    store.adopt_self_aci(&aci).await?;
    info!("receiving from signal-cli");
    // One envelope per call: signal-cli acknowledges an envelope to the Signal server before
    // handing it over, so this bounds what a crash can lose to the one envelope in flight.
    let receive = json!({"account": aci, "maxMessages": 1, "timeout": RECEIVE_TIMEOUT_SECONDS});
    loop {
        let results = daemon.call("receive", receive.clone()).await?;
        let results: Vec<serde_json::Value> =
            serde_json::from_value(results).map_err(|_| retry("unexpected receive result"))?;
        for result in results {
            store.record_arrival().await?;
            // Logs never carry serde's messages, which can quote message content.
            match serde_json::from_value::<ReceiveResult>(result) {
                Ok(ReceiveResult {
                    envelope: Some(envelope),
                }) => {
                    if let Some(incoming) = envelope.into_incoming() {
                        store.apply(incoming).await?;
                    }
                }
                Ok(ReceiveResult { envelope: None }) => {}
                Err(_) => error!("skipping a received envelope of unexpected shape"),
            }
        }
    }
}

/// How long one `receive` call waits for an envelope before returning none.
const RECEIVE_TIMEOUT_SECONDS: f64 = 60.0;

struct Daemon {
    lines: Lines<BufReader<OwnedReadHalf>>,
    writer: OwnedWriteHalf,
    last_id: i64,
}

impl Daemon {
    async fn connect(address: &str) -> Result<Self, SessionError> {
        let (reader, writer) = TcpStream::connect(address).await?.into_split();
        Ok(Self {
            lines: BufReader::new(reader).lines(),
            writer,
            last_id: 0,
        })
    }

    async fn call(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, SessionError> {
        self.last_id += 1;
        let request =
            json!({"jsonrpc": "2.0", "id": self.last_id, "method": method, "params": params});
        self.writer
            .write_all(format!("{request}\n").as_bytes())
            .await?;
        loop {
            let line = self
                .lines
                .next_line()
                .await?
                .ok_or_else(|| retry("signal-cli closed the connection"))?;
            let Ok(response) = serde_json::from_str::<Response>(&line) else {
                continue;
            };
            if response.id != Some(self.last_id) {
                continue;
            }
            if let Some(error) = response.error {
                error!(
                    method,
                    code = error.code,
                    message = error.message,
                    "signal-cli call failed"
                );
                return Err(retry("signal-cli call failed"));
            }
            return response
                .result
                .ok_or_else(|| retry("signal-cli response without result"));
        }
    }
}

#[derive(Deserialize)]
struct Response {
    id: Option<i64>,
    result: Option<serde_json::Value>,
    error: Option<RpcError>,
}

#[derive(Deserialize)]
struct RpcError {
    code: i64,
    message: String,
}

#[derive(Deserialize)]
struct Account {
    aci: Option<String>,
}
