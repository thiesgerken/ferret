//! What chat plugins share: Markdown segments of a chat's messages, and the finite reconciliation
//! of a chat archive's inventory and change journal into those segments.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write,
};

use chrono::{DateTime, Utc};
use ferret_api::{ContentType, DocumentPut, PutResponse, internal::RunStats};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::{PluginError, Run};

pub const SEGMENT_CHARS: usize = 8_000;
/// About one segment per day of conversation, but a conversation running past midnight stays in
/// one segment.
pub const PAUSE_SECONDS: i64 = 24 * 3_600;
const MESSAGE_SEPARATOR: &str = "\n\n";
/// Markdown's hard line break as two trailing spaces: unlike a trailing backslash, it stays
/// invisible in search snippets.
const LINE_BREAK: &str = "  \n";

/// A chat as segments see it.
pub struct Chat<'a> {
    pub id: &'a str,
    pub kind: &'a str,
    /// The chat's name, or its ID for an unnamed chat.
    pub title: String,
}

impl<'a> Chat<'a> {
    pub fn new(id: &'a str, kind: &'a str, name: Option<&str>) -> Self {
        Self {
            id,
            kind,
            title: name.unwrap_or(id).chars().take(256).collect(),
        }
    }
}

/// A message as segments see it, with its sender already resolved by the plugin.
pub struct Message<'a> {
    /// Unique within its chat; a segment is named after the message that opens it.
    pub id: &'a str,
    pub sender: String,
    /// Unix seconds.
    pub ts: i64,
    pub edited: bool,
    pub attachment: Attachment<'a>,
    pub body: Option<&'a str>,
    pub caption: Option<&'a str>,
    /// Recorded in the metadata of a segment this message opens, next to its ID.
    pub segment_meta: Map<String, Value>,
}

/// Attachment metadata; the binary itself is never indexed.
pub struct Attachment<'a> {
    pub kind: Option<&'a str>,
    pub filename: Option<&'a str>,
    pub mime_type: Option<&'a str>,
    /// Bytes.
    pub size: Option<i64>,
}

pub fn digest_id(value: &str) -> String {
    let mut hex = String::with_capacity(64);
    for byte in Sha256::digest(value.as_bytes()) {
        write!(hex, "{byte:02x}").expect("formatting into a string");
    }
    hex
}

pub fn timestamp(ts: i64) -> Result<DateTime<Utc>, PluginError> {
    DateTime::from_timestamp(ts, 0).ok_or_else(|| "Chat message timestamp is out of range".into())
}

pub async fn put(
    run: &Run,
    document: &DocumentPut,
    stats: &mut RunStats,
) -> Result<(), PluginError> {
    match run.api.put_document(document).await? {
        PutResponse::Created { .. } | PutResponse::Updated { .. } => stats.put += 1,
        PutResponse::Unchanged { .. } => stats.unchanged += 1,
        PutResponse::Excluded => {}
    }
    Ok(())
}

/// One message as a Markdown block: a bold sender line, then attachment, text and caption, joined
/// by hard line breaks because the viewer renders single newlines as spaces.
pub fn message_block(message: &Message, at: DateTime<Utc>, with_date: bool) -> String {
    let stamp = at.format(if with_date {
        "%Y-%m-%d %H:%M UTC"
    } else {
        "%H:%M"
    });
    let edited = if message.edited { " (edited)" } else { "" };
    let mut lines = vec![format!(
        "**{}** · {stamp}{edited}",
        escape_inline(&message.sender)
    )];
    lines.extend(attachment_line(&message.attachment));
    for text in [message.body, message.caption].into_iter().flatten() {
        // Untrimmed indentation would turn a line after a blank one into a code block.
        lines.extend(
            text.trim()
                .lines()
                .map(|line| escape_block_start(line.trim())),
        );
    }
    lines.join(LINE_BREAK)
}

