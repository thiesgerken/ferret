//! Hybrid search: BM25 and vector neighbours over chunks, fused with reciprocal rank fusion, one
//! hit per document.

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
    embeddings::{EmbedError, Embedding, Model},
    error::ApiError,
    highlight::highlight,
    search_query, sources,
};

const DEFAULT_K: u32 = 10;
const MAX_K: u32 = 100;
// ponytail: hits are fused from the best chunks of each half only. A document whose best chunk
// ranks below these limits in both halves can't be a hit, which matters only when k approaches
// them. The vector half is smaller, HNSW gets slower with every extra neighbour.
const KEYWORD_CANDIDATES: i64 = 1000;
const SEMANTIC_CANDIDATES: i64 = 100;

pub async fn handle(
    State(state): State<AppState>,
    caller: ApiCaller,
    query: Result<Query<SearchQuery>, QueryRejection>,
) -> Result<Json<SearchResponse>, ApiError> {
    caller.require(Scope::SearchRead)?;
    let Query(query) = query?;
    let response = find(
        &state.pool,
        &state.embedding,
        caller.user_id,
        caller.sources(),
        &query,
    )
    .await?;
    Ok(Json(response))
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
    embedding: &Embedding,
    user_id: Uuid,
    allowed: Option<&[String]>,
    request: &SearchQuery,
) -> Result<SearchResponse, SearchError> {
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
    let model = model(pool, embedding, user_id, request.model.as_deref()).await?;
    let roots = sources::narrow(&request.sources, allowed);
    if roots.as_ref().is_some_and(Vec::is_empty) {
        return Ok(SearchResponse {
            hits: vec![],
            semantic_unavailable: None,
            model: model.name().to_owned(),
        });
    }

    let filters = Filters {
        user_id,
        roots,
        content_types: request
            .content_types
            .iter()
            .map(|content_type| content_type::as_str(*content_type))
            .collect(),
        authors: &request.authors,
        from: request.from,
        to: request.to,
        excluded: &query.excluded,
        phrases: &query.phrases,
    };
    // The vector half gets the words without operators. Without it (TEI down or broken), the
    // keyword half still answers, and the response says so.
    let query_hash = match embedding
        .cache_query(pool, model, &query.words.join(" "))
        .await
    {
        Ok(query_hash) => Some(query_hash),
        Err(EmbedError::Tei(error)) => {
            tracing::warn!(model = %model.config.name, %error, "searching by keyword only, the query can't be embedded");
            None
        }
        Err(EmbedError::Database(error)) => return Err(error.into()),
    };

    // Built rather than checked at compile time (ADR 0005): every word and every excluded word
    // is its own clause, pg_search only takes constant query strings in them, and the vector
    // half needs the model's dimension and name as literals to use its partial index.
    let mut sql = QueryBuilder::<Postgres>::new(
        "with keyword as (
             select id, row_number() over (order by score desc) as rank
             from (
                 select c.id, pdb.score(c.id) as score
                 from chunks c
                 where ",
    );
    filters.push(&mut sql, "c");
    // Every word must match in at least one field, so a compound and an inflected word can
    // match the same chunk in different fields.
    for word in &query.words {
        sql.push(" and (c.text &&& ")
            .push_bind(word)
            .push(" or c.text::pdb.alias('text_en') &&& ")
            .push_bind(word)
            .push(" or c.text::pdb.alias('text_ngram') &&& ")
            .push_bind(word)
            .push(")");
    }
    sql.push(" order by score desc limit ")
        .push_bind(KEYWORD_CANDIDATES)
        .push(") k), semantic as (");
    match &query_hash {
        Some(query_hash) => push_semantic(&mut sql, model, query_hash, &filters),
        None => {
            sql.push("select null::uuid as id, null::bigint as rank where false");
        }
    }
    sql.push(
        "),
         -- Reciprocal rank fusion (k = 60): a chunk scores 1 / (60 + rank) in each half.
         fused as (
             select id, sum(1.0 / (60 + rank))::real as score
             from (select * from keyword union all select * from semantic) ranked
             group by id
         ),
         best as (
             select distinct on (c.document_id)
                    c.document_id, c.text, c.start_offset, c.end_offset, f.score
             from fused f join chunks c on c.id = f.id
             order by c.document_id, f.score desc
         )
         select d.id, d.source, d.external_id, d.content_type, d.title, d.url, d.thread,
                d.authors, d.created_at, d.modified_at,
                b.score, b.text as chunk_text, b.start_offset, b.end_offset
         from best b join documents d on d.id = b.document_id
         order by b.score desc
         limit ",
    )
    .push_bind(i64::from(k));

    let mut tx = pool.begin().await?;
    // Filtered HNSW scans keep going until they have enough rows that pass the filters, instead
    // of silently returning fewer than the limit.
    sqlx::query!(
        "select set_config('hnsw.iterative_scan', 'relaxed_order', true) as iterative_scan,
                set_config('hnsw.ef_search', $1, true) as ef_search",
        SEMANTIC_CANDIDATES.to_string(),
    )
    .fetch_one(&mut *tx)
    .await?;
    let rows: Vec<HitRow> = sql.build_query_as().fetch_all(&mut *tx).await?;
    tx.commit().await?;

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
    Ok(SearchResponse {
        hits,
        semantic_unavailable: query_hash
            .is_none()
            .then(|| format!("the embedding model {} is unavailable", model.name())),
        model: model.name().to_owned(),
    })
}

/// The model the search asks for, else the user's default, else the configured default. The
/// default is read here rather than taken from the session's user: API keys only carry the user's
/// id, and a default set in another tab must apply right away.
async fn model<'a>(
    pool: &PgPool,
    embedding: &'a Embedding,
    user_id: Uuid,
    requested: Option<&str>,
) -> Result<&'a Model, SearchError> {
    if let Some(name) = requested {
        return embedding.model(name).ok_or_else(|| {
            SearchError::Invalid(format!("no embedding model {name:?} is configured"))
        });
    }
    let default = sqlx::query_scalar!(
        "select default_embedding_model from users where id = $1",
        user_id
    )
    .fetch_one(pool)
    .await?;
    match default {
        None => Ok(embedding.default_model()),
        // Only until the next startup resets it, e.g. during a rolling deploy.
        Some(name) => embedding.model(&name).ok_or_else(|| {
            SearchError::Invalid(format!(
                "your default embedding model {name:?} is no longer configured"
            ))
        }),
    }
}

