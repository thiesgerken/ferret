mod common;

use sqlx::{PgPool, migrate::MigrateError};

#[sqlx::test(migrations = false)]
async fn a_started_server_is_healthy_and_has_its_extensions(pool: PgPool) {
    let client = common::start_server(pool.clone()).await;

    client.health().await.unwrap();
    let extensions: Vec<String> = sqlx::query_scalar(
        "select extname::text from pg_extension where extname in ('pg_search', 'vector') order by 1",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(extensions, ["pg_search", "vector"]);
}

#[sqlx::test(migrations = false)]
async fn the_server_refuses_to_start_after_an_applied_migration_was_edited(pool: PgPool) {
    let _applied = ferret_server::app(pool.clone()).await.unwrap();
    sqlx::query("update _sqlx_migrations set checksum = '\\x00'")
        .execute(&pool)
        .await
        .unwrap();

    let result = ferret_server::app(pool).await;

    assert!(matches!(result, Err(MigrateError::VersionMismatch(_))));
}
