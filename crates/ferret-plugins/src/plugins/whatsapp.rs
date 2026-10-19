//! Finite reconciliation of a retained local WhatsApp archive, never a collector.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write,
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

use chrono::{DateTime, Utc};
use ferret_api::{ContentType, DocumentPut, PutResponse, internal::RunStats};
use ferret_whatsapp_archive::{
    Archive, ChangesPage, Chat, ChatsPage, Message, MessageKey, MessagesPage,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::json;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::{Outcome, PluginError, Run};

pub const NAME: &str = "whatsapp";
const MAX_REQUESTS: usize = 20_000;
const SEGMENT_CHARS: usize = 8_000;
const PAUSE_SECONDS: i64 = 3_600;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "WhatsApp archive")]
pub struct WhatsAppConfig {
    /// Private archive API root, reached directly by the worker.
    url: String,
    /// The archive API's 64-character hexadecimal bearer secret.
    #[schemars(extend("writeOnly" = true))]
    token: String,
    /// Exact chat IDs from the archive inventory. Empty includes all direct/group chats.
    #[serde(default)]
    allow_chats: BTreeSet<String>,
    /// Exact chat IDs. Takes precedence over allow_chats; unknown IDs fail closed.
    #[serde(default)]
    block_chats: BTreeSet<String>,
}

#[derive(PartialEq, Eq, Serialize, Deserialize)]
struct Settings {
    url: String,
    allow_chats: BTreeSet<String>,
    block_chats: BTreeSet<String>,
}

#[derive(Serialize, Deserialize)]
struct Cursor {
    version: u32,
    checkpoint: i64,
    settings: Settings,
    chats: BTreeMap<String, Chat>,
}

#[derive(Debug)]
struct Expired;
impl std::fmt::Display for Expired {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WhatsApp journal checkpoint expired")
    }
}
impl std::error::Error for Expired {}

