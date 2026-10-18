//! Karakeep's own readable text and annotations, reconciled as one document per bookmark.

use std::{collections::HashSet, fmt::Write, time::Duration};

use chrono::{DateTime, Utc};
use ferret_api::{ContentType, DocumentPut, PutResponse, internal::RunStats};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, de::DeserializeOwned};
use serde_json::json;

use super::{Outcome, PluginError, Run};

pub const NAME: &str = "karakeep";

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "Karakeep")]
pub struct KarakeepConfig {
    /// The instance root as the worker reaches it, without `/api/v1`. Requires Karakeep 0.34.0.
    url: String,
    /// The browser-facing instance root. Uses url when omitted.
    #[serde(default)]
    public_url: Option<String>,
    /// An API key with bookmarks:read and highlights:read scopes, for the library's owner.
    #[schemars(extend("writeOnly" = true))]
    token: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BookmarkPage {
    bookmarks: Vec<Bookmark>,
    #[serde(deserialize_with = "nullable")]
    next_cursor: Option<String>,
}

// A missing cursor is not a terminal page. Serde otherwise treats missing Option fields as null.
fn nullable<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::deserialize(deserializer)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Bookmark {
    id: String,
    created_at: DateTime<Utc>,
    modified_at: Option<DateTime<Utc>>,
    title: Option<String>,
    archived: bool,
    favourited: bool,
    note: Option<String>,
    tags: Vec<Tag>,
    content: BookmarkContent,
}

#[derive(Deserialize)]
struct Tag {
    name: String,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum BookmarkContent {
    Link {
        url: String,
        title: Option<String>,
        author: Option<String>,
    },
    Text {
        #[serde(rename = "sourceUrl")]
        source_url: Option<String>,
    },
    Asset {
        #[serde(rename = "assetType")]
        asset_type: String,
        #[serde(rename = "fileName")]
        file_name: Option<String>,
        #[serde(rename = "sourceUrl")]
        source_url: Option<String>,
    },
}

impl BookmarkContent {
    fn kind(&self) -> &'static str {
        match self {
            Self::Link { .. } => "link",
            Self::Text { .. } => "text",
            Self::Asset { .. } => "asset",
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ContentPage {
    bookmark_id: String,
    bookmark_type: String,
    format: String,
    content: String,
    content_version: String,
    range: ContentRange,
    #[serde(deserialize_with = "nullable")]
    next_cursor: Option<String>,
    truncated: bool,
}

#[derive(Deserialize)]
struct ContentRange {
    start: usize,
    end: usize,
    total: usize,
}

#[derive(Deserialize)]
struct Highlights {
    highlights: Vec<Highlight>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Highlight {
    id: String,
    bookmark_id: String,
    created_at: DateTime<Utc>,
    text: Option<String>,
    note: Option<String>,
}

pub async fn run(run: &Run) -> Result<Outcome, PluginError> {
    let config: KarakeepConfig = serde_json::from_value(run.config.clone())
        .map_err(|_| "Karakeep configuration is invalid")?;
    let karakeep = Karakeep::new(&config)?;
    let links = instance_url(config.public_url.as_deref().unwrap_or(&config.url))?;
    let sync = run.api.start_sync(&run.source).await?.sync_id;
    let mut stats = RunStats::default();
    let mut ids = HashSet::new();
    let mut cursors = HashSet::new();
    let mut cursor = None;
    loop {
        let mut query = vec![("limit", "100"), ("includeContent", "false")];
        if let Some(cursor) = cursor.as_deref() {
            query.push(("cursor", cursor));
        }
        // With no list/tag/status filters this endpoint lists only the API-key user's library.
        let page: BookmarkPage = karakeep.get(None, "bookmarks", &query).await?;
        if page.bookmarks.len() > 100
            || (page.bookmarks.is_empty() && (cursor.is_some() || page.next_cursor.is_some()))
        {
            return Err(
                "Karakeep returned an incomplete bookmark listing; the next run retries".into(),
            );
        }
        for bookmark in page.bookmarks {
            if bookmark.id.is_empty() || !ids.insert(bookmark.id.clone()) {
                return Err(
                    "Karakeep repeated a bookmark in its listing; the next run retries".into(),
                );
            }
            let body = karakeep.content(&bookmark).await?;
            let mut annotations: Highlights =
                karakeep.get(Some(&bookmark.id), "highlights", &[]).await?;
            let mut highlight_ids = HashSet::new();
            if annotations.highlights.iter().any(|highlight| {
                highlight.id.is_empty()
                    || highlight.bookmark_id != bookmark.id
                    || !highlight_ids.insert(&highlight.id)
            }) {
                return Err(
                    "Karakeep returned inconsistent highlights; the next run retries".into(),
                );
            }
            annotations
                .highlights
                .sort_unstable_by(|a, b| (a.created_at, &a.id).cmp(&(b.created_at, &b.id)));
            let document =
                to_document(bookmark, body, annotations.highlights, &links, &run.source)?;
            match run.api.put_document(&document).await? {
                PutResponse::Created { .. } | PutResponse::Updated { .. } => stats.put += 1,
                PutResponse::Unchanged { .. } => stats.unchanged += 1,
                PutResponse::Excluded => {}
            }
        }
        match page.next_cursor {
            None => break,
            Some(next) if !next.is_empty() && cursors.insert(next.clone()) => cursor = Some(next),
            Some(_) => {
                return Err(
                    "Karakeep repeated or omitted a listing cursor; the next run retries".into(),
                );
            }
        }
    }
    stats.deleted = run.api.commit_sync(sync).await?.deleted;
    run.log.line(format!(
        "{} bookmarks: {} put, {} unchanged, {} deleted",
        ids.len(),
        stats.put,
        stats.unchanged,
        stats.deleted
    ));
    Ok(Outcome {
        state: json!({}),
        stats,
    })
}

fn to_document(
    bookmark: Bookmark,
    body: String,
    highlights: Vec<Highlight>,
    links: &reqwest::Url,
    source: &str,
) -> Result<DocumentPut, PluginError> {
    let kind = bookmark.content.kind();
    let (fallback_title, original_url, authors, asset_type, file_name) = match bookmark.content {
        BookmarkContent::Link { title, url, author } => {
            (title, Some(url), author.into_iter().collect(), None, None)
        }
        BookmarkContent::Text { source_url } => (None, source_url, Vec::new(), None, None),
        BookmarkContent::Asset {
            asset_type,
            file_name,
            source_url,
        } => (
            file_name.clone(),
            source_url,
            Vec::new(),
            Some(asset_type),
            file_name,
        ),
    };
    let mut tags: Vec<_> = bookmark.tags.into_iter().map(|tag| tag.name).collect();
    tags.sort_unstable();
    tags.dedup();
    let mut text = String::new();
    if let Some(url) = &original_url {
        writeln!(text, "URL: {url}").unwrap();
    }
    if let Some(name) = &file_name {
        writeln!(text, "File: {name}").unwrap();
    }
    if !tags.is_empty() {
        writeln!(text, "Tags: {}", tags.join(", ")).unwrap();
    }
    if !body.is_empty() {
        write!(text, "\nContent:\n{body}\n").unwrap();
    }
    if let Some(note) = bookmark.note.filter(|note| !note.is_empty()) {
        write!(text, "\nBookmark notes:\n{note}\n").unwrap();
    }
    let mut modified_at = bookmark.modified_at.unwrap_or(bookmark.created_at);
    if !highlights.is_empty() {
        text.push_str("\nHighlights:\n");
    }
    for highlight in highlights {
        modified_at = modified_at.max(highlight.created_at);
        writeln!(text, "\n{}", highlight.created_at.to_rfc3339()).unwrap();
        if let Some(quote) = highlight.text {
            writeln!(text, "Quote: {quote}").unwrap();
        }
        if let Some(note) = highlight.note {
            writeln!(text, "Note: {note}").unwrap();
        }
    }
    let serde_json::Value::Object(meta) = json!({
        "bookmark_type": kind,
        "archived": bookmark.archived,
        "favourited": bookmark.favourited,
        "original_url": original_url,
        "tags": tags,
        "asset_type": asset_type,
        "file_name": file_name,
    }) else {
        unreachable!("metadata is an object")
    };
    let mut url = links.clone();
    url.path_segments_mut()
        .map_err(|_| "Karakeep needs an HTTP(S) instance URL")?
        .pop_if_empty()
        .extend(["dashboard", "preview", &bookmark.id]);
    Ok(DocumentPut {
        source: source.to_owned(),
        external_id: bookmark.id,
        content_type: ContentType::Plain,
        title: bookmark.title.or(fallback_title),
        text,
        url: Some(url.into()),
        thread: None,
        authors,
        created_at: bookmark.created_at,
        modified_at,
        meta,
    })
}

fn instance_url(value: &str) -> Result<reqwest::Url, PluginError> {
    let url = reqwest::Url::parse(value)
        .map_err(|_| "Karakeep needs an absolute HTTP(S) instance URL")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(
            "Karakeep needs an HTTP(S) instance URL without credentials, query or fragment".into(),
        );
    }
    Ok(url)
}

struct Karakeep {
    http: reqwest::Client,
    base_url: reqwest::Url,
    token: String,
}

impl Karakeep {
    fn new(config: &KarakeepConfig) -> Result<Self, PluginError> {
        Ok(Self {
            http: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(10))
                .read_timeout(Duration::from_secs(120))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            base_url: instance_url(&config.url)?,
            token: config.token.clone(),
        })
    }

    async fn get<T: DeserializeOwned>(
        &self,
        bookmark_id: Option<&str>,
        operation: &'static str,
        query: &[(&str, &str)],
    ) -> Result<T, PluginError> {
        let mut url = self.base_url.clone();
        {
            let mut segments = url
                .path_segments_mut()
                .map_err(|_| "Karakeep needs an HTTP(S) instance URL")?;
            segments.pop_if_empty().extend(["api", "v1", "bookmarks"]);
            if let Some(id) = bookmark_id {
                segments.extend([id, operation]);
            }
        }
        let response = self
            .http
            .get(url)
            .bearer_auth(&self.token)
            .header("Accept", "application/json")
            .query(query)
            .send()
            .await
            .map_err(reqwest::Error::without_url)?;
        let status = response.status();
        if status.is_redirection() {
            return Err(format!("Karakeep redirected {operation} ({status}); use a direct instance address, not a login proxy").into());
        }
        if !status.is_success() {
            return Err(format!("Karakeep answered {operation} with {status}; version 0.34.0 and bookmarks:read/highlights:read scopes are required").into());
        }
        // Origins can echo secrets in bodies and decoder errors. Only the operation is safe to log.
        response.json().await.map_err(|_| {
            format!("Karakeep answered {operation} with an invalid API response").into()
        })
    }

    async fn content(&self, bookmark: &Bookmark) -> Result<String, PluginError> {
        let mut text = String::new();
        let mut cursor = None;
        let mut cursors = HashSet::new();
        let mut version = None;
        let mut total = None;
        let mut offset = 0;
        loop {
            let mut query = vec![("format", "text")];
            if let Some(cursor) = cursor.as_deref() {
                query.push(("cursor", cursor));
            }
            let page: ContentPage = self.get(Some(&bookmark.id), "content", &query).await?;
            let range = page.range;
            if page.bookmark_id != bookmark.id
                || page.bookmark_type != bookmark.content.kind()
                || page.format != "text"
                || page.content_version.is_empty()
                || *version.get_or_insert_with(|| page.content_version.clone())
                    != page.content_version
                || *total.get_or_insert(range.total) != range.total
                || range.start != offset
                || range.end < range.start
                || range.end > range.total
                || range.end - range.start != page.content.chars().count()
                || page.truncated != (range.end < range.total)
                || page.truncated != page.next_cursor.is_some()
                || (page.truncated && range.end == range.start)
            {
                return Err(
                    "Karakeep returned changed or incomplete text content; the next run retries"
                        .into(),
                );
            }
            text.push_str(&page.content);
            offset = range.end;
            match page.next_cursor {
                None => return Ok(text),
                Some(next) if !next.is_empty() && cursors.insert(next.clone()) => {
                    cursor = Some(next)
                }
                Some(_) => {
                    return Err(
                        "Karakeep repeated or omitted a content cursor; the next run retries"
                            .into(),
                    );
                }
            }
        }
    }
}
