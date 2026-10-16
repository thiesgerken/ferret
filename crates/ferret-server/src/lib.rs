use std::sync::Arc;

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
mod content_type;
mod documents;
mod error;
pub mod graphql;
mod highlight;
mod search;
mod search_query;
mod sources;
mod syncs;
mod users;

pub use auth::SessionKey;
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
    schema: Arc<graphql::Schema>,
}

impl AppState {
    pub fn new(pool: PgPool, session_key: SessionKey) -> Self {
        Self {
            pool,
            session_key: Arc::new(session_key),
            schema: Arc::new(graphql::schema()),
        }
    }
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
            axum::routing::put(documents::put).delete(documents::delete),
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
