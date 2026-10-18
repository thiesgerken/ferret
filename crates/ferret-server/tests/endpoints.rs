mod common;

use std::time::{Duration, Instant};

use common::{
    DOCUMENT_PREFIX, ENDPOINT_MODEL, MODEL, QUERY_PREFIX, TestServer, api, document,
    embedding_config, eventually,
    fake_endpoint::{Broken, DIMENSION, FakeEndpoint},
    model, start_server_with,
};
use ferret_api::SearchQuery;
use ferret_server::config::{ApiConfig, EmbeddingConfig};
use serde_json::{Value, json};
use sqlx::PgPool;

const STATUS: &str = "{ embeddingStatus { id embeddingApi { url reachable lastError } } }";

/// The admin's view of `model`'s embedding endpoint.
async fn embedding_api(server: &TestServer, model: &str) -> Value {
    let cookie = server.sign_in().await;
    let body = server.graphql(&cookie, STATUS, json!({})).await;
    assert_eq!(body["errors"], Value::Null, "{body}");
    body["data"]["embeddingStatus"]
        .as_array()
        .unwrap()
        .iter()
        .find(|status| status["id"] == model)
        .expect("the model is listed")["embeddingApi"]
        .clone()
}

async fn embedded(pool: &PgPool) -> i64 {
    sqlx::query_scalar("select count(*) from chunk_embeddings")
        .fetch_one(pool)
        .await
        .unwrap()
}

fn query(q: &str, model: &str) -> SearchQuery {
    SearchQuery {
        q: q.to_owned(),
        model: Some(model.to_owned()),
        ..Default::default()
    }
}

#[sqlx::test(migrations = false)]
async fn an_answer_failing_a_check_is_discarded_and_the_pump_tries_again(pool: PgPool) {
    let endpoint = FakeEndpoint::start().await;
    let config = embedding_config(model(MODEL, &endpoint));
    let server = start_server_with(pool.clone(), endpoint.clone(), config).await;
    let api = server.admin_api().await;

    for (index, (broken, problem)) in [
        (Broken::Dimension, "257 dimensions instead of 256"),
        (Broken::NotFinite, "invalid answer"),
        (Broken::Zero, "a vector of zeros"),
        (Broken::MissingVector, "0 vectors for 1 inputs"),
    ]
    .into_iter()
    .enumerate()
    {
        endpoint.break_answers(broken);
        let id = format!("note-{index}");
        api.put_document(&document("notes", &id, "My car broke down."))
            .await
            .unwrap();
        let status = eventually(&format!("the {broken:?} answer reported"), async || {
            let status = embedding_api(&server, MODEL).await;
            status["lastError"].is_string().then_some(status)
        })
        .await;
        assert!(
            status["lastError"].as_str().unwrap().contains(problem),
            "{broken:?}: {status}"
        );
        assert_eq!(status["reachable"], true, "{status}");
        assert_eq!(embedded(&pool).await, index as i64, "{broken:?}");

        endpoint.recover();
        server.wait_for_embeddings(index as i64 + 1).await;
    }
    assert_eq!(
        embedding_api(&server, MODEL).await["lastError"],
        Value::Null
    );
}

#[sqlx::test(migrations = false)]
async fn an_endpoint_refusing_every_input_fails_instead_of_its_inputs(pool: PgPool) {
    let endpoint = FakeEndpoint::start().await;
    let config = embedding_config(model(MODEL, &endpoint));
    let server = start_server_with(pool.clone(), endpoint.clone(), config).await;
    endpoint.forget_the_model();
    // One document of several chunks, so the pump claims them as one batch.
    let long: String = (0..400)
        .map(|index| format!("Sentence {index} about my car. "))
        .collect();

    server
        .admin_api()
        .await
        .put_document(&document("notes", "car", &long))
        .await
        .unwrap();
    let status = eventually("the endpoint reported", async || {
        let status = embedding_api(&server, MODEL).await;
        status["lastError"]
            .as_str()
            .is_some_and(|error| error.contains("rejected every input"))
            .then_some(status)
    })
    .await;
    let failures: i64 = sqlx::query_scalar("select count(*) from embedding_failures")
        .fetch_one(&pool)
        .await
        .unwrap();

    assert!(
        status["lastError"]
            .as_str()
            .unwrap()
            .contains("Invalid model name"),
        "{status}"
    );
    assert_eq!(failures, 0);
    endpoint.recover();
    let chunks: i64 = sqlx::query_scalar("select count(*) from chunks")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(chunks > 1, "{chunks}");
    server.wait_for_embeddings(chunks).await;
}

