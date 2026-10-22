use crate::common;

use std::time::Duration;

use common::{TestServer, document, session_cookie, start_server};
use ferret_api::internal::{PluginRegistration, WorkerRegistration};
use reqwest::StatusCode;
use serde_json::{Value, json};
use sqlx::PgPool;

const PASSWORD: &str = "a long enough password";

async fn graphql_ok(server: &TestServer, cookie: &str, query: &str, variables: Value) -> Value {
    let body = server.graphql(cookie, query, variables).await;
    assert_eq!(body["errors"], Value::Null, "{body}");
    body["data"].clone()
}

async fn create_user(server: &TestServer, cookie: &str, username: &str, is_admin: bool) -> Value {
    server
        .graphql(
            cookie,
            "mutation ($input: NewUser!) {
                createUser(input: $input) { id username displayName isAdmin enabled }
            }",
            json!({"input": {
                "username": username,
                "displayName": " Grace ",
                "password": PASSWORD,
                "isAdmin": is_admin,
            }}),
        )
        .await
}

async fn sign_in_as(server: &TestServer, username: &str, password: &str) -> String {
    let response = server.login(username, password).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT, "{username}");
    session_cookie(&response)
}

async fn update_user(
    server: &TestServer,
    cookie: &str,
    id: &Value,
    is_admin: bool,
    enabled: bool,
) -> Value {
    server
        .graphql(
            cookie,
            "mutation ($id: ID!, $input: UserUpdate!) {
                updateUser(id: $id, input: $input) { isAdmin enabled }
            }",
            json!({"id": id, "input": {"displayName": null, "isAdmin": is_admin, "enabled": enabled}}),
        )
        .await
}

#[sqlx::test(migrations = false)]
async fn admins_create_users_who_can_then_sign_in(pool: PgPool) {
    let server = start_server(pool).await;
    let admin = server.sign_in().await;

    let created = create_user(&server, &admin, " Grace.H ", false).await;
    let taken = create_user(&server, &admin, "grace.h", false).await;
    let bad_name = create_user(&server, &admin, "grace hopper", false).await;
    let short = server
        .graphql(
            &admin,
            "mutation { createUser(input: {username: \"ada\", password: \"short\", isAdmin: false}) { id } }",
            json!({}),
        )
        .await;

    assert_eq!(created["errors"], Value::Null, "{created}");
    let user = &created["data"]["createUser"];
    assert_eq!(
        (
            &user["username"],
            &user["displayName"],
            &user["isAdmin"],
            &user["enabled"]
        ),
        (
            &json!("grace.h"),
            &json!("Grace"),
            &json!(false),
            &json!(true)
        )
    );
    assert_eq!(
        taken["errors"][0]["message"],
        "the username \"grace.h\" is taken"
    );
    assert_eq!(bad_name["errors"][0]["extensions"]["code"], "INVALID_INPUT");
    assert_eq!(
        short["errors"][0]["message"],
        "a password needs at least 12 characters"
    );
    sign_in_as(&server, "grace.h", PASSWORD).await;
    let listed = graphql_ok(&server, &admin, "{ users { username } }", json!({})).await;
    assert_eq!(
        listed["users"],
        json!([{"username": "admin"}, {"username": "grace.h"}])
    );
}

#[sqlx::test(migrations = false)]
async fn only_admins_see_and_change_users_workers_and_models(pool: PgPool) {
    let server = start_server(pool).await;
    let admin = server.sign_in().await;
    create_user(&server, &admin, "grace", false).await;
    let grace = sign_in_as(&server, "grace", PASSWORD).await;

    for query in [
        "{ users { id } }",
        "{ workers { id } }",
        "{ embeddingStatus { id } }",
        "mutation { createUser(input: {username: \"mallory\", password: \"a long enough password\", isAdmin: true}) { id } }",
    ] {
        let body = server.graphql(&grace, query, json!({})).await;
        assert_eq!(body["errors"][0]["message"], "admins only", "{query}");
        assert_eq!(
            body["errors"][0]["extensions"]["code"], "FORBIDDEN",
            "{query}"
        );
    }
}

#[sqlx::test(migrations = false)]
async fn a_new_password_ends_the_users_sessions(pool: PgPool) {
    let server = start_server(pool).await;
    let admin = server.sign_in().await;
    let id = create_user(&server, &admin, "grace", false).await["data"]["createUser"]["id"].clone();
    let old_session = sign_in_as(&server, "grace", PASSWORD).await;

    graphql_ok(
        &server,
        &admin,
        "mutation ($id: ID!) { setUserPassword(id: $id, password: \"another long password\") }",
        json!({"id": id}),
    )
    .await;

    assert_eq!(
        server.me(&old_session).await.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        server.login("grace", PASSWORD).await.status(),
        StatusCode::UNAUTHORIZED
    );
    sign_in_as(&server, "grace", "another long password").await;
}

