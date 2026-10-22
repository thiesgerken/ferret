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
/// About one segment per day of conversation, but a conversation running past midnight stays in
/// one segment.
const PAUSE_SECONDS: i64 = 24 * 3_600;
const CURSOR_VERSION: u32 = 3;
const MESSAGE_SEPARATOR: &str = "\n\n";
/// Markdown's hard line break as two trailing spaces: unlike a trailing backslash, it stays
/// invisible in search snippets.
const LINE_BREAK: &str = "  \n";

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "WhatsApp archive")]
pub struct WhatsAppConfig {
    /// Private archive API root, reached directly by the worker.
    url: String,
    /// The archive API's 64-character hexadecimal bearer secret.
    #[schemars(extend("writeOnly" = true))]
    token: String,
    /// Required archive identity from authenticated `/v1/archive`, as 32 lowercase hex characters.
    #[schemars(extend("pattern" = "^[0-9a-f]{32}$", "x-ferret-preserve-credentials" = true))]
    expected_store_id: String,
    /// Exact chat IDs from the archive inventory. Empty includes all direct/group chats.
    #[serde(default)]
    #[schemars(extend("x-ferret-preserve-credentials" = true))]
    allow_chats: BTreeSet<String>,
    /// Exact chat IDs. Takes precedence over allow_chats; unknown IDs fail closed.
    #[serde(default)]
    #[schemars(extend("x-ferret-preserve-credentials" = true))]
    block_chats: BTreeSet<String>,
}

#[derive(PartialEq, Eq, Serialize, Deserialize)]
struct Settings {
    url: String,
    expected_store_id: String,
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

