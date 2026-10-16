mod common;

use common::start_server;
use sqlx::PgPool;

const LONG_ENOUGH: &str = "sixteen chars ok";

#[sqlx::test(migrations = false)]
async fn without_an_admin_a_missing_or_short_bootstrap_password_stops_the_server(pool: PgPool) {
    ferret_server::migrate(&pool).await.unwrap();

    for password in [None, Some("fifteen chars!!")] {
        let error = ferret_server::bootstrap_admin(&pool, password)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("FERRET_BOOTSTRAP_PASSWORD"));
    }
}

#[sqlx::test(migrations = false)]
async fn once_an_admin_exists_the_bootstrap_password_is_ignored(pool: PgPool) {
    ferret_server::migrate(&pool).await.unwrap();
    ferret_server::bootstrap_admin(&pool, Some(LONG_ENOUGH))
        .await
        .unwrap();

    ferret_server::bootstrap_admin(&pool, None).await.unwrap();
    ferret_server::bootstrap_admin(&pool, Some("other password, also long"))
        .await
        .unwrap();

    let server = start_server(pool).await;
    assert_eq!(server.login("admin", LONG_ENOUGH).await.status(), 204);
}

#[sqlx::test(migrations = false)]
async fn the_bootstrap_restores_a_disabled_or_demoted_admin_with_the_new_password(pool: PgPool) {
    let server = start_server(pool.clone()).await;

    for (lockout, password) in [
        ("update users set enabled = false", LONG_ENOUGH),
        ("update users set is_admin = false", "another sixteen chars"),
    ] {
        sqlx::query(lockout).execute(&pool).await.unwrap();

        ferret_server::bootstrap_admin(&pool, Some(password))
            .await
            .unwrap();

        assert_eq!(server.login("admin", password).await.status(), 204);
        let restored: bool = sqlx::query_scalar("select is_admin and enabled from users")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(restored);
    }
}
