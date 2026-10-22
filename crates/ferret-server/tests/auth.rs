use crate::common;

use common::{ADMIN_PASSWORD, json_body, set_cookie, start_server};
use reqwest::{Method, header};
use serde_json::json;
use sqlx::PgPool;

#[sqlx::test(migrations = false)]
async fn login_sets_the_session_cookie_and_me_returns_the_user(pool: PgPool) {
    let server = start_server(pool).await;

    let response = server.login("  Admin ", ADMIN_PASSWORD).await;

    assert_eq!(response.status(), 204);
    let attributes = set_cookie(&response)
        .split("; ")
        .skip(1)
        .collect::<Vec<_>>();
    assert_eq!(
        attributes,
        [
            "HttpOnly",
            "Secure",
            "SameSite=Lax",
            "Path=/",
            "Max-Age=1209600"
        ]
    );
    let me = json_body(server.me(&common::session_cookie(&response)).await).await;
    let me = &me["data"]["me"];
    assert_eq!(me["username"], "admin");
    assert_eq!(me["displayName"], json!(null));
    assert_eq!(me["isAdmin"], true);
    assert!(!me["id"].as_str().unwrap().is_empty());
}

#[sqlx::test(migrations = false)]
async fn unknown_users_wrong_passwords_and_disabled_users_get_the_same_answer(pool: PgPool) {
    let server = start_server(pool.clone()).await;
    let unknown_user = server.login("nobody", ADMIN_PASSWORD).await;
    let wrong_password = server.login("admin", "not the password").await;
    sqlx::query("update users set enabled = false")
        .execute(&pool)
        .await
        .unwrap();
    let disabled_user = server.login("admin", ADMIN_PASSWORD).await;

    for response in [unknown_user, wrong_password, disabled_user] {
        assert_eq!(response.status(), 401);
        assert_eq!(
            json_body(response).await,
            json!({"error": "invalid username or password"})
        );
    }
}

#[sqlx::test(migrations = false)]
async fn logout_deletes_the_session_and_clears_the_cookie(pool: PgPool) {
    let server = start_server(pool.clone()).await;
    let cookie = server.sign_in().await;

    let response = server
        .request(Method::POST, "/auth/logout")
        .header(header::COOKIE, &cookie)
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 204);
    assert_eq!(
        set_cookie(&response),
        "ferret_session=; HttpOnly; Secure; SameSite=Lax; Path=/; Max-Age=0"
    );
    let sessions: i64 = sqlx::query_scalar("select count(*) from sessions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(sessions, 0);
    assert_eq!(server.me(&cookie).await.status(), 401);
}

#[sqlx::test(migrations = false)]
async fn logout_succeeds_without_a_valid_session(pool: PgPool) {
    let server = start_server(pool).await;

    for cookie in [None, Some("ferret_session=garbage")] {
        let mut request = server.request(Method::POST, "/auth/logout");
        if let Some(cookie) = cookie {
            request = request.header(header::COOKIE, cookie);
        }
        assert_eq!(request.send().await.unwrap().status(), 204);
    }
}

#[sqlx::test(migrations = false)]
async fn sessions_end_immediately_when_their_user_is_disabled(pool: PgPool) {
    let server = start_server(pool.clone()).await;
    let cookie = server.sign_in().await;
    assert_eq!(server.me(&cookie).await.status(), 200);

    sqlx::query("update users set enabled = false")
        .execute(&pool)
        .await
        .unwrap();

    assert_eq!(server.me(&cookie).await.status(), 401);
}

/// What a password change does to the user row: it bumps `token_version`.
#[sqlx::test(migrations = false)]
async fn a_password_change_ends_all_sessions_of_the_user(pool: PgPool) {
    let server = start_server(pool.clone()).await;
    let first = server.sign_in().await;
    let second = server.sign_in().await;

    sqlx::query("update users set token_version = token_version + 1")
        .execute(&pool)
        .await
        .unwrap();

    assert_eq!(server.me(&first).await.status(), 401);
    assert_eq!(server.me(&second).await.status(), 401);
}

#[sqlx::test(migrations = false)]
async fn an_expired_session_is_rejected(pool: PgPool) {
    let server = start_server(pool.clone()).await;
    let cookie = server.sign_in().await;

    sqlx::query("update sessions set expires_at = now() - interval '1 second'")
        .execute(&pool)
        .await
        .unwrap();

    assert_eq!(server.me(&cookie).await.status(), 401);
}

#[sqlx::test(migrations = false)]
async fn graphql_without_a_valid_session_is_401(pool: PgPool) {
    let server = start_server(pool).await;

    for cookie in ["", "ferret_session=garbage", "other=1"] {
        assert_eq!(server.me(cookie).await.status(), 401);
    }
}

