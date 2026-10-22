use crate::common;

use common::{document, session_cookie, start_server};
use ferret_server::{AppState, ConfigKey, SessionKey, WorkerSecret};
use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::{collections::HashSet, sync::Arc, time::Duration};

const COUNTS: &str = "{ indexSummary { documents chunks sources } embeddingModels { embedded chunks pending failed ratePerMinute } embeddingStatus { embedded chunks backlog { source pending failed } } storedEmbeddingModels { id embedded } writerStatistics { documents chunks } }";

#[sqlx::test(migrations = false)]
async fn homepage_preserves_source_facet_writers_and_user_isolation(pool: PgPool) {
    let server = start_server(pool.clone()).await;
    let admin = server.sign_in().await;
    let first = server.admin_api().await;
    first
        .put_document(&document("shared", "a", "A document."))
        .await
        .unwrap();
    let second_key = server.create_api_key(&["INGEST_WRITE"], None).await;
    let second = server.api_with_key(&second_key);
    for (source, id) in [("shared", "b"), ("unknown/a", "c"), ("unknown/b", "d")] {
        second
            .put_document(&document(source, id, "Another document."))
            .await
            .unwrap();
    }
    sqlx::query("update documents set api_key_id = null where source like 'unknown/%'")
        .execute(&pool)
        .await
        .unwrap();
    let response = server.graphql(&admin, "{ indexSummary { documents chunks sources } explore(input: {}) { total facets { sources { writer { id } } } } }", json!({})).await;
    assert_eq!(response["errors"], Value::Null, "{response}");
    let data = &response["data"];
    let writers: HashSet<_> = data["explore"]["facets"]["sources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|source| source["writer"]["id"].as_str())
        .collect();
    assert_eq!(
        data["indexSummary"],
        json!({"documents":4,"chunks":4,"sources":writers.len()})
    );
    assert_eq!(
        writers.len(),
        2,
        "two document keys in one source are one source-facet writer"
    );
    let user = server.graphql(&admin, "mutation { createUser(input: {username: \"bob\", password: \"a long user password\", isAdmin: false}) { id } }", json!({})).await;
    assert_eq!(user["errors"], Value::Null, "{user}");
    let bob = session_cookie(&server.login("bob", "a long user password").await);
    let mut registry_lock = pool.begin().await.unwrap();
    sqlx::query("lock table embedding_models in access exclusive mode")
        .execute(&mut *registry_lock)
        .await
        .unwrap();
    let progress = tokio::time::timeout(
        Duration::from_secs(2),
        server.graphql(
            &bob,
            "{ embeddingModels { chunks pending embedded failed } }",
            json!({}),
        ),
    )
    .await
    .expect("user progress must not load global model statistics");
    assert_eq!(progress["errors"], Value::Null, "{progress}");
    assert_eq!(progress["data"]["embeddingModels"][0]["chunks"], 0);
    registry_lock.rollback().await.unwrap();
    let empty = server.graphql(&bob, "{ indexSummary { documents chunks sources } embeddingModels { chunks pending embedded failed } writerStatistics { documents chunks } }", json!({})).await;
    assert_eq!(empty["errors"], Value::Null, "{empty}");
    assert_eq!(
        empty["data"]["indexSummary"],
        json!({"documents":0,"chunks":0,"sources":0})
    );
    assert_eq!(empty["data"]["embeddingModels"][0]["pending"], 0);
    assert_eq!(empty["data"]["writerStatistics"]["documents"], 0);
    first
        .put_document(&document("new", "e", "One more."))
        .await
        .unwrap();
    let cached = server
        .graphql(
            &admin,
            "{ indexSummary { documents chunks sources } }",
            json!({}),
        )
        .await;
    assert_eq!(
        cached["data"]["indexSummary"], data["indexSummary"],
        "homepage totals have a 60-second TTL, not write invalidation"
    );
}

#[sqlx::test(migrations = false)]
async fn warm_admin_activity_settings_and_home_counts_do_not_wait_for_content_tables(pool: PgPool) {
    let server = start_server(pool.clone()).await;
    let cookie = server.sign_in().await;
    server
        .admin_api()
        .await
        .put_document(&document("notes", "a", "Existing document."))
        .await
        .unwrap();
    let first = server.graphql(&cookie, COUNTS, json!({})).await;
    assert_eq!(first["errors"], Value::Null, "{first}");
    let mut lock = pool.begin().await.unwrap();
    sqlx::query("lock table documents, chunks, chunk_embeddings in access exclusive mode")
        .execute(&mut *lock)
        .await
        .unwrap();
    let reads = async {
        let mut requests = Vec::new();
        for _ in 0..12 {
            requests.push(server.graphql(&cookie, COUNTS, json!({})));
        }
        for body in futures_util::future::join_all(requests).await {
            assert_eq!(body, first);
        }
    };
    tokio::time::timeout(Duration::from_secs(2), reads)
        .await
        .expect("cached consumers must not run count queries");
    lock.rollback().await.unwrap();
    sqlx::query("insert into embedding_failures (chunk_id, model, error, at) select id, 'fake', 'rejected', now() from chunks").execute(&pool).await.unwrap();
    tokio::time::sleep(Duration::from_millis(5100)).await;
    let refreshed = server.graphql(&cookie, COUNTS, json!({})).await;
    assert_eq!(refreshed["errors"], Value::Null, "{refreshed}");
    assert_eq!(refreshed["data"]["embeddingModels"][0]["pending"], 0);
    assert_eq!(refreshed["data"]["embeddingModels"][0]["failed"], 1);
    assert_eq!(
        refreshed["data"]["embeddingStatus"][0]["backlog"][0]["failed"],
        1
    );
}

#[sqlx::test(migrations = false)]
async fn readiness_uses_isolated_bounded_capacity_and_closes_on_shutdown(pool: PgPool) {
    ferret_server::migrate(&pool).await.unwrap();
    let endpoint = common::fake_endpoint::FakeEndpoint::start().await;
    let embedding = Arc::new(
        ferret_server::embeddings::prepare(
            &pool,
            common::embedding_config(common::model("fake", &endpoint)),
        )
        .await
        .unwrap(),
    );
    let application = PgPoolOptions::new()
        .max_connections(1)
        .connect_with((*pool.connect_options()).clone())
        .await
        .unwrap();
    let state = AppState::new(
        application.clone(),
        SessionKey::new("a session secret of at least 32 bytes").unwrap(),
        WorkerSecret::new(common::WORKER_SECRET.to_owned()).unwrap(),
        embedding,
        ConfigKey::new("dGVzdCBjb25maWcga2V5IG9mIHRoaXJ0eS10d28gYnk=").unwrap(),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/healthz", listener.local_addr().unwrap());
    let app = ferret_server::router(state.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let held = application.acquire().await.unwrap();
    let http = reqwest::Client::new();
    for _ in 0..8 {
        let response = tokio::time::timeout(Duration::from_millis(900), http.get(&url).send())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
    }
    // Both authentication and the isolated connection options must target this test's database.
    let connections: i64 = sqlx::query_scalar("select count(*) from pg_stat_activity where datname = current_database() and query = 'select 1'").fetch_one(&pool).await.unwrap();
    assert_eq!(
        connections, 1,
        "readiness retains only one dedicated connection"
    );
    state.close_readiness().await;
    let response = tokio::time::timeout(Duration::from_millis(900), http.get(&url).send())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::SERVICE_UNAVAILABLE);
    drop(held);
    application.close().await;
    server.abort();
    let _ = server.await;
}

#[sqlx::test(migrations = false)]
async fn coverage_snapshots_remain_coherent_after_ingest_embedding_and_deletion(pool: PgPool) {
    let alpha = common::fake_endpoint::FakeEndpoint::start().await;
    let beta = common::fake_endpoint::FakeEndpoint::start().await;
    beta.break_down();
    let server = common::start_server_with(
        pool.clone(),
        alpha.clone(),
        ferret_server::config::EmbeddingConfig {
            default: "alpha".to_owned(),
            models: vec![common::model("alpha", &alpha), common::model("beta", &beta)],
        },
    )
    .await;
    let cookie = server.sign_in().await;
    let api = server.admin_api().await;
    let query = "{ indexSummary { chunks } embeddingModels { id embedded pending failed chunks } embeddingStatus { id embedded chunks backlog { pending failed } } storedEmbeddingModels { id embedded } }";
    api.put_document(&document("notes", "a", "First passage."))
        .await
        .unwrap();
    server.wait_for_embeddings(1).await;
    let initial = server.graphql(&cookie, query, json!({})).await;
    assert_coverage(&initial, 1);

    api.put_document(&document("notes", "b", "Second passage."))
        .await
        .unwrap();
    server.wait_for_embeddings(2).await;
    sqlx::query(
        "insert into embedding_failures (chunk_id, model, error, at)
        select c.id, 'beta', 'rejected', now() from chunks c
        join documents d on d.id = c.document_id where d.external_id = 'a'",
    )
    .execute(&pool)
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(5100)).await;
    let ingested = server.graphql(&cookie, query, json!({})).await;
    assert_coverage(&ingested, 2);
    assert_eq!(ingested["data"]["embeddingModels"][0]["embedded"], 2);
    assert_eq!(ingested["data"]["embeddingModels"][1]["pending"], 1);
    assert_eq!(ingested["data"]["embeddingModels"][1]["failed"], 1);

    api.delete_document("notes", "a").await.unwrap();
    tokio::time::sleep(Duration::from_millis(5100)).await;
    let deleted = server.graphql(&cookie, query, json!({})).await;
    assert_coverage(&deleted, 1);
    assert_eq!(deleted["data"]["embeddingModels"][0]["embedded"], 1);
    assert_eq!(deleted["data"]["embeddingModels"][1]["pending"], 1);
    assert_eq!(deleted["data"]["embeddingModels"][1]["failed"], 0);

    api.delete_document("notes", "b").await.unwrap();
    tokio::time::sleep(Duration::from_millis(5100)).await;
    let empty = server.graphql(&cookie, query, json!({})).await;
    assert_coverage(&empty, 0);
}

fn assert_coverage(response: &Value, chunks: i64) {
    assert_eq!(response["errors"], Value::Null, "{response}");
    let data = &response["data"];
    assert_eq!(
        data["indexSummary"]["chunks"], 1,
        "homepage keeps its 60-second snapshot"
    );
    for model in data["embeddingModels"].as_array().unwrap() {
        let embedded = model["embedded"].as_i64().unwrap();
        let pending = model["pending"].as_i64().unwrap();
        let failed = model["failed"].as_i64().unwrap();
        assert_eq!(model["chunks"], chunks);
        assert_eq!(embedded + pending + failed, chunks);
    }
    for model in data["embeddingStatus"].as_array().unwrap() {
        let embedded = model["embedded"].as_i64().unwrap();
        let backlog: i64 = model["backlog"]
            .as_array()
            .unwrap()
            .iter()
            .map(|source| source["pending"].as_i64().unwrap() + source["failed"].as_i64().unwrap())
            .sum();
        assert_eq!(model["chunks"], chunks);
        assert_eq!(embedded + backlog, chunks);
        let stored = data["storedEmbeddingModels"]
            .as_array()
            .unwrap()
            .iter()
            .find(|stored| stored["id"] == model["id"])
            .unwrap();
        assert_eq!(stored["embedded"], embedded);
    }
}