pub fn attachment_line(attachment: &Attachment) -> Option<String> {
    let size = attachment.size.filter(|size| *size > 0).map(human_size);
    let parts: Vec<_> = [attachment.kind, attachment.filename, attachment.mime_type]
        .into_iter()
        .flatten()
        .map(escape_inline)
        .chain(size)
        .collect();
    (!parts.is_empty()).then(|| format!("Attachment: {}", parts.join(" · ")))
}

pub fn human_size(bytes: i64) -> String {
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
/// stays unescaped, since agents read this text raw. Inline emphasis stays too, messengers' own
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

/// Splits one chat's messages, pushed in order, into segment documents.
pub struct Segments<'a> {
    source: &'a str,
    chat: &'a Chat<'a>,
    chat_hash: String,
    current: Option<DocumentPut>,
    current_chars: usize,
    previous: Option<DateTime<Utc>>,
}

impl<'a> Segments<'a> {
    pub fn new(source: &'a str, chat: &'a Chat<'a>) -> Self {
        Self {
            source,
            chat,
            chat_hash: digest_id(chat.id),
            current: None,
            current_chars: 0,
            previous: None,
        }
    }

    /// Adds a message to the open segment, or opens a new one and returns the completed one.
    pub fn push(&mut self, message: Message) -> Result<Option<DocumentPut>, PluginError> {
        let at = timestamp(message.ts)?;
        let continuation = self
            .previous
            .filter(|previous| (at - *previous).num_seconds() < PAUSE_SECONDS)
            .map(|previous| message_block(&message, at, at.date_naive() != previous.date_naive()))
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
                let block = message_block(&message, at, true);
                self.current_chars = block.chars().count();
                self.current.replace(self.segment(&message, at, block))
            }
        };
        let doc = self.current.as_mut().expect("segment");
        if !doc.authors.contains(&message.sender) {
            doc.authors.push(message.sender);
        }
        doc.meta
            .insert("last_message_at".to_owned(), json!(at.to_rfc3339()));
        self.previous = Some(at);
        Ok(completed)
    }

    /// The open segment, once every message is pushed.
    pub fn finish(self) -> Option<DocumentPut> {
        self.current
    }

    fn segment(&self, first: &Message, at: DateTime<Utc>, text: String) -> DocumentPut {
        let mut meta = json!({"chat_id": self.chat.id, "chat_kind": self.chat.kind,
            "first_message_id": first.id})
        .as_object()
        .expect("object")
        .clone();
        meta.extend(first.segment_meta.clone());
        DocumentPut {
            source: self.source.to_owned(),
            external_id: format!("{}/{}", self.chat_hash, digest_id(first.id)),
            content_type: ContentType::Markdown,
            title: Some(self.chat.title.clone()),
            text,
            url: None,
            thread: Some(self.chat.id.to_owned()),
            parent: None,
            authors: Vec::new(),
            created_at: Some(at),
            modified_at: None,
            meta,
        }
    }
}

/// What a connection selects from its chat archive; a change forces a full reconciliation.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    pub url: String,
    pub expected_store_id: String,
    pub allow_chats: BTreeSet<String>,
    pub block_chats: BTreeSet<String>,
}

impl Settings {
    fn allows(&self, id: &str) -> bool {
        !self.block_chats.contains(id)
            && (self.allow_chats.is_empty() || self.allow_chats.contains(id))
    }
}

#[derive(Serialize, Deserialize)]
pub struct Cursor<C> {
    pub version: u32,
    pub checkpoint: i64,
    pub settings: Settings,
    pub chats: BTreeMap<String, C>,
}

/// The change journal's current bounds.
pub struct Journal {
    /// Oldest sequence still replayable.
    pub floor: i64,
    pub high: i64,
    /// The collector's health, for the run log.
    pub collector: String,
}

/// A chat archive's API: a chat inventory keyed by chat ID, and a change journal naming the chats
/// whose messages changed.
pub trait ChatArchive {
    type Chat: PartialEq + Serialize;

