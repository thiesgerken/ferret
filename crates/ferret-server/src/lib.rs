use axum::{Router, extract::State, http::StatusCode, routing::get};
use sqlx::{PgPool, migrate::MigrateError};

/// Applies pending migrations and builds the HTTP app.
///
/// Fails on a migration that errors, and on one that was edited or removed after it was
/// applied, so a server never runs against a schema it doesn't know.
pub async fn app(pool: PgPool) -> Result<Router, MigrateError> {
    sqlx::migrate!().run(&pool).await?;
    Ok(Router::new()
        .route("/api/health", get(health))
        .with_state(pool))
}

async fn health(State(pool): State<PgPool>) -> StatusCode {
    match sqlx::query("select 1").execute(&pool).await {
        Ok(_) => StatusCode::OK,
        Err(error) => {
            tracing::warn!(%error, "health check cannot reach the database");
            StatusCode::SERVICE_UNAVAILABLE
        }
    }
}
