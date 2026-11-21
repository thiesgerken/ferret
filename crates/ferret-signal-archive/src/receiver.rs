//! The collector side: one JSON-RPC connection to `signal-cli daemon --receive-mode manual`.
//! In manual mode signal-cli leaves envelopes queued on the Signal server until a client asks
//! for them, so nothing is lost while this process or the connection is down.

use std::{
    convert::Infallible,
    time::{Duration, Instant},
};

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

use crate::{
    Store,
    signal_cli::{Contact, Group, ReceiveResult},
};

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
        [] => {
            store.mark_unlinked().await?;
            return Err(retry("signal-cli has no linked account"));
        }
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
    let mut names_refreshed: Option<Instant> = None;
    loop {
        if names_refreshed.is_none_or(|at| at.elapsed() >= NAMES_REFRESH_INTERVAL) {
            refresh_names(&mut daemon, store, &aci).await?;
            names_refreshed = Some(Instant::now());
        }
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
                    if envelope.changes_names() {
                        names_refreshed = None;
                    }
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

/// Profile names change without an envelope that announces it, so they are polled.
const NAMES_REFRESH_INTERVAL: Duration = Duration::from_secs(60 * 60);

/// Stale names are better than no messages: a list call signal-cli fails, or a result of
/// unexpected shape, is logged and receiving goes on. Only a broken connection ends the session.
async fn refresh_names(daemon: &mut Daemon, store: &Store, aci: &str) -> Result<(), SessionError> {
    let contacts = daemon
        .try_call(
            "listContacts",
            json!({"account": aci, "allRecipients": true}),
        )
        .await?;
    let groups = daemon
        .try_call("listGroups", json!({"account": aci}))
        .await?;
    let (Ok(Ok(contacts)), Ok(Ok(groups))) = (
        contacts.map(serde_json::from_value::<Vec<Contact>>),
        groups.map(serde_json::from_value::<Vec<Group>>),
    ) else {
        error!("skipping a name refresh: listContacts or listGroups failed or was unexpected");
        return Ok(());
    };
    // Own messages are rendered as `Me`, so the account's own name never matters.
    let contacts: Vec<_> = contacts
        .into_iter()
        .filter_map(Contact::into_named)
        .filter(|contact| contact.aci != aci)
        .collect();
    store.refresh_names(&contacts, &groups).await?;
    Ok(())
}

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

    /// A call whose failure signal-cli reports ends the session.
    async fn call(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, SessionError> {
        self.try_call(method, params)
            .await?
            .map_err(|CallFailed| retry("signal-cli call failed"))
    }

    /// A call whose failure signal-cli reports is the caller's to handle; it is logged here.
    async fn try_call(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<Result<serde_json::Value, CallFailed>, SessionError> {
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
                return Ok(Err(CallFailed));
            }
            return response
                .result
                .map(Ok)
                .ok_or_else(|| retry("signal-cli response without result"));
        }
    }
}

/// signal-cli answered a call with a JSON-RPC error.
struct CallFailed;

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
