mod common;

use std::time::Duration;

use common::{TestServer, document, start_server};
use ferret_api::{
    Client, PutResponse,
    internal::{PluginRegistration, RunFinish, RunResult, RunStats, WorkerRegistration},
};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

/// A connection on `notes` and its live run.
struct LiveRun {
    /// The run's API, which writes like its plugin would.
    api: Client,
    run_id: Uuid,
    worker_id: Uuid,
}

async fn connection_run(server: &TestServer) -> LiveRun {
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
            json!({"input": {"plugin": "fake", "source": "notes", "config": "{}", "schedule": "0 * * * *"}}),
        )
        .await;
    assert_eq!(created["errors"], Value::Null, "{created}");
    let run = internal
        .claim(worker, Duration::ZERO)
        .await
        .unwrap()
        .unwrap();
    LiveRun {
        api: server.api_with_key(&run.token),
        run_id: run.run_id,
        worker_id: worker,
    }
}

async fn remove_exclusion(server: &TestServer, id: &Value) -> Value {
    let cookie = server.sign_in().await;
    server
        .graphql(
            &cookie,
            "mutation ($id: ID!) { removeExclusion(id: $id) }",
            json!({"id": id}),
        )
        .await
}

async fn put(api: &Client, external_id: &str, thread: Option<&str>) -> PutResponse {
    let mut note = document("notes/chat", external_id, "Lunch at noon?");
    note.thread = thread.map(str::to_owned);
    api.put_document(&note).await.unwrap()
}

async fn exclude(server: &TestServer, id: Uuid, whole_thread: bool) -> Value {
    let cookie = server.sign_in().await;
    let body = server
        .graphql(
            &cookie,
            "mutation ($id: ID!, $wholeThread: Boolean!) {
                excludeDocument(id: $id, wholeThread: $wholeThread) { deleted remembered }
            }",
            json!({"id": id, "wholeThread": whole_thread}),
        )
        .await;
    assert_eq!(body["errors"], Value::Null, "{body}");
    body["data"]["excludeDocument"].clone()
}

async fn exclusions(server: &TestServer) -> Value {
    let cookie = server.sign_in().await;
    let body = server
        .graphql(
            &cookie,
            "{ connections { exclusions { id source externalId thread } } }",
            json!({}),
        )
        .await;
    body["data"]["connections"][0]["exclusions"].clone()
}

#[sqlx::test(migrations = false)]
async fn an_excluded_document_or_thread_is_deleted_and_never_ingested_again(pool: PgPool) {
    let server = start_server(pool).await;
    let run = connection_run(&server).await.api;
    let single = put(&run, "single", None).await.id().unwrap();
    let in_thread = put(&run, "monday", Some("lunch")).await.id().unwrap();
    put(&run, "tuesday", Some("lunch")).await;
    let kept = put(&run, "other", None).await.id().unwrap();

    assert_eq!(
        exclude(&server, single, false).await,
        json!({"deleted": 1, "remembered": true})
    );
    assert_eq!(
        exclude(&server, in_thread, true).await,
        json!({"deleted": 2, "remembered": true})
    );

    assert_eq!(put(&run, "single", None).await, PutResponse::Excluded);
    assert_eq!(
        put(&run, "wednesday", Some("lunch")).await,
        PutResponse::Excluded
    );
    assert_eq!(
        put(&run, "other", None).await,
        PutResponse::Unchanged { id: kept }
    );
    let listed = exclusions(&server).await;
    assert_eq!(
        listed
            .as_array()
            .unwrap()
            .iter()
            .map(|exclusion| (exclusion["externalId"].clone(), exclusion["thread"].clone()))
            .collect::<Vec<_>>(),
        [
            (Value::Null, json!("lunch")),
            (json!("single"), Value::Null)
        ]
    );
}

#[sqlx::test(migrations = false)]
async fn removing_an_exclusion_lets_the_next_run_start_over_and_ingest_it(pool: PgPool) {
    let server = start_server(pool.clone()).await;
    let run = connection_run(&server).await;
    let single = put(&run.api, "single", None).await.id().unwrap();
    exclude(&server, single, false).await;
    let exclusion = exclusions(&server).await[0]["id"].clone();

    // The live run would write its cursor over the reset when it finishes.
    let refused = remove_exclusion(&server, &exclusion).await;
    assert_eq!(
        refused["errors"][0]["message"],
        "the connection is running: wait for the run to finish"
    );
    let finished = RunFinish {
        result: RunResult::Succeeded {
            state: json!({"cursor": 42}),
            stats: RunStats::default(),
        },
        log_lines: Vec::new(),
    };
    server
        .internal()
        .finish(run.run_id, &finished)
        .await
        .unwrap();
    let removed = remove_exclusion(&server, &exclusion).await;

    assert_eq!(removed["errors"], Value::Null, "{removed}");
    assert_eq!(exclusions(&server).await, json!([]));
    let state: Value = sqlx::query_scalar("select state from connections")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state, json!({}), "the next run starts from scratch");
    sqlx::query("update connections set next_run_at = now()")
        .execute(&pool)
        .await
        .unwrap();
    let next = server
        .internal()
        .claim(run.worker_id, Duration::ZERO)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        put(&server.api_with_key(&next.token), "single", None).await,
        PutResponse::Created { .. }
    ));
}

#[sqlx::test(migrations = false)]
async fn a_put_racing_an_exclusion_of_its_document_stays_out(pool: PgPool) {
    let server = start_server(pool.clone()).await;
    let run = connection_run(&server).await;
    let single = put(&run.api, "single", None).await.id().unwrap();
    // Holds the document's row, so the exclusion and the PUT queue up behind it in that order.
    let mut holder = pool.begin().await.unwrap();
    sqlx::query("select 1 from documents where id = $1 for update")
        .bind(single)
        .execute(&mut *holder)
        .await
        .unwrap();

    let (excluded, raced, ()) = tokio::join!(
        exclude(&server, single, false),
        async {
            tokio::time::sleep(Duration::from_millis(200)).await;
            put(&run.api, "single", None).await
        },
        async {
            tokio::time::sleep(Duration::from_millis(400)).await;
            holder.commit().await.unwrap();
        },
    );

    assert_eq!(excluded, json!({"deleted": 1, "remembered": true}));
    assert_eq!(raced, PutResponse::Excluded);
}

#[sqlx::test(migrations = false)]
async fn a_document_no_connection_owns_is_only_deleted(pool: PgPool) {
    let server = start_server(pool).await;
    let api = server.admin_api().await;
    let note = document("inbox", "scan.txt", "A scanned letter.");
    let id = api.put_document(&note).await.unwrap().id().unwrap();

    assert_eq!(
        exclude(&server, id, false).await,
        json!({"deleted": 1, "remembered": false})
    );
    assert!(matches!(
        api.put_document(&note).await.unwrap(),
        PutResponse::Created { .. }
    ));
}

#[sqlx::test(migrations = false)]
async fn a_document_without_a_thread_cant_exclude_its_thread(pool: PgPool) {
    let server = start_server(pool).await;
    let api = server.admin_api().await;
    let id = api
        .put_document(&document("inbox", "scan.txt", "A scanned letter."))
        .await
        .unwrap()
        .id()
        .unwrap();
    let cookie = server.sign_in().await;

    let refused = server
        .graphql(
            &cookie,
            "mutation ($id: ID!) { excludeDocument(id: $id, wholeThread: true) { deleted } }",
            json!({"id": id}),
        )
        .await;

    assert_eq!(
        refused["errors"][0]["message"],
        "the document isn't part of a thread"
    );
}
