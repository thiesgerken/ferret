//! Shared request and response types of ferret's REST and internal APIs, plus a small HTTP
//! client for them.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

/// A user, as `GET /api/users` lists them.
#[derive(Debug, Serialize, Deserialize)]
pub struct User {
    pub id: Uuid,
    pub username: String,
    pub display_name: Option<String>,
    pub is_admin: bool,
    pub enabled: bool,
}

pub mod internal;

/// How the server chunks a document's text and how the viewer renders it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ContentType {
    Markdown,
    Plain,
    Email,
}

/// The body of `PUT /api/documents`: a document, identified by `(source, external_id)`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentPut {
    /// A source path like `notes` or `mail/zoho`.
    pub source: String,
    /// Stable id given by the ingester, unique within the source.
    pub external_id: String,
    pub content_type: ContentType,
    pub title: Option<String>,
    pub text: String,
    /// Deep link to the origin. Without one, ferret's viewer is the origin.
    pub url: Option<String>,
    /// Groups documents into a sequence, e.g. a chat channel or a mail thread.
    pub thread: Option<String>,
    pub authors: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub modified_at: DateTime<Utc>,
    #[serde(default)]
    pub meta: Map<String, Value>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PutOutcome {
    Created,
    Updated,
    /// Same content as stored: nothing was rewritten.
    Unchanged,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PutResponse {
    pub id: Uuid,
    pub outcome: PutOutcome,
}

/// A stored document, as `GET /api/documents/{id}` returns it.
#[derive(Debug, Serialize, Deserialize)]
pub struct Document {
    pub id: Uuid,
    pub source: String,
    pub external_id: String,
    pub content_type: ContentType,
    pub title: Option<String>,
    pub text: String,
    pub url: Option<String>,
    pub thread: Option<String>,
    pub authors: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub modified_at: DateTime<Utc>,
    pub meta: Map<String, Value>,
}

/// The parameters of `GET /api/search`.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SearchQuery {
    /// Words, `"exact phrases"` and `-excluded` words.
    pub q: String,
    /// Only documents in these sources or below them. Empty: all sources. Repeated `source=` in
    /// the query string.
    #[serde(default, rename = "source")]
    pub sources: Vec<String>,
    /// Only documents created at or after this time.
    pub from: Option<DateTime<Utc>>,
    /// Only documents created before this time.
    pub to: Option<DateTime<Utc>>,
    /// How many hits at most. The server's default applies when `None`.
    pub k: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchResponse {
    pub hits: Vec<Hit>,
}

/// A document returned by a search, with its best-matching chunk.
#[derive(Debug, Serialize, Deserialize)]
pub struct Hit {
    pub document_id: Uuid,
    pub source: String,
    pub external_id: String,
    pub content_type: ContentType,
    pub title: Option<String>,
    pub url: Option<String>,
    pub thread: Option<String>,
    pub authors: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub modified_at: DateTime<Utc>,
    pub score: f32,
    pub chunk: HitChunk,
}

/// The best-matching chunk of a hit. Offsets count characters (Unicode scalar values) into the
/// document's text.
#[derive(Debug, Serialize, Deserialize)]
pub struct HitChunk {
    pub start_offset: u32,
    pub end_offset: u32,
    /// The chunk's text, cut into parts that match the query and parts that don't.
    pub parts: Vec<TextPart>,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextPart {
    pub text: String,
    pub highlight: bool,
}

/// The body of `POST /api/syncs`.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyncStart {
    /// The root to sweep at the commit: this source and everything below it.
    pub source: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SyncStarted {
    pub sync_id: Uuid,
}

/// The body of `POST /api/syncs/{id}/seen`: documents that still exist at the origin and didn't
/// change, so they aren't sent again.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyncSeen {
    /// The sync's source or one below it. External ids are only unique within a source.
    pub source: String,
    pub external_ids: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SyncSeenResponse {
    /// How many of the external ids belong to existing documents.
    pub seen: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SyncCommitted {
    /// How many documents the sweep deleted.
    pub deleted: u64,
}

/// A client for ferret's public REST API.
pub struct Client {
    http: reqwest::Client,
    base_url: String,
    api_key: Option<String>,
}

impl Client {
    /// `base_url` is the server's origin without a trailing slash, e.g. `http://localhost:8380`.
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: base_url.into(),
            api_key: None,
        }
    }

    /// Authenticates every `/api` call with a bearer credential: an API key (`fk_...`) or a
    /// plugin run's token.
    pub fn with_api_key(self, api_key: impl Into<String>) -> Self {
        Self {
            api_key: Some(api_key.into()),
            ..self
        }
    }

    /// Succeeds when the server is up and reaches its database.
    pub async fn health(&self) -> Result<(), reqwest::Error> {
        self.http
            .get(format!("{}/healthz", self.base_url))
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    /// Every user. Needs the `admin:read` scope.
    pub async fn users(&self) -> Result<Vec<User>, reqwest::Error> {
        self.api(reqwest::Method::GET, "/api/users")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
    }

    /// Creates or updates a document. Needs the `ingest:write` scope.
    pub async fn put_document(
        &self,
        document: &DocumentPut,
    ) -> Result<PutResponse, reqwest::Error> {
        self.api(reqwest::Method::PUT, "/api/documents")
            .json(document)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
    }

    /// Deletes a document and its chunks. Needs the `ingest:write` scope.
    pub async fn delete_document(
        &self,
        source: &str,
        external_id: &str,
    ) -> Result<(), reqwest::Error> {
        self.api(reqwest::Method::DELETE, "/api/documents")
            .query(&[("source", source), ("external_id", external_id)])
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    /// A whole document. Needs the `search:read` or the `ingest:read` scope.
    pub async fn document(&self, id: Uuid) -> Result<Document, reqwest::Error> {
        self.api(reqwest::Method::GET, &format!("/api/documents/{id}"))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
    }

    /// Needs the `search:read` scope.
    pub async fn search(&self, query: &SearchQuery) -> Result<SearchResponse, reqwest::Error> {
        let mut params = vec![("q", query.q.clone())];
        params.extend(
            query
                .sources
                .iter()
                .map(|source| ("source", source.clone())),
        );
        params.extend(query.from.map(|from| ("from", from.to_rfc3339())));
        params.extend(query.to.map(|to| ("to", to.to_rfc3339())));
        params.extend(query.k.map(|k| ("k", k.to_string())));
        self.api(reqwest::Method::GET, "/api/search")
            .query(&params)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
    }

    /// Starts a sync over `source` and everything below it. Needs the `ingest:write` scope.
    pub async fn start_sync(&self, source: &str) -> Result<SyncStarted, reqwest::Error> {
        self.api(reqwest::Method::POST, "/api/syncs")
            .json(&SyncStart {
                source: source.to_owned(),
            })
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
    }

    /// Reports existing, unchanged documents of `source` to the sync.
    pub async fn sync_seen(
        &self,
        sync_id: Uuid,
        source: &str,
        external_ids: &[&str],
    ) -> Result<SyncSeenResponse, reqwest::Error> {
        self.api(reqwest::Method::POST, &format!("/api/syncs/{sync_id}/seen"))
            .json(&SyncSeen {
                source: source.to_owned(),
                external_ids: external_ids.iter().map(|id| (*id).to_owned()).collect(),
            })
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
    }

    /// Commits the sync: deletes the documents below its source that weren't put or seen in it.
    pub async fn commit_sync(&self, sync_id: Uuid) -> Result<SyncCommitted, reqwest::Error> {
        self.api(
            reqwest::Method::POST,
            &format!("/api/syncs/{sync_id}/commit"),
        )
        .send()
        .await?
        .error_for_status()?
        .json()
        .await
    }

    fn api(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let request = self
            .http
            .request(method, format!("{}{path}", self.base_url));
        match &self.api_key {
            Some(api_key) => request.bearer_auth(api_key),
            None => request,
        }
    }
}