#[sqlx::test(migrations = false)]
async fn a_rate_limited_pump_waits_as_long_as_it_is_told(pool: PgPool) {
    let endpoint = FakeEndpoint::start().await;
    let config = embedding_config(model(MODEL, &endpoint));
    let server = start_server_with(pool, endpoint.clone(), config).await;
    endpoint.rate_limit_for(Duration::from_secs(2));

    let started = Instant::now();
    server
        .admin_api()
        .await
        .put_document(&document("notes", "car", "My car broke down."))
        .await
        .unwrap();
    server.wait_for_embeddings(1).await;

    assert!(started.elapsed() >= Duration::from_secs(2));
    // The 429 and the batch after the pause, no retries in between.
    assert_eq!(endpoint.rate_limited(), 1);
    assert_eq!(endpoint.models().len(), 2);
}

#[sqlx::test(migrations = false)]
async fn a_429_pauses_every_pump_loop_of_the_model(pool: PgPool) {
    let endpoint = FakeEndpoint::start().await;
    let mut many_loops = model(MODEL, &endpoint);
    many_loops.embedding_api.as_mut().unwrap().concurrency = Some(4);
    let server = start_server_with(pool, endpoint.clone(), embedding_config(many_loops)).await;
    let api = server.admin_api().await;
    endpoint.rate_limit_for(Duration::from_secs(3));

    api.put_document(&document("notes", "note-0", "My car broke down."))
        .await
        .unwrap();
    eventually("the 429 reported", async || {
        let status = embedding_api(&server, MODEL).await;
        status["lastError"]
            .as_str()
            .is_some_and(|error| error.contains("rate limited"))
            .then_some(())
    })
    .await;
    // Chunks for the other loops, which would run into the limit unless they pause too.
    for index in 1..4 {
        api.put_document(&document(
            "notes",
            &format!("note-{index}"),
            "My car broke down.",
        ))
        .await
        .unwrap();
    }
    server.wait_for_embeddings(4).await;

    assert_eq!(endpoint.rate_limited(), 1);
}

#[sqlx::test(migrations = false)]
async fn an_unreachable_endpoint_only_holds_up_its_own_model(pool: PgPool) {
    let alpha = FakeEndpoint::start().await;
    let mut beta = model("beta", &alpha);
    // Nothing listens on port 1.
    beta.embedding_api = Some(ApiConfig {
        url: "http://127.0.0.1:1/v1".to_owned(),
        ..api(&alpha)
    });
    let config = EmbeddingConfig {
        default: "alpha".to_owned(),
        models: vec![model("alpha", &alpha), beta],
    };
    let server = start_server_with(pool, alpha, config).await;
    let api = server.admin_api().await;
    api.put_document(&document("notes", "car", "My car broke down."))
        .await
        .unwrap();

    server.wait_for_embeddings(1).await;
    let unreachable = eventually("beta's endpoint reported", async || {
        let status = embedding_api(&server, "beta").await;
        status["lastError"].is_string().then_some(status)
    })
    .await;
    let with_alpha = api.search(&query("vehicle", "alpha")).await.unwrap();
    let with_beta = api.search(&query("vehicle", "beta")).await.unwrap();

    assert_eq!(unreachable["reachable"], false, "{unreachable}");
    assert!(
        unreachable["lastError"]
            .as_str()
            .unwrap()
            .contains("unreachable"),
        "{unreachable}"
    );
    assert_eq!(with_alpha.hits.len(), 1);
    assert!(with_alpha.hits[0].similarity.is_some());
    assert_eq!(
        with_beta.semantic_unavailable.as_deref(),
        Some("the embedding model beta is unavailable")
    );
}

