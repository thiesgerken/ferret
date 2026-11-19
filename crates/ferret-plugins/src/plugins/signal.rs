//! Finite reconciliation of the Signal chat archive that `ferret-signal-archive` keeps.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::atomic::{AtomicUsize, Ordering},
};

use chrono::DateTime;
use ferret_api::internal::RunStats;
use ferret_signal_archive::{
    Archive, ChangesPage, Chat, ChatKind, ChatsPage, CollectorStatus, Message, MessageKey,
    MessagesPage,
};
use schemars::JsonSchema;
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Map, json};

use super::{
    Outcome, PluginError, Run,
    chat::{self, ChatArchive, Cursor, Journal, Segments, Settings},
};

pub const NAME: &str = "signal";
const MAX_REQUESTS: usize = 20_000;
const CURSOR_VERSION: u32 = 1;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "Signal chat archive")]
pub struct SignalConfig {
    /// Private chat archive API root, reached directly by the worker.
    url: String,
    /// The chat archive API's 64-character hexadecimal bearer secret.
    #[schemars(extend("writeOnly" = true))]
    token: String,
    /// Required chat archive identity from authenticated `/v1/archive`, as 32 lowercase hex
    /// characters.
    #[schemars(extend("pattern" = "^[0-9a-f]{32}$", "x-ferret-preserve-credentials" = true))]
    expected_store_id: String,
    /// Exact chat IDs from the chat archive inventory. Empty includes all chats.
    #[serde(default)]
    #[schemars(extend("x-ferret-preserve-credentials" = true))]
    allow_chats: BTreeSet<String>,
    /// Exact chat IDs. Takes precedence over allow_chats; unknown IDs fail closed.
    #[serde(default)]
    #[schemars(extend("x-ferret-preserve-credentials" = true))]
    block_chats: BTreeSet<String>,
}

struct ArchiveClient {
    http: reqwest::Client,
    url: String,
    token: String,
    expected_store_id: String,
    requests: AtomicUsize,
}

impl ArchiveClient {
    async fn get<T: DeserializeOwned>(
        &self,
        path: &str,
        params: &[(&str, String)],
    ) -> Result<T, PluginError> {
        if self.requests.fetch_add(1, Ordering::Relaxed) >= MAX_REQUESTS {
            return Err(
                "Signal chat archive request budget exhausted; no sweep or checkpoint advance"
                    .into(),
            );
        }
        let response = self
            .http
            .get(format!("{}{path}", self.url))
            .bearer_auth(&self.token)
            .query(params)
            .send()
            .await
            .map_err(|_| "Signal chat archive request failed")?;
        if !response.status().is_success() {
            return Err(format!("Signal chat archive returned HTTP {}; check API availability, credential and chat archive identity", response.status()).into());
        }
        response
            .json()
            .await
            .map_err(|_| "Signal chat archive returned invalid data".into())
    }
}

impl ChatArchive for ArchiveClient {
    type Chat = Chat;

    async fn journal(&self) -> Result<Journal, PluginError> {
        let archive: Archive = self.get("/v1/archive", &[]).await?;
        if archive.store_id != self.expected_store_id {
            return Err("Signal chat archive does not match expected_store_id; inspect the authenticated chat archive before deliberately updating the connection configuration".into());
        }
        if archive.high < 0 {
            return Err("Signal chat archive returned invalid journal bounds".into());
        }
        let status = match archive.status {
            CollectorStatus::Unknown => "unknown",
            CollectorStatus::Unlinked => "unlinked",
        };
        let last_received = archive
            .last_received_at
            .and_then(DateTime::from_timestamp_millis)
            .map_or("never".to_owned(), |at| {
                at.format("%Y-%m-%d %H:%M:%S UTC").to_string()
            });
        Ok(Journal {
            // The journal is never pruned.
            floor: 0,
            high: archive.high,
            collector: format!("collector status {status}, last received envelope {last_received}"),
        })
    }

    async fn chats(&self) -> Result<BTreeMap<String, Chat>, PluginError> {
        let mut chats = BTreeMap::new();
        let mut after = String::new();
        loop {
            let page: ChatsPage = self
                .get(
                    "/v1/chats",
                    &[
                        ("store_id", self.expected_store_id.clone()),
                        ("after", after.clone()),
                    ],
                )
                .await?;
            for chat in page.chats {
                if chat.id <= after || chats.contains_key(&chat.id) {
                    return Err("Signal chat inventory is incomplete or unordered".into());
                }
                after = chat.id.clone();
                chats.insert(chat.id.clone(), chat);
            }
            match page.next {
                None => return Ok(chats),
                Some(next) if next == after && !next.is_empty() => {}
                _ => return Err("Signal chat inventory has invalid continuation".into()),
            }
        }
    }