    async fn archive(&self) -> Result<Archive, PluginError> {
        let archive: Archive = self.get("/v1/archive", &[]).await?;
        if archive.store_id != self.expected_store_id {
            return Err("WhatsApp archive does not match expected_store_id; inspect the authenticated archive before deliberately updating the connection configuration".into());
        }
        if !matches!(archive.schema_version, 32 | 33)
            || archive.floor < 0
            || archive.high < archive.floor
        {
            return Err("WhatsApp archive returned unsupported schema or journal bounds".into());
        }
        Ok(archive)
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

    async fn ingest_chat(
        &self,
        run: &Run,
        chat: &Chat,
        stats: &mut RunStats,
    ) -> Result<(), PluginError> {
        let mut segments = Segments::new(&run.source, chat);
        let mut after: Option<MessageKey> = None;
        let mut ids = BTreeSet::new();
        loop {
            let mut params = vec![
                ("store_id", self.expected_store_id.clone()),
                ("chat", chat.jid.clone()),
            ];
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
                if let Some(document) = segments.push(&message)? {
                    put(run, &document, stats).await?;
                }
            }
            match page.next {
                None => {
                    if let Some(document) = segments.current {
                        put(run, &document, stats).await?;
                    }
                    return Ok(());
                }
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
    if config.expected_store_id.len() != 32
        || !config
            .expected_store_id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(
            "WhatsApp expected_store_id must be 32 lowercase hexadecimal characters".into(),
        );
    }
    let settings = Settings {
        url: url.as_str().trim_end_matches('/').to_owned(),
        expected_store_id: config.expected_store_id,
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
        expected_store_id: settings.expected_store_id.clone(),
        requests: AtomicUsize::new(0),
    };
    let previous = if run.state == json!({}) {
        None
    } else {
        let cursor: Cursor = serde_json::from_value(run.state.clone())
            .map_err(|_| "WhatsApp saved cursor is invalid")?;
        match cursor.version {
            1 => return Err("WhatsApp cursor uses the old index layout; delete/recreate the WhatsApp connection to rebuild the flat index (indexed documents and remembered exclusions will be cleared; the native archive is untouched)".into()),
            2 => {
                run.log.line("Saved cursor predates the Markdown rendering and day-scale segments; re-rendering every chat with a full authoritative reconciliation");
                None
            }
            CURSOR_VERSION => Some(cursor),
            _ => return Err("WhatsApp saved cursor version is unsupported".into()),
        }
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
    let start = client.archive().await?;
    run.log.line(format!(
        "Local archive only; collector status {}, last successful collector sync {:?}",
        start.collector_status, start.last_sync_at
    ));
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
    let mut chats = client.chats().await?;
    validate_filters(settings, &chats)?;
    let root_sync = if full {
        Some(run.api.start_sync(&run.source).await?.sync_id)
    } else {
        None
    };
    let mut affected = if full {
        chats.keys().cloned().collect()
    } else {
        let mut affected = client.changes(beginning, start.high).await?;
        affected.extend(metadata_changes(
            &previous.expect("incremental cursor").chats,
            &chats,
        ));
        affected
    };
    let mut syncs = Vec::new();
    rebuild(run, client, settings, &chats, &affected, &mut syncs, stats).await?;
    // Capture the replay boundary after traversal, not before it: backfill can arrive behind
    // both keysets. This fixed drain is finite even when the collector keeps writing.
    let end = client.archive().await?;
    let latest_chats = client.chats().await?;
    validate_filters(settings, &latest_chats)?;
    affected = client.changes(start.high, end.high).await?;
    affected.extend(metadata_changes(&chats, &latest_chats));
    chats = latest_chats;
    rebuild(run, client, settings, &chats, &affected, &mut syncs, stats).await?;
    // No traversal failure authorizes any sweep. Recheck retention and identity immediately
    // before destructive commits, even when the final change page was empty.
    client.changes(beginning, beginning).await?;
    for sync in syncs.iter().copied().chain(root_sync) {
        client.changes(beginning, beginning).await?;
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
        version: CURSOR_VERSION,
        checkpoint: end.high,
        settings: Settings {
            url: settings.url.clone(),
            expected_store_id: settings.expected_store_id.clone(),
            allow_chats: settings.allow_chats.clone(),
            block_chats: settings.block_chats.clone(),
        },
        chats,
    })?)
}

async fn rebuild(
    run: &Run,
    client: &ArchiveClient,
    settings: &Settings,
    chats: &BTreeMap<String, Chat>,
    affected: &BTreeSet<String>,
    syncs: &mut Vec<Uuid>,
    stats: &mut RunStats,
) -> Result<(), PluginError> {
    for jid in affected {
        let sync = run.api.start_thread_sync(&run.source, jid).await?.sync_id;
        syncs.push(sync);
        if let Some(chat) = chats.get(jid).filter(|chat| eligible(settings, chat)) {
            client.ingest_chat(run, chat, stats).await?;
        }
    }
    Ok(())
}

async fn put(run: &Run, document: &DocumentPut, stats: &mut RunStats) -> Result<(), PluginError> {
    match run.api.put_document(document).await? {
        PutResponse::Created { .. } | PutResponse::Updated { .. } => stats.put += 1,
        PutResponse::Unchanged { .. } => stats.unchanged += 1,
        PutResponse::Excluded => {}
    }
    Ok(())
}

fn timestamp(ts: i64) -> Result<DateTime<Utc>, PluginError> {
    DateTime::from_timestamp(ts, 0)
        .ok_or_else(|| "WhatsApp message timestamp is out of range".into())
}

/// One message as a Markdown block: a bold sender line, then attachment, text and caption, joined
/// by hard line breaks because the viewer renders single newlines as spaces.
fn message_block(message: &Message, sender: &str, at: DateTime<Utc>, with_date: bool) -> String {
    let stamp = at.format(if with_date {
        "%Y-%m-%d %H:%M UTC"
    } else {
        "%H:%M"
    });
    let edited = if message.edited { " (edited)" } else { "" };
    let mut lines = vec![format!("**{}** · {stamp}{edited}", escape_inline(sender))];
    lines.extend(attachment_line(message));
    let body = message
        .text
        .as_deref()
        .filter(|text| !text.is_empty())
        .or(message.display_text.as_deref());
    let caption = message
        .media_caption
        .as_deref()
        .filter(|caption| Some(*caption) != message.text.as_deref());
    for text in [body, caption].into_iter().flatten() {
        // Untrimmed indentation would turn a line after a blank one into a code block.
        lines.extend(
            text.trim()
                .lines()
                .map(|line| escape_block_start(line.trim())),
        );
    }
    lines.join(LINE_BREAK)
}

fn attachment_line(message: &Message) -> Option<String> {
    let size = message.file_length.filter(|size| *size > 0).map(human_size);
    let parts: Vec<_> = [&message.media_type, &message.filename, &message.mime_type]
        .into_iter()
        .flatten()
        .map(|part| escape_inline(part))
        .chain(size)
        .collect();
    (!parts.is_empty()).then(|| format!("Attachment: {}", parts.join(" · ")))
}

fn human_size(bytes: i64) -> String {
    if bytes < 1_000 {
        return format!("{bytes} B");
    }
    let mut size = bytes as f64 / 1_000.0;
    for unit in ["kB", "MB", "GB"] {
        if size < 1_000.0 {
            return format!("{size:.1} {unit}");
        }
        size /= 1_000.0;
    }
    format!("{size:.1} TB")
}

/// Keeps a line of user text from opening a Markdown block (heading, list, quote, code fence,
/// table, HTML, thematic break, setext underline, link definition). Other punctuation like `:)`
/// stays unescaped, since agents read this text raw. Inline emphasis stays too, WhatsApp's own
/// `*bold*` is close enough.
fn escape_block_start(line: &str) -> String {
    let digits = line.len() - line.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    // A table's delimiter row may also start with the alignment colon.
    let delimiter_row = line.contains('-')
        && line
            .chars()
            .all(|c| matches!(c, '|' | ':' | '-' | ' ' | '\t'));
    match line[digits..].chars().next() {
        Some('#' | '>' | '-' | '+' | '*' | '=' | '_' | '`' | '~' | '|' | '<' | '[')
            if digits == 0 =>
        {
            format!("\\{line}")
        }
        Some(':') if delimiter_row => format!("\\{line}"),
        Some('.' | ')') if digits > 0 => format!("{}\\{}", &line[..digits], &line[digits..]),
        _ => line.to_owned(),
    }
}

/// Keeps text inside our own Markdown (the bold sender, attachment metadata) from closing it or
/// starting code, links or emphasis.
fn escape_inline(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '\\' | '`' | '*' | '_' | '~' | '[' | ']' | '<') {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
}

struct Segments<'a> {
    source: &'a str,
    chat: &'a Chat,
    chat_hash: String,
    name: String,
    current: Option<DocumentPut>,
    current_chars: usize,
    previous: Option<DateTime<Utc>>,
}

