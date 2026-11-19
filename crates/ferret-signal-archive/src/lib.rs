//! Wire types of the Signal chat archive API. All timestamps are Unix milliseconds, because a
//! message's sent timestamp is part of its Signal identity and must not lose precision.

use serde::{Deserialize, Serialize};

#[cfg(feature = "server")]
mod import;
#[cfg(feature = "server")]
mod receiver;
#[cfg(feature = "server")]
mod server;
#[cfg(feature = "server")]
mod signal_cli;
#[cfg(feature = "server")]
mod store;
#[cfg(feature = "server")]
pub use {
    import::{ImportSummary, import},
    receiver::receive,
    server::router,
    store::Store,
};

pub const PAGE_SIZE: i64 = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollectorStatus {
    Unknown,
    Unlinked,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Archive {
    pub store_id: String,
    pub status: CollectorStatus,
    #[serde(deserialize_with = "Option::deserialize")]
    pub last_received_at: Option<i64>,
    /// The journal's highest sequence: the fixed `through` boundary of one changes drain.
    pub high: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(
    feature = "server",
    derive(sqlx::Type),
    sqlx(rename_all = "snake_case")
)]
pub enum ChatKind {
    Direct,
    Group,
    NoteToSelf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "server", derive(sqlx::FromRow))]
pub struct Chat {
    /// The counterpart's ACI, the group ID, or the account's own ACI for Note to Self.
    pub id: String,
    pub kind: ChatKind,
    #[serde(deserialize_with = "Option::deserialize")]
    pub name: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChatsPage {
    pub chats: Vec<Chat>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub next: Option<String>,
}

/// A message's identity within its chat, also the `messages` keyset position.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct MessageKey {
    pub sent_at: i64,
    pub author: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attachment {
    #[serde(deserialize_with = "Option::deserialize")]
    pub mime_type: Option<String>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub filename: Option<String>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub size: Option<i64>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub caption: Option<String>,
    /// Signal's only attachment type that the MIME type does not carry.
    pub voice_note: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    #[serde(flatten)]
    pub key: MessageKey,
    /// The author's nickname, else system contact name, else profile name.
    #[serde(deserialize_with = "Option::deserialize")]
    pub author_name: Option<String>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub author_number: Option<String>,
    pub from_me: bool,
    #[serde(deserialize_with = "Option::deserialize")]
    pub text: Option<String>,
    pub attachments: Vec<Attachment>,
    pub edited: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MessagesPage {
    pub messages: Vec<Message>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub next: Option<MessageKey>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(
    feature = "server",
    derive(sqlx::Type),
    sqlx(rename_all = "snake_case")
)]
pub enum ChangeOperation {
    Insert,
    /// The message's text changed; it keeps its identity.
    Edit,
    /// The message was deleted and no longer appears in its chat.
    Delete,
    /// A chat's title or one of its authors' names changed.
    Rename,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Change {
    pub sequence: i64,
    pub recorded_at: i64,
    pub operation: ChangeOperation,
    pub chat: String,
    /// The message concerned; null for an entry about the whole chat.
    #[serde(deserialize_with = "Option::deserialize")]
    pub key: Option<MessageKey>,
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
        assert!(serde_json::from_str::<Chat>(r#"{"id":"g","kind":"group"}"#).is_err());
    }

    #[test]
    fn message_key_is_flat_on_the_wire() {
        let message: Message = serde_json::from_str(
            r#"{"sent_at":1,"author":"a","author_name":null,"author_number":null,"from_me":false,"text":"hi","attachments":[],"edited":false}"#,
        )
        .unwrap();
        assert_eq!(
            message.key,
            MessageKey {
                sent_at: 1,
                author: "a".into()
            }
        );
    }
}
