//! What ingests did: plugin runs and bursts of an API key's writes, counted as the writes arrive.

use sqlx::PgConnection;

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
