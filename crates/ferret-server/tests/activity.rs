mod common;

use std::time::Duration;

use common::{TestServer, document, session_cookie, start_semantic_server, start_server};
use ferret_api::internal::{
    PluginRegistration, RunFinish, RunResult, RunStats, WorkerRegistration,
};
use serde_json::{Value, json};
use sqlx::PgPool;

const ACTIVITY: &str = "query ($writer: ID, $allUsers: Boolean) {
    activity(writer: $writer, allUsers: $allUsers) {
        kind status writer { kind name } put unchanged deleted log owner
    }
}";

async fn activity(server: &TestServer, variables: Value) -> Value {
    let cookie = server.sign_in().await;
    let body = server.graphql(&cookie, ACTIVITY, variables).await;
    assert_eq!(body["errors"], Value::Null, "{body}");
    body["data"]["activity"].clone()
}

#[sqlx::test(migrations = false)]
async fn an_api_keys_writes_form_one_burst_with_their_counts(pool: PgPool) {
    let server = start_server(pool).await;
    let api = server.admin_api().await;
    let note = document("inbox", "a", "First.");
    api.put_document(&note).await.unwrap();
    api.put_document(&note).await.unwrap();
    api.put_document(&document("inbox", "b", "Second."))
        .await
        .unwrap();
    api.delete_document("inbox", "b").await.unwrap();

    assert_eq!(
        activity(&server, json!({})).await,
        json!([{
            "kind": "API", "status": "RUNNING", "writer": {"kind": "API_KEY", "name": "test key"},
            "put": 2, "unchanged": 1, "deleted": 1, "log": null, "owner": null,
        }])
    );
}

#[sqlx::test(migrations = false)]
async fn a_run_counts_its_writes_while_running_and_shows_its_stats_after(pool: PgPool) {
    let server = start_server(pool).await;
    let internal = server.internal();
    let worker = internal
        .register(&WorkerRegistration {
            name: "worker-0".to_owned(),
            plugins: vec![PluginRegistration {
                name: "fake".to_owned(),
                config_schema: json!({"type": "object"}),
            }],
        })
        .await
        .unwrap()
        .worker_id;
    let cookie = server.sign_in().await;
    let created = server
        .graphql(
            &cookie,
            "mutation ($input: NewConnection!) { createConnection(input: $input) { id } }",
            json!({"input": {"name": "Notes", "icon": "notebook", "plugin": "fake", "source": "notes", "config": "{}", "schedule": "0 * * * *"}}),
        )
        .await;
    let connection = created["data"]["createConnection"]["id"].clone();
    let run = internal
        .claim(worker, Duration::ZERO)
        .await
        .unwrap()
        .unwrap();
    let run_api = server.api_with_key(&run.token);
    run_api
        .put_document(&document("notes", "a", "A note."))
        .await
        .unwrap();
    // Another writer, which the filter leaves out.
    server
        .admin_api()
        .await
        .put_document(&document("inbox", "b", "Elsewhere."))
        .await
        .unwrap();

    let running = activity(&server, json!({"writer": connection})).await;
    assert_eq!(running.as_array().unwrap().len(), 1, "{running}");
    assert_eq!(running[0]["status"], "RUNNING");
    assert_eq!(
        running[0]["writer"],
        json!({"kind": "CONNECTION", "name": "Notes"})
    );
    assert_eq!(running[0]["put"], 1);

    let stats = RunStats {
        put: 7,
        unchanged: 3,
        deleted: 1,
    };
    internal
        .finish(
            run.run_id,
            &RunFinish {
                result: RunResult::Succeeded {
                    state: json!({}),
                    stats,
                },
                log_lines: vec!["done".to_owned()],
            },
        )
        .await
        .unwrap();
    let finished = activity(&server, json!({"writer": connection})).await;
    assert_eq!(finished[0]["status"], "SUCCEEDED");
    assert_eq!(
        (
            &finished[0]["put"],
            &finished[0]["unchanged"],
            &finished[0]["deleted"]
        ),
        (&json!(7), &json!(3), &json!(1)),
        "the worker's stats replace the live counts"
    );
    assert!(finished[0]["log"].as_str().unwrap().contains("done"));
}