    async fn journal(&self) -> Result<Journal, PluginError>;
    async fn chats(&self) -> Result<BTreeMap<String, Self::Chat>, PluginError>;
    /// The chats with changes in `(after, through]`.
    async fn changes(&self, after: i64, through: i64) -> Result<BTreeSet<String>, PluginError>;
    /// Whether the plugin indexes this kind of chat at all, before the connection's filters.
    fn indexable(chat: &Self::Chat) -> bool;
    async fn ingest_chat(
        &self,
        run: &Run,
        chat: &Self::Chat,
        stats: &mut RunStats,
    ) -> Result<(), PluginError>;
}

/// Full when there is no usable cursor, incremental from the cursor's checkpoint otherwise.
/// Returns the next cursor.
pub async fn reconcile<A: ChatArchive>(
    run: &Run,
    archive: &A,
    settings: &Settings,
    previous: Option<&Cursor<A::Chat>>,
    cursor_version: u32,
    stats: &mut RunStats,
) -> Result<Value, PluginError> {
    let start = archive.journal().await?;
    run.log
        .line(format!("Local archive only; {}", start.collector));
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
    let mut chats = archive.chats().await?;
    validate_filters(settings, &chats)?;
    let root_sync = if full {
        Some(run.api.start_sync(&run.source).await?.sync_id)
    } else {
        None
    };
    let mut affected = if full {
        chats.keys().cloned().collect()
    } else {
        let mut affected = archive.changes(beginning, start.high).await?;
        affected.extend(metadata_changes(
            &previous.expect("incremental cursor").chats,
            &chats,
        ));
        affected
    };
    let mut syncs = Vec::new();
    rebuild(run, archive, settings, &chats, &affected, &mut syncs, stats).await?;
    // Capture the replay boundary after traversal, not before it: backfill can arrive behind
    // both keysets. This fixed drain is finite even when the collector keeps writing.
    let end = archive.journal().await?;
    let latest_chats = archive.chats().await?;
    validate_filters(settings, &latest_chats)?;
    affected = archive.changes(start.high, end.high).await?;
    affected.extend(metadata_changes(&chats, &latest_chats));
    chats = latest_chats;
    rebuild(run, archive, settings, &chats, &affected, &mut syncs, stats).await?;
    // No traversal failure authorizes any sweep. Recheck retention and identity immediately
    // before destructive commits, even when the final change page was empty.
    archive.changes(beginning, beginning).await?;
    for sync in syncs.iter().copied().chain(root_sync) {
        archive.changes(beginning, beginning).await?;
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
        version: cursor_version,
        checkpoint: end.high,
        settings: settings.clone(),
        chats,
    })?)
}

fn validate_filters<C>(
    settings: &Settings,
    chats: &BTreeMap<String, C>,
) -> Result<(), PluginError> {
    if settings
        .allow_chats
        .iter()
        .chain(&settings.block_chats)
        .any(|id| !chats.contains_key(id))
    {
        return Err("Chat filter references a missing chat ID; inspect the authenticated archive inventory and update moved/removed IDs before retrying (filters fail closed)".into());
    }
    Ok(())
}

fn metadata_changes<C: PartialEq>(
    before: &BTreeMap<String, C>,
    after: &BTreeMap<String, C>,
) -> BTreeSet<String> {
    before
        .keys()
        .chain(after.keys())
        .filter(|id| before.get(*id) != after.get(*id))
        .cloned()
        .collect()
}

async fn rebuild<A: ChatArchive>(
    run: &Run,
    archive: &A,
    settings: &Settings,
    chats: &BTreeMap<String, A::Chat>,
    affected: &BTreeSet<String>,
    syncs: &mut Vec<Uuid>,
    stats: &mut RunStats,
) -> Result<(), PluginError> {
    for id in affected {
        let sync = run.api.start_thread_sync(&run.source, id).await?.sync_id;
        syncs.push(sync);
        if let Some(chat) = chats
            .get(id)
            .filter(|chat| A::indexable(chat) && settings.allows(id))
        {
            archive.ingest_chat(run, chat, stats).await?;
        }
    }
    Ok(())
}