#[sqlx::test(migrations = false)]
async fn graphql_login_and_password_change_accept_only_json(pool: PgPool) {
    let server = start_server(pool).await;
    let cookie = server.sign_in().await;

    let graphql_as_text = server
        .request(Method::POST, "/graphql")
        .header(header::COOKIE, &cookie)
        .header(header::CONTENT_TYPE, "text/plain")
        .body(r#"{"query": "{ me { id } }"}"#)
        .send()
        .await
        .unwrap();
    let graphql_via_get = server
        .request(Method::GET, "/graphql?query=%7Bme%7Bid%7D%7D")
        .header(header::COOKIE, &cookie)
        .send()
        .await
        .unwrap();
    let login_as_form = server
        .request(Method::POST, "/auth/login")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(format!("username=admin&password={ADMIN_PASSWORD}"))
        .send()
        .await
        .unwrap();

    let password_as_form = server
        .request(Method::POST, "/auth/password")
        .header(header::COOKIE, &cookie)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(format!(
            "current_password={ADMIN_PASSWORD}&new_password=a+brand+new+password"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(graphql_via_get.status(), 405);
    for response in [graphql_as_text, login_as_form, password_as_form] {
        assert_eq!(response.status(), 415);
        assert!(json_body(response).await["error"].is_string());
    }
}

#[sqlx::test(migrations = false)]
async fn a_login_without_password_is_rejected_with_an_error_body(pool: PgPool) {
    let server = start_server(pool).await;

    let response = server
        .request(Method::POST, "/auth/login")
        .json(&json!({"username": "admin"}))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 422);
    assert!(json_body(response).await["error"].is_string());
}

#[sqlx::test(migrations = false)]
async fn users_change_their_own_password_and_only_this_session_stays(pool: PgPool) {
    let server = start_server(pool).await;
    let this = server.sign_in().await;
    let other = server.sign_in().await;
    let new_password = "a brand new password";
    let change = async |current: &str, new: &str| {
        server
            .request(Method::POST, "/auth/password")
            .header(header::COOKIE, &this)
            .json(&json!({"current_password": current, "new_password": new}))
            .send()
            .await
            .unwrap()
    };

    let wrong = change("not the password", new_password).await;
    assert_eq!(wrong.status(), 403);
    assert_eq!(
        json_body(wrong).await,
        json!({"error": "the current password is wrong"})
    );
    let short = change(ADMIN_PASSWORD, "too short").await;
    assert_eq!(short.status(), 400);
    assert_eq!(server.me(&other).await.status(), 200, "nothing changed yet");

    let changed = change(ADMIN_PASSWORD, new_password).await;
    assert_eq!(changed.status(), 204);
    let reissued = common::session_cookie(&changed);

    assert_eq!(server.me(&reissued).await.status(), 200);
    assert_eq!(
        server.me(&this).await.status(),
        401,
        "the old cookie is spent"
    );
    assert_eq!(server.me(&other).await.status(), 401);
    assert_eq!(server.login("admin", ADMIN_PASSWORD).await.status(), 401);
    assert_eq!(server.login("admin", new_password).await.status(), 204);
}

#[sqlx::test(migrations = false)]
async fn changing_a_password_needs_a_session(pool: PgPool) {
    let server = start_server(pool).await;

    let response = server
        .request(Method::POST, "/auth/password")
        .json(&json!({"current_password": ADMIN_PASSWORD, "new_password": "a brand new password"}))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 401);
}

/// Both pass the session and password checks, but only one may store its password: the other's
/// session ended with the first change.
#[sqlx::test(migrations = false)]
async fn concurrent_password_changes_dont_overwrite_each_other(pool: PgPool) {
    let server = start_server(pool).await;
    let cookie = server.sign_in().await;
    let change = |new: &'static str| {
        server
            .request(Method::POST, "/auth/password")
            .header(header::COOKIE, &cookie)
            .json(&json!({"current_password": ADMIN_PASSWORD, "new_password": new}))
            .send()
    };

    let (first, second) = tokio::join!(
        change("the first new password"),
        change("the second new password")
    );
    let (first, second) = (first.unwrap(), second.unwrap());

    let mut statuses = [first.status().as_u16(), second.status().as_u16()];
    statuses.sort_unstable();
    assert_eq!(statuses[0], 204, "{statuses:?}");
    assert!(matches!(statuses[1], 401 | 403), "{statuses:?}");
    let winner = if first.status() == 204 { first } else { second };
    assert_eq!(
        server.me(&common::session_cookie(&winner)).await.status(),
        200
    );
}