struct ArchiveClient {
    http: reqwest::Client,
    url: String,
    token: String,
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
                "WhatsApp archive request budget exhausted; no sweep or checkpoint advance".into(),
            );
        }
        let response = self
            .http
            .get(format!("{}{path}", self.url))
            .bearer_auth(&self.token)
            .query(params)
            .send()
            .await
            .map_err(|_| "WhatsApp archive request failed")?;
        if response.status() == reqwest::StatusCode::GONE {
            return Err(Box::new(Expired));
        }
        if !response.status().is_success() {
            return Err(format!("WhatsApp archive returned HTTP {}; check API availability, credential and archive identity", response.status()).into());
        }
        response
            .json()
            .await
            .map_err(|_| "WhatsApp archive returned invalid data".into())
    }

    async fn archive(&self, store: &str) -> Result<Archive, PluginError> {
        let archive: Archive = self.get("/v1/archive", &[]).await?;
        if !store.is_empty() && archive.store_id != store {
            return Err("WhatsApp archive store changed during the run".into());
        }
        if !matches!(archive.schema_version, 32 | 33)
            || archive.floor < 0
            || archive.high < archive.floor
        {
            return Err("WhatsApp archive returned unsupported schema or journal bounds".into());
        }
        Ok(archive)
    }

    async fn chats(&self, store: &str) -> Result<BTreeMap<String, Chat>, PluginError> {
        let mut chats = BTreeMap::new();
        let mut after = String::new();
        loop {
            let page: ChatsPage = self
                .get(
                    "/v1/chats",
                    &[("store_id", store.to_owned()), ("after", after.clone())],
                )
                .await?;
            for chat in page.chats {
                if chat.jid <= after || chats.contains_key(&chat.jid) {
                    return Err("WhatsApp chat inventory is incomplete or unordered".into());
                }
                after = chat.jid.clone();
                chats.insert(chat.jid.clone(), chat);
            }
            match page.next {
                None => return Ok(chats),
                Some(next) if next == after && !next.is_empty() => {}
                _ => return Err("WhatsApp chat inventory has invalid continuation".into()),
            }
        }
    }

    async fn changes(
        &self,
        store: &str,
        mut after: i64,
        through: i64,
    ) -> Result<BTreeSet<String>, PluginError> {
        let mut affected = BTreeSet::new();
        loop {
            let page: ChangesPage = self
                .get(
                    "/v1/changes",
                    &[
                        ("store_id", store.to_owned()),
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
                return Err("WhatsApp change journal has invalid continuation".into());
            }
            let mut sequence = after;
            for change in page.changes {
                if change.sequence <= sequence || change.sequence > page.next {
                    return Err("WhatsApp change journal is unordered".into());
                }
                sequence = change.sequence;
                match change.operation.as_str() {
                    "upsert" | "delete" => {}
                    "identity_move" => {
                        let target = change
                            .new_chat_jid
                            .ok_or("WhatsApp identity move lacks its target")?;
                        affected.insert(target);
                    }
                    _ => return Err("WhatsApp change journal has unsupported operation".into()),
                }
                affected.insert(change.chat_jid);
            }
            after = page.next;
            if !page.has_more {
                if after != through {
                    return Err("WhatsApp journal drain is incomplete".into());
                }
                return Ok(affected);
            }
        }
    }

    async fn messages(&self, store: &str, chat: &str) -> Result<Vec<Message>, PluginError> {
        let mut messages = Vec::new();
        let mut after: Option<MessageKey> = None;
        let mut ids = BTreeSet::new();
        loop {
            let mut params = vec![("store_id", store.to_owned()), ("chat", chat.to_owned())];
            if let Some(key) = after {
                params.extend([
                    ("after_ts", key.ts.to_string()),
                    ("after_rowid", key.rowid.to_string()),
                ]);
            }
            let page: MessagesPage = self.get("/v1/messages", &params).await?;
            let previous = after.map(|key| (key.ts, key.rowid));
            let mut last = previous;
            for message in page.messages {
                let key = (message.ts, message.rowid);
                if last.is_some_and(|last| key <= last) || !ids.insert(message.msg_id.clone()) {
                    return Err("WhatsApp message traversal is incomplete or unordered".into());
                }
                last = Some(key);
                messages.push(message);
            }
            match page.next {
                None => return Ok(messages),
                Some(key) if last == Some((key.ts, key.rowid)) && last != previous => {
                    after = Some(key)
                }
                _ => return Err("WhatsApp message traversal has invalid continuation".into()),
            }
        }
    }
}

pub async fn run(run: &Run) -> Result<Outcome, PluginError> {
    let config: WhatsAppConfig = serde_json::from_value(run.config.clone())
        .map_err(|_| "WhatsApp configuration is invalid")?;
    let url = reqwest::Url::parse(&config.url)
        .map_err(|_| "WhatsApp requires an absolute HTTP(S) archive URL")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(
            "WhatsApp requires an HTTP(S) URL without credentials, query or fragment".into(),
        );
    }
    if config.token.len() != 64 || !config.token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("WhatsApp requires a 64-character hexadecimal archive bearer token".into());
    }
    let settings = Settings {
        url: url.as_str().trim_end_matches('/').to_owned(),
        allow_chats: config.allow_chats,
        block_chats: config.block_chats,
    };
    let client = ArchiveClient {
        http: reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(120))
            .redirect(reqwest::redirect::Policy::none())
            .build()?,
        url: settings.url.clone(),
        token: config.token,
        requests: AtomicUsize::new(0),
    };
    let archive = client.archive("").await?;
    // Binding commits before any document/sync write, including a failed first traversal.
    run.api.bind_archive(&archive.store_id).await.map_err(|_| "WhatsApp archive binding rejected; verify identity or recreate the connection for intentional replacement")?;
    run.log.line(format!(
        "Local archive only; collector status {}, last successful collector sync {:?}",
        archive.collector_status, archive.last_sync_at
    ));
    let previous = if run.state == json!({}) {
        None
    } else {
        let cursor: Cursor = serde_json::from_value(run.state.clone())
            .map_err(|_| "WhatsApp saved cursor is invalid")?;
        if cursor.version != 1 {
            return Err("WhatsApp saved cursor version is unsupported".into());
        }
        Some(cursor)
    };
    let mut stats = RunStats::default();
    match reconcile(run, &client, &settings, previous.as_ref(), &mut stats).await {
        Err(error) if error.is::<Expired>() => {
            run.log.line("Journal replay range expired; restarting authoritative local-archive reconciliation (lost replay range)");
            let state = reconcile(run, &client, &settings, None, &mut stats).await?;
            Ok(Outcome { state, stats })
        }
        result => Ok(Outcome {
            state: result?,
            stats,
        }),
    }
}

