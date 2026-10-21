mod common;

use std::time::Duration;

use common::{TestServer, document, eventually, start_server};
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
            json!({"input": {"name": "test", "icon": "plug", "plugin": "fake", "source": "notes", "config": "{}", "schedule": "0 * * * *"}}),
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

/// Waits until a statement in this test's database waits for a lock of `kind`, a wait event of
/// `pg_stat_activity`.
async fn wait_for_a_lock_wait(pool: &PgPool, kind: &str) {
    eventually(&format!("a wait for a {kind} lock"), async || {
        let waiting: bool = sqlx::query_scalar(
            "select exists (select 1 from pg_stat_activity
                            where datname = current_database()
                              and wait_event_type = 'Lock' and wait_event = $1)",
        )
        .bind(kind)
        .fetch_one(pool)
        .await
        .unwrap();
        waiting.then_some(())
    })
    .await
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
            // The exclusion holds the source lock and waits for the row.
            wait_for_a_lock_wait(&pool, "transactionid").await;
            put(&run.api, "single", None).await
        },
        async {
            // The PUT waits for the source lock.
            wait_for_a_lock_wait(&pool, "advisory").await;
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

#[sqlx::test(migrations = false)]
async fn flattening_preserves_payloads_uuids_embeddings_and_absent_document_thread_exclusions(
    pool: PgPool,
) {
    let server = common::start_semantic_server(pool.clone()).await;
    let run = connection_run(&server).await;
    let reader = server.admin_api().await;
    let mut kept = document("notes/old/a", "kept", "The car needs repair.");
    kept.thread = Some("active".into());
    let kept_id = run.api.put_document(&kept).await.unwrap().id().unwrap();
    for index in 0..35 {
        run.api
            .put_document(&document(
                "notes/old/b",
                &format!("page-{index}"),
                "Relocation page boundary.",
            ))
            .await
            .unwrap();
    }
    let single = document(
        "notes/old/absent",
        "removed",
        "Excluded before layout change.",
    );
    exclude(
        &server,
        run.api.put_document(&single).await.unwrap().id().unwrap(),
        false,
    )
    .await;
    let mut thread = document("notes/old/absent-thread", "first", "Excluded whole thread.");
    thread.thread = Some("gone-thread".into());
    exclude(
        &server,
        run.api.put_document(&thread).await.unwrap().id().unwrap(),
        true,
    )
    .await;
    server.wait_for_embeddings(36).await;
    let before: Vec<(Uuid, Vec<u8>, String)> = sqlx::query_as(
        "select c.id, c.text_hash, md5(e.embedding::text) from chunks c join chunk_embeddings e on e.chunk_id = c.id order by c.id",
    ).fetch_all(&pool).await.unwrap();
    let seen_before: chrono::DateTime<chrono::Utc> =
        sqlx::query_scalar("select seen_at from documents where id = $1")
            .bind(kept_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let old_exclusions = exclusions(&server).await;
    let transform = ferret_api::NamespaceFlatten {
        from_source_prefix: "notes/old".into(),
        to_source: "notes".into(),
    };
    let result = run.api.flatten_namespace(&transform).await.unwrap();
    assert_eq!((result.documents, result.exclusions), (36, 2));
    let relocated = reader.document(kept_id).await.unwrap();
    assert_eq!(relocated.source, "notes");
    assert_eq!(relocated.external_id, "a/kept");
    assert_eq!(relocated.text, kept.text);
    assert_eq!(relocated.thread, kept.thread);
    assert_eq!(
        sqlx::query_scalar::<_, chrono::DateTime<chrono::Utc>>(
            "select seen_at from documents where id = $1"
        )
        .bind(kept_id)
        .fetch_one(&pool)
        .await
        .unwrap(),
        seen_before
    );
    let after: Vec<(Uuid, Vec<u8>, String)> = sqlx::query_as(
        "select c.id, c.text_hash, md5(e.embedding::text) from chunks c join chunk_embeddings e on e.chunk_id = c.id order by c.id",
    ).fetch_all(&pool).await.unwrap();
    assert_eq!(after, before);
    let wrong_sources: i64 = sqlx::query_scalar(
        "select (select count(*) from chunks where source != 'notes') + (select count(*) from chunk_embeddings where source != 'notes')",
    ).fetch_one(&pool).await.unwrap();
    assert_eq!(wrong_sources, 0);
    kept.source = "notes".into();
    kept.external_id = "a/kept".into();
    assert_eq!(
        run.api.put_document(&kept).await.unwrap(),
        PutResponse::Unchanged { id: kept_id }
    );
    let moved_exclusions = exclusions(&server).await;
    for old in old_exclusions.as_array().unwrap() {
        let moved = moved_exclusions
            .as_array()
            .unwrap()
            .iter()
            .find(|moved| moved["id"] == old["id"])
            .unwrap();
        assert_eq!(moved["source"], "notes");
        if let Some(external_id) = old["externalId"].as_str() {
            assert_eq!(moved["externalId"], format!("absent/{external_id}"));
        } else {
            assert_eq!(moved["thread"], old["thread"]);
        }
    }
    let mut single = single;
    single.source = "notes".into();
    single.external_id = "absent/removed".into();
    assert_eq!(
        run.api.put_document(&single).await.unwrap(),
        PutResponse::Excluded
    );
    thread.source = "notes".into();
    thread.external_id = "new-first-message".into();
    assert_eq!(
        run.api.put_document(&thread).await.unwrap(),
        PutResponse::Excluded
    );
    let repeated = run.api.flatten_namespace(&transform).await.unwrap();
    assert_eq!((repeated.documents, repeated.exclusions), (0, 0));
    assert_eq!(
        reader
            .flatten_namespace(&transform)
            .await
            .unwrap_err()
            .status(),
        Some(reqwest::StatusCode::FORBIDDEN)
    );
    let outside = ferret_api::NamespaceFlatten {
        from_source_prefix: "other".into(),
        to_source: "notes".into(),
    };
    assert_eq!(
        run.api
            .flatten_namespace(&outside)
            .await
            .unwrap_err()
            .status(),
        Some(reqwest::StatusCode::FORBIDDEN)
    );
}

#[sqlx::test(migrations = false)]
async fn colliding_document_or_exclusion_identities_roll_back_the_entire_flatten(pool: PgPool) {
    let server = start_server(pool.clone()).await;
    let run = connection_run(&server).await;
    let reader = server.admin_api().await;
    let legacy = document("notes/old/a", "kept", "Original payload.");
    let original = run.api.put_document(&legacy).await.unwrap().id().unwrap();
    let target = run
        .api
        .put_document(&document("notes", "a/kept", "Destination payload."))
        .await
        .unwrap()
        .id()
        .unwrap();
    let transform = ferret_api::NamespaceFlatten {
        from_source_prefix: "notes/old".into(),
        to_source: "notes".into(),
    };
    assert_eq!(
        run.api
            .flatten_namespace(&transform)
            .await
            .unwrap_err()
            .status(),
        Some(reqwest::StatusCode::CONFLICT)
    );
    assert_eq!(
        reader.document(original).await.unwrap().source,
        legacy.source
    );
    assert_eq!(
        reader.document(target).await.unwrap().text,
        "Destination payload."
    );
    run.api.delete_document("notes", "a/kept").await.unwrap();
    for source in ["notes/old/b", "notes/old/c"] {
        let mut excluded = document(source, "first", "Thread to exclude.");
        excluded.thread = Some("same-thread".into());
        exclude(
            &server,
            run.api.put_document(&excluded).await.unwrap().id().unwrap(),
            true,
        )
        .await;
    }
    let before = exclusions(&server).await;
    assert_eq!(
        run.api
            .flatten_namespace(&transform)
            .await
            .unwrap_err()
            .status(),
        Some(reqwest::StatusCode::CONFLICT)
    );
    assert_eq!(
        reader.document(original).await.unwrap().source,
        legacy.source
    );
    assert_eq!(exclusions(&server).await, before);
}

#[sqlx::test(migrations = false)]
async fn an_exclusion_waiting_for_relocation_uses_the_new_identity_and_does_not_block_heartbeats(
    pool: PgPool,
) {
    let server = start_server(pool.clone()).await;
    let run = connection_run(&server).await;
    let mut note = document("notes/old/a", "one", "Race-safe relocation.");
    let id = run.api.put_document(&note).await.unwrap().id().unwrap();
    let mut holder = pool.begin().await.unwrap();
    sqlx::query("select 1 from documents where id = $1 for update")
        .bind(id)
        .execute(&mut *holder)
        .await
        .unwrap();
    let transform = ferret_api::NamespaceFlatten {
        from_source_prefix: "notes/old".into(),
        to_source: "notes".into(),
    };
    let (moved, excluded, ()) = tokio::join!(
        run.api.flatten_namespace(&transform),
        async {
            wait_for_a_lock_wait(&pool, "transactionid").await;
            exclude(&server, id, false).await
        },
        async {
            wait_for_a_lock_wait(&pool, "advisory").await;
            tokio::time::timeout(
                Duration::from_secs(5),
                server
                    .internal()
                    .heartbeat(run.run_id, &ferret_api::internal::Heartbeat::default()),
            )
            .await
            .expect("relocation must not block its own lease heartbeat")
            .unwrap();
            holder.commit().await.unwrap();
        },
    );
    assert_eq!(moved.unwrap().documents, 1);
    assert_eq!(excluded, json!({"deleted": 1, "remembered": true}));
    note.source = "notes".into();
    note.external_id = "a/one".into();
    assert_eq!(
        run.api.put_document(&note).await.unwrap(),
        PutResponse::Excluded
    );
    assert_eq!(exclusions(&server).await[0]["source"], "notes");
}

#[sqlx::test(migrations = false)]
async fn a_run_finishing_during_relocation_cannot_commit_identity_changes(pool: PgPool) {
    let server = start_server(pool.clone()).await;
    let run = connection_run(&server).await;
    let note = document("notes/old/a", "one", "Original payload.");
    let id = run.api.put_document(&note).await.unwrap().id().unwrap();
    let mut holder = pool.begin().await.unwrap();
    sqlx::query("select 1 from documents where id = $1 for update")
        .bind(id)
        .execute(&mut *holder)
        .await
        .unwrap();
    let transform = ferret_api::NamespaceFlatten {
        from_source_prefix: "notes/old".into(),
        to_source: "notes".into(),
    };
    let (moved, ()) = tokio::join!(run.api.flatten_namespace(&transform), async {
        wait_for_a_lock_wait(&pool, "transactionid").await;
        server
            .internal()
            .finish(
                run.run_id,
                &RunFinish {
                    result: RunResult::Failed {
                        error: "finished during relocation".into(),
                    },
                    log_lines: vec![],
                },
            )
            .await
            .unwrap();
        holder.commit().await.unwrap();
    },);
    assert_eq!(
        moved.unwrap_err().status(),
        Some(reqwest::StatusCode::CONFLICT)
    );
    let stored = server.admin_api().await.document(id).await.unwrap();
    assert_eq!(stored.source, note.source);
    assert_eq!(stored.external_id, note.external_id);
    assert_eq!(stored.text, note.text);
}
