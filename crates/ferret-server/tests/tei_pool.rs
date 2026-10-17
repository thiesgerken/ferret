mod common;

use common::{
    DOCUMENT_PREFIX, MODEL, QUERY_PREFIX, TestServer, document, embedding_config, eventually,
    fake_tei::{FAKE_MODEL_ID, FakeTei, MAX_INPUT_LENGTH},
    model, start_server_with, tei_config,
};
use ferret_api::SearchQuery;
use ferret_server::config::TeiConfig;
use serde_json::{Value, json};
use sqlx::PgPool;

const TEIS: &str =
    "{ teis { url external enabled queries embedding state problem servedModel model lastError } }";

async fn teis(server: &TestServer) -> Vec<Value> {
    let cookie = server.sign_in().await;
    let response = server.graphql(&cookie, TEIS, json!({})).await;
    assert_eq!(response["errors"], Value::Null, "{response}");
    response["data"]["teis"].as_array().unwrap().clone()
}

async fn listed(server: &TestServer, url: &str) -> Value {
    teis(server)
        .await
        .into_iter()
        .find(|tei| tei["url"] == url)
        .expect("the TEI is listed")
}

fn query(q: &str) -> SearchQuery {
    SearchQuery {
        q: q.to_owned(),
        ..Default::default()
    }
}

#[sqlx::test(migrations = false)]
async fn a_tei_too_short_for_the_longest_input_is_unsuitable_and_search_still_works(pool: PgPool) {
    // Takes every chunk, but not with a title in front.
    let tei = FakeTei::start(2201).await;
    let config = embedding_config(model(MODEL, &tei), &[&tei]);
    let server = start_server_with(pool, tei.clone(), config).await;
    let api = server.admin_api().await;
    api.put_document(&document("notes", "car", "My car broke down."))
        .await
        .unwrap();

    let listed = listed(&server, &tei.url).await;
    let response = api.search(&query("car")).await.unwrap();

    assert_eq!(listed["state"], "UNSUITABLE");
    assert_eq!(listed["model"], MODEL);
    assert!(
        listed["problem"].as_str().unwrap().contains("2201 tokens"),
        "{listed}"
    );
    assert_eq!(response.hits.len(), 1);
    assert!(response.semantic_unavailable.is_some());
}

#[sqlx::test(migrations = false)]
async fn queries_and_the_pump_use_the_teis_switched_on_for_them(pool: PgPool) {
    let internal = FakeTei::start(MAX_INPUT_LENGTH).await;
    let laptop = FakeTei::start(MAX_INPUT_LENGTH).await;
    let mut config = embedding_config(model(MODEL, &internal), &[&internal]);
    config.teis.push(TeiConfig {
        url: laptop.url.clone(),
        external: true,
    });
    let server = start_server_with(pool, internal.clone(), config).await;
    let cookie = server.sign_in().await;
    // An external TEI starts with queries off.
    assert_eq!(
        listed(&server, &laptop.url).await["queries"],
        false,
        "{:?}",
        teis(&server).await
    );

    let updated = server
        .graphql(
            &cookie,
            "mutation ($url: String!) { updateTei(url: $url, input: {embedding: false}) { enabled queries embedding } }",
            json!({"url": internal.url}),
        )
        .await;
    let api = server.admin_api().await;
    api.put_document(&document("notes", "car", "My car broke down."))
        .await
        .unwrap();
    server.wait_for_embeddings(1).await;
    let hits = api.search(&query("vehicle")).await.unwrap().hits;

    assert_eq!(
        updated["data"]["updateTei"],
        json!({"enabled": true, "queries": true, "embedding": false})
    );
    let document_text = format!("{DOCUMENT_PREFIX}car\n\nMy car broke down.");
    assert_eq!(hits.len(), 1);
    assert!(laptop.embedded().contains(&document_text));
    assert!(!internal.embedded().contains(&document_text));
    let query_text = format!("{QUERY_PREFIX}vehicle");
    assert!(internal.embedded().contains(&query_text));
    assert!(!laptop.embedded().contains(&query_text));
}

#[sqlx::test(migrations = false)]
async fn only_admins_switch_configured_teis(pool: PgPool) {
    let tei = FakeTei::start(MAX_INPUT_LENGTH).await;
    let config = embedding_config(model(MODEL, &tei), &[&tei]);
    let server = start_server_with(pool, tei, config).await;
    let cookie = server.sign_in().await;

    let unknown = server
        .graphql(
            &cookie,
            "mutation { updateTei(url: \"http://elsewhere\", input: {enabled: false}) { url } }",
            json!({}),
        )
        .await;

    assert!(
        unknown["errors"][0]["message"]
            .as_str()
            .unwrap()
            .contains("no TEI"),
        "{unknown}"
    );
}

