//! Keyword search: BM25 over chunks, one hit per document.

use axum::{Json, extract::State};
use axum_extra::extract::{Query, QueryRejection};
use chrono::{DateTime, Utc};
use ferret_api::{Hit, HitChunk, SearchResponse};
use serde::Deserialize;

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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SearchParams {
    q: String,
    #[serde(default, rename = "source")]
    sources: Vec<String>,
    from: Option<DateTime<Utc>>,
    to: Option<DateTime<Utc>>,
    k: Option<u32>,
}

pub async fn search(
    State(state): State<AppState>,
    caller: ApiCaller,
    params: Result<Query<SearchParams>, QueryRejection>,
) -> Result<Json<SearchResponse>, ApiError> {
    caller.require(Scope::SearchRead)?;
    let Query(params) = params?;
    let query = search_query::parse(&params.q);
    if query.words.is_empty() {
        return Err(ApiError::bad_request(
            "the query needs at least one word that isn't excluded",
        ));
    }
    let k = params.k.unwrap_or(DEFAULT_K);
    if !(1..=MAX_K).contains(&k) {
        return Err(ApiError::bad_request(&format!(
            "k must be between 1 and {MAX_K}"
        )));
    }
    if let Some(invalid) = params
        .sources
        .iter()
        .find(|source| !sources::is_valid(source))
    {
        return Err(ApiError::bad_request(&format!(
            "{invalid:?} is not a source path like `notes` or `mail/zoho`"
        )));
    }
    let roots = sources::narrow(&params.sources, caller.sources());
    if roots.as_ref().is_some_and(Vec::is_empty) {
        return Ok(Json(SearchResponse { hits: vec![] }));
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
        caller.user_id,
        query.words.join(" "),
        query.excluded.join(" "),
        roots.as_deref(),
        params.from,
        params.to,
        &query.phrases,
        CANDIDATE_CHUNKS,
        i64::from(k),
    )
    .fetch_all(&state.pool)
    .await?;

    let hits = rows
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
        .collect();
    Ok(Json(SearchResponse { hits }))
}

fn offset(stored: i32) -> u32 {
    u32::try_from(stored).expect("chunk offsets are never negative")
}
