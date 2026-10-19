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
    pub name: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChatsPage {
    pub chats: Vec<Chat>,
    pub next: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "server", derive(sqlx::FromRow))]
pub struct Message {
    pub rowid: i64,
    pub msg_id: String,
    pub sender_jid: Option<String>,
    pub sender_name: Option<String>,
    pub ts: i64,
    pub from_me: bool,
    pub text: Option<String>,
    pub display_text: Option<String>,
    pub media_type: Option<String>,
    pub media_caption: Option<String>,
    pub filename: Option<String>,
    pub mime_type: Option<String>,
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
    pub new_chat_jid: Option<String>,
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