#[sqlx::test(migrations = false)]
async fn an_offline_tei_is_reported_and_the_others_keep_working(pool: PgPool) {
    let tei = FakeTei::start(MAX_INPUT_LENGTH).await;
    let mut config = embedding_config(model(MODEL, &tei), &[&tei]);
    // Nothing listens on port 1.
    config.teis.push(TeiConfig {
        url: "http://127.0.0.1:1".to_owned(),
        external: true,
    });
    let server = start_server_with(pool, tei, config).await;
    let api = server.admin_api().await;
    api.put_document(&document("notes", "car", "My car broke down."))
        .await
        .unwrap();
    server.wait_for_embeddings(1).await;

    let offline = listed(&server, "http://127.0.0.1:1").await;

    assert_eq!(offline["state"], "OFFLINE");
    assert_eq!(offline["model"], Value::Null);
    assert!(offline["problem"].as_str().is_some(), "{offline}");
    assert_eq!(api.search(&query("vehicle")).await.unwrap().hits.len(), 1);
}

#[sqlx::test(migrations = false)]
async fn an_answer_from_another_model_is_discarded_and_the_tei_looked_at_again(pool: PgPool) {
    let tei = FakeTei::start(MAX_INPUT_LENGTH).await;
    let config = embedding_config(model(MODEL, &tei), &[&tei]);
    let server = start_server_with(pool.clone(), tei.clone(), config).await;

    tei.switch_on_next_embedding("other/model");
    server
        .admin_api()
        .await
        .put_document(&document("notes", "car", "My car broke down."))
        .await
        .unwrap();
    let listed = eventually("the TEI looked at again", async || {
        let listed = listed(&server, &tei.url).await;
        // Discovery may see the new model before the pump records its error.
        (listed["state"] == "UNUSED" && listed["lastError"].is_string()).then_some(listed)
    })
    .await;

    assert_eq!(listed["servedModel"], "other/model");
    assert_eq!(listed["model"], Value::Null);
    assert!(
        listed["lastError"].as_str().unwrap().contains("instead of"),
        "{listed}"
    );
    let written: i64 = sqlx::query_scalar(
        "select (select count(*) from chunk_embeddings) + (select count(*) from embedding_cache)",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(written, 0);
}

#[sqlx::test(migrations = false)]
async fn a_tei_serving_an_unconfigured_model_is_unused_until_it_loads_a_configured_one(
    pool: PgPool,
) {
    let internal = FakeTei::start(MAX_INPUT_LENGTH).await;
    let laptop = FakeTei::serving("other/model", MAX_INPUT_LENGTH).await;
    let mut config = embedding_config(model(MODEL, &internal), &[&internal]);
    config.teis.push(tei_config(&laptop));
    let server = start_server_with(pool, internal.clone(), config).await;

    let unused = listed(&server, &laptop.url).await;
    laptop.serve(FAKE_MODEL_ID);
    let online = eventually("the next look at the TEIs", async || {
        let listed = listed(&server, &laptop.url).await;
        (listed["state"] == "ONLINE").then_some(listed)
    })
    .await;

    assert_eq!(unused["state"], "UNUSED");
    assert_eq!(unused["servedModel"], "other/model");
    assert!(
        unused["problem"]
            .as_str()
            .unwrap()
            .contains("no configured model"),
        "{unused}"
    );
    assert_eq!(online["model"], MODEL);
}

#[sqlx::test(migrations = false)]
async fn a_tei_serving_its_model_under_another_name_is_unsuitable(pool: PgPool) {
    let tei = FakeTei::start(MAX_INPUT_LENGTH).await;
    // Under a fixed name, its answers couldn't tell another model loaded meanwhile.
    tei.serve_as("embed");
    let config = embedding_config(model(MODEL, &tei), &[&tei]);
    let server = start_server_with(pool, tei.clone(), config).await;

    let listed = listed(&server, &tei.url).await;

    assert_eq!(listed["state"], "UNSUITABLE");
    assert!(
        listed["problem"]
            .as_str()
            .unwrap()
            .contains("--served-model-name"),
        "{listed}"
    );
}

#[sqlx::test(migrations = false)]
async fn an_answer_of_another_dimension_is_discarded(pool: PgPool) {
    let tei = FakeTei::start(MAX_INPUT_LENGTH).await;
    let mut config = embedding_config(model(MODEL, &tei), &[&tei]);
    // The fake's vectors have 256.
    config.models[0].dimension = 1024;
    let server = start_server_with(pool.clone(), tei.clone(), config).await;
    server
        .admin_api()
        .await
        .put_document(&document("notes", "car", "My car broke down."))
        .await
        .unwrap();

    let error = eventually("the pump's error", async || {
        listed(&server, &tei.url).await["lastError"]
            .as_str()
            .map(str::to_owned)
    })
    .await;

    assert!(error.contains("256 dimensions"), "{error}");
    let written: i64 = sqlx::query_scalar(
        "select (select count(*) from chunk_embeddings) + (select count(*) from embedding_cache)",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(written, 0);
}
