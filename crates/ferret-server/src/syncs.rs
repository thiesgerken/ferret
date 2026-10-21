//! Syncs: an ingester that can list everything still present at its origin reports it (`PUT`
//! for changed documents, `seen` for unchanged ones), and the commit sweeps whatever below the
//! sync's source wasn't reported.

use axum::{
    Json,
    extract::{
        Path, State,
        rejection::{JsonRejection, PathRejection},
    },
};
use ferret_api::{SyncCommitted, SyncSeen, SyncSeenResponse, SyncStart, SyncStarted};
use uuid::Uuid;

use crate::{
    AppState,
    activity::{self, Counts},
    api_auth::{ApiCaller, Scope},
    connections,
    error::ApiError,
    sources,
};

pub async fn start(
    State(state): State<AppState>,
    caller: ApiCaller,
    body: Result<Json<SyncStart>, JsonRejection>,
) -> Result<Json<SyncStarted>, ApiError> {
    caller.require(Scope::IngestWrite)?;
    let Json(body) = body?;
    if !sources::is_valid(&body.source) {
        return Err(ApiError::bad_request(&format!(
            "{:?} is not a source path like `notes` or `mail/personal`",
            body.source
        )));
    }
    caller.require_source(&body.source)?;
    connections::check_api_key_may_write(&state.pool, &caller, &body.source, body.thread.is_none())
        .await?;
    let sync_id = sqlx::query_scalar!(
        "insert into syncs (id, user_id, source, thread, started_at)
         values (uuidv7(), $1, $2, $3, now())
         returning id",
        caller.user_id,
        body.source,
        body.thread,
    )
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(SyncStarted { sync_id }))
}

/// Marks existing documents as seen in the sync, without sending their content again. Unknown
/// external ids are skipped: `seen` counts the documents that exist.
pub async fn seen(
    State(state): State<AppState>,
    caller: ApiCaller,
    id: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<SyncSeen>, JsonRejection>,
) -> Result<Json<SyncSeenResponse>, ApiError> {
    caller.require(Scope::IngestWrite)?;
    let Path(id) = id?;
    let Json(body) = body?;
    let sync = open_sync(&state, &caller, id).await?;
    if sync.thread.is_some() && body.source != sync.source {
        return Err(ApiError::bad_request(
            "a thread sync only covers its exact source",
        ));
    }
    if !sources::covers(&sync.source, &body.source) {
        return Err(ApiError::bad_request(&format!(
            "{:?} is not the sync's source {:?} or below it",
            body.source, sync.source
        )));
    }
    let seen = sqlx::query!(
        "update documents set seen_at = now()
         where user_id = $1 and source = $2 and external_id = any($3)
           and ($4::text is null or thread = $4)",
        caller.user_id,
        body.source,
        &body.external_ids,
        sync.thread,
    )
    .execute(&state.pool)
    .await?
    .rows_affected();
    Ok(Json(SyncSeenResponse { seen }))
}

/// Deletes every document below the sync's source that wasn't put or seen since the sync
/// started, and closes the sync.
pub async fn commit(
    State(state): State<AppState>,
    caller: ApiCaller,
    id: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<SyncCommitted>, ApiError> {
    caller.require(Scope::IngestWrite)?;
    let Path(id) = id?;
    let mut tx = state.pool.begin().await?;
    // The row lock makes concurrent commits of one sync wait for each other; the second finds it
    // committed.
    let sync = sqlx::query!(
        "select source, thread, started_at, committed_at is not null as \"committed!\"
         from syncs where id = $1 and user_id = $2
         for update",
        id,
        caller.user_id,
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| ApiError::not_found("no such sync"))?;
    caller.require_source(&sync.source)?;
    if sync.committed {
        return Err(ApiError::conflict("the sync is already committed"));
    }
    // Checked again: a connection may have been created below the source since the start.
    connections::check_api_key_may_write(&state.pool, &caller, &sync.source, sync.thread.is_none())
        .await?;
    let deleted = sqlx::query!(
        "delete from documents
         where user_id = $1 and seen_at < $3
           and (($4::text is null and source_covers($2, source))
             or (source = $2 and thread = $4))",
        caller.user_id,
        sync.source,
        sync.started_at,
        sync.thread,
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();
    sqlx::query!("update syncs set committed_at = now() where id = $1", id)
        .execute(&mut *tx)
        .await?;
    let counts = Counts {
        deleted: i32::try_from(deleted).expect("a user holds fewer than 2^31 documents"),
        ..Counts::default()
    };
    activity::record(&mut tx, &caller, counts).await?;
    tx.commit().await?;
    Ok(Json(SyncCommitted { deleted }))
}

struct OpenSync {
    source: String,
    thread: Option<String>,
}

/// The caller's uncommitted sync `id`, within the caller's sources.
async fn open_sync(state: &AppState, caller: &ApiCaller, id: Uuid) -> Result<OpenSync, ApiError> {
    let sync = sqlx::query!(
        "select source, thread, committed_at is not null as \"committed!\"
         from syncs where id = $1 and user_id = $2",
        id,
        caller.user_id,
    )
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApiError::not_found("no such sync"))?;
    caller.require_source(&sync.source)?;
    if sync.committed {
        return Err(ApiError::conflict("the sync is already committed"));
    }
    Ok(OpenSync {
        source: sync.source,
        thread: sync.thread,
    })
}
