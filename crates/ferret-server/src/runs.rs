//! The internal API for plugin workers: register, claim due runs by long polling, heartbeat,
//! finish. Plus the run tokens that let a run ingest through the public API.

use std::time::Duration;

use axum::{
    Json,
    extract::{
        FromRequestParts, Path, Query, State,
        rejection::{JsonRejection, PathRejection},
    },
    http::{StatusCode, header::AUTHORIZATION, request::Parts},
    response::{IntoResponse, Response},
};
use chrono::{DateTime, Utc};
use ferret_api::internal::{
    ClaimRequest, ClaimedRun, Heartbeat, Registered, RunFinish, RunResult, WorkerRegistration,
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use subtle::ConstantTimeEq;
use uuid::Uuid;

use crate::{
    AppState,
    api_auth::{ApiCaller, CallerKind, Scope},
    auth::{ISSUER, SessionKey},
    connections, credentials,
    error::ApiError,
};

/// A run whose last heartbeat is older than this counts as lost. Workers beat every 30 s.
const LEASE: Duration = Duration::from_secs(120);
/// Hard cap on a run token's life. Liveness is the heartbeat's job, this only bounds a leak.
const RUN_TOKEN_TTL: Duration = Duration::from_secs(24 * 60 * 60);
const MAX_WAIT: Duration = Duration::from_secs(60);
/// A worker that hasn't asked for a run for this long is offline: an idle worker asks at least
/// every claim wait (30 s), a busy one holds a running run instead.
const WORKER_OFFLINE_AFTER: Duration = Duration::from_secs(120);
/// The tail of a run's log that is kept.
const LOG_CAP_CHARS: i32 = 64 * 1024;
const RUN_TOKEN_AUDIENCE: &str = "ferret-run";
const RUN_TOKEN_TYPE: &str = "plugin_run";

/// The worker PSK, checked in constant time.
pub struct WorkerSecret(String);

impl WorkerSecret {
    pub fn new(secret: String) -> Result<Self, crate::StartupError> {
        if secret.len() < 32 {
            return Err("FERRET_WORKER_SECRET must be at least 32 bytes".into());
        }
        Ok(Self(secret))
    }
}

/// A request that carries the worker PSK. Every internal route takes one.
pub struct Worker;

impl FromRequestParts<AppState> for Worker {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        let presented = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .unwrap_or_default();
        if bool::from(presented.as_bytes().ct_eq(state.worker_secret.0.as_bytes())) {
            Ok(Worker)
        } else {
            Err(ApiError::unauthorized("missing or invalid worker secret"))
        }
    }
}

pub struct WorkerInfo {
    pub id: Uuid,
    pub name: String,
    pub plugins: Vec<String>,
    pub last_seen_at: DateTime<Utc>,
    pub online: bool,
}

/// The workers seen in the last day, online ones first, then the most recently seen. A restarted
/// worker registers anew, so its old row stays behind offline until the day is over.
pub async fn workers(pool: &PgPool) -> Result<Vec<WorkerInfo>, sqlx::Error> {
    sqlx::query_as!(
        WorkerInfo,
        r#"select id, name, plugins, last_seen_at, online as "online!" from (
               select w.*,
                      w.last_seen_at > now() - make_interval(secs => $1)
                      or exists (select 1 from plugin_runs r
                                 where r.worker_id = w.id and r.status = 'running') as online
               from workers w
           ) workers
           where online or last_seen_at > now() - interval '1 day'
           order by online desc, last_seen_at desc"#,
        WORKER_OFFLINE_AFTER.as_secs_f64(),
    )
    .fetch_all(pool)
    .await
}

