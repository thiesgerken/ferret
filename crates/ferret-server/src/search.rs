//! Keyword search: BM25 over chunks, one hit per document.

use axum::{Json, extract::State};
use axum_extra::extract::{Query, QueryRejection};
use chrono::{DateTime, Utc};
use ferret_api::{Hit, HitChunk, SearchQuery, SearchResponse};
use sqlx::{PgPool, Postgres, QueryBuilder};
use uuid::Uuid;

use crate::{
    AppState,
    api_auth::{ApiCaller, Scope},
    content_type,
    error::ApiError,
    highlight::highlight,
    search_query, sources,
};

const DEFAULT_K: u32 = 10;
const MAX_K: u32 = 100;
// ponytail: hits are aggregated from the best chunks only. A document whose best chunk ranks
// below this many chunks can't be a hit, which matters only when k approaches it.
const CANDIDATE_CHUNKS: i64 = 1000;

pub async fn handle(
    State(state): State<AppState>,
    caller: ApiCaller,
    query: Result<Query<SearchQuery>, QueryRejection>,
) -> Result<Json<SearchResponse>, ApiError> {
    caller.require(Scope::SearchRead)?;
    let Query(query) = query?;
    let hits = find(&state.pool, caller.user_id, caller.sources(), &query).await?;
    Ok(Json(SearchResponse { hits }))
}

pub enum SearchError {
    /// The query can't be run as posed, the message says why.
    Invalid(String),
    Database(sqlx::Error),
}

impl From<sqlx::Error> for SearchError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

impl From<SearchError> for ApiError {
    fn from(error: SearchError) -> Self {
        match error {
            SearchError::Invalid(message) => ApiError::bad_request(&message),
            SearchError::Database(error) => error.into(),
        }
    }
}

/// Searches `user_id`'s documents, within the `allowed` source roots (`None`: all sources).
/// Both APIs answer searches with this.
pub async fn find(
    pool: &PgPool,
    user_id: Uuid,
    allowed: Option<&[String]>,
    request: &SearchQuery,
) -> Result<Vec<Hit>, SearchError> {
    let query = search_query::parse(&request.q);
    if query.words.is_empty() {
        return Err(SearchError::Invalid(
            "the query needs at least one word that isn't excluded".to_owned(),
        ));
    }
    let k = request.k.unwrap_or(DEFAULT_K);
    if !(1..=MAX_K).contains(&k) {
        return Err(SearchError::Invalid(format!(
            "k must be between 1 and {MAX_K}"
        )));
    }
    if let Some(invalid) = request
        .sources
        .iter()
        .find(|source| !sources::is_valid(source))
    {
        return Err(SearchError::Invalid(format!(
            "{invalid:?} is not a source path like `notes` or `mail/zoho`"
        )));
    }
    let roots = sources::narrow(&request.sources, allowed);
    if roots.as_ref().is_some_and(Vec::is_empty) {
        return Ok(vec![]);
    }

    let content_types: Vec<&str> = request
        .content_types
        .iter()
        .map(|content_type| content_type::as_str(*content_type))
        .collect();

    // Built rather than checked at compile time (ADR 0005): every word and every excluded word
    // is its own clause, and pg_search only takes constant query strings in them.
    let mut sql = QueryBuilder::<Postgres>::new(
        "with matches as (
             select id, document_id, text, start_offset, end_offset, pdb.score(id) as score
             from chunks c
             where user_id = ",
    );
    sql.push_bind(user_id);
    // Every word must match in at least one field, so a compound and an inflected word can
    // match the same chunk in different fields.
    for word in &query.words {
        sql.push(" and (text &&& ")
            .push_bind(word)
            .push(" or text::pdb.alias('text_en') &&& ")
            .push_bind(word)
            .push(" or text::pdb.alias('text_ngram') &&& ")
            .push_bind(word)
            .push(")");
    }
    // Excluded words drop whole documents, before the candidate limit.
    for word in &query.excluded {
        sql.push(" and document_id not in (select document_id from chunks where user_id = ")
            .push_bind(user_id)
            .push(" and (text &&& ")
            .push_bind(word)
            .push(" or text::pdb.alias('text_en') &&& ")
            .push_bind(word)
            .push(" or text::pdb.alias('text_ngram') &&& ")
            .push_bind(word)
            .push("))");
    }
    if let Some(roots) = roots {
        sql.push(" and exists (select 1 from unnest(")
            .push_bind(roots)
            .push("::text[]) root where source_covers(root, c.source))");
    }
    if !content_types.is_empty() {
        sql.push(" and content_type = any(")
            .push_bind(content_types)
            .push("::text[])");
    }
    // Authors live on documents only: a join, inside the candidates, so a rare author doesn't
    // lose hits to the limit.
    if !request.authors.is_empty() {
        sql.push(
            " and exists (select 1 from documents d where d.id = c.document_id and d.authors && ",
        )
        .push_bind(&request.authors)
        .push("::text[])");
    }
    if let Some(from) = request.from {
        sql.push(" and ts >= ").push_bind(from);
    }
    if let Some(to) = request.to {
        sql.push(" and ts < ").push_bind(to);
    }
    for phrase in &query.phrases {
        sql.push(" and strpos(lower(regexp_replace(text, '\\s+', ' ', 'g')), lower(")
            .push_bind(phrase)
            .push(")) > 0");
    }
    sql.push(" order by score desc limit ")
        .push_bind(CANDIDATE_CHUNKS)
        .push(
            "),
             best as (
                 select distinct on (document_id) * from matches
                 order by document_id, score desc
             )
             select d.id, d.source, d.external_id, d.content_type, d.title, d.url, d.thread,
                    d.authors, d.created_at, d.modified_at,
                    b.score, b.text as chunk_text, b.start_offset, b.end_offset
             from best b join documents d on d.id = b.document_id
             order by b.score desc
             limit ",
        )
        .push_bind(i64::from(k));
    let rows: Vec<HitRow> = sql.build_query_as().fetch_all(pool).await?;

    Ok(rows
        .into_iter()
        .map(|row| Hit {
            document_id: row.id,
            source: row.source,
            external_id: row.external_id,
            content_type: content_type::parse(&row.content_type),
            title: row.title,
            url: row.url,
            thread: row.thread,
            authors: row.authors,
            created_at: row.created_at,
            modified_at: row.modified_at,
            score: row.score,
            chunk: HitChunk {
                start_offset: offset(row.start_offset),
                end_offset: offset(row.end_offset),
                parts: highlight(&row.chunk_text, &query.words),
            },
        })
        .collect())
}

#[derive(sqlx::FromRow)]
struct HitRow {
    id: Uuid,
    source: String,
    external_id: String,
    content_type: String,
    title: Option<String>,
    url: Option<String>,
    thread: Option<String>,
    authors: Vec<String>,
    created_at: DateTime<Utc>,
    modified_at: DateTime<Utc>,
    score: f32,
    chunk_text: String,
    start_offset: i32,
    end_offset: i32,
}

fn offset(stored: i32) -> u32 {
    u32::try_from(stored).expect("chunk offsets are never negative")
}