#[sqlx::test(migrations = false)]
async fn a_token_from_the_environment_is_sent_with_every_request(pool: PgPool) {
    let endpoint = FakeEndpoint::start().await;
    endpoint.require_token(&std::env::var("FERRET_TEST_EMBEDDING_TOKEN").unwrap());
    let mut with_token = model(MODEL, &endpoint);
    with_token.embedding_api.as_mut().unwrap().token_env =
        Some("FERRET_TEST_EMBEDDING_TOKEN".to_owned());
    let mut without_token = model("plain", &endpoint);
    without_token.embedding_api.as_mut().unwrap().token_env = None;
    let config = EmbeddingConfig {
        default: MODEL.to_owned(),
        models: vec![with_token, without_token],
    };
    let server = start_server_with(pool, endpoint.clone(), config).await;
    let api = server.admin_api().await;
    api.put_document(&document("notes", "car", "My car broke down."))
        .await
        .unwrap();

    server.wait_for_embeddings(1).await;
    let hits = api.search(&query("vehicle", MODEL)).await.unwrap().hits;
    let refused = eventually("the request without a token refused", async || {
        let status = embedding_api(&server, "plain").await;
        status["lastError"].as_str().map(str::to_owned)
    })
    .await;

    assert!(hits[0].similarity.is_some());
    assert!(refused.contains("401"), "{refused}");
}

#[sqlx::test(migrations = false)]
async fn an_unset_token_variable_stops_the_server(pool: PgPool) {
    let endpoint = FakeEndpoint::start().await;
    let mut config = embedding_config(model(MODEL, &endpoint));
    config.models[0].embedding_api.as_mut().unwrap().token_env =
        Some("FERRET_TEST_UNSET_TOKEN".to_owned());

    let error = ferret_server::embeddings::prepare(&pool, config)
        .await
        .err()
        .unwrap();

    assert!(
        error.to_string().contains("FERRET_TEST_UNSET_TOKEN"),
        "{error}"
    );
}

#[sqlx::test(migrations = false)]
async fn queries_and_chunks_go_to_their_own_endpoints(pool: PgPool) {
    let chunks = FakeEndpoint::start().await;
    let queries = FakeEndpoint::start().await;
    let mut split = model(MODEL, &chunks);
    split.embedding_api.as_mut().unwrap().concurrency = Some(3);
    split.query_api = Some(ApiConfig {
        model: "query-model".to_owned(),
        ..api(&queries)
    });
    let server = start_server_with(pool, chunks.clone(), embedding_config(split)).await;
    let api = server.admin_api().await;
    for index in 0..10 {
        api.put_document(&document(
            "notes",
            &format!("note-{index}"),
            &format!("Note {index} about my car."),
        ))
        .await
        .unwrap();
    }

    server.wait_for_embeddings(10).await;
    let hits = api.search(&query("vehicle", MODEL)).await.unwrap().hits;

    assert_eq!(hits.len(), 10);
    let embedded = chunks.embedded();
    // Three pump loops, and still every chunk once.
    assert_eq!(embedded.len(), 10, "{embedded:?}");
    assert!(
        embedded
            .iter()
            .all(|text| text.starts_with(DOCUMENT_PREFIX))
    );
    assert!(chunks.models().iter().all(|model| model == ENDPOINT_MODEL));
    assert_eq!(queries.embedded(), [format!("{QUERY_PREFIX}vehicle")]);
    assert_eq!(queries.models(), ["query-model"]);
}

