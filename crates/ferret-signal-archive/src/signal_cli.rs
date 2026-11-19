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
    edit_message: Option<EditMessage>,
    sync_message: Option<SyncMessage>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DataMessage {
    timestamp: i64,
    message: Option<String>,
    attachments: Option<Vec<JsonAttachment>>,
    group_info: Option<GroupInfo>,
    remote_delete: Option<RemoteDelete>,
    admin_delete: Option<AdminDelete>,
}

/// An edit's new revision of the message it targets.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EditMessage {
    target_sent_timestamp: i64,
    data_message: DataMessage,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoteDelete {
    target_timestamp: i64,
}

/// A group admin deleting another member's message.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AdminDelete {
    target_author_uuid: Option<String>,
    target_timestamp: i64,
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
    edit_message: Option<EditMessage>,
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

/// What an envelope changes in the chat archive.
#[derive(Debug, PartialEq, Eq)]
pub enum Incoming {
    Message(Received),
    /// The text of revision `edited_at` of the message `target` names.
    Edit {
        target: Target,
        edited_at: i64,
        text: Option<String>,
    },
    Delete(Target),
}

/// A message as an edit or delete names it: by its author and one of its revision timestamps,
/// not necessarily the original send.
#[derive(Debug, PartialEq, Eq)]
pub struct Target {
    pub chat: String,
    pub kind: ChatKind,
    pub author: String,
    pub sent_at: i64,
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
    /// Timestamps of later revisions of a message that arrives already edited, as an imported
    /// one can; the live stream delivers edits separately.
    pub revisions: Vec<i64>,
}

impl Envelope {
    /// The message, edit or delete, or `None` for everything the chat archive skips: receipts,
    /// typing, calls, stories, reactions, polls, pins, group and timer changes and other
    /// content-free data messages. Story replies are ordinary messages in their chat.
    pub fn into_incoming(self) -> Option<Incoming> {
        let author = self.source_uuid?;
        let mut numbers: Vec<_> = self
            .source_number
            .map(|number| (author.clone(), number))
            .into_iter()
            .collect();
        let (data, edit_target, chat, kind, from_me) = if let Some(data) = self.data_message {
            (data, None, author.clone(), ChatKind::Direct, false)
        } else if let Some(edit) = self.edit_message {
            let target = Some(edit.target_sent_timestamp);
            (
                edit.data_message,
                target,
                author.clone(),
                ChatKind::Direct,
                false,
            )
        } else {
            let sent = self.sync_message?.sent_message?;
            let (data, edit_target) = match sent.edit_message {
                Some(edit) => (edit.data_message, Some(edit.target_sent_timestamp)),
                None => {
                    let Ok(data) = serde_json::from_value::<DataMessage>(sent.data) else {
                        warn!("skipping a sent transcript whose data message does not parse");
                        return None;
                    };
                    (data, None)
                }
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
            (data, edit_target, chat, kind, true)
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
        if let Some(delete) = data.remote_delete {
            return Some(Incoming::Delete(Target {
                chat,
                kind,
                author,
                sent_at: delete.target_timestamp,
            }));
        }
        // signal-cli drops admin deletes from senders who aren't admins of the group.
        if let Some(delete) = data.admin_delete {
            let Some(target_author) = delete.target_author_uuid else {
                warn!("skipping an admin delete without target author ACI");
                return None;
            };
            return Some(Incoming::Delete(Target {
                chat,
                kind,
                author: target_author,
                sent_at: delete.target_timestamp,
            }));
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
        if let Some(target) = edit_target {
            // The chat archive takes only the text from an edit, as the design asks; attachment
            // metadata stays the original's.
            return Some(Incoming::Edit {
                target: Target {
                    chat,
                    kind,
                    author,
                    sent_at: target,
                },
                edited_at: data.timestamp,
                text,
            });
        }
        if text.is_none() && attachments.is_empty() {
            return None;
        }
        Some(Incoming::Message(Received {
            chat,
            kind,
            author,
            sent_at: data.timestamp,
            from_me,
            text,
            attachments,
            numbers,
            revisions: Vec::new(),
        }))
    }
}
