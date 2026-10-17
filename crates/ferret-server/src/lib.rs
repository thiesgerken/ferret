use std::{convert::Infallible, sync::Arc, time::Duration};

use tokio::sync::Notify;

use axum::{
    Router,
    extract::State,
    http::StatusCode,
    routing::{get, post},
};
use sqlx::{PgPool, migrate::MigrateError};

mod api_auth;
mod api_keys;
mod auth;
mod chunking;
pub mod config;
mod connections;
mod content_type;
mod credentials;
mod documents;
pub mod embeddings;
mod error;
mod exclusions;
pub mod graphql;
mod highlight;
mod judgments;
mod runs;
mod search;
mod search_query;
mod sources;
mod syncs;
mod tei;
mod users;

pub use auth::SessionKey;
pub use credentials::ConfigKey;
use embeddings::Embedding;
pub use runs::WorkerSecret;
pub use users::bootstrap_admin;

pub type StartupError = Box<dyn std::error::Error + Send + Sync>;

/// Applies pending migrations.
///
/// Fails on a migration that errors or broke off on an earlier start, and on one that was
/// edited or removed after it was applied, so a server never runs against a schema it doesn't
/// know.
pub async fn migrate(pool: &PgPool) -> Result<(), MigrateError> {
    sqlx::migrate!().run(pool).await
}

#[derive(Clone)]
pub struct AppState {
    pool: PgPool,
    session_key: Arc<SessionKey>,
    worker_secret: Arc<WorkerSecret>,
    config_key: Arc<ConfigKey>,
    /// Wakes waiting claims, fed by [`listen_for_due_runs`].
    run_wakeup: Arc<Notify>,
    schema: Arc<graphql::Schema>,
    embedding: Arc<Embedding>,
}

impl AppState {
    pub fn new(
        pool: PgPool,
        session_key: SessionKey,
        worker_secret: WorkerSecret,
        embedding: Arc<Embedding>,
        config_key: ConfigKey,
    ) -> Self {
        Self {
            pool,
            session_key: Arc::new(session_key),
            worker_secret: Arc::new(worker_secret),
            config_key: Arc::new(config_key),
            run_wakeup: Arc::new(Notify::new()),
            schema: Arc::new(graphql::schema()),
            embedding,
        }
    }
}

/// Wakes this replica's waiting claims whenever a connection becomes due on any replica. Runs
/// forever: when the database goes away, it reconnects until it's back.
pub async fn listen_for_due_runs(state: AppState) -> ! {
    loop {
        let Err(error) = listen(&state).await;
        tracing::error!(%error, "listening for due runs failed, reconnecting");
        tokio::time::sleep(LISTEN_RETRY).await;
    }
}

/// Between attempts to listen again. Meanwhile claims only wake at their next event or timeout.
const LISTEN_RETRY: Duration = Duration::from_secs(5);

async fn listen(state: &AppState) -> Result<Infallible, sqlx::Error> {
    let mut listener = sqlx::postgres::PgListener::connect_with(&state.pool).await?;
    listener.listen(connections::RUNS_CHANNEL).await?;
    // Notifications sent while no one listened are lost, so waiting claims look again.
    state.run_wakeup.notify_waiters();
    loop {
        listener.recv().await?;
        state.run_wakeup.notify_waiters();
    }
}

/// The internal API for plugin workers, on its own port, which the Ingress doesn't expose. Every
/// route takes the worker PSK.
pub fn internal_router(state: AppState) -> Router {
    Router::new()
        .route("/internal/workers/register", post(runs::register))
        .route("/internal/runs/claim", post(runs::claim))
        .route("/internal/runs/{id}/heartbeat", post(runs::heartbeat))
        .route("/internal/runs/{id}/finish", post(runs::finish))
        .with_state(state)
}

/// The path decides the credential: `/graphql` takes the session cookie, `/auth` exchanges a
/// password for it, `/api` takes bearer credentials (`api_auth::ApiCaller`).
pub fn router(state: AppState) -> Router {
    Router::new()
        // Outside /api, whose routes all take a bearer credential. Probes hit the pod directly.
        .route("/healthz", get(health))
        .route("/auth/login", post(auth::login))
        .route("/auth/logout", post(auth::logout))
        .route("/graphql", post(graphql::handle))
        .route("/api/users", get(users::list))
        .route(
            "/api/documents",
            axum::routing::put(documents::put)
                .delete(documents::delete)
                .get(documents::thread),
        )
        .route("/api/documents/{id}", get(documents::get))
        .route("/api/search", get(search::handle))
        .route("/api/syncs", post(syncs::start))
        .route("/api/syncs/{id}/seen", post(syncs::seen))
        .route("/api/syncs/{id}/commit", post(syncs::commit))
        .with_state(state)
}

async fn health(State(state): State<AppState>) -> StatusCode {
    match sqlx::query("select 1").execute(&state.pool).await {
        Ok(_) => StatusCode::OK,
        Err(error) => {
            tracing::warn!(%error, "health check cannot reach the database");
            StatusCode::SERVICE_UNAVAILABLE
        }
    }
}