    /// Every change is a reason to rebuild its chat from the chat archive's current state.
    async fn changes(&self, mut after: i64, through: i64) -> Result<BTreeSet<String>, PluginError> {
        let mut affected = BTreeSet::new();
        loop {
            let page: ChangesPage = self
                .get(
                    "/v1/changes",
                    &[
                        ("store_id", self.expected_store_id.clone()),
                        ("after", after.to_string()),
                        ("through", through.to_string()),
                    ],
                )
                .await?;
            if page.through != through
                || page.next < after
                || page.next > through
                || (page.has_more && page.next <= after)
            {
                return Err("Signal change journal has invalid continuation".into());
            }
            let mut sequence = after;
            for change in page.changes {
                if change.sequence <= sequence || change.sequence > page.next {
                    return Err("Signal change journal is unordered".into());
                }
                sequence = change.sequence;
                affected.insert(change.chat);
            }
            after = page.next;
            if !page.has_more {
                if after != through {
                    return Err("Signal journal drain is incomplete".into());
                }
                return Ok(affected);
            }
        }
    }

    /// An empty drain still sends the pinned `store_id`, which a replaced chat archive rejects.
    async fn recheck(&self, checkpoint: i64) -> Result<(), PluginError> {
        self.changes(checkpoint, checkpoint).await.map(drop)
    }

    fn indexable(_: &Chat) -> bool {
        true
    }

    async fn ingest_chat(
        &self,
        run: &Run,
        chat: &Chat,
        stats: &mut RunStats,
    ) -> Result<(), PluginError> {
        let shared = shared_chat(chat);
        let mut segments = Segments::new(&run.source, &shared);
        let mut after: Option<MessageKey> = None;
        loop {
            let mut params = vec![
                ("store_id", self.expected_store_id.clone()),
                ("chat", chat.id.clone()),
            ];
            if let Some(key) = &after {
                params.extend([
                    ("after_sent_at", key.sent_at.to_string()),
                    ("after_author", key.author.clone()),
                ]);
            }
            let page: MessagesPage = self.get("/v1/messages", &params).await?;
            let mut last = after.clone();
            for message in &page.messages {
                if last.as_ref().is_some_and(|last| message.key <= *last) {
                    return Err("Signal message traversal is incomplete or unordered".into());
                }
                last = Some(message.key.clone());
                if let Some(document) = segments.push(shared_message(message))? {
                    chat::put(run, &document, stats).await?;
                }
            }
            match page.next {
                None => {
                    if let Some(document) = segments.finish() {
                        chat::put(run, &document, stats).await?;
                    }
                    return Ok(());
                }
                Some(key) if last.as_ref() == Some(&key) && last != after => after = Some(key),
                _ => return Err("Signal message traversal has invalid continuation".into()),
            }
        }
    }
}

pub async fn run(run: &Run) -> Result<Outcome, PluginError> {
    let config: SignalConfig = serde_json::from_value(run.config.clone())
        .map_err(|_| "Signal configuration is invalid")?;
    let url = chat::validate_connection(
        "Signal",
        &config.url,
        &config.token,
        &config.expected_store_id,
    )?;
    let settings = Settings {
        url,
        expected_store_id: config.expected_store_id,
        allow_chats: config.allow_chats,
        block_chats: config.block_chats,
    };
    let client = ArchiveClient {
        http: chat::http_client()?,
        url: settings.url.clone(),
        token: config.token,
        expected_store_id: settings.expected_store_id.clone(),
        requests: AtomicUsize::new(0),
    };
    let previous = if run.state == json!({}) {
        None
    } else {
        let cursor: Cursor<Chat> = serde_json::from_value(run.state.clone())
            .map_err(|_| "Signal saved cursor is invalid")?;
        if cursor.version != CURSOR_VERSION {
            return Err("Signal saved cursor version is unsupported".into());
        }
        Some(cursor)
    };
    let mut stats = RunStats::default();
    let state = chat::reconcile(
        run,
        &client,
        &settings,
        previous.as_ref(),
        CURSOR_VERSION,
        &mut stats,
    )
    .await?;
    Ok(Outcome { state, stats })
}

