//! signal-cli v0.14.9's JSON-RPC `receive` notifications (`src/main/java/org/asamk/signal/json`),
//! reduced to what the chat archive stores. Unknown fields are ignored, so newer signal-cli
//! releases that add fields keep working.

use serde::Deserialize;

use crate::{Attachment, ChatKind};

/// A line from the daemon: a notification such as
/// `{"jsonrpc":"2.0","method":"receive","params":{"subscription":0,"result":{...}}}`, or the
/// response to `subscribeReceive`.
#[derive(Debug, Deserialize)]
pub struct Notification {
    pub method: Option<String>,
    pub params: Option<serde_json::Value>,
    pub error: Option<RpcError>,
}

#[derive(Debug, Deserialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct ReceiveParams {
    pub result: ReceiveResult,
}

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
    story_context: Option<serde_json::Value>,
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
    /// signal-cli unwraps the data message into the transcript; it is absent for edits.
    #[serde(flatten)]
    data: Option<DataMessage>,
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
    /// data messages.
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
            let data = sent.data?;
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
            None if chat.is_empty() => return None,
            None => (chat, kind),
        };
        if data.story_context.is_some() {
            return None;
        }
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
