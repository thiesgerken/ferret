//! The MCP server at `/mcp`: search, documents and sources for agents, over Streamable HTTP. It
//! takes the API keys of `/api` as bearer tokens, with their scopes and sources, and answers with
//! the code paths of the other APIs.

use std::sync::Arc;

use axum::{extract::Request, http::request::Parts, middleware::Next, response::Response};
use chrono::{DateTime, Utc};
use ferret_api::{ContentType, SearchMode, SearchQuery};
use rmcp::{
    Json, ServerHandler,
    handler::server::{router::tool::ToolRouter, tool::Extension, wrapper::Parameters},
    model::{ServerCapabilities, ServerConfig},
    tool, tool_handler, tool_router,
    transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::never::NeverSessionManager,
    },
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::{
    AppState,
    api_auth::{ApiCaller, Scope},
    documents,
    error::ApiError,
    search::{self, SearchError},
    writers::Writers,
};

const INSTRUCTIONS: &str = "ferret is the user's personal search index: notes, documents, mail, \
chats and agent memory from many origins, searchable by keyword and meaning in one place. Each \
document points back to where it lives (its `url`). Search ferret first when you need to find \
something the user wrote, received or stored, before reaching for origin-specific tools: it \
tells you what exists and where. Sources are path-shaped namespaces like `mail/zoho` or \
`notes`, and a source includes every source below it.";

/// The `/mcp` endpoint. Stateless: every request stands alone, so any replica can answer it.
pub fn service(state: AppState) -> StreamableHttpService<Ferret, NeverSessionManager> {
    let tools = Ferret::tool_router();
    let config = StreamableHttpServerConfig::default()
        .with_legacy_session_mode(false)
        .with_json_response(true)
        // Host validation guards local servers against DNS rebinding of cookie-less browser
        // requests. Every request here carries an API key, and the Host is the deployment's.
        .disable_allowed_hosts();
    StreamableHttpService::new(
        move || {
            Ok(Ferret {
                state: state.clone(),
                tool_router: tools.clone(),
            })
        },
        Arc::new(NeverSessionManager::default()),
        config,
    )
}

/// Lets only API keys with `search:read` through, and hands their caller to the tools.
pub async fn authenticate(
    caller: ApiCaller,
    mut request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    caller.require(Scope::SearchRead)?;
    request.extensions_mut().insert(caller);
    Ok(next.run(request).await)
}

#[derive(Clone)]
pub struct Ferret {
    state: AppState,
    tool_router: ToolRouter<Self>,
}

#[derive(Deserialize, JsonSchema)]
pub struct SearchArgs {
    /// Words, "exact phrases" and -excluded words. Several phrasings of the same thing in one
    /// query work well with the default hybrid mode.
    query: String,
    /// Only documents in these sources or below them, e.g. `mail` or `notes/work`. All sources
    /// the key may read when left out. `list_sources` shows them.
    #[serde(default)]
    sources: Vec<String>,
    /// `hybrid` (the default) finds documents by keyword and by meaning. `keyword` only matches
    /// the words, for names, ids and exact terms. `semantic` only matches by meaning.
    mode: Option<SearchMode>,
    /// The embedding model for matching by meaning. The user's default model when left out.
    model: Option<String>,
    /// How many hits at most, 1 to 100. 10 when left out.
    limit: Option<u32>,
    /// `next_cursor` of the previous page, for the hits after it.
    cursor: Option<String>,
}

#[derive(Serialize, JsonSchema)]
pub struct SearchHits {
    /// Best first.
    hits: Vec<SearchHit>,
    /// Matching documents in all pages. A minimum when `total_is_lower_bound`.
    total: u32,
    total_is_lower_bound: bool,
    /// Pass as `cursor` for the next page. Absent on the last page.
    next_cursor: Option<String>,
    /// Why matching by meaning didn't run, so the hits only match the words. Absent when it ran.
    semantic_unavailable: Option<String>,
}

#[derive(Serialize, JsonSchema)]
pub struct SearchHit {
    /// For `get_document`.
    document_id: Uuid,
    title: Option<String>,
    source: String,
    /// Where the document lives, e.g. a mail client or Paperless link. Absent when ferret is
    /// the only place it lives: then `get_document` has it all.
    url: Option<String>,
    /// The connection or API key that put the document into ferret.
    writer: Option<String>,
    authors: Vec<String>,
    created_at: DateTime<Utc>,
    modified_at: DateTime<Utc>,
    /// The thread (mail conversation, chat) the document belongs to.
    thread: Option<String>,
    /// More documents of the same thread that matched, folded into this hit.
    collapsed: u32,
    /// The document's chunk that matches the query best.
    chunk: String,
    /// BM25 score, absent when the words don't match.
    keyword_score: Option<f32>,
    /// Cosine similarity to the query, absent when the meaning doesn't match.
    similarity: Option<f32>,
}

#[derive(Deserialize, JsonSchema)]
pub struct DocumentArgs {
    /// A hit's `document_id`.
    id: Uuid,
}

#[derive(Serialize, JsonSchema)]
pub struct FullDocument {
    id: Uuid,
    title: Option<String>,
    source: String,
    /// The id the origin knows the document by.
    external_id: String,
    content_type: ContentType,
    /// Where the document lives. Absent when ferret is the only place it lives.
    url: Option<String>,
    /// The connection or API key that put the document into ferret.
    writer: Option<String>,
    authors: Vec<String>,
    created_at: DateTime<Utc>,
    modified_at: DateTime<Utc>,
    thread: Option<String>,
    /// The whole text.
    text: String,
    /// Whatever else the origin knows, e.g. a mail's recipients.
    meta: Map<String, Value>,
}

