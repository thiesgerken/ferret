mod common;

use common::{TestServer, start_server};
use ferret_api::Client;
use reqwest::{Method, StatusCode, header};
use serde_json::{Value, json};
use sqlx::PgPool;

const CREATE: &str = "mutation ($input: NewApiKey!) {
    createApiKey(input: $input) { key apiKey { id name prefix scopes sources lastUsedAt } }
}";

/// Signs in as the admin, creates a key with `input` and returns the response's `createApiKey`.
async fn create_key(server: &TestServer, input: Value) -> Value {
    let cookie = server.sign_in().await;
    let body = server
        .graphql(&cookie, CREATE, json!({"input": input}))
        .await;
    assert_eq!(body["errors"], Value::Null, "{body}");
    body["data"]["createApiKey"].clone()
}

async fn admin_key(server: &TestServer, scopes: &[&str]) -> String {
    let created = create_key(server, json!({"name": "admin tooling", "scopes": scopes})).await;
    created["key"].as_str().unwrap().to_owned()
}

fn client(server: &TestServer, key: &str) -> Client {
    server.api_with_key(key)
}

async fn users_status(server: &TestServer, key: &str) -> StatusCode {
    match client(server, key).users().await {
        Ok(_) => StatusCode::OK,
        Err(error) => error.status().unwrap(),
    }
}

#[sqlx::test(migrations = false)]
async fn a_new_key_is_shown_once_and_only_its_hash_is_stored(pool: PgPool) {
    let server = start_server(pool.clone()).await;

    let created = create_key(
        &server,
        json!({"name": " paperless ", "scopes": ["INGEST_WRITE"], "sources": ["paperless"]}),
    )
    .await;

    let key = created["key"].as_str().unwrap();
    let (prefix, secret) = key.strip_prefix("fk_").unwrap().split_once('.').unwrap();
    assert_eq!(prefix.len(), 8);
    assert!(prefix.chars().all(|c| c.is_ascii_hexdigit()));
    assert_eq!(secret.len(), 43, "32 bytes, base64url without padding");
    let api_key = &created["apiKey"];
    assert_eq!(api_key["name"], "paperless");
    assert_eq!(api_key["prefix"], prefix);
    assert_eq!(api_key["scopes"], json!(["INGEST_WRITE"]));
    assert_eq!(api_key["sources"], json!(["paperless"]));

    let cookie = server.sign_in().await;
    let listed = server
        .graphql(&cookie, "{ apiKeys { id name } }", json!({}))
        .await;
    assert_eq!(
        listed["data"]["apiKeys"],
        json!([{"id": api_key["id"], "name": "paperless"}])
    );
    let stored: Vec<u8> = sqlx::query_scalar("select secret_hash from api_keys")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(stored.len(), 32);
    assert!(!String::from_utf8_lossy(&stored).contains(secret));
}

#[sqlx::test(migrations = false)]
async fn a_key_authenticates_rest_calls_within_its_scopes(pool: PgPool) {
    let server = start_server(pool).await;
    let reader = admin_key(&server, &["ADMIN_READ"]).await;
    let writer = admin_key(&server, &["ADMIN_WRITE"]).await;
    let searcher = admin_key(&server, &["SEARCH_READ"]).await;

    let users = client(&server, &reader).users().await.unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].username, "admin");
    assert!(users[0].is_admin);
    // Write implies read.
    assert_eq!(users_status(&server, &writer).await, StatusCode::OK);
    assert_eq!(
        users_status(&server, &searcher).await,
        StatusCode::FORBIDDEN
    );
}

