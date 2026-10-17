//! What ingests did: plugin runs and bursts of an API key's writes, counted as the writes arrive.

use chrono::{DateTime, Utc};
use ferret_api::{Writer, WriterKind};
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::api_auth::{ApiCaller, CallerKind};

/// How many documents a write put, found unchanged or deleted.
#[derive(Default)]
pub struct Counts {
    pub put: i32,
    pub unchanged: i32,
    pub deleted: i32,
}

/// Adds `counts` to the caller's live run, or to its key's current burst.
pub async fn record(
    tx: &mut PgConnection,
    caller: &ApiCaller,
    counts: Counts,
) -> Result<(), sqlx::Error> {
    match caller.kind() {
        CallerKind::PluginRun(run_id) => {
            sqlx::query!(
                "update plugin_runs
                 set put = put + $2, unchanged = unchanged + $3, deleted = deleted + $4
                 where id = $1",
                run_id,
                counts.put,
                counts.unchanged,
                counts.deleted,
            )
            .execute(&mut *tx)
            .await?;
        }
        CallerKind::ApiKey(api_key_id) => {
            // Serializes a key's writes here, so two of them can't each start a burst.
            sqlx::query!(
                "select pg_advisory_xact_lock(hashtextextended($1::text, 0))",
                api_key_id.to_string()
            )
            .execute(&mut *tx)
            .await?;
            let continued = sqlx::query!(
                "update api_ingests
                 set put = put + $2, unchanged = unchanged + $3, deleted = deleted + $4,
                     last_at = now()
                 where id = (
                     select id from api_ingests
                     where api_key_id = $1 and last_at > now() - interval '10 minutes'
                     order by last_at desc limit 1
                 )",
                api_key_id,
                counts.put,
                counts.unchanged,
                counts.deleted,
            )
            .execute(&mut *tx)
            .await?
            .rows_affected();
            if continued == 0 {
                sqlx::query!(
                    "insert into api_ingests
                         (id, user_id, api_key_id, started_at, last_at, put, unchanged, deleted)
                     values (uuidv7(), $1, $2, now(), now(), $3, $4, $5)",
                    caller.user_id,
                    api_key_id,
                    counts.put,
                    counts.unchanged,
                    counts.deleted,
                )
                .execute(&mut *tx)
                .await?;
            }
        }
    }
    Ok(())
}

/// A plugin run or an API key's burst, as the feed lists it.
pub struct Ingest {
    pub id: Uuid,
    pub kind: IngestKind,
    pub writer: Writer,
    /// `running`, `succeeded`, `failed` or `lost`. A burst is `running` while its key wrote in
    /// the last 10 minutes, then `succeeded`: it never fails or gets lost.
    pub status: String,
    pub started_at: DateTime<Utc>,
    /// A burst's last write.
    pub finished_at: Option<DateTime<Utc>>,
    pub put: i32,
    pub unchanged: i32,
    pub deleted: i32,
    pub error: Option<String>,
    /// Runs only.
    pub log: Option<String>,
    pub owner: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum IngestKind {
    Run,
    Api,
}

pub struct FeedFilter {
    /// `None`: every user's, for admins.
    pub user_id: Option<Uuid>,
    /// A connection's or API key's id.
    pub writer_id: Option<Uuid>,
    /// Only ingests that started before this, for the next page. Ingests starting at the same
    /// instant may be skipped across a page boundary, which two ingests practically never do.
    pub before: Option<DateTime<Utc>>,
    pub limit: i64,
}

/// Runs and bursts, newest first. A running run counts its writes so far, a finished one shows
/// its worker's stats.
pub async fn feed(pool: &PgPool, filter: &FeedFilter) -> Result<Vec<Ingest>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"select id as "id!", kind as "kind!", writer_id as "writer_id!", writer_name as "writer_name!",
                  writer_icon as "writer_icon!", status as "status!", started_at as "started_at!",
                  finished_at, put as "put!", unchanged as "unchanged!", deleted as "deleted!",
                  error, log, owner as "owner!"
           from (
               select r.id, 'run' as kind, c.id as writer_id, c.name as writer_name,
                      c.icon as writer_icon, r.status, r.started_at, r.finished_at,
                      coalesce((r.stats->>'put')::integer, r.put) as put,
                      coalesce((r.stats->>'unchanged')::integer, r.unchanged) as unchanged,
                      coalesce((r.stats->>'deleted')::integer, r.deleted) as deleted,
                      r.error, r.log, u.username as owner
               from plugin_runs r
               join connections c on c.id = r.connection_id
               join users u on u.id = c.user_id
               where ($1::uuid is null or c.user_id = $1)
                 and ($2::uuid is null or c.id = $2)
                 and ($3::timestamptz is null or r.started_at < $3)
               union all
               select i.id, 'api', k.id, k.name, k.icon,
                      case when i.last_at > now() - interval '10 minutes'
                           then 'running' else 'succeeded' end,
                      i.started_at, i.last_at, i.put, i.unchanged, i.deleted,
                      null, null, u.username
               from api_ingests i
               join api_keys k on k.id = i.api_key_id
               join users u on u.id = i.user_id
               where ($1::uuid is null or i.user_id = $1)
                 and ($2::uuid is null or k.id = $2)
                 and ($3::timestamptz is null or i.started_at < $3)
           ) ingests
           order by started_at desc, id desc
           limit $4"#,
        filter.user_id,
        filter.writer_id,
        filter.before,
        filter.limit,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| {
            let kind = if row.kind == "run" {
                IngestKind::Run
            } else {
                IngestKind::Api
            };
            Ingest {
                id: row.id,
                kind,
                writer: Writer {
                    kind: match kind {
                        IngestKind::Run => WriterKind::Connection,
                        IngestKind::Api => WriterKind::ApiKey,
                    },
                    id: row.writer_id,
                    name: row.writer_name,
                    icon: row.writer_icon,
                },
                status: row.status,
                started_at: row.started_at,
                finished_at: row.finished_at,
                put: row.put,
                unchanged: row.unchanged,
                deleted: row.deleted,
                error: row.error,
                log: row.log,
                owner: row.owner,
            }
        })
        .collect())
}
