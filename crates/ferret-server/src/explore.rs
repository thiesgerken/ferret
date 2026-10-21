//! Explore: the user's documents without a query, as a sortable table with facet counts. The
//! filters mean what search's mean: source prefix, content type, any of the authors, and
//! `created_at` from inclusive to exclusive.

use chrono::{DateTime, Utc};
use ferret_api::{ContentType, SourceCount, ThreadCount, ThreadIdentity, Writer};
use sqlx::{PgPool, Postgres, QueryBuilder};
use uuid::Uuid;

use crate::{content_type, sources, threads, writers::Writers};

pub const MAX_LIMIT: i64 = 100;
/// Authors are the one facet that grows with the data, so only the most frequent ones count,
/// plus the selected ones.
const MAX_AUTHOR_FACETS: i64 = 20;

#[derive(Default)]
pub struct ExploreFilter {
    /// Documents in these sources or below them. Empty: all sources.
    pub sources: Vec<String>,
    /// Empty: all content types.
    pub content_types: Vec<ContentType>,
    /// Documents with at least one of these authors. Empty: any author.
    pub authors: Vec<String>,
    pub threads: Vec<ThreadIdentity>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
}

#[derive(Clone, Copy)]
pub enum Order {
    CreatedAt,
    ModifiedAt,
    /// By title, the external id for documents without one, as the table shows them.
    Title,
    Source,
}

pub struct Page {
    pub order: Order,
    pub descending: bool,
    pub offset: i64,
    pub limit: i64,
}

#[derive(sqlx::FromRow)]
pub struct ExploredDocument {
    pub id: Uuid,
    pub source: String,
    pub external_id: String,
    /// As stored: `markdown`, `plain` or `email`.
    pub content_type: String,
    pub title: Option<String>,
    pub authors: Vec<String>,
    pub created_at: Option<DateTime<Utc>>,
    pub modified_at: Option<DateTime<Utc>>,
    api_key_id: Option<Uuid>,
    /// Resolved from `api_key_id` and the source once the query ran.
    #[sqlx(skip)]
    pub writer: Option<Writer>,
}

#[derive(sqlx::FromRow)]
struct SourceRow {
    value: String,
    count: i64,
    api_key_id: Option<Uuid>,
}

#[derive(sqlx::FromRow)]
pub struct FacetCount {
    pub value: String,
    pub count: i64,
}

#[derive(sqlx::FromRow)]
struct ThreadRow {
    source: String,
    thread: String,
    title: Option<String>,
    count: i64,
}

/// How many documents each facet value has. A facet's counts apply every filter except its own,
/// so they show what choosing another value of it would give.
pub struct Facets {
    pub sources: Vec<SourceCount>,
    /// The stored names: `markdown`, `plain`, `email`.
    pub content_types: Vec<FacetCount>,
    /// The most frequent ones and the selected ones.
    pub authors: Vec<FacetCount>,
    pub threads: Vec<ThreadCount>,
    /// `YYYY-MM` of `created_at` in UTC, newest first.
    pub months: Vec<FacetCount>,
}

pub struct Exploration {
    /// How many documents match, across all pages.
    pub total: i64,
    pub documents: Vec<ExploredDocument>,
    pub facets: Facets,
}

pub enum ExploreError {
    /// The request can't be answered as posed, the message says why.
    Invalid(String),
    Database(sqlx::Error),
}

impl From<sqlx::Error> for ExploreError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Facet {
    Source,
    ContentType,
    Author,
    Thread,
    Month,
}

