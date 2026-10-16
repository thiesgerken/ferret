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
    embeddings::{EmbedError, Embedding},
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
    let hits = find(
        &state.pool,
        &state.embedding,
        caller.user_id,
        caller.sources(),
        &query,
    )
    .await?;
    Ok(Json(SearchResponse { hits }))
}

pub enum SearchError {
    /// The query can't be run as posed, the message says why.
    Invalid(String),
    /// The query couldn't be embedded, e.g. TEI is down.
    Embedding(EmbedError),
    Database(sqlx::Error),
}

impl From<EmbedError> for SearchError {
    fn from(error: EmbedError) -> Self {
        match error {
            EmbedError::Database(error) => Self::Database(error),
            error => Self::Embedding(error),
        }
    }
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
            SearchError::Embedding(error) => {
                tracing::error!(%error, "can't embed a query");
                ApiError::bad_gateway("the embedding model is unavailable")
            }
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

    let filters = Filters {
        user_id,
        roots,
        content_types: request
            .content_types
            .iter()
            .map(|content_type| content_type::as_str(*content_type))
            .collect(),
        from: request.from,
        to: request.to,
        excluded: &query.excluded,
        phrases: &query.phrases,
    };
    let model = embedding.default_model();
    // The vector half gets the words without operators.
    let query_hash = embedding
        .cache_query(pool, model, &query.words.join(" "))
        .await?;

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
        .push(
            ") k
         ),
         semantic as (
             select id, row_number() over (order by distance) as rank
             from (
                 select e.chunk_id as id, ",
        );
    // Validated model names need no quoting (config.rs).
    let vector = format!("e.embedding::halfvec({})", model.dimension);
    sql.push(&vector)
        .push(" <=> (select embedding from embedding_cache where text_hash = ")
        .push_bind(query_hash)
        .push(format!(
            " and model = '{name}')::halfvec({dimension}) as distance
                 from chunk_embeddings e",
            name = model.config.name,
            dimension = model.dimension,
        ));
    if !query.phrases.is_empty() {
        sql.push(" join chunks c on c.id = e.chunk_id");
    }
    sql.push(format!(" where e.model = '{}' and ", model.config.name));
    filters.push(&mut sql, "e");
    sql.push(" order by distance limit ")
        .push_bind(SEMANTIC_CANDIDATES)
        .push(
            ") s
             -- No similarity at all is no hit, however few neighbours there are.
             where distance < 1
         ),
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

/// The filters both halves apply, on the denormalized chunk columns of `chunks` or
/// `chunk_embeddings` (which pgvector's iterative scans need).
struct Filters<'a> {
    user_id: Uuid,
    roots: Option<Vec<String>>,
    content_types: Vec<&'static str>,
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
