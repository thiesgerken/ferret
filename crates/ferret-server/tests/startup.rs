mod common;

use std::process::Command;

use sqlx::{PgPool, migrate::MigrateError};

#[sqlx::test(migrations = false)]
async fn a_started_server_is_healthy_and_has_its_extensions(pool: PgPool) {
    let server = common::start_server(pool.clone()).await;

    server.api.health().await.unwrap();
    let extensions: Vec<String> = sqlx::query_scalar(
        "select extname::text from pg_extension where extname in ('pg_search', 'vector') order by 1",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(extensions, ["pg_search", "vector"]);
}

#[sqlx::test(migrations = false)]
async fn startup_fails_after_an_applied_migration_was_edited(pool: PgPool) {
    ferret_server::migrate(&pool).await.unwrap();
    sqlx::query("update _sqlx_migrations set checksum = '\\x00'")
        .execute(&pool)
        .await
        .unwrap();

    let result = ferret_server::migrate(&pool).await;

    assert!(matches!(result, Err(MigrateError::VersionMismatch(_))));
}

#[sqlx::test(migrations = false)]
async fn startup_fails_after_a_migration_broke_off(pool: PgPool) {
    ferret_server::migrate(&pool).await.unwrap();
    sqlx::query("update _sqlx_migrations set success = false")
        .execute(&pool)
        .await
        .unwrap();

    let result = ferret_server::migrate(&pool).await;

    assert!(matches!(result, Err(MigrateError::Dirty(_))));
}

#[test]
fn the_server_refuses_to_start_without_a_valid_session_secret() {
    for secret in [None, Some("shorter than 32 bytes")] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_ferret"));
        command
            .env_clear()
            .env("FERRET_DATABASE_URL", "postgres://never-connected");
        if let Some(secret) = secret {
            command.env("FERRET_SESSION_SECRET", secret);
        }

        let output = command.output().unwrap();

        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("FERRET_SESSION_SECRET"));
    }
}
