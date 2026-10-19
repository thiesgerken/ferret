//! Hybrid search: BM25 and vector neighbours over chunks, fused with reciprocal rank fusion, one
//! hit per document. Either half can also run alone.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    ops::Range,
    sync::Arc,
    time::Instant,
};

use axum::{Json, extract::State};
use axum_extra::extract::{Query, QueryRejection};
use chrono::{DateTime, Utc};
use ferret_api::{
    FacetCount, Hit, HitChunk, SearchFacets, SearchMode, SearchQuery, SearchResponse, SourceCount,
};
use sqlx::{PgConnection, PgPool, Postgres, QueryBuilder};
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
    statistics::Statistics,
    writers::Writers,
};

const DEFAULT_K: u32 = 10;
pub const MAX_K: u32 = 100;
/// The most frequent authors in a search's facets, besides the selected ones.
const MAX_AUTHOR_FACETS: usize = 20;
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
        &state.statistics,
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
    /// A semantic search without its embedding model.
    Unavailable(String),
    Database(Arc<sqlx::Error>),
}

impl From<sqlx::Error> for SearchError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(Arc::new(error))
    }
}

impl From<SearchError> for ApiError {
    fn from(error: SearchError) -> Self {
        match error {
            SearchError::Invalid(message) => ApiError::bad_request(&message),
            SearchError::Unavailable(message) => ApiError::service_unavailable(&message),
            SearchError::Database(error) => ApiError::database(error),
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
            "{invalid:?} is not a source path like `notes` or `mail/personal`"
        )));
    }
    Ok(query)
}

/// Searches `user_id`'s documents, within the `allowed` source roots (`None`: all sources).
/// Every API answers searches with this.
pub async fn find(
    pool: &PgPool,
    embedding: &Embedding,
    statistics: &Statistics,
    user_id: Uuid,
    allowed: Option<&[String]>,
    request: &SearchQuery,
) -> Result<SearchResponse, SearchError> {
    let started = Instant::now();
    let query = validate(request)?;
    let k = request.k.unwrap_or(DEFAULT_K);
    let skip = request.offset.unwrap_or(0);
    let model = model(pool, embedding, user_id, request.model.as_deref()).await?;
    let roots = sources::narrow(&request.sources, allowed);
    if roots.as_ref().is_some_and(Vec::is_empty) {
        return Ok(SearchResponse {
            hits: vec![],
            has_more: false,
            total: 0,
            total_is_lower_bound: false,
            documents_in_scope: 0,
            facets: SearchFacets::default(),
            took_ms: millis_since(started),
            semantic_unavailable: None,
            model: model.id().to_owned(),
        });
    }
    let documents_in_scope = statistics
        .documents_in_scope(pool, user_id, roots.as_deref())
        .await
        .map_err(SearchError::Database)?;

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
    // The vector half gets the words without operators. Without it (endpoint down or broken), a
    // hybrid search still answers by keyword, and the response says so.
    let unavailable = format!("the embedding model {} is unavailable", model.id());
    let query_hash = match request.mode {
        SearchMode::Keyword => None,
        mode => match embedding
            .cache_query(pool, model, &query.words.join(" "))
            .await
        {
            Ok(query_hash) => Some(query_hash),
            Err(EmbedError::Database(error)) => return Err(error.into()),
            Err(error) => {
                tracing::warn!(model = %model.config.id, %error, "the query can't be embedded");
                if mode == SearchMode::Semantic {
                    return Err(SearchError::Unavailable(unavailable));
                }
                None
            }
        },
    };
    let semantic_unavailable =
        (request.mode == SearchMode::Hybrid && query_hash.is_none()).then_some(unavailable);

    let collapse = request.collapse.unwrap_or(true);
    let hybrid = Hybrid {
        model,
        keyword: request.mode != SearchMode::Semantic,
        query_hash: query_hash.as_deref(),
        words: &query.words,
        collapse,
    };
    // `total` and the facets come from every match rather than the page, so they stay the same on
    // every page. Each facet applies every filter except its own, so choosing another value shows
    // what it would give: one more run of the query per facet whose filter is set.
    let without_sources = Filters {
        roots: sources::narrow(&[], allowed),
        ..filters.clone()
    };
    let without_authors = Filters {
        authors: &[],
        ..filters.clone()
    };

    let mut tx = pool.begin().await?;
    // One snapshot for the page, `total` and the facets. Not `read only`: pg_search's field
    // aliases write to its type cache.
    sqlx::query!("set transaction isolation level repeatable read")
        .execute(&mut *tx)
        .await?;
    // Filtered HNSW scans keep going until they have enough rows that pass the filters, instead
    // of silently returning fewer than the limit.
    sqlx::query!(
        "select set_config('hnsw.iterative_scan', 'relaxed_order', true) as iterative_scan,
                set_config('hnsw.ef_search', $1, true) as ef_search",
        SEMANTIC_CANDIDATES.to_string(),
    )
    .fetch_one(&mut *tx)
    .await?;
    let page = skip..skip + k;
    let matches = hybrid.matches(&mut tx, &filters, page.clone()).await?;
    let by_source = if request.sources.is_empty() {
        None
    } else {
        Some(hybrid.matches(&mut tx, &without_sources, 0..0).await?)
    };
    let by_author = if request.authors.is_empty() {
        None
    } else {
        Some(hybrid.matches(&mut tx, &without_authors, 0..0).await?)
    };
    let writers = Writers::load(&mut tx, user_id).await?;
    tx.commit().await?;
    let by_source = by_source.as_deref().unwrap_or(&matches);
    let by_author = by_author.as_deref().unwrap_or(&matches);
    let facets = SearchFacets {
        sources: source_counts(by_source, &request.sources, &writers),
        authors: author_counts(by_author, collapse, &request.authors),
        are_lower_bounds: is_lower_bound(by_source) || is_lower_bound(by_author),
    };
    let total = u32::try_from(matches.iter().filter(|row| row.is_top()).count())
        .expect("at most the candidates match");
    let has_more = page.end < total;
    let total_is_lower_bound = is_lower_bound(&matches);
    let mut page_rows: Vec<MatchRow> = matches
        .into_iter()
        .filter(|row| row.chunk_text.is_some())
        .collect();
    page_rows.sort_by_key(|row| row.page_rank);
    let hits = page_rows
        .into_iter()
        .map(|row| Hit {
            writer: writers.of(&row.source, row.api_key_id),
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
                parts: highlight(
                    &row.chunk_text.expect("the page's rows carry their chunk"),
                    &query.words,
                ),
            },
        })
        .collect();
    Ok(SearchResponse {
        hits,
        has_more,
        total,
        total_is_lower_bound,
        documents_in_scope: u32::try_from(documents_in_scope).expect("a count is never negative"),
        facets,
        took_ms: millis_since(started),
        semantic_unavailable,
        model: model.id().to_owned(),
    })
}