/// The vector half: the nearest chunks to the cached query embedding, ranked.
fn push_semantic(
    sql: &mut QueryBuilder<Postgres>,
    model: &Model,
    query_hash: &[u8],
    filters: &Filters,
) {
    // Validated model names need no quoting (config.rs). The dimension and name are literals so
    // the planner can use the model's partial index.
    sql.push(format!(
        "select id, row_number() over (order by distance) as rank
         from (
             select e.chunk_id as id, e.embedding::halfvec({dimension})
                    <=> (select embedding from embedding_cache where text_hash = ",
        dimension = model.dimension,
    ))
    .push_bind(query_hash.to_vec())
    .push(format!(
        " and model = '{name}')::halfvec({dimension}) as distance
             from chunk_embeddings e",
        name = model.config.name,
        dimension = model.dimension,
    ));
    if !filters.phrases.is_empty() {
        sql.push(" join chunks c on c.id = e.chunk_id");
    }
    sql.push(format!(" where e.model = '{}' and ", model.config.name));
    filters.push(sql, "e");
    sql.push(" order by distance limit ")
        .push_bind(SEMANTIC_CANDIDATES)
        .push(") s");
}

/// The filters both halves apply, on the denormalized chunk columns of `chunks` or
/// `chunk_embeddings` (which pgvector's iterative scans need).
struct Filters<'a> {
    user_id: Uuid,
    roots: Option<Vec<String>>,
    content_types: Vec<&'static str>,
    authors: &'a [String],
    from: Option<DateTime<Utc>>,
    to: Option<DateTime<Utc>>,
    excluded: &'a [String],
    /// Checked on `c.text`, so the vector half joins `chunks c` when there are phrases.
    phrases: &'a [String],
}

impl Filters<'_> {
    fn push(&self, sql: &mut QueryBuilder<Postgres>, table: &str) {
        sql.push(format!("{table}.user_id = "))
            .push_bind(self.user_id);
        // Excluded words drop whole documents, before the candidate limits.
        for word in self.excluded {
            sql.push(format!(
                " and {table}.document_id not in (select document_id from chunks where user_id = "
            ))
            .push_bind(self.user_id)
            .push(" and (text &&& ")
            .push_bind(word.clone())
            .push(" or text::pdb.alias('text_en') &&& ")
            .push_bind(word.clone())
            .push(" or text::pdb.alias('text_ngram') &&& ")
            .push_bind(word.clone())
            .push("))");
        }
        if let Some(roots) = &self.roots {
            sql.push(" and exists (select 1 from unnest(")
                .push_bind(roots.clone())
                .push(format!(
                    "::text[]) root where source_covers(root, {table}.source))"
                ));
        }
        if !self.content_types.is_empty() {
            sql.push(format!(" and {table}.content_type = any("))
                .push_bind(self.content_types.clone())
                .push("::text[])");
        }
        // Authors live on documents only: a join, inside the candidates, so a rare author doesn't
        // lose hits to the limits.
        if !self.authors.is_empty() {
            sql.push(format!(
                " and exists (select 1 from documents d where d.id = {table}.document_id and d.authors && "
            ))
            .push_bind(self.authors.to_vec())
            .push("::text[])");
        }
        if let Some(from) = self.from {
            sql.push(format!(" and {table}.ts >= ")).push_bind(from);
        }
        if let Some(to) = self.to {
            sql.push(format!(" and {table}.ts < ")).push_bind(to);
        }
        for phrase in self.phrases {
            sql.push(" and strpos(lower(regexp_replace(c.text, '\\s+', ' ', 'g')), lower(")
                .push_bind(phrase.clone())
                .push(")) > 0");
        }
    }
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