/// Upserts the worker's plugin types and registers the worker with its capabilities.
pub async fn register(
    State(state): State<AppState>,
    _: Worker,
    body: Result<Json<WorkerRegistration>, JsonRejection>,
) -> Result<Json<Registered>, ApiError> {
    let Json(registration) = body?;
    let mut tx = state.pool.begin().await?;
    for plugin in &registration.plugins {
        sqlx::query!(
            "insert into plugin_types (name, config_schema, registered_at) values ($1, $2, now())
             on conflict (name) do update set config_schema = excluded.config_schema,
                                              registered_at = excluded.registered_at",
            plugin.name,
            plugin.config_schema,
        )
        .execute(&mut *tx)
        .await?;
    }
    let plugins: Vec<&str> = registration
        .plugins
        .iter()
        .map(|plugin| plugin.name.as_str())
        .collect();
    let worker_id = sqlx::query_scalar!(
        "insert into workers (id, name, plugins, last_seen_at) values (uuidv7(), $1, $2, now())
         returning id",
        registration.name,
        &plugins as &[&str],
    )
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    tracing::info!(worker = %registration.name, ?plugins, "worker registered");
    Ok(Json(Registered { worker_id }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimParams {
    /// Seconds to wait for a due run, at most a minute.
    #[serde(default)]
    wait: u64,
}

/// Hands out a due run of one of the worker's plugins, or waits for one: until the next
/// connection is due, a `run now` (NOTIFY), a lease runs out, or the wait is over (204).
pub async fn claim(
    State(state): State<AppState>,
    _: Worker,
    params: Result<Query<ClaimParams>, axum::extract::rejection::QueryRejection>,
    body: Result<Json<ClaimRequest>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Query(params) = params?;
    let Json(request) = body?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(params.wait).min(MAX_WAIT);
    let plugins = sqlx::query_scalar!(
        "update workers set last_seen_at = now() where id = $1 returning plugins",
        request.worker_id,
    )
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApiError::not_found("unknown worker, register again"))?;

    loop {
        // Registered before looking, so a NOTIFY between the look and the wait isn't missed.
        let woken = state.run_wakeup.notified();
        tokio::pin!(woken);
        woken.as_mut().enable();

        if let Some(run) = try_claim(&state, request.worker_id, &plugins).await? {
            return Ok(Json(run).into_response());
        }
        let now = tokio::time::Instant::now();
        if now >= deadline {
            return Ok(StatusCode::NO_CONTENT.into_response());
        }
        let wake_at = next_event(&state.pool, &plugins)
            .await?
            .map(|in_| now + in_.max(Duration::from_millis(50)))
            .map_or(deadline, |at| at.min(deadline));
        tokio::select! {
            () = &mut woken => {}
            () = tokio::time::sleep_until(wake_at) => {}
        }
    }
}

/// How long until a connection of `plugins` becomes due or a running run's lease runs out.
async fn next_event(pool: &PgPool, plugins: &[String]) -> Result<Option<Duration>, sqlx::Error> {
    let seconds = sqlx::query_scalar!(
        r#"select extract(epoch from min(at) - now())::float8 from (
               select c.next_run_at as at from connections c
               join users u on u.id = c.user_id
               where c.enabled and u.enabled and c.plugin = any($1)
                 and not exists (select 1 from plugin_runs r
                                 where r.connection_id = c.id and r.status = 'running')
               union all
               select r.heartbeat_at + make_interval(secs => $2) from plugin_runs r
               join connections c on c.id = r.connection_id
               where r.status = 'running' and c.plugin = any($1)
           ) events"#,
        plugins,
        LEASE.as_secs_f64(),
    )
    .fetch_one(pool)
    .await?;
    Ok(seconds.map(|seconds| Duration::from_secs_f64(seconds.max(0.0))))
}

async fn try_claim(
    state: &AppState,
    worker_id: Uuid,
    plugins: &[String],
) -> Result<Option<ClaimedRun>, ApiError> {
    let mut tx = state.pool.begin().await?;
    // An uncommitted sync deletes nothing, so giving up on a silent run is harmless.
    sqlx::query!(
        "update plugin_runs set status = 'lost', finished_at = now(),
                error = 'no heartbeat within the lease, the worker is gone'
         where status = 'running' and heartbeat_at < now() - make_interval(secs => $1)",
        LEASE.as_secs_f64(),
    )
    .execute(&mut *tx)
    .await?;
    let Some(connection) = sqlx::query!(
        r#"select c.id, c.user_id, c.plugin, c.source, c.config, c.credentials, c.schedule, c.state,
                  coalesce(
                      (select r.status = 'succeeded' from plugin_runs r
                       where r.connection_id = c.id
                       order by r.started_at desc, r.id desc limit 1),
                      true
                  ) as "state_usable!"
         from connections c
         -- A disabled user's connections don't run: they would ingest for someone who can't
         -- sign in, with credentials an admin meant to switch off.
         where c.enabled and c.plugin = any($1) and c.next_run_at <= now()
           and exists (select 1 from users u where u.id = c.user_id and u.enabled)
           and not exists (select 1 from plugin_runs r
                           where r.connection_id = c.id and r.status = 'running')
         order by c.next_run_at
         limit 1
         for update skip locked"#,
        plugins,
    )
    .fetch_optional(&mut *tx)
    .await?
    else {
        tx.commit().await?;
        return Ok(None);
    };

    let config = match &connection.credentials {
        None => Ok(connection.config),
        Some(sealed) => {
            let owner = credentials::Owner {
                user_id: connection.user_id,
                connection_id: connection.id,
            };
            (state.config_key.open(sealed, owner))
                .map(|credentials| credentials::merge(connection.config, credentials))
        }
    };
    // A run that can't get its credentials fails right here, so its connection doesn't block the
    // queue and the user sees why in the run history.
    let run_id = match &config {
        Ok(_) => sqlx::query_scalar!(
            "insert into plugin_runs (id, connection_id, worker_id, status, started_at, heartbeat_at)
             values (uuidv7(), $1, $2, 'running', now(), now())
             returning id",
            connection.id,
            worker_id,
        )
        .fetch_one(&mut *tx)
        .await?,
        Err(error) => sqlx::query_scalar!(
            "insert into plugin_runs
                 (id, connection_id, worker_id, status, started_at, heartbeat_at, finished_at, error)
             values (uuidv7(), $1, $2, 'failed', now(), now(), now(), $3)
             returning id",
            connection.id,
            worker_id,
            error.to_string(),
        )
        .fetch_one(&mut *tx)
        .await?,
    };
    match connections::next_slot(&connection.schedule, Utc::now()) {
        Ok(next_run_at) => {
            sqlx::query!(
                "update connections set next_run_at = $2 where id = $1",
                connection.id,
                next_run_at,
            )
            .execute(&mut *tx)
            .await?;
        }
        // Validated on save, so only a schedule without future slots (a date that passed) gets
        // here: this was its last run.
        Err(_) => {
            sqlx::query!(
                "update connections set enabled = false where id = $1",
                connection.id
            )
            .execute(&mut *tx)
            .await?;
        }
    }
    tx.commit().await?;
    let Ok(config) = config else {
        return Ok(None);
    };

    let token = issue_token(
        &state.session_key,
        connection.user_id,
        run_id,
        &connection.source,
    );
    tracing::info!(%run_id, connection = %connection.id, plugin = %connection.plugin, "run claimed");
    Ok(Some(ClaimedRun {
        run_id,
        connection_id: connection.id,
        plugin: connection.plugin,
        source: connection.source,
        config,
        // Failed/lost runs may have written newer documents without saving their cursor.
        state: if connection.state_usable {
            connection.state
        } else {
            serde_json::json!({})
        },
        token,
    }))
}

/// Keeps a live run alive and appends its log lines. 409 once it isn't live anymore, which tells
/// the worker to stop.
pub async fn heartbeat(
    State(state): State<AppState>,
    _: Worker,
    id: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<Heartbeat>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    let Path(id) = id?;
    let Json(heartbeat) = body?;
    let updated = sqlx::query!(
        "update plugin_runs set heartbeat_at = now(), log = right(log || $2, $3)
         where id = $1 and status = 'running'",
        id,
        log_text(&heartbeat.log_lines),
        LOG_CAP_CHARS,
    )
    .execute(&state.pool)
    .await?
    .rows_affected();
    if updated == 0 {
        return Err(ApiError::conflict("the run isn't running anymore"));
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Ends a live run. Only a successful run stores its state, so a failed one retries from the
/// old cursor.
pub async fn finish(
    State(state): State<AppState>,
    _: Worker,
    id: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<RunFinish>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    let Path(id) = id?;
    let Json(finish) = body?;
    let (status, stats, error, cursor) = match finish.result {
        RunResult::Succeeded { state, stats } => ("succeeded", Some(stats), None, Some(state)),
        RunResult::Failed { error } => ("failed", None, Some(error), None),
    };
    let mut tx = state.pool.begin().await?;
    let Some(connection_id) = sqlx::query_scalar!(
        "update plugin_runs set status = $2, finished_at = now(), stats = $3, error = $4,
                log = right(log || $5, $6)
         where id = $1 and status = 'running'
         returning connection_id",
        id,
        status,
        stats.map(|stats| serde_json::to_value(stats).expect("stats serialize")),
        error,
        log_text(&finish.log_lines),
        LOG_CAP_CHARS,
    )
    .fetch_optional(&mut *tx)
    .await?
    else {
        return Err(ApiError::conflict("the run isn't running anymore"));
    };
    if let Some(cursor) = cursor {
        sqlx::query!(
            "update connections set state = $2 where id = $1",
            connection_id,
            cursor,
        )
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

fn log_text(lines: &[String]) -> String {
    lines.iter().map(|line| format!("{line}\n")).collect()
}

#[derive(Serialize, Deserialize)]
struct RunClaims {
    iss: String,
    aud: String,
    typ: String,
    /// The connection's user.
    sub: Uuid,
    run: Uuid,
    /// The connection's source root.
    source: String,
    iat: i64,
    exp: i64,
}

fn issue_token(key: &SessionKey, user_id: Uuid, run_id: Uuid, source: &str) -> String {
    let now = Utc::now().timestamp();
    key.sign_claims(&RunClaims {
        iss: ISSUER.to_owned(),
        aud: RUN_TOKEN_AUDIENCE.to_owned(),
        typ: RUN_TOKEN_TYPE.to_owned(),
        sub: user_id,
        run: run_id,
        source: source.to_owned(),
        iat: now,
        exp: now + i64::try_from(RUN_TOKEN_TTL.as_secs()).expect("the TTL fits"),
    })
}

/// The caller behind a run token: `ingest:write` below the connection's root, as long as the run
/// is live. `None` for anything that isn't a valid run token or whose run ended.
pub async fn authenticate(
    pool: &PgPool,
    key: &SessionKey,
    token: &str,
) -> Result<Option<ApiCaller>, sqlx::Error> {
    let Some(claims) = key
        .verify_claims::<RunClaims>(token, RUN_TOKEN_AUDIENCE)
        .filter(|claims| claims.typ == RUN_TOKEN_TYPE)
    else {
        return Ok(None);
    };
    let live = sqlx::query_scalar!(
        r#"select exists (
               select 1 from plugin_runs r
               join connections c on c.id = r.connection_id
               join users u on u.id = c.user_id
               where r.id = $1 and c.user_id = $2 and c.source = $3 and r.status = 'running'
                 and r.heartbeat_at >= now() - make_interval(secs => $4)
                 -- A run in flight stops ingesting when its owner is disabled.
                 and u.enabled
           ) as "live!""#,
        claims.run,
        claims.sub,
        claims.source,
        LEASE.as_secs_f64(),
    )
    .fetch_one(pool)
    .await?;
    Ok(live.then(|| {
        ApiCaller::new(
            claims.sub,
            vec![Scope::IngestWrite],
            Some(vec![claims.source]),
            CallerKind::PluginRun(claims.run),
        )
    }))
}