impl<'a> Segments<'a> {
    fn new(source: &'a str, chat: &'a Chat) -> Self {
        let name: String = chat
            .name
            .as_deref()
            .unwrap_or(&chat.jid)
            .chars()
            .take(256)
            .collect();
        Self {
            source,
            chat,
            chat_hash: digest_id(&chat.jid),
            name,
            current: None,
            current_chars: 0,
            previous: None,
        }
    }

    /// A direct chat's JID is the chat ID itself, and wacli stores it as the sender of my own
    /// history-synced messages there, so a JID only names an otherwise unnamed group member.
    fn sender(&self, message: &Message) -> String {
        let name = message
            .sender_name
            .as_deref()
            .filter(|name| !name.is_empty());
        if message.from_me {
            "Me"
        } else if self.chat.kind == "dm" {
            name.unwrap_or(&self.name)
        } else {
            name.or(message.sender_jid.as_deref())
                .unwrap_or("Unknown sender")
        }
        .to_owned()
    }

    fn push(&mut self, message: &Message) -> Result<Option<DocumentPut>, PluginError> {
        let at = timestamp(message.ts)?;
        let sender = self.sender(message);
        let continuation = self
            .previous
            .filter(|previous| (at - *previous).num_seconds() < PAUSE_SECONDS)
            .map(|previous| {
                message_block(
                    message,
                    &sender,
                    at,
                    at.date_naive() != previous.date_naive(),
                )
            })
            .filter(|block| {
                self.current_chars + MESSAGE_SEPARATOR.len() + block.chars().count()
                    <= SEGMENT_CHARS
            });
        let completed = match continuation {
            Some(block) => {
                let doc = self.current.as_mut().expect("segment");
                doc.text.push_str(MESSAGE_SEPARATOR);
                doc.text.push_str(&block);
                self.current_chars += MESSAGE_SEPARATOR.len() + block.chars().count();
                None
            }
            // One oversized message stays intact; the server's ordinary chunker handles its size.
            None => {
                let block = message_block(message, &sender, at, true);
                self.current_chars = block.chars().count();
                self.current.replace(self.segment(message, at, block))
            }
        };
        let doc = self.current.as_mut().expect("segment");
        if !doc.authors.contains(&sender) {
            doc.authors.push(sender);
        }
        doc.meta
            .insert("last_message_at".to_owned(), json!(at.to_rfc3339()));
        self.previous = Some(at);
        Ok(completed)
    }

