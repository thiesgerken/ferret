use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, tungstenite::Message};

use super::PluginError;

const DEADLINE: Duration = Duration::from_secs(60);

pub struct Session {
    socket: WebSocketStream<MaybeTlsStream<TcpStream>>,
    next_id: u64,
}

pub fn instance_url(value: &str) -> Result<reqwest::Url, PluginError> {
    let mut url = reqwest::Url::parse(value)
        .map_err(|_| "Home Assistant needs an absolute HTTP(S) instance root URL")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "" | "/")
    {
        return Err("Home Assistant needs an HTTP(S) instance root without credentials, path, query or fragment".into());
    }
    url.set_path("/");
    Ok(url)
}

impl Session {
    pub async fn connect(mut url: reqwest::Url, token: &str) -> Result<Self, PluginError> {
        let scheme = if url.scheme() == "https" { "wss" } else { "ws" };
        url.set_scheme(scheme)
            .map_err(|_| "Invalid Home Assistant WebSocket URL")?;
        url.set_path("/api/websocket");
        // No redirect following: credentials belong only to the configured origin.
        let (socket, _) =
            tokio::time::timeout(DEADLINE, tokio_tungstenite::connect_async(url.as_str()))
                .await
                .map_err(|_| "Home Assistant connection timed out")?
                .map_err(|_| "Home Assistant WebSocket connection failed")?;
        let mut session = Self { socket, next_id: 1 };
        tokio::time::timeout(DEADLINE, async {
            if session.receive().await?["type"] != "auth_required" {
                return Err::<(), PluginError>(
                    "Home Assistant did not request authentication".into(),
                );
            }
            session
                .send(&json!({"type": "auth", "access_token": token}))
                .await?;
            if session.receive().await?["type"] != "auth_ok" {
                return Err(
                    "Home Assistant authentication failed; check the dedicated administrator token"
                        .into(),
                );
            }
            Ok(())
        })
        .await
        .map_err(|_| "Home Assistant authentication timed out")??;
        Ok(session)
    }

    async fn send(&mut self, value: &Value) -> Result<(), PluginError> {
        self.socket
            .send(Message::Text(value.to_string().into()))
            .await
            .map_err(|_| "Home Assistant WebSocket write failed".into())
    }

    async fn receive(&mut self) -> Result<Value, PluginError> {
        loop {
            match self.socket.next().await {
                Some(Ok(Message::Text(text))) => {
                    return serde_json::from_str(&text)
                        .map_err(|_| "Home Assistant returned invalid JSON".into());
                }
                Some(Ok(Message::Ping(payload))) => self
                    .socket
                    .send(Message::Pong(payload))
                    .await
                    .map_err(|_| "Home Assistant WebSocket pong failed")?,
                Some(Ok(Message::Pong(_))) => {}
                _ => return Err("Home Assistant disconnected or returned an invalid frame".into()),
            }
        }
    }

    pub async fn command<T: DeserializeOwned>(
        &mut self,
        kind: &str,
        fields: Value,
    ) -> Result<T, PluginError> {
        let id = self.next_id;
        self.next_id += 1;
        let Value::Object(mut request) = fields else {
            return Err("Invalid Home Assistant command fields".into());
        };
        request.insert("id".into(), id.into());
        request.insert("type".into(), kind.into());
        let request = Value::Object(request);
        tokio::time::timeout(DEADLINE, async {
            self.send(&request).await?;
            let Value::Object(mut response) = self.receive().await? else {
                return Err::<T, PluginError>(
                    "Home Assistant returned an unexpected command result".into(),
                );
            };
            if response.get("type").and_then(Value::as_str) != Some("result")
                || response.get("id").and_then(Value::as_u64) != Some(id)
            {
                return Err::<T, PluginError>(
                    "Home Assistant returned an unexpected command result".into(),
                );
            }
            if response.get("success").and_then(Value::as_bool) != Some(true) {
                // Only fixed error categories and validated entity IDs are safe to expose.
                let code = match response
                    .get("error")
                    .and_then(|error| error.get("code"))
                    .and_then(Value::as_str)
                {
                    Some("unauthorized") => "unauthorized",
                    Some("not_found") => "not_found",
                    Some("unknown_command") => "unknown_command",
                    Some("invalid_format") => "invalid_format",
                    Some("unknown_error") => "unknown_error",
                    _ => "unrecognized_error",
                };
                let entity = request
                    .get("entity_id")
                    .and_then(Value::as_str)
                    .filter(|entity| {
                        entity.split_once('.').is_some_and(|(domain, object)| {
                            !domain.is_empty()
                                && !object.is_empty()
                                && domain
                                    .bytes()
                                    .chain(object.bytes())
                                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                        })
                    });
                return Err(match entity {
                    Some(entity) => {
                        format!("Home Assistant command {kind} for {entity} failed ({code})")
                    }
                    None => format!("Home Assistant command {kind} failed ({code})"),
                }
                .into());
            }
            serde_json::from_value(
                response
                    .remove("result")
                    .ok_or("Home Assistant omitted a command result")?,
            )
            .map_err(|_| {
                format!("Home Assistant command {kind} returned malformed or incomplete data")
                    .into()
            })
        })
        .await
        .map_err(|_| format!("Home Assistant command {kind} timed out"))?
    }

    pub async fn close(mut self) -> Result<(), PluginError> {
        tokio::time::timeout(DEADLINE, self.socket.close(None))
            .await
            .map_err(|_| "Home Assistant close timed out")?
            .map_err(|_| "Home Assistant close failed".into())
    }
}
