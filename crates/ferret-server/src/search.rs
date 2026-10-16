//! Keyword search: BM25 over chunks, one hit per document.

use axum::{Json, extract::State};
use axum_extra::extract::{Query, QueryRejection};
use ferret_api::{Hit, HitChunk, SearchQuery, SearchResponse};
use sqlx::PgPool;
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

    let rows = sqlx::query!(
        r#"with excluded as (
               select distinct document_id from chunks
               where user_id = $1 and (text ||| $3 or text::pdb.alias('text_en') ||| $3)
           ),
           matches as (
               select id, document_id, text, start_offset, end_offset, pdb.score(id) as score
               from chunks c
               where user_id = $1
                 and (text &&& $2
                      or text::pdb.alias('text_en') &&& $2
                      or text::pdb.alias('text_ngram') &&& $2)
                 and ($4::text[] is null or exists (
                      select 1 from unnest($4::text[]) root where source_covers(root, c.source)))
                 and ($5::timestamptz is null or ts >= $5)
                 and ($6::timestamptz is null or ts < $6)
                 -- Authors live on documents only: rare and selective enough for a join here,
                 -- where it doesn't count against the candidate limit.
                 and (cardinality($10::text[]) = 0 or exists (
                      select 1 from documents d
                      where d.id = c.document_id and d.authors && $10::text[]))
                 and (select coalesce(bool_and(
                          strpos(lower(regexp_replace(c.text, '\s+', ' ', 'g')), lower(phrase)) > 0),
                          true)
                      from unnest($7::text[]) phrase)
               order by score desc
               limit $8
           ),
           best as (
               select distinct on (document_id) * from matches
               where document_id not in (select document_id from excluded)
               order by document_id, score desc
           )
           select d.id, d.source, d.external_id, d.content_type, d.title, d.url, d.thread,
                  d.authors, d.created_at, d.modified_at,
                  b.score as "score!", b.text as "chunk_text!",
                  b.start_offset as "start_offset!", b.end_offset as "end_offset!"
           from best b join documents d on d.id = b.document_id
           order by b.score desc
           limit $9"#,
        user_id,
        query.words.join(" "),
        query.excluded.join(" "),
        roots.as_deref(),
        request.from,
        request.to,
        &query.phrases,
        CANDIDATE_CHUNKS,
        i64::from(k),
        &request.authors,
    )
    .fetch_all(pool)
    .await?;

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

fn offset(stored: i32) -> u32 {
    u32::try_from(stored).expect("chunk offsets are never negative")
}