fn validate_filters(
    settings: &Settings,
    chats: &BTreeMap<String, Chat>,
) -> Result<(), PluginError> {
    if settings
        .allow_chats
        .iter()
        .chain(&settings.block_chats)
        .any(|id| !chats.contains_key(id))
    {
        return Err("WhatsApp filter references a missing chat ID; inspect the authenticated archive inventory and update moved/removed IDs before retrying (filters fail closed)".into());
    }
    Ok(())
}

fn eligible(settings: &Settings, chat: &Chat) -> bool {
    matches!(chat.kind.as_str(), "dm" | "group")
        && chat.jid != "status@broadcast"
        && !settings.block_chats.contains(&chat.jid)
        && (settings.allow_chats.is_empty() || settings.allow_chats.contains(&chat.jid))
}

fn digest_id(value: &str) -> String {
    let mut hex = String::with_capacity(64);
    for byte in Sha256::digest(value.as_bytes()) {
        write!(hex, "{byte:02x}").expect("formatting into a string");
    }
    hex
}

fn source(root: &str, jid: &str) -> String {
    format!("{root}/chats/{}", digest_id(jid))
}

fn metadata_changes(
    before: &BTreeMap<String, Chat>,
    after: &BTreeMap<String, Chat>,
) -> BTreeSet<String> {
    before
        .keys()
        .chain(after.keys())
        .filter(|id| before.get(*id) != after.get(*id))
        .cloned()
        .collect()
}

async fn reconcile(
    run: &Run,
    client: &ArchiveClient,
    settings: &Settings,
    previous: Option<&Cursor>,
    stats: &mut RunStats,
) -> Result<serde_json::Value, PluginError> {
    let start = client.archive("").await?;
    run.api
        .bind_archive(&start.store_id)
        .await
        .map_err(|_| "WhatsApp archive binding rejected")?;
    let full = previous
        .is_none_or(|cursor| cursor.settings != *settings || cursor.checkpoint < start.floor);
    if previous.is_some_and(|cursor| cursor.checkpoint < start.floor) {
        run.log.line("Journal replay range expired; performing authoritative local-archive reconciliation (lost replay range)");
    }
    let beginning = if full {
        start.high
    } else {
        previous.expect("incremental cursor").checkpoint
    };
    let mut chats = client.chats(&start.store_id).await?;
    validate_filters(settings, &chats)?;
    let root_sync = if full {
        Some(run.api.start_sync(&run.source).await?.sync_id)
    } else {
        None
    };
    let mut affected = if full {
        chats.keys().cloned().collect()
    } else {
        let mut affected = client
            .changes(&start.store_id, beginning, start.high)
            .await?;
        affected.extend(metadata_changes(
            &previous.expect("incremental cursor").chats,
            &chats,
        ));
        affected
    };
    let mut syncs = BTreeMap::new();
    rebuild(
        run,
        client,
        settings,
        &start.store_id,
        &chats,
        &affected,
        &mut syncs,
        stats,
    )
    .await?;
    // Capture the replay boundary after traversal, not before it: backfill can arrive behind
    // both keysets. This fixed drain is finite even when the collector keeps writing.
    let end = client.archive(&start.store_id).await?;
    let latest_chats = client.chats(&start.store_id).await?;
    validate_filters(settings, &latest_chats)?;
    affected = client
        .changes(&start.store_id, start.high, end.high)
        .await?;
    affected.extend(metadata_changes(&chats, &latest_chats));
    chats = latest_chats;
    rebuild(
        run,
        client,
        settings,
        &start.store_id,
        &chats,
        &affected,
        &mut syncs,
        stats,
    )
    .await?;
    // No traversal failure authorizes any sweep. Recheck retention and identity immediately
    // before destructive commits, even when the final change page was empty.
    client
        .changes(&start.store_id, beginning, beginning)
        .await?;
    for sync in syncs.values().copied().chain(root_sync) {
        client
            .changes(&start.store_id, beginning, beginning)
            .await?;
        stats.deleted += run.api.commit_sync(sync).await?.deleted;
    }
    run.log.line(format!(
        "{} put, {} unchanged, {} deleted; {} chats checked; unchanged chats skipped",
        stats.put,
        stats.unchanged,
        stats.deleted,
        chats.len()
    ));
    Ok(serde_json::to_value(Cursor {
        version: 1,
        checkpoint: end.high,
        settings: Settings {
            url: settings.url.clone(),
            allow_chats: settings.allow_chats.clone(),
            block_chats: settings.block_chats.clone(),
        },
        chats,
    })?)
}