#[sqlx::test(migrations = false)]
async fn every_users_activity_is_for_admins_only(pool: PgPool) {
    let server = start_server(pool).await;
    server
        .admin_api()
        .await
        .put_document(&document("inbox", "a", "A."))
        .await
        .unwrap();

    let all = activity(&server, json!({"allUsers": true})).await;
    assert_eq!(all[0]["owner"], "admin");

    let admin = server.sign_in().await;
    let created = server
        .graphql(
            &admin,
            "mutation ($input: NewUser!) { createUser(input: $input) { id } }",
            json!({"input": {"username": "bob", "password": "bob's long password", "isAdmin": false}}),
        )
        .await;
    assert_eq!(created["errors"], Value::Null, "{created}");
    let user = session_cookie(&server.login("bob", "bob's long password").await);
    let body = server
        .graphql(&user, ACTIVITY, json!({"allUsers": true}))
        .await;
    assert!(body["errors"][0]["message"].as_str().is_some(), "{body}");
    let own = server.graphql(&user, ACTIVITY, json!({})).await;
    assert_eq!(own["data"]["activity"], json!([]), "{own}");
}

#[sqlx::test(migrations = false)]
async fn a_model_reports_the_users_backlog_the_pumps_rate_and_its_endpoints(pool: PgPool) {
    let server = start_semantic_server(pool).await;
    let api = server.admin_api().await;
    api.put_document(&document("notes", "a", "A car."))
        .await
        .unwrap();
    server.wait_for_embeddings(1).await;
    let cookie = server.sign_in().await;

    let body = server
        .graphql(
            &cookie,
            "{
                embeddingModels { pending failed ratePerMinute etaSeconds online embedding }
                embeddingStatus { embeddingApi { reachable lastError } }
            }",
            json!({}),
        )
        .await;

    assert_eq!(body["errors"], Value::Null, "{body}");
    let model = &body["data"]["embeddingModels"][0];
    assert_eq!(
        (&model["pending"], &model["failed"], &model["etaSeconds"]),
        (&json!(0), &json!(0), &Value::Null)
    );
    assert!(model["ratePerMinute"].as_f64().unwrap() > 0.0, "{model}");
    assert_eq!(
        (&model["online"], &model["embedding"]),
        (&json!(true), &json!(true))
    );
    assert_eq!(
        body["data"]["embeddingStatus"],
        json!([{"embeddingApi": {"reachable": true, "lastError": null}}])
    );
}

#[sqlx::test(migrations = false)]
async fn the_feed_pages_by_cursor_without_skipping(pool: PgPool) {
    let server = start_server(pool).await;
    // Three keys, three bursts.
    for index in 0..3 {
        let key = server.create_api_key(&["INGEST_WRITE"], None).await;
        server
            .api_with_key(&key)
            .put_document(&document("inbox", &format!("doc-{index}"), "Text."))
            .await
            .unwrap();
    }
    let cookie = server.sign_in().await;
    const PAGE: &str = "query ($before: String) {
        activity(before: $before, limit: 2) { id cursor }
    }";

    let first = server.graphql(&cookie, PAGE, json!({})).await;
    let first = first["data"]["activity"].as_array().unwrap().clone();
    let second = server
        .graphql(&cookie, PAGE, json!({"before": first[1]["cursor"]}))
        .await;
    let second = second["data"]["activity"].as_array().unwrap().clone();

    assert_eq!((first.len(), second.len()), (2, 1));
    let mut ids: Vec<&Value> = first
        .iter()
        .chain(&second)
        .map(|ingest| &ingest["id"])
        .collect();
    ids.dedup();
    assert_eq!(ids.len(), 3);

    let rejected = server
        .graphql(&cookie, PAGE, json!({"before": "yesterday"}))
        .await;
    assert_eq!(
        rejected["errors"][0]["message"],
        "not a cursor of this feed"
    );
}
