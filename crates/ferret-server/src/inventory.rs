//! Bounded connection inventory. Staged rows are reusable only after successful run finish.

use axum::{
    Json,
    extract::{State, rejection::JsonRejection},
};
use ferret_api::{InventoryEntry, InventoryLookup, InventoryStaged};
use uuid::Uuid;

use crate::{
    AppState,
    api_auth::{ApiCaller, CallerKind},
    error::ApiError,
};

const MAX_BATCH: usize = 512;

fn run_id(caller: &ApiCaller) -> Result<Uuid, ApiError> {
    match caller.kind() {
        CallerKind::PluginRun(id) => Ok(id),
        CallerKind::ApiKey(_) => Err(ApiError::forbidden("inventory requires a live plugin run")),
    }
}

pub async fn lookup(
    State(state): State<AppState>,
    caller: ApiCaller,
    body: Result<Json<InventoryLookup>, JsonRejection>,
) -> Result<Json<Vec<InventoryEntry>>, ApiError> {
    let run = run_id(&caller)?;
    let Json(body) = body?;
    if body.external_ids.len() > MAX_BATCH {
        return Err(ApiError::bad_request(
            "inventory batches may contain at most 512 entries",
        ));
    }
    // The stored cursor is not usable after a failed/lost run or an exclusion reset.
    let entries = sqlx::query_as!(
        InventoryEntry,
        "select i.external_id, i.fingerprint, i.indexed
         from plugin_inventory i
         join connections c on c.id = i.connection_id
         join plugin_runs current on current.connection_id = c.id
         join plugin_runs previous on previous.id = i.generation
         where current.id = $1 and current.status = 'running'
           and current.heartbeat_at >= now() - interval '120 seconds'
           and c.user_id = $2 and i.generation = $3
           and c.state ->> 'inventory_generation' = $3::uuid::text
           and previous.status = 'succeeded'
           and not exists (select 1 from plugin_runs newer
               where newer.connection_id = c.id and newer.id != current.id
                 and (newer.started_at, newer.id) > (previous.started_at, previous.id))
           and i.external_id = any($4)",
        run,
        caller.user_id,
        body.generation,
        &body.external_ids,
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(entries))
}

pub async fn stage(
    State(state): State<AppState>,
    caller: ApiCaller,
    body: Result<Json<Vec<InventoryEntry>>, JsonRejection>,
) -> Result<Json<InventoryStaged>, ApiError> {
    let run = run_id(&caller)?;
    let Json(entries) = body?;
    if entries.len() > MAX_BATCH
        || entries
            .iter()
            .any(|entry| entry.external_id.len() > 16384 || entry.fingerprint.len() > 256)
    {
        return Err(ApiError::bad_request(
            "inventory batch exceeds entry or field limits",
        ));
    }
    let ids: Vec<_> = entries
        .iter()
        .map(|entry| entry.external_id.as_str())
        .collect();
    let fingerprints: Vec<_> = entries
        .iter()
        .map(|entry| entry.fingerprint.as_str())
        .collect();
    let indexed: Vec<_> = entries.iter().map(|entry| entry.indexed).collect();
    let mut tx = state.pool.begin().await?;
    // Serialize with finish. A request authenticated before finish must not stage after it.
    let connection = sqlx::query_scalar!(
        "select r.connection_id from plugin_runs r join connections c on c.id = r.connection_id
         where r.id = $1 and r.status = 'running' and c.user_id = $2
           and r.heartbeat_at >= now() - interval '120 seconds'
         for update of r",
        run,
        caller.user_id,
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| ApiError::conflict("the run isn't running anymore"))?;
    sqlx::query!(
        "insert into plugin_inventory (connection_id, generation, external_id, fingerprint, indexed)
         select $1, $2, external_id, fingerprint, indexed
         from unnest($3::text[], $4::text[], $5::boolean[]) as entries(external_id, fingerprint, indexed)
         on conflict (connection_id, generation, external_id)
         do update set fingerprint = excluded.fingerprint, indexed = excluded.indexed",
        connection, run, &ids as &[&str], &fingerprints as &[&str], &indexed,
    ).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(InventoryStaged { generation: run }))
}