#[sqlx::test(migrations = false)]
async fn the_last_enabled_admin_stays_one(pool: PgPool) {
    let server = start_server(pool).await;
    let admin = server.sign_in().await;
    let me = graphql_ok(&server, &admin, "{ me { id } }", json!({})).await["me"]["id"].clone();

    let demoted = update_user(&server, &admin, &me, false, true).await;
    let disabled = update_user(&server, &admin, &me, true, false).await;
    assert_eq!(
        demoted["errors"][0]["message"],
        "this is the last enabled admin: make someone else an admin first"
    );
    assert_eq!(demoted["errors"], disabled["errors"]);

    // With a second admin, the first may step down, and a disabled user is signed out.
    let grace =
        create_user(&server, &admin, "grace", true).await["data"]["createUser"]["id"].clone();
    let grace_session = sign_in_as(&server, "grace", PASSWORD).await;
    let stepped_down = update_user(&server, &admin, &me, false, true).await;
    assert_eq!(stepped_down["errors"], Value::Null, "{stepped_down}");
    let disabled_grace = update_user(&server, &grace_session, &grace, true, false).await;
    assert_eq!(
        disabled_grace["errors"][0]["message"],
        "this is the last enabled admin: make someone else an admin first"
    );
    graphql_ok(
        &server,
        &grace_session,
        "mutation ($id: ID!, $input: UserUpdate!) { updateUser(id: $id, input: $input) { id } }",
        json!({"id": me, "input": {"displayName": null, "isAdmin": false, "enabled": false}}),
    )
    .await;
    assert_eq!(server.me(&admin).await.status(), StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrations = false)]
async fn admins_see_workers_tei_health_and_the_backlog_per_source(pool: PgPool) {
    let server = start_server(pool).await;
    let admin = server.sign_in().await;
    server
        .internal()
        .register(&WorkerRegistration {
            name: "ferret-plugins-0".to_owned(),
            plugins: vec![PluginRegistration {
                name: "git".to_owned(),
                config_schema: json!({"type": "object"}),
            }],
        })
        .await
        .unwrap();
    let api = server.admin_api().await;
    for (source, external_id) in [("notes", "a"), ("notes", "b"), ("mail", "c")] {
        api.put_document(&document(source, external_id, "Some text."))
            .await
            .unwrap();
    }
    const STATUS: &str = "{
        workers { name plugins }
        embeddingStatus {
            id isDefault dimension backlog { source pending failed }
            embeddingApi { model reachable lastError } queryApi { model }
        }
    }";

    let status = graphql_ok(&server, &admin, STATUS, json!({})).await;

    assert_eq!(
        status["workers"],
        json!([{"name": "ferret-plugins-0", "plugins": ["git"]}])
    );
    // The pump doesn't run in these tests, so every chunk is still pending.
    assert_eq!(
        status["embeddingStatus"],
        json!([{
            "id": common::MODEL,
            "isDefault": true,
            "dimension": common::fake_endpoint::DIMENSION,
            "embeddingApi": {"model": common::ENDPOINT_MODEL, "reachable": null, "lastError": null},
            "queryApi": {"model": common::ENDPOINT_MODEL},
            "backlog": [
                {"source": "mail", "pending": 1, "failed": 0},
                {"source": "notes", "pending": 2, "failed": 0},
            ],
        }])
    );
}

#[sqlx::test(migrations = false)]
async fn a_worker_is_offline_once_it_stops_asking_unless_it_is_running_a_run(pool: PgPool) {
    let server = start_server(pool.clone()).await;
    let admin = server.sign_in().await;
    let internal = server.internal();
    let mut ids = Vec::new();
    for name in ["idle", "gone", "busy", "ancient"] {
        let registered = internal
            .register(&WorkerRegistration {
                name: name.to_owned(),
                plugins: vec![PluginRegistration {
                    name: "fake".to_owned(),
                    config_schema: json!({"type": "object"}),
                }],
            })
            .await
            .unwrap();
        ids.push(registered.worker_id);
    }
    let created = server
        .graphql(
            &admin,
            "mutation ($input: NewConnection!) { createConnection(input: $input) { id } }",
            json!({"input": {"name": "test", "icon": "plug", "plugin": "fake", "source": "notes", "config": "{}", "schedule": "0 * * * *"}}),
        )
        .await;
    assert_eq!(created["errors"], Value::Null, "{created}");
    internal
        .claim(ids[2], Duration::ZERO)
        .await
        .unwrap()
        .expect("the new connection is due");
    for (id, ago) in [
        (ids[1], "10 minutes"),
        (ids[2], "10 minutes"),
        (ids[3], "2 days"),
    ] {
        sqlx::query("update workers set last_seen_at = now() - $2::interval where id = $1")
            .bind(id)
            .bind(ago)
            .execute(&pool)
            .await
            .unwrap();
    }
    const WORKERS: &str = "{ workers { name online } }";

    let before = graphql_ok(&server, &admin, WORKERS, json!({})).await;
    assert!(
        internal
            .claim(ids[1], Duration::ZERO)
            .await
            .unwrap()
            .is_none()
    );
    let back = graphql_ok(&server, &admin, WORKERS, json!({})).await;

    assert_eq!(
        before["workers"],
        json!([
            {"name": "idle", "online": true},
            {"name": "busy", "online": true},
            {"name": "gone", "online": false},
        ])
    );
    assert_eq!(back["workers"][0], json!({"name": "gone", "online": true}));
}