fn shared_chat(chat: &Chat) -> chat::Chat<'_> {
    let kind = match chat.kind {
        ChatKind::Direct => "direct",
        ChatKind::Group => "group",
        ChatKind::NoteToSelf => "note_to_self",
    };
    chat::Chat::new(&chat.id, kind, chat.name.as_deref())
}

fn shared_message(message: &Message) -> chat::Message<'_> {
    chat::Message {
        // Signal's own message identity within a chat.
        id: format!("{}/{}", message.key.author, message.key.sent_at),
        // ponytail: raw number or ACI; contact and profile names come with the name refresh.
        sender: if message.from_me {
            "Me".to_owned()
        } else {
            message
                .author_number
                .clone()
                .unwrap_or_else(|| message.key.author.clone())
        },
        ts: message.key.sent_at.div_euclid(1_000),
        edited: message.edited,
        attachments: message
            .attachments
            .iter()
            .map(|attachment| chat::Attachment {
                kind: attachment.voice_note.then_some("voice note"),
                filename: attachment.filename.as_deref(),
                mime_type: attachment.mime_type.as_deref(),
                size: attachment.size,
                caption: attachment.caption.as_deref(),
            })
            .collect(),
        body: message.text.as_deref().filter(|text| !text.is_empty()),
        segment_meta: Map::new(),
    }
}

#[cfg(test)]
mod tests {
    use ferret_signal_archive::Attachment;

    use super::*;

    fn message(author: &str, sent_at: i64, text: Option<&str>) -> Message {
        Message {
            key: MessageKey {
                sent_at,
                author: author.to_owned(),
            },
            author_number: None,
            from_me: false,
            text: text.map(str::to_owned),
            attachments: Vec::new(),
            edited: false,
        }
    }

    fn documents(chat: &Chat, messages: &[Message]) -> Vec<ferret_api::DocumentPut> {
        let shared = shared_chat(chat);
        let mut segments = Segments::new("chat/signal", &shared);
        let mut documents: Vec<_> = messages
            .iter()
            .flat_map(|message| segments.push(shared_message(message)).unwrap())
            .collect();
        documents.extend(segments.finish());
        documents
    }

    #[test]
    fn senders_are_me_the_number_or_the_aci() {
        let group = Chat {
            id: "group-id".to_owned(),
            kind: ChatKind::Group,
            name: Some("Climbing".to_owned()),
        };
        let mut mine = message("own-aci", 1_760_000_000_000, Some("Mine"));
        mine.from_me = true;
        mine.author_number = Some("+15555550100".to_owned());
        let mut numbered = message("bob-aci", 1_760_000_001_000, Some("Numbered"));
        numbered.author_number = Some("+15555550101".to_owned());
        let unnumbered = message("carol-aci", 1_760_000_002_000, Some("Unnumbered"));
        let docs = documents(&group, &[mine, numbered, unnumbered]);
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].authors, ["Me", "+15555550101", "carol-aci"]);
        assert_eq!(docs[0].thread.as_deref(), Some("group-id"));
        assert_eq!(docs[0].title.as_deref(), Some("Climbing"));
        assert_eq!(docs[0].meta["chat_kind"], "group");
        assert_eq!(docs[0].meta["first_message_id"], "own-aci/1760000000000");
    }

    #[test]
    fn attachments_render_each_with_its_caption_after_the_text() {
        let chat = Chat {
            id: "alice-aci".to_owned(),
            kind: ChatKind::Direct,
            name: None,
        };
        let mut message = message("alice-aci", 1_760_000_000_999, Some("Two files"));
        message.edited = true;
        message.attachments = vec![
            Attachment {
                mime_type: Some("image/jpeg".to_owned()),
                filename: Some("beach.jpg".to_owned()),
                size: Some(1_234_567),
                caption: Some("Sunset".to_owned()),
                voice_note: false,
            },
            Attachment {
                mime_type: Some("audio/aac".to_owned()),
                filename: None,
                size: None,
                caption: None,
                voice_note: true,
            },
        ];
        let docs = documents(&chat, &[message]);
        assert_eq!(docs[0].title.as_deref(), Some("alice-aci"));
        assert_eq!(
            docs[0].text,
            "**alice-aci** · 2025-10-09 08:53 UTC (edited)  \n\
             Attachment: beach.jpg · image/jpeg · 1.2 MB  \n\
             Attachment: voice note · audio/aac  \nTwo files  \nSunset"
        );
    }
}
