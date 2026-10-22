use crate::common;

use std::time::{Duration, Instant};

use common::{TestServer, document, start_server};
use ferret_api::{
    Client, SearchQuery,
    internal::{
        ClaimedRun, Heartbeat, InternalClient, PluginRegistration, RunFinish, RunResult, RunStats,
        WorkerRegistration,
    },
};
use reqwest::StatusCode;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

/// A plugin whose config needs a `path` and may have a `secret` of at least 8 characters.
fn fake_plugin() -> PluginRegistration {
    PluginRegistration {
        name: "fake".to_owned(),
        config_schema: json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "label": {"type": "string", "x-ferret-preserve-credentials": true},
                "secret": {"type": "string", "writeOnly": true, "minLength": 8},
            },
            "required": ["path"],
            "additionalProperties": false,
        }),
    }
}

async fn register(internal: &InternalClient) -> Uuid {
    internal
        .register(&WorkerRegistration {
            name: "worker-0".to_owned(),
            plugins: vec![fake_plugin()],
        })
        .await
        .unwrap()
        .worker_id
}

/// Creates a connection of the fake plugin as the admin and returns the GraphQL response.
async fn create_connection(server: &TestServer, source: &str, schedule: &str) -> Value {
    let cookie = server.sign_in().await;
    server
        .graphql(
            &cookie,
            "mutation ($input: NewConnection!) { createConnection(input: $input) { id source } }",
            json!({"input": {
                "name": "test", "icon": "plug", "plugin": "fake",
                "source": source,
                "config": r#"{"path": "/data"}"#,
                "schedule": schedule,
            }}),
        )
        .await
}