#[derive(Serialize, JsonSchema)]
pub struct Sources {
    /// Most documents first.
    sources: Vec<SourceSummary>,
}

#[derive(Serialize, JsonSchema)]
pub struct SourceSummary {
    source: String,
    documents: u32,
    /// The connection or API key that puts its documents into ferret, absent when unknown.
    writer: Option<String>,
}

#[tool_router]
impl Ferret {
    #[tool(
        description = "Search the user's personal index (notes, documents, mail, chats, agent \
        memory) by keyword and meaning. Use it first, before origin-specific tools, to find what \
        exists and where it lives. Hits come with their best-matching chunk; use get_document \
        for a hit's whole text.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn search(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<SearchArgs>,
    ) -> Result<Json<SearchHits>, String> {
        let caller = caller(&parts);
        let offset = args
            .cursor
            .map(|cursor| {
                cursor
                    .parse::<u32>()
                    .map_err(|_| format!("{cursor:?} isn't a cursor from a previous page"))
            })
            .transpose()?;
        let request = SearchQuery {
            q: args.query,
            sources: args.sources,
            k: args.limit,
            offset,
            model: args.model,
            mode: args.mode.unwrap_or_default(),
            ..SearchQuery::default()
        };
        let found = search::find(
            &self.state.pool,
            &self.state.embedding,
            caller.user_id,
            caller.sources(),
            &request,
        )
        .await
        .map_err(|error| match error {
            SearchError::Invalid(message) | SearchError::Unavailable(message) => message,
            SearchError::Database(error) => internal(error),
        })?;
        let next_cursor = found
            .has_more
            .then(|| (offset.unwrap_or(0) + found.hits.len() as u32).to_string());
        let hits = found
            .hits
            .into_iter()
            .map(|hit| SearchHit {
                document_id: hit.document_id,
                title: hit.title,
                source: hit.source,
                url: hit.url,
                writer: hit.writer.map(|writer| writer.name),
                authors: hit.authors,
                created_at: hit.created_at,
                modified_at: hit.modified_at,
                thread: hit.thread,
                collapsed: hit.collapsed,
                chunk: hit.chunk.parts.into_iter().map(|part| part.text).collect(),
                keyword_score: hit.keyword_score,
                similarity: hit.similarity,
            })
            .collect();
        Ok(Json(SearchHits {
            hits,
            total: found.total,
            total_is_lower_bound: found.total_is_lower_bound,
            next_cursor,
            semantic_unavailable: found.semantic_unavailable,
        }))
    }

    #[tool(
        description = "Read one document of the user's index in full: its whole text and \
        metadata, and the url of where it lives. Takes a search hit's document_id.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_document(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<DocumentArgs>,
    ) -> Result<Json<FullDocument>, String> {
        let caller = caller(&parts);
        let document = documents::find(&self.state.pool, caller.user_id, caller.sources(), args.id)
            .await
            .map_err(internal)?
            .ok_or_else(|| format!("there's no document {} for this key", args.id))?;
        Ok(Json(FullDocument {
            id: document.id,
            title: document.title,
            source: document.source,
            external_id: document.external_id,
            content_type: document.content_type,
            url: document.url,
            writer: document.writer.map(|writer| writer.name),
            authors: document.authors,
            created_at: document.created_at,
            modified_at: document.modified_at,
            thread: document.thread,
            text: document.text,
            meta: document.meta,
        }))
    }

    #[tool(
        description = "List the sources of the user's index this key may read, with how many \
        documents each holds and what puts them there. Shows what ferret knows about before \
        searching, and which sources to narrow a search to.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn list_sources(
        &self,
        Extension(parts): Extension<Parts>,
    ) -> Result<Json<Sources>, String> {
        let caller = caller(&parts);
        let sources = list_sources(&self.state.pool, &caller)
            .await
            .map_err(internal)?;
        Ok(Json(Sources { sources }))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for Ferret {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(INSTRUCTIONS)
    }
}

/// `authenticate` put it there, every request to `/mcp` passes it.
fn caller(parts: &Parts) -> ApiCaller {
    parts
        .extensions
        .get::<ApiCaller>()
        .expect("/mcp authenticates every request")
        .clone()
}

fn internal(error: sqlx::Error) -> String {
    tracing::error!(%error, "database error");
    "internal error".to_owned()
}

/// The caller's sources with their document counts and writers, most documents first.
async fn list_sources(
    pool: &sqlx::PgPool,
    caller: &ApiCaller,
) -> Result<Vec<SourceSummary>, sqlx::Error> {
    let mut connection = pool.acquire().await?;
    // The key of the most recently modified document that has one, see `Writers::of_source`.
    let rows = sqlx::query!(
        r#"select source, count(*) as "documents!",
                  (array_agg(api_key_id order by modified_at desc)
                       filter (where api_key_id is not null))[1] as api_key_id
           from documents
           where user_id = $1
             and ($2::text[] is null or exists (
                  select 1 from unnest($2::text[]) root where source_covers(root, source)))
           group by source
           order by 2 desc, 1"#,
        caller.user_id,
        caller.sources(),
    )
    .fetch_all(&mut *connection)
    .await?;
    let writers = Writers::load(&mut connection, caller.user_id).await?;
    Ok(rows
        .into_iter()
        .map(|row| SourceSummary {
            writer: writers
                .of_source(&row.source, row.api_key_id)
                .map(|writer| writer.name),
            documents: u32::try_from(row.documents).expect("a count is never negative"),
            source: row.source,
        })
        .collect())
}
