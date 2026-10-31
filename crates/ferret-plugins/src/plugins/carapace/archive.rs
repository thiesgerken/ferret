//! Parses carapace's session archives: the `conversation.json` that
//! `carapace/session/archive.py` commits for a session, with its whole event history. Like the
//! mirror, an archive that doesn't parse is an error rather than skipped, since its turns would
//! just be missing.

use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Map, Value};

use crate::plugins::PluginError;

/// The archive format this plugin reads. A new one may move or rename what it relies on.
const SCHEMA_VERSION: u64 = 1;

#[derive(Debug, Deserialize)]
pub struct Archive {
    pub session: SessionInfo,
    /// The session's events in order. An event's index is its `seq`.
    pub history: Vec<Event>,
}

#[derive(Debug, Deserialize)]
pub struct SessionInfo {
    pub session_id: String,
    /// How the session is reached, e.g. `cli` or `job`.
    pub channel_type: String,
    /// `None` for a session carapace never titled.
    pub title: Option<String>,
    pub created_at: DateTime<Utc>,
    /// Set for a session carapace copied from another one. Archives of other sessions, and those
    /// written before carapace recorded it, lack it.
    pub forked_from: Option<Fork>,
}

/// Where a copied session came from.
#[derive(Debug, Deserialize)]
pub struct Fork {
    /// How many leading events of the history are the source session's, copied with their
    /// timestamps.
    pub event_count: usize,
}

/// One event of a session's history. Events stored before carapace stamped them on write have no
/// `timestamp`.
#[derive(Debug, Deserialize)]
#[serde(tag = "role", rename_all = "snake_case")]
pub enum Event {
    User(UserMessage),
    Assistant(AgentMessage),
    ToolCall(ToolCall),
    ToolResult(ToolResult),
    /// The server's answer to the slash command right before it.
    Command,
    /// Thinking, approvals and anything else no turn shows.
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
pub struct UserMessage {
    pub content: String,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
    pub timestamp: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct Attachment {
    pub name: String,
    pub mime: Option<String>,
}

/// The agent's answer, or narration between its tool calls.
#[derive(Debug, Deserialize)]
pub struct AgentMessage {
    pub content: String,
    pub usage: Option<Usage>,
    pub timestamp: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct Usage {
    pub model: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ToolCall {
    pub tool: String,
    #[serde(default)]
    pub args: Map<String, Value>,
    /// Pairs the call with its result.
    pub tool_id: Option<String>,
    pub timestamp: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct ToolResult {
    pub tool: String,
    /// A string, or JSON for structured results.
    #[serde(default)]
    pub result: Value,
    pub exit_code: Option<i64>,
    pub tool_id: Option<String>,
    pub timestamp: Option<DateTime<Utc>>,
}

impl Event {
    pub fn timestamp(&self) -> Option<DateTime<Utc>> {
        match self {
            Event::User(message) => message.timestamp,
            Event::Assistant(message) => message.timestamp,
            Event::ToolCall(call) => call.timestamp,
            Event::ToolResult(result) => result.timestamp,
            Event::Command | Event::Other => None,
        }
    }
}

pub fn parse(json: &str) -> Result<Archive, PluginError> {
    // Read first, so another version fails as such rather than on the first field it changed.
    #[derive(Deserialize)]
    struct Version {
        schema_version: u64,
    }
    let Version { schema_version } = serde_json::from_str(json)?;
    if schema_version != SCHEMA_VERSION {
        return Err(format!("unknown archive schema_version {schema_version}").into());
    }
    Ok(serde_json::from_str(json)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ARCHIVE: &str =
        include_str!("../../../fixtures/carapace/sessions/2026/09/s-trip/conversation.json");

    #[test]
    fn an_archive_parses_into_its_session_and_events() {
        let archive = parse(ARCHIVE).unwrap();

        assert_eq!(archive.session.session_id, "s-trip");
        assert_eq!(archive.session.channel_type, "cli");
        assert_eq!(archive.session.title.as_deref(), Some("Weekend trip"));
        assert!(archive.session.forked_from.is_none());
        assert_eq!(archive.history.len(), 20);
        assert!(matches!(&archive.history[1], Event::User(message) if message.timestamp.is_none()));
        assert!(matches!(archive.history[2], Event::Other), "thinking");
        assert!(
            archive
                .history
                .iter()
                .any(|event| matches!(event, Event::Command))
        );
    }

    #[test]
    fn another_schema_version_or_broken_json_fails() {
        let newer = ARCHIVE.replace("\"schema_version\": 1", "\"schema_version\": 2");

        assert_eq!(
            parse(&newer).unwrap_err().to_string(),
            "unknown archive schema_version 2"
        );
        assert!(parse(&ARCHIVE[..ARCHIVE.len() / 2]).is_err());
        assert!(parse(r#"{"schema_version": 1, "session": {}, "history": []}"#).is_err());
    }
}