// ponytail: every page view scans the user's filtered documents once per facet. Fine for a
// household's index; materialized facet counts (or sampling) if Explore gets slow.
pub async fn explore(
    pool: &PgPool,
    user_id: Uuid,
    filter: &ExploreFilter,
    page: &Page,
) -> Result<Exploration, ExploreError> {
    if let Some(invalid) = filter
        .sources
        .iter()
        .find(|source| !sources::is_valid(source))
    {
        return Err(ExploreError::Invalid(format!(
            "{invalid:?} is not a source path like `notes` or `mail/personal`"
        )));
    }
    threads::validate(&filter.threads).map_err(ExploreError::Invalid)?;
    if !(1..=MAX_LIMIT).contains(&page.limit) || page.offset < 0 {
        return Err(ExploreError::Invalid(format!(
            "the limit must be between 1 and {MAX_LIMIT}, the offset at least 0"
        )));
    }

    let mut total = QueryBuilder::new("select count(*) from documents d");
    push_filters(&mut total, user_id, filter, None);

    let mut documents = QueryBuilder::new(
        "select d.id, d.source, d.external_id, d.content_type, d.title, d.authors, d.created_at,
                d.modified_at, d.api_key_id
         from documents d",
    );
    push_filters(&mut documents, user_id, filter, None);
    let column = match page.order {
        Order::CreatedAt => "d.created_at",
        Order::ModifiedAt => "d.modified_at",
        Order::Title => "coalesce(d.title, d.external_id)",
        Order::Source => "d.source",
    };
    let direction = if page.descending { "desc" } else { "asc" };
    // The id breaks ties, so pages don't overlap or skip rows.
    documents
        .push(format!(
            " order by {column} {direction} nulls last, d.id {direction} offset "
        ))
        .push_bind(page.offset)
        .push(" limit ")
        .push_bind(page.limit);

    // The key of the most recently modified document that has one, see `Writers::of_source`.
    let mut by_source = QueryBuilder::new(
        "select d.source as value, count(*),
                (array_agg(d.api_key_id order by d.modified_at desc nulls last)
                     filter (where d.api_key_id is not null))[1] as api_key_id
         from documents d",
    );
    push_filters(&mut by_source, user_id, filter, Some(Facet::Source));
    by_source.push(" group by 1 order by 2 desc, 1");

    // A selected source may be a parent of the stored ones, so count it as the filter matches.
    let mut by_selected_source =
        QueryBuilder::new("select root as value, covered.count, covered.api_key_id from unnest(");
    by_selected_source.push_bind(filter.sources.clone()).push(
        "::text[]) root cross join lateral (
                 select count(*),
                        (array_agg(d.api_key_id order by d.modified_at desc nulls last)
                             filter (where d.api_key_id is not null))[1] as api_key_id
                 from documents d",
    );
    push_filters(
        &mut by_selected_source,
        user_id,
        filter,
        Some(Facet::Source),
    );
    by_selected_source.push(" and source_covers(root, d.source)) covered");

    let mut by_content_type =
        QueryBuilder::new("select d.content_type as value, count(*) from documents d");
    push_filters(
        &mut by_content_type,
        user_id,
        filter,
        Some(Facet::ContentType),
    );
    by_content_type.push(" group by 1 order by 2 desc, 1");

    let mut by_author = QueryBuilder::new(
        "select value, count from (
             select author as value, count(*),
                    row_number() over (order by count(*) desc, author) as rank
             from documents d cross join unnest(d.authors) author",
    );
    push_filters(&mut by_author, user_id, filter, Some(Facet::Author));
    by_author
        .push(" group by 1) authors where rank <= ")
        .push_bind(MAX_AUTHOR_FACETS)
        .push(" or value = any(")
        .push_bind(filter.authors.clone())
        .push("::text[]) order by count desc, value");

    let mut by_thread = QueryBuilder::new(
        "select d.source, d.thread, count(*),
                (array_agg(d.title order by d.created_at desc nulls last, d.id desc)
                     filter (where d.title is not null and d.title != ''))[1] as title
         from documents d",
    );
    push_filters(&mut by_thread, user_id, filter, Some(Facet::Thread));
    by_thread.push(" and d.thread is not null group by d.source, d.thread order by count(*) desc, d.source, d.thread");

    let mut by_month = QueryBuilder::new(
        "select to_char(d.created_at at time zone 'UTC', 'YYYY-MM') as value, count(*)
         from documents d",
    );
    push_filters(&mut by_month, user_id, filter, Some(Facet::Month));
    by_month.push(" and d.created_at is not null group by 1 order by 1 desc");

    // One after another on one connection, so a page view never holds more than one of the pool's.
    let mut connection = pool.acquire().await?;
    let total = total
        .build_query_scalar::<i64>()
        .fetch_one(&mut *connection)
        .await?;
    let mut documents: Vec<ExploredDocument> = documents
        .build_query_as()
        .fetch_all(&mut *connection)
        .await?;
    let mut sources: Vec<SourceRow> = by_source
        .build_query_as()
        .fetch_all(&mut *connection)
        .await?;
    let selected_sources: Vec<SourceRow> = by_selected_source
        .build_query_as()
        .fetch_all(&mut *connection)
        .await?;
    let content_types = by_content_type
        .build_query_as()
        .fetch_all(&mut *connection)
        .await?;
    let authors = by_author
        .build_query_as()
        .fetch_all(&mut *connection)
        .await?;
    let thread_rows: Vec<ThreadRow> = by_thread
        .build_query_as()
        .fetch_all(&mut *connection)
        .await?;
    let mut threads: Vec<ThreadCount> = thread_rows
        .into_iter()
        .map(|row| ThreadCount {
            source: row.source,
            thread: row.thread,
            title: row.title,
            count: u32::try_from(row.count).expect("a count is never negative"),
        })
        .collect();
    threads::complete_counts(
        &mut connection,
        user_id,
        None,
        &filter.threads,
        &mut threads,
    )
    .await?;
    let months = by_month
        .build_query_as()
        .fetch_all(&mut *connection)
        .await?;
    let writers = Writers::load(&mut connection, user_id).await?;
    for document in &mut documents {
        document.writer = writers.of(&document.source, document.api_key_id);
    }
    sources.retain(|source| !filter.sources.contains(&source.value));
    sources.extend(selected_sources);
    sources.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.value.cmp(&b.value)));
    let sources = sources
        .into_iter()
        .map(|source| SourceCount {
            writer: writers.of_source(&source.value, source.api_key_id),
            count: u32::try_from(source.count).expect("a count is never negative"),
            value: source.value,
        })
        .collect();
    Ok(Exploration {
        total,
        documents,
        facets: Facets {
            sources,
            content_types,
            authors,
            threads,
            months,
        },
    })
}

