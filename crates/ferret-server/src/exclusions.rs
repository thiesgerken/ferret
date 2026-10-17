//! Exclusions: documents and threads a user removed from the index for good. Excluding deletes
//! them and, when a connection owns their source, records them on it, so its later PUTs for them
//! are dropped. Plugins never learn about the list.

use chrono::{DateTime, Utc};
use ferret_api::DocumentPut;
use sqlx::{PgExecutor, PgPool};
use uuid::Uuid;

pub struct Exclusion {
    pub id: Uuid,
    pub source: String,
    /// Exactly one of `external_id` and `thread` is set.
    pub external_id: Option<String>,
    pub thread: Option<String>,
    pub created_at: DateTime<Utc>,
}

pub struct Excluded {
    /// How many documents were deleted: one, or the whole thread.
    pub deleted: u64,
    /// False when no connection owns the source (an API key wrote it): the documents are
    /// deleted, but nothing stops whatever wrote them from writing them again.
    pub remembered: bool,
}

pub enum ExclusionError {
    /// Rejected input, worded for the user.
    Invalid(String),
    Database(sqlx::Error),
}

impl From<sqlx::Error> for ExclusionError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

/// Excludes `user_id`'s document `document_id`, or with `whole_thread` every document of its
/// thread. `None` if there's no such document.
pub async fn exclude(
    pool: &PgPool,
    user_id: Uuid,
    document_id: Uuid,
    whole_thread: bool,
) -> Result<Option<Excluded>, ExclusionError> {
    let mut tx = pool.begin().await?;
    let Some(document) = sqlx::query!(
        "select source, external_id, thread from documents
         where id = $1 and user_id = $2
         for update",
        document_id,
        user_id,
    )
    .fetch_optional(&mut *tx)
    .await?
    else {
        return Ok(None);
    };
    let thread = match (whole_thread, document.thread) {
        (true, None) => {
            return Err(ExclusionError::Invalid(
                "the document isn't part of a thread".to_owned(),
            ));
        }
        (true, thread) => thread,
        (false, _) => None,
    };
    let external_id = thread.is_none().then_some(document.external_id);

    // Roots don't overlap, so at most one connection owns the source.
    let connection = sqlx::query_scalar!(
        "select id from connections where user_id = $1 and source_covers(source, $2)",
        user_id,
        document.source,
    )
    .fetch_optional(&mut *tx)
    .await?;
    if let Some(connection) = connection {
        sqlx::query!(
            "insert into exclusions (id, connection_id, source, external_id, thread, created_at)
             values (uuidv7(), $1, $2, $3, $4, now())
             on conflict do nothing",
            connection,
            document.source,
            external_id,
            thread,
        )
        .execute(&mut *tx)
        .await?;
    }
    let deleted = sqlx::query!(
        "delete from documents
         where user_id = $1 and source = $2 and (external_id = $3 or thread = $4)",
        user_id,
        document.source,
        external_id,
        thread,
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();
    tx.commit().await?;
    Ok(Some(Excluded {
        deleted,
        remembered: connection.is_some(),
    }))
}

/// The connection's exclusions, newest first.
pub async fn list(pool: &PgPool, connection_id: Uuid) -> Result<Vec<Exclusion>, sqlx::Error> {
    sqlx::query_as!(
        Exclusion,
        "select id, source, external_id, thread, created_at from exclusions
         where connection_id = $1
         order by created_at desc, id desc",
        connection_id,
    )
    .fetch_all(pool)
    .await
}

/// Removes one of `user_id`'s exclusions. The connection's next run starts over from scratch
/// (its state is reset), so a plugin that only fetches what changed puts the document again.
/// False if there's no such exclusion.
pub async fn remove(pool: &PgPool, user_id: Uuid, id: Uuid) -> Result<bool, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let Some(connection) = sqlx::query_scalar!(
        "delete from exclusions e using connections c
         where e.id = $1 and c.id = e.connection_id and c.user_id = $2
         returning c.id",
        id,
        user_id,
    )
    .fetch_optional(&mut *tx)
    .await?
    else {
        return Ok(false);
    };
    sqlx::query!(
        "update connections set state = '{}' where id = $1",
        connection
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(true)
}

/// Whether `user_id` excluded `document` or its thread.
pub async fn is_excluded(
    executor: impl PgExecutor<'_>,
    user_id: Uuid,
    document: &DocumentPut,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar!(
        r#"select exists (
               select 1 from exclusions e join connections c on c.id = e.connection_id
               where c.user_id = $1 and e.source = $2
                 and (e.external_id = $3 or e.thread = $4)
           ) as "excluded!""#,
        user_id,
        document.source,
        document.external_id,
        document.thread,
    )
    .fetch_one(executor)
    .await
}
