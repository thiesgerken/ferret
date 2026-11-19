//! signal-cli v0.14.9's JSON-RPC `receive` results (`src/main/java/org/asamk/signal/json`),
//! reduced to what the chat archive stores. Unknown fields are ignored, so newer signal-cli
//! releases that add fields keep working.

use serde::Deserialize;
use tracing::warn;

use crate::{Attachment, ChatKind};

/// One element of a JSON-RPC `receive` result.
#[derive(Debug, Deserialize)]
pub struct ReceiveResult {
    /// Absent when signal-cli reports an exception without an envelope.
    pub envelope: Option<Envelope>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Envelope {
    source_uuid: Option<String>,
    source_number: Option<String>,
    data_message: Option<DataMessage>,
    sync_message: Option<SyncMessage>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DataMessage {
    timestamp: i64,
    message: Option<String>,
    attachments: Option<Vec<JsonAttachment>>,
    group_info: Option<GroupInfo>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SyncMessage {
    sent_message: Option<SentMessage>,
}

/// A sync transcript: a message the account sent from its phone or another linked device.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SentMessage {
    destination_uuid: Option<String>,
    destination_number: Option<String>,
    edit_message: Option<serde_json::Value>,
    /// signal-cli unwraps the data message into the transcript, absent for edits. Kept raw so
    /// that a data message failing to parse is reported instead of read as absent.
    #[serde(flatten)]
    data: serde_json::Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GroupInfo {
    group_id: String,
    /// `UPDATE` for group changes, `DELIVER` for messages.
    r#type: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JsonAttachment {
    content_type: Option<String>,
    filename: Option<String>,
    size: Option<i64>,
    caption: Option<String>,
    is_voice_note: bool,
}

/// A message the chat archive stores, in its Signal identity.
#[derive(Debug, PartialEq, Eq)]
pub struct Received {
    pub chat: String,
    pub kind: ChatKind,
    pub author: String,
    pub sent_at: i64,
    pub from_me: bool,
    pub text: Option<String>,
    pub attachments: Vec<Attachment>,
    /// ACI and phone number pairs the envelope revealed.
    pub numbers: Vec<(String, String)>,
}

impl Envelope {
    /// The storable message, or `None` for everything the chat archive skips: receipts, typing,
    /// calls, stories, reactions, polls, pins, group and timer changes and other content-free
    /// data messages. Story replies are ordinary messages in their chat.
    pub fn into_received(self) -> Option<Received> {
        let author = self.source_uuid?;
        let mut numbers: Vec<_> = self
            .source_number
            .map(|number| (author.clone(), number))
            .into_iter()
            .collect();
        let (data, chat, kind, from_me) = if let Some(data) = self.data_message {
            (data, author.clone(), ChatKind::Direct, false)
        } else {
            let sent = self.sync_message?.sent_message?;
            if sent.edit_message.is_some() {
                return None;
            }
            let Ok(data) = serde_json::from_value::<DataMessage>(sent.data) else {
                warn!("skipping a sent transcript whose data message does not parse");
                return None;
            };
            let destination = sent.destination_uuid;
            if let (Some(aci), Some(number)) = (&destination, sent.destination_number) {
                numbers.push((aci.clone(), number));
            }
            // Group transcripts have no destination; the group ID replaces it below.
            let chat = destination.unwrap_or_default();
            let kind = if chat == author {
                ChatKind::NoteToSelf
            } else {
                ChatKind::Direct
            };
            (data, chat, kind, true)
        };
        let (chat, kind) = match data.group_info {
            Some(group) if group.r#type == "DELIVER" => (group.group_id, ChatKind::Group),
            Some(_) => return None,
            None if chat.is_empty() => {
                warn!("skipping a direct sent transcript without destination ACI");
                return None;
            }
            None => (chat, kind),
        };
        let text = data.message.filter(|text| !text.is_empty());
        let attachments: Vec<Attachment> = data
            .attachments
            .unwrap_or_default()
            .into_iter()
            .map(|attachment| Attachment {
                mime_type: attachment.content_type,
                filename: attachment.filename,
                size: attachment.size,
                caption: attachment.caption,
                voice_note: attachment.is_voice_note,
            })
            .collect();
        if text.is_none() && attachments.is_empty() {
            return None;
        }
        Some(Received {
            chat,
            kind,
            author,
            sent_at: data.timestamp,
            from_me,
            text,
            attachments,
            numbers,
        })
    }
}
