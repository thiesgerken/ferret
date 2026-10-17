//! Hybrid search: BM25 and vector neighbours over chunks, fused with reciprocal rank fusion, one
//! hit per document.

use axum::{Json, extract::State};
use axum_extra::extract::{Query, QueryRejection};
use std::time::Instant;

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
    search_query::{self, ParsedQuery},
    sources,
};

const DEFAULT_K: u32 = 10;
const MAX_K: u32 = 100;
// ponytail: hits are fused from the best chunks of each half only. A document whose best chunk
// ranks below these limits in both halves can't be a hit, which matters only when k approaches
// them. The vector half is smaller, HNSW gets slower with every extra neighbour. Titles are on
// every chunk, so a common title word matches all chunks of long documents and can fill the
// keyword candidates (BM25's document frequency counts those chunks, too). #21's evaluation
// shows whether that bites.
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

/// A time range up to now, as the UI offers it. Judgments store it as such, so a judged "past
/// week" search is still the same search tomorrow.
#[derive(Clone, Copy)]
pub enum RelativeRange {
    Day,
    Week,
    Month,
    Year,
}

impl RelativeRange {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Day => "day",
            Self::Week => "week",
            Self::Month => "month",
            Self::Year => "year",
        }
    }

    pub fn start(self, now: DateTime<Utc>) -> DateTime<Utc> {
        let days = match self {
            Self::Day => 1,
            Self::Week => 7,
            Self::Month => 31,
            Self::Year => 366,
        };
        now - chrono::Duration::days(days)
    }
}