    fn segment(&self, first: &Message, at: DateTime<Utc>, text: String) -> DocumentPut {
        DocumentPut {
            source: self.source.to_owned(),
            external_id: format!("{}/{}", self.chat_hash, digest_id(&first.msg_id)),
            content_type: ContentType::Markdown,
            title: Some(self.name.clone()),
            text,
            url: None,
            thread: Some(self.chat.jid.clone()),
            parent: None,
            authors: Vec::new(),
            created_at: Some(at),
            modified_at: None,
            meta: json!({"chat_id": self.chat.jid, "chat_kind": self.chat.kind,
                "first_message_id": first.msg_id, "first_message_rowid": first.rowid})
            .as_object()
            .expect("object")
            .clone(),
        }
    }
}

#[cfg(test)]
fn documents(
    source: &str,
    chat: &Chat,
    messages: &[Message],
) -> Result<Vec<DocumentPut>, PluginError> {
    let mut segments = Segments::new(source, chat);
    let mut documents = Vec::new();
    for message in messages {
        documents.extend(segments.push(message)?);
    }
    documents.extend(segments.current);
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

    fn chat(jid: &str, kind: &str, name: Option<&str>) -> Chat {
        Chat {
            jid: jid.to_owned(),
            kind: kind.to_owned(),
            name: name.map(str::to_owned),
        }
    }

    fn ts(rfc3339: &str) -> i64 {
        DateTime::parse_from_rfc3339(rfc3339).unwrap().timestamp()
    }

    #[test]
    fn segments_are_unicode_safe_day_scale_and_deterministic_without_run_time() {
        let alice = chat("1@s.whatsapp.net", "dm", Some("Alice"));
        let last_night = ts("2026-02-21T23:30:00Z");
        let next_evening = last_night + 3_600 + PAUSE_SECONDS - 1;
        let messages = vec![
            message("first", last_night, "First"),
            message("after-midnight", last_night + 3_600, "After midnight"),
            message("next-evening", next_evening, "Next evening"),
            message("pause", next_evening + PAUSE_SECONDS, "After pause"),
            message(
                "large",
                next_evening + PAUSE_SECONDS + 1,
                &"🦡漢字".repeat(4_000),
            ),
        ];
        let docs = documents("chat/example", &alice, &messages).unwrap();
        assert_eq!(docs.len(), 3);
        for text in ["First", "After midnight", "Next evening"] {
            assert!(docs[0].text.contains(text));
        }
        assert!(docs[1].text.contains("After pause"));
        assert!(docs[0].text.chars().count() <= SEGMENT_CHARS);
        assert!(docs[1].text.chars().count() <= SEGMENT_CHARS);
        assert!(docs[2].text.contains(&"🦡漢字".repeat(4_000)));
        let first = serde_json::to_value(&docs).unwrap();
        assert_eq!(
            first,
            serde_json::to_value(documents("chat/example", &alice, &messages).unwrap()).unwrap()
        );
        assert!(docs.iter().all(|doc| doc.modified_at.is_none()));
    }

    #[test]
    fn segments_close_before_exceeding_the_size_cap() {
        let alice = chat("1@s.whatsapp.net", "dm", Some("Alice"));
        let messages: Vec<_> = (0..20)
            .map(|n| message(&format!("m{n}"), 100 + n, &"x".repeat(1_000)))
            .collect();
        let docs = documents("chat/example", &alice, &messages).unwrap();
        assert!(docs.len() > 2);
        assert!(
            docs.iter()
                .all(|doc| doc.text.chars().count() <= SEGMENT_CHARS)
        );
        assert_eq!(
            docs.iter()
                .map(|doc| doc.text.matches(&"x".repeat(1_000)).count())
                .sum::<usize>(),
            20
        );
        // Every segment names its first message's full date.
        assert!(
            docs.iter()
                .all(|doc| doc.text.starts_with("**Alice** · 1970-01-01 00:"))
        );
    }

    #[test]
    fn messages_render_as_markdown_with_dates_only_at_segment_start_and_day_change() {
        let alex = chat("49@s.whatsapp.net", "dm", Some("Alex Example"));
        let mut mine = message(
            "mine",
            ts("2026-03-14T18:30:12Z"),
            "Shall we continue with the shelves this afternoon?",
        );
        mine.from_me = true;
        // wacli records the chat as the sender of my own history-synced direct messages.
        mine.sender_jid = Some(alex.jid.clone());
        mine.sender_name = Some("Alex Example".to_owned());
        let mut reply = message("reply", ts("2026-03-14T18:30:40Z"), "Sure! 🙂");
        reply.sender_name = Some("Alex Example".to_owned());
        reply.sender_jid = Some(alex.jid.clone());
        let mut next_day = message(
            "next-day",
            ts("2026-03-15T09:05:00Z"),
            "Line one\n\nLine two\n",
        );
        next_day.sender_name = None;
        next_day.edited = true;
        let docs = documents("chats", &alex, &[mine, reply, next_day]).unwrap();
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].content_type, ContentType::Markdown);
        assert_eq!(
            docs[0].text,
            "**Me** · 2026-03-14 18:30 UTC  \nShall we continue with the shelves this afternoon?\n\n\
             **Alex Example** · 18:30  \nSure! 🙂\n\n\
             **Alex Example** · 2026-03-15 09:05 UTC (edited)  \nLine one  \n  \nLine two"
        );
        assert_eq!(docs[0].authors, ["Me", "Alex Example"]);
    }

    #[test]
    fn group_senders_show_a_jid_only_without_a_name() {
        let group = chat("friends@g.us", "group", Some("Friends"));
        let mut mine = message("mine", 100, "Mine");
        mine.from_me = true;
        let named = message("named", 101, "Named");
        let mut unnamed = message("unnamed", 102, "Unnamed");
        unnamed.sender_name = Some(String::new());
        unnamed.sender_jid = Some("2@s.whatsapp.net".to_owned());
        let mut unknown = message("unknown", 103, "Unknown");
        unknown.sender_name = None;
        unknown.sender_jid = None;
        let docs = documents("chats", &group, &[mine, named, unnamed, unknown]).unwrap();
        assert_eq!(
            docs[0].authors,
            ["Me", "Alice", "2@s.whatsapp.net", "Unknown sender"]
        );
        assert!(docs[0].text.contains("**Me** · "));
        assert!(docs[0].text.contains("**Alice** · "));
        assert!(!docs[0].text.contains("1@s.whatsapp.net"));
        assert!(docs[0].text.contains("**2@s.whatsapp.net** · "));
    }

    #[test]
    fn user_text_cannot_open_markdown_blocks_or_break_our_markup() {
        let group = chat("friends@g.us", "group", Some("Friends"));
        let mut tricky = message(
            "tricky",
            100,
            "# Heading\n- item\n1. first\n2024) note\n  > quote\n<div>x</div>\n---\n|a|b|\n:-|-:\n\n    indented\n```\n+49 170 *bold* 3.5 kg\n:) \"Hallo\" (@Bob) :-)",
        );
        tricky.sender_name = Some("A*B_`C".to_owned());
        tricky.media_type = Some("document".to_owned());
        tricky.filename = Some("__init__.py".to_owned());
        let text = &documents("chats", &group, &[tricky]).unwrap()[0].text;
        assert_eq!(
            text,
            "**A\\*B\\_\\`C** · 1970-01-01 00:01 UTC  \nAttachment: document · \\_\\_init\\_\\_.py  \n\
             \\# Heading  \n\\- item  \n1\\. first  \n2024\\) note  \n\\> quote  \n\\<div>x</div>  \n\
             \\---  \n\\|a|b|  \n\\:-|-:  \n  \nindented  \n\\```  \n\\+49 170 *bold* 3.5 kg  \n\
             :) \"Hallo\" (@Bob) :-)"
        );
    }

    #[test]
    fn attachments_render_compactly_and_omit_unknown_and_zero_sizes() {
        let mut attachment = message("attachment", 100, "Photo");
        attachment.media_type = Some("image".to_owned());
        attachment.filename = Some("beach.jpg".to_owned());
        attachment.mime_type = Some("image/jpeg".to_owned());
        attachment.media_caption = Some("Sunset".to_owned());
        for size in [None, Some(0)] {
            attachment.file_length = size;
            assert_eq!(
                attachment_line(&attachment).unwrap(),
                "Attachment: image · beach.jpg · image/jpeg"
            );
        }
        attachment.file_length = Some(1_234_567);
        assert_eq!(
            attachment_line(&attachment).unwrap(),
            "Attachment: image · beach.jpg · image/jpeg · 1.2 MB"
        );
        let block = message_block(&attachment, "Alice", timestamp(100).unwrap(), false);
        assert_eq!(
            block,
            "**Alice** · 00:01  \nAttachment: image · beach.jpg · image/jpeg · 1.2 MB  \nPhoto  \nSunset"
        );
        assert_eq!(attachment_line(&message("plain", 100, "Text")), None);
        assert_eq!(human_size(999), "999 B");
        assert_eq!(human_size(12_345), "12.3 kB");
        assert_eq!(human_size(5_000_000_000), "5.0 GB");
    }

    #[test]
    fn flat_segments_keep_chat_identity_and_actual_speakers_separate() {
        let alice = chat("alice@s.whatsapp.net", "dm", Some("Alice"));
        let group = chat("friends@g.us", "group", Some("Friends"));
        let mut outgoing = message("same-first-id", 100, "Outgoing only");
        outgoing.from_me = true;
        let dm = documents("chats", &alice, &[outgoing.clone()]).unwrap();
        let mut bob = message("bob", 102, "Bob speaks");
        bob.sender_name = Some("Bob".to_owned());
        let group_docs = documents(
            "chats",
            &group,
            &[outgoing, message("alice", 101, "Alice speaks"), bob],
        )
        .unwrap();
        assert_eq!(dm[0].source, "chats");
        assert_eq!(group_docs[0].source, "chats");
        assert_ne!(dm[0].external_id, group_docs[0].external_id);
        assert_eq!(dm[0].thread.as_deref(), Some(alice.jid.as_str()));
        assert_eq!(dm[0].title.as_deref(), Some("Alice"));
        assert_eq!(dm[0].authors, ["Me"]);
        assert_eq!(group_docs[0].thread.as_deref(), Some(group.jid.as_str()));
        assert_eq!(group_docs[0].title.as_deref(), Some("Friends"));
        assert_eq!(group_docs[0].authors, ["Me", "Alice", "Bob"]);
    }

    #[test]
    fn unnamed_direct_counterparts_are_named_like_the_chat() {
        let unnamed = chat("49@s.whatsapp.net", "dm", None);
        let mut incoming = message("incoming", 100, "Hi");
        incoming.sender_name = None;
        incoming.sender_jid = Some(unnamed.jid.clone());
        let docs = documents("chats", &unnamed, &[incoming]).unwrap();
        assert_eq!(docs[0].title.as_deref(), Some("49@s.whatsapp.net"));
        assert_eq!(docs[0].authors, ["49@s.whatsapp.net"]);
    }
}