/// The hybrid query up to its `hits`: the documents that match, best chunk first, with their
/// thread rank. A search runs it once for its hits and again for each facet. A half that doesn't
/// run contributes no chunks, so the fused ranking is the other half's.
struct Hybrid<'a> {
    model: &'a Model,
    /// Whether the keyword half runs.
    keyword: bool,
    /// `None` without the semantic half.
    query_hash: Option<&'a [u8]>,
    words: &'a [String],
    collapse: bool,
}

impl Hybrid<'_> {
    fn max_distance(&self) -> f64 {
        1.0 - self.model.config.min_similarity
    }

    /// `with … hits as (…)`. Built rather than checked at compile time (ADR 0005): every word
    /// and every excluded word is its own clause, pg_search only takes constant query strings in
    /// them, and the vector half needs the model's dimension and name as literals to use its
    /// partial index.
    fn push_hits(&self, sql: &mut QueryBuilder<Postgres>, filters: &Filters) {
        sql.push("with keyword as (");
        if self.keyword {
            self.push_keyword(sql, filters);
        } else {
            sql.push(
                "select null::uuid as id, null::bigint as rank, null::real as keyword_score,
                        null::real as similarity
                 where false",
            );
        }
        sql.push("), nearest as (");
        match self.query_hash {
            Some(query_hash) => push_nearest(sql, self.model, query_hash, filters),
            None => {
                sql.push("select null::uuid as id, null::float8 as distance where false");
            }
        }
        // The cut-off applies to the nearest candidates rather than inside the scan: a filtered HNSW
        // scan keeps searching for rows that pass, through the whole index when few do.
        let max_distance = self.max_distance();
        sql.push(
            "), semantic as (
                 select id, row_number() over (order by distance, id) as rank,
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
                        d.authors, d.created_at, d.modified_at, d.api_key_id,
                        b.score, b.keyword_score, b.similarity,
                        b.text as chunk_text, b.start_offset, b.end_offset,
                        row_number() over (thread_hits order by b.score desc, d.created_at, d.id) as thread_rank,
                        -- Unordered: with an order, count(*) would count only up to the current row.
                        count(*) over thread_hits - 1 as collapsed
                 from best b join documents d on d.id = b.document_id
                 window thread_hits as (partition by ",
        )
        // A document without a thread is a group of its own.
        .push(if self.collapse {
            "d.source, d.thread, case when d.thread is null then d.id end"
        } else {
            "d.id"
        })
        .push(
            ")
             )",
        );
    }

    /// The keyword half's candidates, ranked.
    fn push_keyword(&self, sql: &mut QueryBuilder<Postgres>, filters: &Filters) {
        // Ties rank by id, so every page of a search sees the same ranking.
        sql.push(
            "select id, row_number() over (order by score desc, id) as rank,
                    score::real as keyword_score, null::real as similarity
             from (
                 select c.id, pdb.score(c.id) as score
                 from chunks c
                 where ",
        );
        filters.push(sql, "c");
        // Every word must match in at least one field, so a compound and an inflected word can
        // match the same chunk in different fields.
        for word in self.words {
            sql.push(" and ");
            push_word_match(sql, "c.", word);
        }
        // pg_search keeps its top-N scan with the id as second key, so ties at the candidate limit
        // don't decide by chance. The vector half has no such key: HNSW is approximate anyway.
        sql.push(" order by score desc, c.id limit ")
            .push_bind(KEYWORD_CANDIDATES)
            .push(") k");
    }

    /// Whether the hits may be short of all matches, as a boolean expression over the CTEs.
    fn push_lower_bound(&self, sql: &mut QueryBuilder<Postgres>, filters: &Filters) {
        sql.push("(false");
        if self.keyword {
            // The keyword half may have left out matches when it filled its candidates.
            sql.push(" or (select count(*) from keyword) >= ")
                .push_bind(KEYWORD_CANDIDATES);
        }
        if self.query_hash.is_some() {
            // The vector half may have, when all its neighbours passed the cut-off and the scope
            // holds more. HNSW is approximate and may return fewer neighbours than asked for, so
            // their count alone doesn't tell. For the same reason, and because `relaxed_order` may
            // return them slightly out of order, `false` means no sign of more, not a guarantee.
            sql.push(" or (not exists (select 1 from nearest where distance > ")
                .push_bind(self.max_distance())
                .push(") and exists (select 1 ");
            push_embeddings_in_scope(sql, self.model, filters);
            sql.push(" offset (select count(*) from nearest) limit 1))");
        }
        sql.push(")");
    }

    /// Every match, with each thread's other members, and whether there may be more. Only the
    /// rows of `page` (by rank among the threads' best) carry their chunk text, the others are for
    /// counting.
    async fn matches(
        &self,
        tx: &mut PgConnection,
        filters: &Filters<'_>,
        page: Range<u32>,
    ) -> Result<Vec<MatchRow>, sqlx::Error> {
        let mut sql = QueryBuilder::<Postgres>::new("");
        self.push_hits(&mut sql, filters);
        sql.push(
            ", ranked as (
                 select *,
                        -- The id breaks ties, so pages neither overlap nor skip.
                        case when thread_rank = 1
                             then row_number() over (partition by thread_rank = 1 order by score desc, id)
                        end as page_rank
                 from hits
             )
             select id, source, external_id, content_type, title, url, thread, authors,
                    created_at, modified_at, api_key_id, keyword_score, similarity, collapsed,
                    page_rank,
                    case when page_rank > ",
        )
        .push_bind(i64::from(page.start))
        .push(" and page_rank <= ")
        .push_bind(i64::from(page.end))
        .push(" then chunk_text end as chunk_text, start_offset, end_offset, ");
        self.push_lower_bound(&mut sql, filters);
        sql.push(" as total_is_lower_bound from ranked");
        sql.build_query_as().fetch_all(tx).await
    }
}