const TEST: &str = "mutation ($id: String!) {
    testEmbeddingModel(id: $id) { role url model durationMs dimension error }
}";

/// An admin's test of `model`'s endpoints.
async fn test_model(server: &TestServer, model: &str) -> Value {
    let cookie = server.sign_in().await;
    server.graphql(&cookie, TEST, json!({ "id": model })).await
}

async fn cached(pool: &PgPool) -> i64 {
    sqlx::query_scalar("select count(*) from embedding_cache")
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test(migrations = false)]
async fn a_test_embeds_through_the_endpoint_and_changes_nothing(pool: PgPool) {
    let endpoint = FakeEndpoint::start().await;
    let config = embedding_config(model(MODEL, &endpoint));
    let server = start_server_with(pool.clone(), endpoint.clone(), config).await;

    let passed = test_model(&server, MODEL).await;
    endpoint.break_down();
    let failed = test_model(&server, MODEL).await;
    let unknown = test_model(&server, "unknown").await;

    assert_eq!(passed["errors"], Value::Null, "{passed}");
    let [test] = passed["data"]["testEmbeddingModel"]
        .as_array()
        .unwrap()
        .as_slice()
    else {
        panic!("one endpoint for chunks and queries: {passed}");
    };
    assert_eq!(test["role"], "CHUNKS_AND_QUERIES");
    assert_eq!(test["url"], endpoint.api_url());
    assert_eq!(test["model"], ENDPOINT_MODEL);
    assert_eq!(test["dimension"], DIMENSION);
    assert_eq!(test["error"], Value::Null);
    assert!(test["durationMs"].is_i64(), "{test}");
    assert!(endpoint.embedded()[0].starts_with(DOCUMENT_PREFIX));
    let error = &failed["data"]["testEmbeddingModel"][0]["error"];
    assert!(error.as_str().unwrap().contains("503"), "{failed}");
    assert_eq!(
        failed["data"]["testEmbeddingModel"][0]["dimension"],
        Value::Null
    );
    assert!(
        unknown["errors"][0]["message"]
            .as_str()
            .unwrap()
            .contains("no embedding model \"unknown\" is configured"),
        "{unknown}"
    );
    // Neither the failure nor the success shows as the endpoint's status, and nothing is cached.
    let status = embedding_api(&server, MODEL).await;
    assert_eq!(status["reachable"], Value::Null, "{status}");
    assert_eq!(status["lastError"], Value::Null, "{status}");
    assert_eq!(cached(&pool).await, 0);
}

#[sqlx::test(migrations = false)]
async fn a_test_checks_a_separate_query_endpoint_with_the_query_prefix(pool: PgPool) {
    let chunks = FakeEndpoint::start().await;
    let queries = FakeEndpoint::start().await;
    let mut split = model(MODEL, &chunks);
    split.query_api = Some(ApiConfig {
        model: "query-model".to_owned(),
        ..api(&queries)
    });
    let server = start_server_with(pool, chunks.clone(), embedding_config(split)).await;
    queries.break_answers(Broken::Dimension);

    let body = test_model(&server, MODEL).await;

    assert_eq!(body["errors"], Value::Null, "{body}");
    let tests = body["data"]["testEmbeddingModel"].as_array().unwrap();
    assert_eq!(tests.len(), 2, "{body}");
    assert_eq!(tests[0]["role"], "CHUNKS");
    assert_eq!(tests[0]["dimension"], DIMENSION);
    assert_eq!(tests[1]["role"], "QUERIES");
    assert_eq!(tests[1]["model"], "query-model");
    assert!(
        tests[1]["error"]
            .as_str()
            .unwrap()
            .contains("257 dimensions instead of 256"),
        "{body}"
    );
    assert!(chunks.embedded()[0].starts_with(DOCUMENT_PREFIX));
    assert!(queries.embedded()[0].starts_with(QUERY_PREFIX));
}