#[sqlx::test(migrations = false)]
async fn missing_malformed_wrong_revoked_and_expired_keys_are_rejected(pool: PgPool) {
    let server = start_server(pool.clone()).await;
    let key = admin_key(&server, &["ADMIN_READ"]).await;
    let (prefix, _) = key.split_once('.').unwrap();
    let missing = server
        .request(Method::GET, "/api/users")
        .send()
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        missing.json::<Value>().await.unwrap(),
        json!({"error": "missing or invalid API key"})
    );
    for bad in [
        "not a key",
        "fk_nodot",
        &format!("{prefix}.wrongsecret"),
        "fk_00000000.unknownprefix",
    ] {
        assert_eq!(
            users_status(&server, bad).await,
            StatusCode::UNAUTHORIZED,
            "{bad}"
        );
    }
    // The session cookie is no credential for /api.
    let cookie = server.sign_in().await;
    let with_cookie = server
        .request(Method::GET, "/api/users")
        .header(header::COOKIE, cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(with_cookie.status(), StatusCode::UNAUTHORIZED);

    sqlx::query("update api_keys set expires_at = now() - interval '1 second'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(users_status(&server, &key).await, StatusCode::UNAUTHORIZED);

    let other = create_key(&server, json!({"name": "other", "scopes": ["ADMIN_READ"]})).await;
    let other_key = other["key"].as_str().unwrap();
    assert_eq!(users_status(&server, other_key).await, StatusCode::OK);
    let cookie = server.sign_in().await;
    let revoked = server
        .graphql(
            &cookie,
            "mutation ($id: ID!) { revokeApiKey(id: $id) }",
            json!({"id": other["apiKey"]["id"]}),
        )
        .await;
    assert_eq!(revoked["data"]["revokeApiKey"], other["apiKey"]["id"]);
    assert_eq!(
        users_status(&server, other_key).await,
        StatusCode::UNAUTHORIZED
    );
}

#[sqlx::test(migrations = false)]
async fn a_disabled_owner_disables_their_keys(pool: PgPool) {
    let server = start_server(pool.clone()).await;
    let key = admin_key(&server, &["ADMIN_READ"]).await;

    sqlx::query("update users set enabled = false")
        .execute(&pool)
        .await
        .unwrap();

    assert_eq!(users_status(&server, &key).await, StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrations = false)]
async fn admin_scopes_need_an_admin_owner(pool: PgPool) {
    let server = start_server(pool.clone()).await;
    let key = admin_key(&server, &["ADMIN_READ", "SEARCH_READ"]).await;
    let cookie = server.sign_in().await;

    sqlx::query("update users set is_admin = false")
        .execute(&pool)
        .await
        .unwrap();

    assert_eq!(users_status(&server, &key).await, StatusCode::FORBIDDEN);
    let refused = server
        .graphql(
            &cookie,
            CREATE,
            json!({"input": {"name": "sneaky", "scopes": ["ADMIN_WRITE"]}}),
        )
        .await;
    assert_eq!(
        refused["errors"][0]["message"],
        "only admins can create keys with an admin scope"
    );
}

#[sqlx::test(migrations = false)]
async fn invalid_keys_are_refused_at_creation(pool: PgPool) {
    let server = start_server(pool).await;
    let cookie = server.sign_in().await;

    for (input, message) in [
        (
            json!({"name": " ", "scopes": ["SEARCH_READ"]}),
            "a key needs a name",
        ),
        (
            json!({"name": "k", "scopes": []}),
            "a key needs at least one scope",
        ),
        (
            json!({"name": "k", "scopes": ["SEARCH_READ"], "sources": []}),
            "leave sources out for all sources, or name at least one",
        ),
        (
            json!({"name": "k", "scopes": ["SEARCH_READ"], "sources": ["notes/"]}),
            "\"notes/\" is not a source path",
        ),
        (
            json!({"name": "k", "scopes": ["SEARCH_READ"], "expiresAt": "2020-01-01T00:00:00Z"}),
            "the expiry date must lie in the future",
        ),
    ] {
        let body = server
            .graphql(&cookie, CREATE, json!({"input": input}))
            .await;
        assert_eq!(body["errors"][0]["message"], message, "{input}");
    }
}

#[sqlx::test(migrations = false)]
async fn last_used_at_is_written_at_most_once_a_minute(pool: PgPool) {
    let server = start_server(pool.clone()).await;
    let key = admin_key(&server, &["ADMIN_READ"]).await;
    let last_used = || async {
        sqlx::query_scalar::<_, Option<chrono::DateTime<chrono::Utc>>>(
            "select last_used_at from api_keys",
        )
        .fetch_one(&pool)
        .await
        .unwrap()
    };
    assert_eq!(last_used().await, None);

    client(&server, &key).users().await.unwrap();
    let first = last_used().await.unwrap();
    client(&server, &key).users().await.unwrap();
    assert_eq!(last_used().await, Some(first));

    sqlx::query("update api_keys set last_used_at = now() - interval '2 minutes'")
        .execute(&pool)
        .await
        .unwrap();
    client(&server, &key).users().await.unwrap();
    assert!(last_used().await.unwrap() >= first);
}