#[derive(sqlx::FromRow)]
struct MatchRow {
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
    api_key_id: Option<Uuid>,
    keyword_score: Option<f32>,
    similarity: Option<f32>,
    collapsed: i64,
    /// Among the threads' best rows, `None` for a thread's other members.
    page_rank: Option<i64>,
    /// On the page's rows only.
    chunk_text: Option<String>,
    start_offset: i32,
    end_offset: i32,
    total_is_lower_bound: bool,
}

impl MatchRow {
    /// The best row of its thread, which counts as the match.
    fn is_top(&self) -> bool {
        self.page_rank.is_some()
    }
}

fn is_lower_bound(rows: &[MatchRow]) -> bool {
    // The same on every row, and no row means nothing matched at all.
    rows.first().is_some_and(|row| row.total_is_lower_bound)
}

/// Matches per source, and per selected root, which may be a parent of the stored sources. Each
/// with its writer, see `Writers::of_source`.
fn source_counts(rows: &[MatchRow], selected: &[String], writers: &Writers) -> Vec<SourceCount> {
    let tops: Vec<&MatchRow> = rows.iter().filter(|row| row.is_top()).collect();
    let mut by_source: BTreeMap<&str, Vec<&MatchRow>> = BTreeMap::new();
    for row in &tops {
        by_source.entry(&row.source).or_default().push(row);
    }
    let stored = by_source
        .into_iter()
        .filter(|(source, _)| !selected.iter().any(|root| root == source))
        .map(|(source, rows)| (source.to_owned(), rows));
    let selected = selected.iter().map(|root| {
        let covered = tops
            .iter()
            .filter(|row| sources::covers(root, &row.source))
            .copied()
            .collect();
        (root.clone(), covered)
    });
    let mut counts: Vec<SourceCount> = stored
        .chain(selected)
        .map(|(value, rows): (String, Vec<&MatchRow>)| {
            let newest_key = rows
                .iter()
                .filter(|row| row.api_key_id.is_some())
                .max_by_key(|row| row.modified_at)
                .and_then(|row| row.api_key_id);
            SourceCount {
                writer: writers.of_source(&value, newest_key),
                count: u32::try_from(rows.len()).expect("at most the candidates match"),
                value,
            }
        })
        .collect();
    counts.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.value.cmp(&b.value)));
    counts
}

