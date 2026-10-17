//! Judgments: a user's ✓ on a hit, stored with the search it answers, so model evaluation can
//! replay the search later (embeddings.md, Collecting judgments).

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use ferret_api::SearchQuery;
use serde::Serialize;
use sqlx::{PgPool, types::Json};
use uuid::Uuid;

use crate::content_type;

/// A search's filters as stored: sorted and without duplicates, so chip order doesn't make two
/// searches different. `k` isn't part of it, it limits a search without changing what it asks.
#[derive(Serialize)]
struct Filters<'a> {
    sources: Vec<&'a str>,
    authors: Vec<&'a str>,
    content_types: Vec<&'static str>,
    from: Option<DateTime<Utc>>,
    to: Option<DateTime<Utc>>,
}

impl<'a> Filters<'a> {
    fn of(search: &'a SearchQuery) -> Self {
        fn canonical<T: Ord>(mut values: Vec<T>) -> Vec<T> {
            values.sort();
            values.dedup();
            values
        }
        Self {
            sources: canonical(search.sources.iter().map(String::as_str).collect()),
            authors: canonical(search.authors.iter().map(String::as_str).collect()),
            content_types: canonical(
                search
                    .content_types
                    .iter()
                    .map(|content_type| content_type::as_str(*content_type))
                    .collect(),
            ),
            from: search.from,
            to: search.to,
        }
    }
}

/// Judges `user_id`'s document as the right hit for `search` (`judged`), or takes that back.
/// `false` when the user has no such document.
pub async fn set(
    pool: &PgPool,
    user_id: Uuid,
    search: &SearchQuery,
    document_id: Uuid,
    judged: bool,
) -> Result<bool, sqlx::Error> {
    let owned = sqlx::query_scalar!(
        r#"select exists (select 1 from documents where id = $1 and user_id = $2) as "owned!""#,
        document_id,
        user_id,
    )
    .fetch_one(pool)
    .await?;
    if !owned {
        return Ok(false);
    }
    let query = search.q.trim();
    let filters = Json(Filters::of(search));
    if judged {
        sqlx::query!(
            "insert into relevance_judgments (id, user_id, query, filters, document_id, created_at)
             values (uuidv7(), $1, $2, $3, $4, now())
             on conflict do nothing",
            user_id,
            query,
            filters as _,
            document_id,
        )
        .execute(pool)
        .await?;
    } else {
        sqlx::query!(
            "delete from relevance_judgments
             where user_id = $1 and query = $2 and filters = $3 and document_id = $4",
            user_id,
            query,
            filters as _,
            document_id,
        )
        .execute(pool)
        .await?;
    }
    Ok(true)
}

/// The documents `user_id` judged as right hits for exactly this search.
pub async fn judged_documents(
    pool: &PgPool,
    user_id: Uuid,
    search: &SearchQuery,
) -> Result<HashSet<Uuid>, sqlx::Error> {
    let documents = sqlx::query_scalar!(
        "select document_id from relevance_judgments
         where user_id = $1 and query = $2 and filters = $3",
        user_id,
        search.q.trim(),
        Json(Filters::of(search)) as _,
    )
    .fetch_all(pool)
    .await?;
    Ok(documents.into_iter().collect())
}