/// The user's documents that pass `filter`, without the filter of `except`.
fn push_filters(
    sql: &mut QueryBuilder<Postgres>,
    user_id: Uuid,
    filter: &ExploreFilter,
    except: Option<Facet>,
) {
    let applies = |facet: Facet| except != Some(facet);
    sql.push(" where d.user_id = ").push_bind(user_id);
    if applies(Facet::Source) && !filter.sources.is_empty() {
        sql.push(" and exists (select 1 from unnest(")
            .push_bind(filter.sources.clone())
            .push("::text[]) root where source_covers(root, d.source))");
    }
    if applies(Facet::ContentType) && !filter.content_types.is_empty() {
        let stored: Vec<&str> = filter
            .content_types
            .iter()
            .map(|content_type| content_type::as_str(*content_type))
            .collect();
        sql.push(" and d.content_type = any(")
            .push_bind(stored)
            .push("::text[])");
    }
    if applies(Facet::Author) && !filter.authors.is_empty() {
        sql.push(" and d.authors && ")
            .push_bind(filter.authors.clone())
            .push("::text[]");
    }
    if applies(Facet::Thread) && !filter.threads.is_empty() {
        sql.push(" and ");
        threads::push_filter(sql, &filter.threads);
    }
    if applies(Facet::Month) {
        if let Some(from) = filter.from {
            sql.push(" and d.created_at >= ").push_bind(from);
        }
        if let Some(to) = filter.to {
            sql.push(" and d.created_at < ").push_bind(to);
        }
    }
}