/// Matches per author, counting a thread once for every author of its members: the author filter
/// keeps a thread in which one of its documents is theirs.
fn author_counts(rows: &[MatchRow], collapse: bool, selected: &[String]) -> Vec<FacetCount> {
    let mut thread_authors: HashMap<(&str, &str), HashSet<&String>> = HashMap::new();
    let mut document_authors = Vec::new();
    for row in rows {
        match (&row.thread, collapse) {
            (Some(thread), true) => thread_authors
                .entry((&row.source, thread))
                .or_default()
                .extend(&row.authors),
            _ => document_authors.extend(&row.authors),
        }
    }
    let authors = thread_authors
        .into_values()
        .flatten()
        .chain(document_authors);
    counts(authors, |rank, author| {
        rank < MAX_AUTHOR_FACETS || selected.contains(author)
    })
}

/// How often each value occurs, most frequent first, keeping those `keep` accepts by their rank
/// and value.
fn counts<'a>(
    values: impl Iterator<Item = &'a String>,
    keep: impl Fn(usize, &String) -> bool,
) -> Vec<FacetCount> {
    let mut counts: HashMap<&String, u32> = HashMap::new();
    for value in values {
        *counts.entry(value).or_default() += 1;
    }
    let mut counts: Vec<_> = counts.into_iter().collect();
    counts.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    counts
        .into_iter()
        .enumerate()
        .filter(|(rank, (value, _))| keep(*rank, value))
        .map(|(_, (value, count))| FacetCount {
            value: value.clone(),
            count,
        })
        .collect()
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
    // Validated model ids need no quoting (config.rs). The dimension and name are literals so
    // the planner can use the model's partial index.
    let dimension = model.dimension();
    sql.push(format!(
        "select e.chunk_id as id, e.embedding::halfvec({dimension})
                    <=> (select embedding from embedding_cache where text_hash = ",
    ))
    .push_bind(query_hash.to_vec())
    .push(format!(
        " and model = '{id}')::halfvec({dimension}) as distance ",
        id = model.config.id,
    ));
    push_embeddings_in_scope(sql, model, filters);
    sql.push(" order by distance limit ")
        .push_bind(SEMANTIC_CANDIDATES);
}

/// `from` and `where` for the chunks the vector half searches.
fn push_embeddings_in_scope(sql: &mut QueryBuilder<Postgres>, model: &Model, filters: &Filters) {
    sql.push("from chunk_embeddings e");
    if !filters.phrases.is_empty() {
        sql.push(" join chunks c on c.id = e.chunk_id");
    }
    // Validated model ids need no quoting (config.rs).
    sql.push(format!(" where e.model = '{}' and ", model.config.id));
    filters.push(sql, "e");
}

/// The filters both halves apply, on the denormalized chunk columns of `chunks` or
/// `chunk_embeddings` (which pgvector's iterative scans need).
#[derive(Clone)]
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

fn offset(stored: i32) -> u32 {
    u32::try_from(stored).expect("chunk offsets are never negative")
}

fn millis_since(started: Instant) -> u32 {
    u32::try_from(started.elapsed().as_millis()).expect("a search takes less than 49 days")
}
