//! The collector side: one JSON-RPC connection to `signal-cli daemon --receive-mode manual`.
//! In manual mode signal-cli leaves envelopes queued on the Signal server until a client
//! subscribes, so nothing is lost while this process or the connection is down.

use std::{convert::Infallible, time::Duration};

use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
};
use tracing::{error, info, warn};

use crate::{
    Store,
    signal_cli::{Notification, ReceiveParams},
};

const RECONNECT_DELAY: Duration = Duration::from_secs(5);

/// Receives from the daemon at `address` (`host:port` of `daemon --tcp`) forever, reconnecting
/// whenever the connection drops. Returns only when the chat archive cannot be written.
pub async fn receive(store: Store, address: String) -> Result<Infallible, sqlx::Error> {
    loop {
        match session(&store, &address).await {
            Ok(()) => warn!("signal-cli closed the connection"),
            Err(SessionError::Connection(error)) => warn!(%error, "signal-cli connection failed"),
            Err(SessionError::Store(error)) => return Err(error),
        }
        tokio::time::sleep(RECONNECT_DELAY).await;
    }
}

enum SessionError {
    Connection(std::io::Error),
    Store(sqlx::Error),
}

impl From<std::io::Error> for SessionError {
    fn from(error: std::io::Error) -> Self {
        Self::Connection(error)
    }
}

impl From<sqlx::Error> for SessionError {
    fn from(error: sqlx::Error) -> Self {
        Self::Store(error)
    }
}

async fn session(store: &Store, address: &str) -> Result<(), SessionError> {
    let (reader, mut writer) = TcpStream::connect(address).await?.into_split();
    writer
        .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"subscribeReceive\"}\n")
        .await?;
    info!("subscribed to signal-cli");
    let mut lines = BufReader::new(reader).lines();
    while let Some(line) = lines.next_line().await? {
        handle(store, &line).await?;
    }
    Ok(())
}

/// Envelopes are already acknowledged to the Signal server when signal-cli forwards them, so a
/// malformed one is logged and skipped: failing would not get it delivered again. Logs never
/// carry serde's messages, which can quote message content.
async fn handle(store: &Store, line: &str) -> Result<(), SessionError> {
    let notification: Notification = match serde_json::from_str(line) {
        Ok(notification) => notification,
        Err(error) => {
            error!(
                column = error.column(),
                "skipping unparseable JSON-RPC line"
            );
            return Ok(());
        }
    };
    if let Some(rpc_error) = notification.error {
        error!(
            code = rpc_error.code,
            message = rpc_error.message,
            "signal-cli refused the subscription"
        );
        return Err(std::io::Error::other("subscribeReceive failed").into());
    }
    if notification.method.as_deref() != Some("receive") {
        return Ok(());
    }
    let params: ReceiveParams = match notification.params.map(serde_json::from_value) {
        Some(Ok(params)) => params,
        _ => {
            error!("skipping a receive notification of unexpected shape");
            return Ok(());
        }
    };
    store.record_arrival().await?;
    if let Some(message) = params
        .result
        .envelope
        .and_then(|envelope| envelope.into_received())
    {
        store.insert(message).await?;
    }
    Ok(())
}
