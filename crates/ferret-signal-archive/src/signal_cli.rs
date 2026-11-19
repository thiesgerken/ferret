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
    /// `CONTACTS_SYNC` or `GROUPS_SYNC` when another device pushed its contacts or groups.
    r#type: Option<String>,
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
    group_name: Option<String>,
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
    /// The group's name, for a group chat the message creates.
    pub group_name: Option<String>,
}

impl Envelope {
    /// Whether the envelope announces changed contact or group names: a group update, or
    /// another device syncing its contacts or groups.
    pub fn changes_names(&self) -> bool {
        let group_update = self
            .data_message
            .as_ref()
            .and_then(|data| data.group_info.as_ref())
            .is_some_and(|group| group.r#type == "UPDATE");
        let sync = self
            .sync_message
            .as_ref()
            .and_then(|sync| sync.r#type.as_deref())
            .is_some_and(|kind| matches!(kind, "CONTACTS_SYNC" | "GROUPS_SYNC"));
        group_update || sync
    }

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
        let mut group_name = None;
        let (chat, kind) = match data.group_info {
            Some(group) if group.r#type == "DELIVER" => {
                group_name = group.group_name.filter(|name| !name.is_empty());
                (group.group_id, ChatKind::Group)
            }
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
            group_name,
        })
    }
}

/// One entry of `listContacts` with `allRecipients`, v0.14.9 `JsonContact`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Contact {
    uuid: Option<String>,
    number: Option<String>,
    /// The system contact's given and family name, empty when unset.
    name: Option<String>,
    nick_name: Option<String>,
    nick_given_name: Option<String>,
    nick_family_name: Option<String>,
    profile: Option<Profile>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Profile {
    given_name: Option<String>,
    family_name: Option<String>,
}

/// A recipient's name and number as the chat archive keeps them.
#[derive(Debug, PartialEq, Eq)]
pub struct Named {
    pub aci: String,
    pub number: Option<String>,
    pub name: Option<String>,
}

impl Contact {
    /// The contact keyed by ACI, named like Signal's apps do: the nickname the account gave,
    /// else the system contact name, else the profile name the contact chose.
    pub fn into_named(self) -> Option<Named> {
        let profile = self
            .profile
            .and_then(|profile| joined(profile.given_name, profile.family_name));
        let name = joined(self.nick_given_name, self.nick_family_name)
            .or(non_empty(self.nick_name))
            .or(non_empty(self.name))
            .or(profile);
        Some(Named {
            aci: self.uuid?,
            number: non_empty(self.number),
            name,
        })
    }
}

/// One entry of `listGroups`, v0.14.9 `JsonGroup`.
#[derive(Debug, Deserialize)]
pub struct Group {
    pub id: String,
    name: Option<String>,
}

impl Group {
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref().filter(|name| !name.is_empty())
    }
}

fn non_empty(text: Option<String>) -> Option<String> {
    text.filter(|text| !text.trim().is_empty())
}

fn joined(given: Option<String>, family: Option<String>) -> Option<String> {
    let parts: Vec<_> = [given, family].into_iter().flat_map(non_empty).collect();
    non_empty(Some(parts.join(" ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contact(json: serde_json::Value) -> Option<String> {
        serde_json::from_value::<Contact>(json)
            .unwrap()
            .into_named()
            .unwrap()
            .name
    }

    #[test]
    fn names_follow_signals_precedence() {
        let profile = serde_json::json!({"givenName": "Pro", "familyName": "File"});
        let all = serde_json::json!({"uuid": "a", "name": "Sys Tem", "nickName": "",
            "nickGivenName": "Nick", "nickFamilyName": "", "profile": profile});
        assert_eq!(contact(all).as_deref(), Some("Nick"));
        let system = serde_json::json!({"uuid": "a", "name": "Sys Tem", "profile": profile});
        assert_eq!(contact(system).as_deref(), Some("Sys Tem"));
        let profile_only = serde_json::json!({"uuid": "a", "name": "", "profile": profile});
        assert_eq!(contact(profile_only).as_deref(), Some("Pro File"));
        let nothing = serde_json::json!({"uuid": "a", "name": "",
            "profile": {"givenName": null, "familyName": ""}});
        assert_eq!(contact(nothing), None);
    }
}
