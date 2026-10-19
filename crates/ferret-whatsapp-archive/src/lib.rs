//! Raw transport for wacli v0.22.0 schema 32 and c45422b schema 33.

use serde::{Deserialize, Serialize};

#[cfg(feature = "server")]
mod server;
#[cfg(feature = "server")]
pub use server::{credential, router};

pub const PAGE_SIZE: i64 = 500;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Archive {
    pub store_id: String,
    pub schema_version: i64,
    pub revision: i64,
    #[serde(deserialize_with = "Option::deserialize")]
    pub last_sync_at: Option<i64>,
    /// The archive does not contain authoritative live collector/login state.
    pub collector_status: String,
    pub floor: i64,
    pub high: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "server", derive(sqlx::FromRow))]
pub struct Chat {
    pub jid: String,
    pub kind: String,
    #[serde(deserialize_with = "Option::deserialize")]
    pub name: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChatsPage {
    pub chats: Vec<Chat>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub next: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "server", derive(sqlx::FromRow))]
pub struct Message {
    pub rowid: i64,
    pub msg_id: String,
    #[serde(deserialize_with = "Option::deserialize")]
    pub sender_jid: Option<String>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub sender_name: Option<String>,
    pub ts: i64,
    pub from_me: bool,
    #[serde(deserialize_with = "Option::deserialize")]
    pub text: Option<String>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub display_text: Option<String>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub media_type: Option<String>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub media_caption: Option<String>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub filename: Option<String>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub mime_type: Option<String>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub file_length: Option<i64>,
    pub edited: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct MessageKey {
    pub ts: i64,
    pub rowid: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MessagesPage {
    pub messages: Vec<Message>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub next: Option<MessageKey>,
}

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "server", derive(sqlx::FromRow))]
pub struct Change {
    pub sequence: i64,
    pub recorded_at: i64,
    pub operation: String,
    pub chat_jid: String,
    pub msg_id: String,
    #[serde(deserialize_with = "Option::deserialize")]
    pub new_chat_jid: Option<String>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub new_msg_id: Option<String>,
    pub reason: String,
    pub message_ts: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChangesPage {
    pub changes: Vec<Change>,
    pub next: i64,
    pub through: i64,
    pub has_more: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ErrorBody {
    pub code: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_nullable_fields_are_not_an_empty_page() {
        assert!(serde_json::from_str::<MessagesPage>(r#"{"messages":[]}"#).is_err());
        assert!(serde_json::from_str::<ChatsPage>(r#"{"chats":[]}"#).is_err());
        assert!(serde_json::from_str::<Chat>(r#"{"jid":"1@s.whatsapp.net","kind":"dm"}"#).is_err());
    }
}
