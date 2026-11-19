//! One-time history import from Signal Desktop's chat export (`main.jsonl`): libsignal backup
//! frames as JSON lines, libsignal v0.106.0 `backup.proto`. Imported messages follow the live
//! stream's content rules and are journaled as ordinary inserts.

use std::{
    collections::HashMap,
    io::{BufRead, BufReader},
    path::Path,
};

use base64::{Engine, engine::general_purpose::STANDARD};
use serde::Deserialize;
use uuid::Uuid;

use crate::{Attachment, ChatKind, Store, receiver::Fatal, signal_cli::Received};

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ImportSummary {
    /// Messages inserted, i.e. absent from the chat archive before.
    pub inserted: u64,
    /// Messages whose identity the chat archive already held.
    pub present: u64,
    /// Chat items the chat archive does not store (calls, stickers, deletes, updates, ...).
    pub skipped: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Frame {
    recipient: Option<Recipient>,
    chat: Option<Chat>,
    chat_item: Option<ChatItem>,
}

#[derive(Deserialize)]
struct Recipient {
    id: String,
    contact: Option<Contact>,
    #[serde(rename = "self")]
    own: Option<serde_json::Value>,
    group: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct Contact {
    /// Base64 of the 16 ACI bytes; absent for contacts known only by number.
    aci: Option<String>,
    /// The phone number's digits, without signal-cli's leading `+`.
    e164: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Chat {
    id: String,
    recipient_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChatItem {
    chat_id: String,
    author_id: String,
    date_sent: String,
    #[serde(default)]
    revisions: Vec<ChatItem>,
    outgoing: Option<serde_json::Value>,
    standard_message: Option<StandardMessage>,
}

#[derive(Deserialize)]
struct StandardMessage {
    text: Option<Text>,
    #[serde(default)]
    attachments: Vec<MessageAttachment>,
}

#[derive(Deserialize)]
struct Text {
    body: String,
}

#[derive(Deserialize)]
struct MessageAttachment {
    pointer: FilePointer,
    flag: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FilePointer {
    content_type: Option<String>,
    file_name: Option<String>,
    caption: Option<String>,
    locator_info: Option<LocatorInfo>,
}

#[derive(Deserialize)]
struct LocatorInfo {
    size: Option<i64>,
}

/// Who a backup recipient id stands for, as far as the chat archive cares.
enum Party {
    Account,
    Contact {
        aci: Option<String>,
        number: Option<String>,
    },
    Other,
}

/// Imports `path` into the chat archive. Fails before writing anything if the receiver has never
/// recorded the account's own ACI, which the export does not carry.
pub async fn import(store: &Store, path: &Path) -> Result<ImportSummary, Fatal> {
    let self_aci = store
        .self_aci()
        .await?
        .ok_or("the receiver has not recorded the account's own ACI yet: start it against the linked signal-cli first")?;
    let reader = BufReader::new(std::fs::File::open(path)?);
    let mut parties = HashMap::new();
    let mut chats = HashMap::new();
    let mut summary = ImportSummary::default();
    for (number, line) in reader.lines().enumerate() {
        // Line numbers only: serde's messages can quote message content.
        let frame: Frame = serde_json::from_str(&line?)
            .map_err(|_| format!("line {} is not a backup frame", number + 1))?;
        if let Some(recipient) = frame.recipient {
            parties.insert(recipient.id.clone(), party(recipient)?);
        } else if let Some(chat) = frame.chat {
            chats.insert(chat.id, chat.recipient_id);
        } else if let Some(item) = frame.chat_item {
            match received(item, &parties, &chats, &self_aci)? {
                Some(message) => match store.insert(message).await? {
                    true => summary.inserted += 1,
                    false => summary.present += 1,
                },
                None => summary.skipped += 1,
            }
        }
    }
    Ok(summary)
}

fn party(recipient: Recipient) -> Result<Party, Fatal> {
    if recipient.group.is_some() {
        // signal-cli names a group by an ID derived from its master key with zkgroup; no export
        // with groups was available to verify a derivation against.
        return Err("group import unsupported".into());
    }
    if recipient.own.is_some() {
        return Ok(Party::Account);
    }
    let Some(contact) = recipient.contact else {
        return Ok(Party::Other);
    };
    let aci = contact
        .aci
        .map(|aci| {
            let bytes = STANDARD
                .decode(aci)
                .map_err(|_| "contact ACI is not base64")?;
            Ok::<_, Fatal>(
                Uuid::from_slice(&bytes)
                    .map_err(|_| "contact ACI is not 16 bytes")?
                    .to_string(),
            )
        })
        .transpose()?;
    Ok(Party::Contact {
        aci,
        number: contact.e164.map(|digits| format!("+{digits}")),
    })
}

fn received(
    item: ChatItem,
    parties: &HashMap<String, Party>,
    chats: &HashMap<String, String>,
    self_aci: &str,
) -> Result<Option<Received>, Fatal> {
    let Some(message) = item.standard_message else {
        return Ok(None);
    };
    let text = message
        .text
        .map(|text| text.body)
        .filter(|body| !body.is_empty());
    let attachments: Vec<Attachment> = message
        .attachments
        .into_iter()
        .map(|attachment| Attachment {
            mime_type: attachment.pointer.content_type,
            filename: attachment.pointer.file_name,
            size: attachment
                .pointer
                .locator_info
                .and_then(|locator| locator.size),
            caption: attachment.pointer.caption,
            voice_note: attachment.flag.as_deref() == Some("VOICE_MESSAGE"),
        })
        .collect();
    if text.is_none() && attachments.is_empty() {
        return Ok(None);
    }
    let lookup = |id: &str| {
        parties
            .get(id)
            .ok_or("chat item references an unknown recipient")
    };
    let recipient = chats
        .get(&item.chat_id)
        .ok_or("chat item references an unknown chat")?;
    let mut numbers = Vec::new();
    let (chat, kind) = match lookup(recipient)? {
        Party::Account => (self_aci.to_owned(), ChatKind::NoteToSelf),
        Party::Contact {
            aci: Some(aci),
            number,
        } => {
            numbers.extend(number.clone().map(|number| (aci.clone(), number)));
            (aci.clone(), ChatKind::Direct)
        }
        Party::Contact { aci: None, .. } => return Err("chat with a contact without ACI".into()),
        Party::Other => return Ok(None),
    };
    let author = match lookup(&item.author_id)? {
        Party::Account => self_aci.to_owned(),
        Party::Contact { aci: Some(aci), .. } => aci.clone(),
        _ => return Err("chat item author without ACI".into()),
    };
    // An edited item carries the latest revision's dateSent, but Signal (and signal-cli's live
    // stream) identifies a message by its original sent timestamp: the oldest revision's.
    let edited = !item.revisions.is_empty();
    let date_sent = item
        .revisions
        .first()
        .map_or(&item.date_sent, |original| &original.date_sent);
    Ok(Some(Received {
        chat,
        kind,
        author,
        sent_at: date_sent
            .parse()
            .map_err(|_| "dateSent is not an integer")?,
        from_me: item.outgoing.is_some(),
        text,
        attachments,
        numbers,
        edited,
    }))
}
