//! The harness every black-box server test uses.
//!
//! Tests take their pool from `#[sqlx::test(migrations = false)]`, which gives each test a fresh
//! database in the ParadeDB that `DATABASE_URL` points at, and leaves migrating to the server's
//! own startup.

use ferret_api::Client;
use sqlx::PgPool;
use tokio::net::TcpListener;

/// Starts the server in-process on a random port and returns a client for it.
pub async fn start_server(pool: PgPool) -> Client {
    ferret_server::migrate(&pool)
        .await
        .expect("migrations apply");
    let app = ferret_server::router(pool);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    Client::new(format!("http://{addr}"))
}