async fn connection_id(server: &TestServer, source: &str) -> String {
    let created = create_connection(server, source, "0 * * * *").await;
    assert_eq!(created["errors"], Value::Null, "{created}");
    created["data"]["createConnection"]["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

async fn claim_now(internal: &InternalClient, worker: Uuid) -> Option<ClaimedRun> {
    internal.claim(worker, Duration::ZERO).await.unwrap()
}

fn success(state: Value) -> RunFinish {
    RunFinish {
        result: RunResult::Succeeded {
            state,
            stats: RunStats {
                put: 1,
                unchanged: 0,
                deleted: 0,
            },
        },
        log_lines: vec!["done".to_owned()],
    }
}

fn failure() -> RunFinish {
    RunFinish {
        result: RunResult::Failed {
            error: "it broke".to_owned(),
        },
        log_lines: vec!["done".to_owned()],
    }
}

async fn connection_state(pool: &PgPool) -> Value {
    sqlx::query_scalar("select state from connections")
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn put_status(api: &Client, source: &str) -> StatusCode {
    match api
        .put_document(&document(source, "doc", "Some text."))
        .await
    {
        Ok(_) => StatusCode::OK,
        Err(error) => error.status().unwrap(),
    }
}

#[sqlx::test(migrations = false)]
async fn the_internal_api_has_its_own_port_and_needs_the_worker_secret(pool: PgPool) {
    let server = start_server(pool).await;

    let wrong_secret = InternalClient::new(server.internal_url.clone(), "not the secret")
        .register(&WorkerRegistration {
            name: "intruder".to_owned(),
            plugins: vec![],
        })
        .await
        .unwrap_err();
    let on_public_port = server
        .request(reqwest::Method::POST, "/internal/workers/register")
        .send()
        .await
        .unwrap();
    let api_on_internal_port = reqwest::get(format!("{}/api/search?q=x", server.internal_url))
        .await
        .unwrap();

    assert_eq!(wrong_secret.status(), Some(StatusCode::UNAUTHORIZED));
    assert_eq!(on_public_port.status(), StatusCode::NOT_FOUND);
    assert_eq!(api_on_internal_port.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = false)]
async fn a_new_connection_is_claimed_once_with_a_token_for_its_root(pool: PgPool) {
    let server = start_server(pool.clone()).await;
    let internal = server.internal();
    let worker = register(&internal).await;
    let other_worker = register(&internal).await;
    assert!(claim_now(&internal, worker).await.is_none());

    let id = connection_id(&server, "notes").await;
    let run = claim_now(&internal, worker)
        .await
        .expect("the new connection is due");

    assert_eq!(run.connection_id.to_string(), id);
    assert_eq!(run.plugin, "fake");
    assert_eq!(run.source, "notes");
    assert_eq!(run.config, json!({"path": "/data"}));
    assert_eq!(run.state, json!({}));
    assert!(
        claim_now(&internal, other_worker).await.is_none(),
        "one live run per connection"
    );

    let token = server.api_with_key(&run.token);
    assert_eq!(put_status(&token, "notes").await, StatusCode::OK);
    assert_eq!(put_status(&token, "notes/inbox").await, StatusCode::OK);
    assert_eq!(put_status(&token, "notes-old").await, StatusCode::FORBIDDEN);
    let search = token
        .search(&SearchQuery {
            q: "text".to_owned(),
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert_eq!(search.status(), Some(StatusCode::FORBIDDEN), "ingest only");
}

#[sqlx::test(migrations = false)]
async fn a_disabled_users_connections_dont_run(pool: PgPool) {
    let server = start_server(pool.clone()).await;
    let internal = server.internal();
    let worker = register(&internal).await;
    connection_id(&server, "notes").await;
    let set_enabled = async |enabled: bool| {
        // Straight in SQL: the admin UI guards the last enabled admin, not what's tested here.
        sqlx::query("update users set enabled = $1")
            .bind(enabled)
            .execute(&pool)
            .await
            .unwrap();
    };

    set_enabled(false).await;
    let waited = internal
        .claim(worker, Duration::from_millis(500))
        .await
        .unwrap();
    assert!(waited.is_none(), "the due connection isn't claimed");

    set_enabled(true).await;
    let run = claim_now(&internal, worker).await.unwrap();
    let token = server.api_with_key(&run.token);
    assert_eq!(put_status(&token, "notes").await, StatusCode::OK);

    set_enabled(false).await;
    assert_eq!(
        put_status(&token, "notes").await,
        StatusCode::UNAUTHORIZED,
        "the run in flight stops ingesting"
    );
}

#[sqlx::test(migrations = false)]
async fn only_a_successful_run_stores_its_state_and_a_finished_runs_token_dies(pool: PgPool) {
    let server = start_server(pool.clone()).await;
    let internal = server.internal();
    let worker = register(&internal).await;
    let id = connection_id(&server, "notes").await;
    let cookie = server.sign_in().await;
    let run_now = || async {
        let body = server
            .graphql(
                &cookie,
                "mutation ($id: ID!) { runConnectionNow(id: $id) }",
                json!({"id": id}),
            )
            .await;
        assert_eq!(body["errors"], Value::Null, "{body}");
    };

    let failed = claim_now(&internal, worker).await.unwrap();
    internal
        .heartbeat(
            failed.run_id,
            &Heartbeat {
                log_lines: vec!["cloning".to_owned()],
            },
        )
        .await
        .unwrap();
    internal.finish(failed.run_id, &failure()).await.unwrap();
    assert_eq!(connection_state(&pool).await, json!({}));
    assert_eq!(
        put_status(&server.api_with_key(&failed.token), "notes").await,
        StatusCode::UNAUTHORIZED
    );

    run_now().await;
    let succeeded = claim_now(&internal, worker).await.unwrap();
    assert_eq!(succeeded.state, json!({}));
    internal
        .finish(succeeded.run_id, &success(json!({"cursor": "abc"})))
        .await
        .unwrap();
    assert_eq!(connection_state(&pool).await, json!({"cursor": "abc"}));
    let again = internal
        .finish(succeeded.run_id, &success(json!({})))
        .await
        .unwrap_err();
    assert_eq!(again.status(), Some(StatusCode::CONFLICT));

    let runs = server
        .graphql(
            &cookie,
            "{ connections { runs { status log error stats { put } } } }",
            json!({}),
        )
        .await;
    assert_eq!(
        runs["data"]["connections"][0]["runs"],
        json!([
            {"status": "SUCCEEDED", "log": "done\n", "error": null, "stats": {"put": 1}},
            {"status": "FAILED", "log": "cloning\ndone\n", "error": "it broke", "stats": null},
        ])
    );
}

#[sqlx::test(migrations = false)]
async fn a_run_without_heartbeat_is_lost_and_the_connection_runs_again(pool: PgPool) {
    let server = start_server(pool.clone()).await;
    let internal = server.internal();
    let worker = register(&internal).await;
    connection_id(&server, "notes").await;
    let completed = claim_now(&internal, worker).await.unwrap();
    internal
        .finish(completed.run_id, &success(json!({"cursor": "successful"})))
        .await
        .unwrap();
    sqlx::query("update connections set next_run_at = now()")
        .execute(&pool)
        .await
        .unwrap();
    let stale = claim_now(&internal, worker).await.unwrap();
    assert_eq!(stale.state, json!({"cursor": "successful"}));

    for statement in [
        "update plugin_runs set heartbeat_at = now() - interval '3 minutes'",
        "update connections set next_run_at = now()",
    ] {
        sqlx::query(statement).execute(&pool).await.unwrap();
    }

    assert_eq!(
        put_status(&server.api_with_key(&stale.token), "notes").await,
        StatusCode::UNAUTHORIZED,
        "a silent run's token stops working before anyone marks it lost"
    );
    let next = claim_now(&internal, worker)
        .await
        .expect("the connection runs again");
    assert_ne!(next.run_id, stale.run_id);
    assert_eq!(next.state, json!({}));
    assert_eq!(
        connection_state(&pool).await,
        json!({"cursor": "successful"})
    );
    let status: String = sqlx::query_scalar("select status from plugin_runs where id = $1")
        .bind(stale.run_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "lost");
    let heartbeat = internal
        .heartbeat(stale.run_id, &Heartbeat::default())
        .await
        .unwrap_err();
    assert_eq!(heartbeat.status(), Some(StatusCode::CONFLICT));
}

#[sqlx::test(migrations = false)]
async fn run_now_wakes_a_waiting_claim(pool: PgPool) {
    let server = start_server(pool).await;
    let internal = server.internal();
    let worker = register(&internal).await;
    // Yearly, so after its first run the connection isn't due for a long time.
    let created = create_connection(&server, "notes", "0 0 1 1 *").await;
    let id = created["data"]["createConnection"]["id"].clone();
    let first = claim_now(&internal, worker).await.unwrap();
    internal
        .finish(first.run_id, &success(json!({})))
        .await
        .unwrap();

    let started = Instant::now();
    let waiting = tokio::spawn({
        let internal = internal.clone();
        async move {
            internal
                .claim(worker, Duration::from_secs(30))
                .await
                .unwrap()
        }
    });
    tokio::time::sleep(Duration::from_millis(300)).await;
    let cookie = server.sign_in().await;
    server
        .graphql(
            &cookie,
            "mutation ($id: ID!) { runConnectionNow(id: $id) }",
            json!({"id": id}),
        )
        .await;

    assert!(waiting.await.unwrap().is_some());
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "{:?}",
        started.elapsed()
    );
}

#[sqlx::test(migrations = false)]
async fn connection_roots_are_exclusive(pool: PgPool) {
    let server = start_server(pool.clone()).await;
    register(&server.internal()).await;
    connection_id(&server, "notes/work").await;
    let key = server.admin_api().await;

    for source in ["notes", "notes/work", "notes/work/project"] {
        let overlapping = create_connection(&server, source, "0 * * * *").await;
        assert_eq!(
            overlapping["errors"][0]["extensions"]["code"], "INVALID_INPUT",
            "{source}"
        );
    }
    assert_eq!(put_status(&key, "notes/work").await, StatusCode::FORBIDDEN);
    assert_eq!(
        put_status(&key, "notes/work/sub").await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        put_status(&key, "notes").await,
        StatusCode::OK,
        "above the root is free"
    );
    assert_eq!(put_status(&key, "notes/work-old").await, StatusCode::OK);
    // A sync over `notes` would sweep the connection's documents.
    assert_eq!(
        key.start_sync("notes").await.unwrap_err().status(),
        Some(StatusCode::FORBIDDEN)
    );
    assert!(key.start_sync("notes/private").await.is_ok());
}

#[sqlx::test(migrations = false)]
async fn deleting_a_connection_takes_its_documents_after_a_typed_confirmation(pool: PgPool) {
    let server = start_server(pool.clone()).await;
    let internal = server.internal();
    let worker = register(&internal).await;
    let id = connection_id(&server, "notes").await;
    let run = claim_now(&internal, worker).await.unwrap();
    let token = server.api_with_key(&run.token);
    token
        .put_document(&document("notes", "a", "A."))
        .await
        .unwrap();
    token
        .put_document(&document("notes/inbox", "b", "B."))
        .await
        .unwrap();
    server
        .admin_api()
        .await
        .put_document(&document("notes-old", "c", "C."))
        .await
        .unwrap();
    let cookie = server.sign_in().await;
    let delete = |confirm: &'static str| {
        server.graphql(
            &cookie,
            "mutation ($id: ID!, $confirm: String!) { deleteConnection(id: $id, confirmSource: $confirm) }",
            json!({"id": id, "confirm": confirm}),
        )
    };

    let unconfirmed = delete("note").await;
    let deleted = delete("notes").await;

    assert_eq!(
        unconfirmed["errors"][0]["message"],
        "type the source \"notes\" to confirm"
    );
    assert_eq!(deleted["data"]["deleteConnection"], 2);
    let left: Vec<String> = sqlx::query_scalar("select source from documents")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(left, ["notes-old"]);
    assert_eq!(put_status(&token, "notes").await, StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrations = false)]
async fn configs_and_schedules_are_validated(pool: PgPool) {
    let server = start_server(pool).await;
    register(&server.internal()).await;
    let cookie = server.sign_in().await;
    let create = |plugin: &str, config: &str, schedule: &str| {
        server.graphql(
            &cookie,
            "mutation ($input: NewConnection!) { createConnection(input: $input) { id } }",
            json!({"input": {"name": "test", "icon": "plug", "plugin": plugin, "source": "notes", "config": config, "schedule": schedule}}),
        )
    };

    for (body, message) in [
        (
            create("fake", "{}", "0 * * * *").await,
            "invalid config: \"path\" is a required property",
        ),
        (
            create("fake", r#"{"path": 1}"#, "0 * * * *").await,
            "invalid config at /path: 1 is not of type \"string\"",
        ),
        (
            create("fake", "not json", "0 * * * *").await,
            "the config isn't JSON: expected ident at line 1 column 2",
        ),
        (
            create("nope", r#"{"path": "/"}"#, "0 * * * *").await,
            "no worker offers the plugin \"nope\"",
        ),
    ] {
        assert_eq!(body["errors"][0]["message"], message);
    }
    let schedule = create("fake", r#"{"path": "/"}"#, "every hour").await;
    assert!(
        schedule["errors"][0]["message"]
            .as_str()
            .unwrap()
            .starts_with("\"every hour\" is not a cron schedule"),
        "{schedule}"
    );
}

async fn update_config(server: &TestServer, id: &str, config: Value) -> Value {
    let cookie = server.sign_in().await;
    server
        .graphql(
            &cookie,
            "mutation ($id: ID!, $input: ConnectionUpdate!) { updateConnection(id: $id, input: $input) { id } }",
            json!({"id": id, "input": {"name": "test", "icon": "plug", "config": config.to_string(), "schedule": "0 * * * *", "enabled": true}}),
        )
        .await
}

async fn credentials_of(pool: &PgPool, id: &str) -> Option<Vec<u8>> {
    sqlx::query_scalar("select credentials from connections where id = $1::uuid")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test(migrations = false)]
async fn credentials_sealed_under_another_key_fail_the_run_and_must_be_entered_again(pool: PgPool) {
    let server = start_server(pool.clone()).await;
    let internal = server.internal();
    let worker = register(&internal).await;
    let id = connection_id(&server, "notes").await;
    // As after restoring the database without its FERRET_CONFIG_KEY.
    sqlx::query("update connections set credentials = $1")
        .bind(vec![7u8; 64])
        .execute(&pool)
        .await
        .unwrap();

    assert!(claim_now(&internal, worker).await.is_none());
    let (status, error): (String, String) = sqlx::query_as("select status, error from plugin_runs")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "failed");
    assert!(error.contains("enter them again"), "{error}");

    let unchanged = update_config(&server, &id, json!({"path": "/data"})).await;
    let safe_change =
        update_config(&server, &id, json!({"path": "/data", "label": "display"})).await;
    let entered = update_config(
        &server,
        &id,
        json!({"path": "/data", "secret": "a new secret"}),
    )
    .await;

    assert_eq!(
        unchanged["errors"][0]["message"],
        "the stored credentials can't be decrypted with FERRET_CONFIG_KEY: enter secret"
    );
    assert_eq!(
        safe_change["errors"][0]["message"],
        "the stored credentials can't be decrypted with FERRET_CONFIG_KEY: enter secret"
    );
    assert_eq!(entered["errors"], Value::Null, "{entered}");
}

#[sqlx::test(migrations = false)]
async fn credentials_open_only_for_the_connection_they_were_sealed_for(pool: PgPool) {
    let server = start_server(pool.clone()).await;
    let internal = server.internal();
    let worker = register(&internal).await;
    let notes = connection_id(&server, "notes").await;
    let mail = connection_id(&server, "mail").await;
    update_config(
        &server,
        &notes,
        json!({"path": "/data", "secret": "notes secret"}),
    )
    .await;
    // Someone with write access to the database copies one connection's credentials to another.
    sqlx::query("update connections set credentials = $1 where id = $2::uuid")
        .bind(credentials_of(&pool, &notes).await)
        .bind(&mail)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "update connections set next_run_at = now() + interval '1 day' where id = $1::uuid",
    )
    .bind(&notes)
    .execute(&pool)
    .await
    .unwrap();

    while claim_now(&internal, worker).await.is_some() {}

    let failed: Vec<String> = sqlx::query_scalar(
        "select c.source from plugin_runs r join connections c on c.id = r.connection_id
         where r.status = 'failed'",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(failed, ["mail"]);
}

#[sqlx::test(migrations = false)]
async fn null_removes_a_stored_credential(pool: PgPool) {
    let server = start_server(pool.clone()).await;
    register(&server.internal()).await;
    let id = connection_id(&server, "notes").await;
    update_config(
        &server,
        &id,
        json!({"path": "/data", "secret": "a stored secret"}),
    )
    .await;
    assert!(credentials_of(&pool, &id).await.is_some());

    let cleared = update_config(&server, &id, json!({"path": "/data", "secret": null})).await;

    assert_eq!(cleared["errors"], Value::Null, "{cleared}");
    assert_eq!(credentials_of(&pool, &id).await, None);
}

#[sqlx::test(migrations = false)]
async fn validation_errors_never_quote_a_credential(pool: PgPool) {
    let server = start_server(pool).await;
    register(&server.internal()).await;
    let id = connection_id(&server, "notes").await;

    let too_short = update_config(&server, &id, json!({"path": "/data", "secret": "s3cr3t"})).await;
    let wrong_type =
        update_config(&server, &id, json!({"path": 42, "secret": "a long secret"})).await;

    assert_eq!(
        too_short["errors"][0]["message"],
        "invalid config at /secret"
    );
    let message = wrong_type["errors"][0]["message"].as_str().unwrap();
    assert!(
        message.starts_with("invalid config at /path: "),
        "{message}"
    );
    assert!(!message.contains("a long secret"), "{message}");
}

#[sqlx::test(migrations = false)]
async fn a_root_with_documents_below_it_is_refused(pool: PgPool) {
    let server = start_server(pool).await;
    register(&server.internal()).await;
    let api = server.admin_api().await;
    assert_eq!(put_status(&api, "notes/work").await, StatusCode::OK);

    let refused = create_connection(&server, "notes", "0 * * * *").await;
    let elsewhere = create_connection(&server, "mail", "0 * * * *").await;

    assert_eq!(
        refused["errors"][0]["message"],
        "1 document already lives below \"notes\": delete them or pick another root"
    );
    assert_eq!(elsewhere["errors"], Value::Null, "{elsewhere}");
}