#[allow(clippy::too_many_arguments)]
async fn rebuild(
    run: &Run,
    client: &ArchiveClient,
    settings: &Settings,
    store: &str,
    chats: &BTreeMap<String, Chat>,
    affected: &BTreeSet<String>,
    syncs: &mut BTreeMap<String, Uuid>,
    stats: &mut RunStats,
) -> Result<(), PluginError> {
    for jid in affected {
        let source = source(&run.source, jid);
        let sync = run.api.start_sync(&source).await?.sync_id;
        syncs.insert(source.clone(), sync);
        if let Some(chat) = chats.get(jid).filter(|chat| eligible(settings, chat)) {
            let messages = client.messages(store, jid).await?;
            for document in documents(&source, chat, &messages)? {
                match run.api.put_document(&document).await? {
                    PutResponse::Created { .. } | PutResponse::Updated { .. } => stats.put += 1,
                    PutResponse::Unchanged { .. } => stats.unchanged += 1,
                    PutResponse::Excluded => {}
                }
            }
        }
    }
    Ok(())
}

fn timestamp(ts: i64) -> Result<DateTime<Utc>, PluginError> {
    DateTime::from_timestamp(ts, 0)
        .ok_or_else(|| "WhatsApp message timestamp is out of range".into())
}

fn message_text(message: &Message) -> Result<(String, String), PluginError> {
    let sender = if message.from_me {
        "Me"
    } else {
        message
            .sender_name
            .as_deref()
            .filter(|name| !name.is_empty())
            .or(message.sender_jid.as_deref())
            .unwrap_or("Unknown sender")
    }
    .to_owned();
    let mut text = format!("[{}] {}", timestamp(message.ts)?.to_rfc3339(), sender);
    if let Some(jid) = &message.sender_jid {
        write!(text, " ({jid})")?;
    }
    if message.edited {
        text.push_str(" [edited]");
    }
    text.push('\n');
    if let Some(body) = message
        .text
        .as_deref()
        .filter(|text| !text.is_empty())
        .or(message.display_text.as_deref())
    {
        writeln!(text, "{body}")?;
    }
    if let Some(caption) = &message.media_caption {
        if Some(caption.as_str()) != message.text.as_deref() {
            writeln!(text, "Caption: {caption}")?;
        }
    }
    if let Some(kind) = &message.media_type {
        writeln!(text, "Attachment type: {kind}")?;
    }
    if let Some(filename) = &message.filename {
        writeln!(text, "Filename: {filename}")?;
    }
    if let Some(mime) = &message.mime_type {
        writeln!(text, "MIME type: {mime}")?;
    }
    if let Some(size) = message.file_length {
        writeln!(text, "Size: {size} bytes")?;
    }
    Ok((text, sender))
}