/// Rejects a search `find` couldn't answer, and returns its parsed query.
pub fn validate(request: &SearchQuery) -> Result<ParsedQuery, SearchError> {
    let query = search_query::parse(&request.q);
    if query.words.is_empty() {
        return Err(SearchError::Invalid(
            "the query needs at least one word that isn't excluded".to_owned(),
        ));
    }
    if !(1..=MAX_K).contains(&request.k.unwrap_or(DEFAULT_K)) {
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
    Ok(query)
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
    let started = Instant::now();
    let query = validate(request)?;
    let k = request.k.unwrap_or(DEFAULT_K);
    let model = model(pool, embedding, user_id, request.model.as_deref()).await?;
    let roots = sources::narrow(&request.sources, allowed);
    if roots.as_ref().is_some_and(Vec::is_empty) {
        return Ok(SearchResponse {
            hits: vec![],
            total: 0,
            total_is_lower_bound: false,
            documents_in_scope: 0,
            took_ms: millis_since(started),
            semantic_unavailable: None,
            model: model.name().to_owned(),
        });
    }
    let documents_in_scope = sqlx::query_scalar!(
        r#"select count(*) as "count!" from documents d
           where d.user_id = $1
           and ($2::text[] is null
                or exists (select 1 from unnest($2::text[]) root where source_covers(root, d.source)))"#,
        user_id,
        roots.as_deref() as Option<&[String]>,
    )
    .fetch_one(pool)
    .await?;

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
             select id, row_number() over (order by score desc) as rank,
                    score::real as keyword_score, null::real as similarity
             from (
                 select c.id, pdb.score(c.id) as score
                 from chunks c
                 where ",
    );
    filters.push(&mut sql, "c");
    // Every word must match in at least one field, so a compound and an inflected word can
    // match the same chunk in different fields.
    for word in &query.words {
        sql.push(" and ");
        push_word_match(&mut sql, "c.", word);
    }
    sql.push(" order by score desc limit ")
        .push_bind(KEYWORD_CANDIDATES)
        .push(") k), nearest as (");
    match &query_hash {
        Some(query_hash) => push_nearest(&mut sql, model, query_hash, &filters),
        None => {
            sql.push("select null::uuid as id, null::float8 as distance where false");
        }
    }
    // The cut-off applies to the nearest candidates rather than inside the scan: a filtered HNSW
    // scan keeps searching for rows that pass, through the whole index when few do.
    let max_distance = 1.0 - model.config.min_similarity;
    sql.push(
        "), semantic as (
             select id, row_number() over (order by distance) as rank,
                    null::real as keyword_score, (1 - distance)::real as similarity
             from nearest where distance <= ",
    )
    .push_bind(max_distance)
    .push(
        "),
         -- Reciprocal rank fusion (k = 60): a chunk scores 1 / (60 + rank) in each half.
         fused as (
             select id, sum(1.0 / (60 + rank))::real as score,
                    max(keyword_score) as keyword_score, max(similarity) as similarity
             from (select * from keyword union all select * from semantic) ranked
             group by id
         ),
         -- The shown chunk is the best fused one, the scores are the document's best.
         best as (
             select distinct on (c.document_id)
                    c.document_id, c.text, c.start_offset, c.end_offset, f.score,
                    max(f.keyword_score) over document as keyword_score,
                    max(f.similarity) over document as similarity
             from fused f join chunks c on c.id = f.id
             window document as (partition by c.document_id)
             order by c.document_id, f.score desc
         ),
         hits as (
             select d.id, d.source, d.external_id, d.content_type, d.title, d.url, d.thread,
                    d.authors, d.created_at, d.modified_at,
                    b.score, b.keyword_score, b.similarity,
                    b.text as chunk_text, b.start_offset, b.end_offset,
                    row_number() over (thread_hits order by b.score desc, d.created_at, d.id) as thread_rank,
                    -- Unordered: with an order, count(*) would count only up to the current row.
                    count(*) over thread_hits - 1 as collapsed
             from best b join documents d on d.id = b.document_id
             window thread_hits as (partition by ",
    )
    // A document without a thread is a group of its own.
    .push(if request.collapse.unwrap_or(true) {
        "d.source, d.thread, case when d.thread is null then d.id end"
    } else {
        "d.id"
    })
    .push(
        ")
         )
         select *, count(*) over () as total,
                -- The keyword half may have left out matches when it filled its candidates, the
                -- vector half unless a neighbour fell below the cut-off. HNSW is approximate and
                -- may return fewer neighbours than asked for, so their count doesn't tell.
                (select count(*) from keyword) >= ",
    )
    .push_bind(KEYWORD_CANDIDATES)
    .push(
        " or (exists (select 1 from nearest)
                    and not exists (select 1 from nearest where distance > ",
    )
    .push_bind(max_distance)
    .push(
        ")) as total_is_lower_bound
         from hits where thread_rank = 1
         order by score desc
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

    let (total, total_is_lower_bound) = rows.first().map_or((0, false), |row| {
        (
            u32::try_from(row.total).expect("a count is never negative"),
            row.total_is_lower_bound,
        )
    });
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
            keyword_score: row.keyword_score,
            similarity: row.similarity,
            collapsed: u32::try_from(row.collapsed)
                .expect("at most KEYWORD_CANDIDATES + SEMANTIC_CANDIDATES hits collapse"),
            chunk: HitChunk {
                start_offset: offset(row.start_offset),
                end_offset: offset(row.end_offset),
                parts: highlight(&row.chunk_text, &query.words),
            },
        })
        .collect();
    Ok(SearchResponse {
        hits,
        total,
        total_is_lower_bound,
        documents_in_scope: u32::try_from(documents_in_scope).expect("a count is never negative"),
        took_ms: millis_since(started),
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

/// The vector half's candidates: the nearest chunks to the cached query embedding.
fn push_nearest(
    sql: &mut QueryBuilder<Postgres>,
    model: &Model,
    query_hash: &[u8],
    filters: &Filters,
) {
    // Validated model names need no quoting (config.rs). The dimension and name are literals so
    // the planner can use the model's partial index.
    sql.push(format!(
        "select e.chunk_id as id, e.embedding::halfvec({dimension})
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
        .push_bind(SEMANTIC_CANDIDATES);
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
            .push(" and ");
            push_word_match(sql, "", word);
            sql.push(")");
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

/// `word` matches a chunk's text or its document's title in any of the BM25 index's fields.
/// `table` qualifies the columns, e.g. `c.`.
fn push_word_match(sql: &mut QueryBuilder<Postgres>, table: &str, word: &str) {
    sql.push("(");
    for (index, field) in [
        "text",
        "text::pdb.alias('text_en')",
        "text::pdb.alias('text_ngram')",
        "title",
        "title::pdb.alias('title_en')",
        "title::pdb.alias('title_ngram')",
    ]
    .into_iter()
    .enumerate()
    {
        if index > 0 {
            sql.push(" or ");
        }
        sql.push(format!("{table}{field} &&& "))
            .push_bind(word.to_owned());
    }
    sql.push(")");
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
    keyword_score: Option<f32>,
    similarity: Option<f32>,
    collapsed: i64,
    chunk_text: String,
    start_offset: i32,
    end_offset: i32,
    total: i64,
    total_is_lower_bound: bool,
}

fn offset(stored: i32) -> u32 {
    u32::try_from(stored).expect("chunk offsets are never negative")
}

fn millis_since(started: Instant) -> u32 {
    u32::try_from(started.elapsed().as_millis()).expect("a search takes less than 49 days")
}