fn documents(
    source: &str,
    chat: &Chat,
    messages: &[Message],
) -> Result<Vec<DocumentPut>, PluginError> {
    let name: String = chat
        .name
        .as_deref()
        .unwrap_or(&chat.jid)
        .chars()
        .take(256)
        .collect();
    let header = format!("WhatsApp chat: {name}\nChat ID: {}\n\n", chat.jid);
    let header_chars = header.chars().count();
    if header_chars >= SEGMENT_CHARS {
        return Err("WhatsApp chat ID is too long".into());
    }
    let mut documents: Vec<DocumentPut> = Vec::new();
    let mut previous_ts: Option<i64> = None;
    let mut current_chars = 0;
    for message in messages {
        let (rendered, sender) = message_text(message)?;
        let message_chars = rendered.chars().count();
        let pause = previous_ts.is_some_and(|ts| message.ts.saturating_sub(ts) >= PAUSE_SECONDS);
        if documents.is_empty() || pause || current_chars + message_chars > SEGMENT_CHARS {
            let external_id = digest_id(&message.msg_id);
            documents.push(DocumentPut {
                source: source.to_owned(),
                external_id,
                content_type: ContentType::Plain,
                title: Some(format!("{name} · {}", timestamp(message.ts)?.to_rfc3339())),
                text: header.clone(),
                url: None,
                thread: Some(chat.jid.clone()),
                authors: Vec::new(),
                created_at: Some(timestamp(message.ts)?),
                modified_at: None,
                meta: json!({"chat_id": chat.jid, "chat_kind": chat.kind,
                    "first_message_id": message.msg_id, "first_message_rowid": message.rowid})
                .as_object()
                .expect("object")
                .clone(),
            });
            current_chars = header_chars;
        }
        // One oversized message stays intact; the server's ordinary chunker handles its size.
        let doc = documents.last_mut().expect("segment");
        doc.text.push_str(&rendered);
        current_chars += message_chars;
        if !doc.authors.contains(&sender) {
            doc.authors.push(sender);
        }
        doc.meta.insert(
            "last_message_at".to_owned(),
            json!(timestamp(message.ts)?.to_rfc3339()),
        );
        previous_ts = Some(message.ts);
    }
    Ok(documents)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(id: &str, ts: i64, text: &str) -> Message {
        Message {
            rowid: 1,
            msg_id: id.to_owned(),
            sender_jid: Some("1@s.whatsapp.net".to_owned()),
            sender_name: Some("Alice".to_owned()),
            ts,
            from_me: false,
            text: Some(text.to_owned()),
            display_text: None,
            media_type: None,
            media_caption: None,
            filename: None,
            mime_type: None,
            file_length: None,
            edited: false,
        }
    }

    #[test]
    fn segments_are_unicode_safe_pause_bounded_and_deterministic_without_run_time() {
        let chat = Chat {
            jid: "1@s.whatsapp.net".to_owned(),
            kind: "dm".to_owned(),
            name: Some("Alice".to_owned()),
        };
        let messages = vec![
            message("first", 100, "First"),
            message("second", 101, "Second"),
            message("pause", 3_701, "After pause"),
            message("large", 3_702, &"🦡漢字".repeat(4_000)),
        ];
        let docs = documents("chat/example", &chat, &messages).unwrap();
        assert!(docs[0].text.contains("First") && docs[0].text.contains("Second"));
        assert!(docs[1].text.contains("After pause"));
        assert!(
            docs[..docs.len() - 1]
                .iter()
                .all(|doc| doc.text.chars().count() <= SEGMENT_CHARS)
        );
        assert!(docs.last().unwrap().text.contains(&"🦡漢字".repeat(4_000)));
        let first = serde_json::to_value(&docs).unwrap();
        assert_eq!(
            first,
            serde_json::to_value(documents("chat/example", &chat, &messages).unwrap()).unwrap()
        );
        assert_eq!(
            docs.iter()
                .map(|doc| doc.text.matches('🦡').count())
                .sum::<usize>(),
            4_000
        );
        assert!(docs.iter().all(|doc| doc.modified_at.is_none()));
    }
}
