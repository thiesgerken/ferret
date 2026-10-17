mod common;

use common::{
    TestServer, document,
    fake_tei::{FakeTei, MAX_INPUT_LENGTH},
    model, start_server_with,
};
use ferret_api::SearchQuery;
use ferret_server::config::EmbeddingConfig;
use reqwest::StatusCode;
use serde_json::{Value, json};
use sqlx::PgPool;

/// Two models on their own fake TEIs, `alpha` the configured default.
async fn two_models(pool: PgPool) -> (TestServer, FakeTei) {
    let alpha = FakeTei::start(MAX_INPUT_LENGTH).await;
    let beta = FakeTei::start(MAX_INPUT_LENGTH).await;
    let config = EmbeddingConfig {
        default: "alpha".to_owned(),
        models: vec![model("beta", &beta), model("alpha", &alpha)],
    };
    (start_server_with(pool, alpha, config).await, beta)
}

fn query(q: &str, model: Option<&str>) -> SearchQuery {
    SearchQuery {
        q: q.to_owned(),
        model: model.map(str::to_owned),
        ..Default::default()
    }
}

async fn set_default(server: &TestServer, cookie: &str, model: Value) -> Value {
    server
        .graphql(
            cookie,
            "mutation ($model: String) { setDefaultEmbeddingModel(model: $model) }",
            json!({"model": model}),
        )
        .await
}

#[sqlx::test(migrations = false)]
async fn searches_use_the_requested_model_else_the_users_default_else_the_configured_one(
    pool: PgPool,
) {
    let (server, beta) = two_models(pool).await;
    let api = server.admin_api().await;
    let cookie = server.sign_in().await;
    let model_of =
        async |model: Option<&str>| api.search(&query("car", model)).await.unwrap().model;

    assert_eq!(model_of(None).await, "alpha");
    let set = set_default(&server, &cookie, json!("beta")).await;
    assert_eq!(set["data"]["setDefaultEmbeddingModel"], "beta", "{set}");
    assert_eq!(model_of(None).await, "beta");
    assert!(beta.embedded().contains(&"query: car".to_owned()));
    assert_eq!(model_of(Some("alpha")).await, "alpha");
    let graphql = server
        .graphql(
            &cookie,
            "{ me { defaultEmbeddingModel } search(input: {q: \"car\"}) { model } }",
            json!({}),
        )
        .await;
    assert_eq!(
        graphql["data"],
        json!({"me": {"defaultEmbeddingModel": "beta"}, "search": {"model": "beta"}})
    );

    let reset = set_default(&server, &cookie, Value::Null).await;
    assert_eq!(
        reset["data"]["setDefaultEmbeddingModel"],
        Value::Null,
        "{reset}"
    );
    assert_eq!(model_of(None).await, "alpha");
}

#[sqlx::test(migrations = false)]
async fn unknown_models_are_rejected(pool: PgPool) {
    let (server, _beta) = two_models(pool.clone()).await;
    let api = server.admin_api().await;
    let cookie = server.sign_in().await;

    let error = api.search(&query("car", Some("gamma"))).await.unwrap_err();
    let set = set_default(&server, &cookie, json!("gamma")).await;

    assert_eq!(error.status(), Some(StatusCode::BAD_REQUEST));
    assert_eq!(
        set["errors"][0]["message"],
        "no embedding model \"gamma\" is configured"
    );

    // A stale default, as during a rolling deploy before the startup reset.
    sqlx::query("update users set default_embedding_model = 'gamma'")
        .execute(&pool)
        .await
        .unwrap();
    let stale = server
        .graphql(
            &cookie,
            "{ search(input: {q: \"car\"}) { model } }",
            json!({}),
        )
        .await;
    assert_eq!(
        stale["errors"][0]["message"],
        "your default embedding model \"gamma\" is no longer configured"
    );
}

#[sqlx::test(migrations = false)]
async fn models_list_the_users_coverage(pool: PgPool) {
    let (server, _beta) = two_models(pool).await;
    let api = server.admin_api().await;
    api.put_document(&document("notes", "car", "My car broke down."))
        .await
        .unwrap();
    server.wait_for_embeddings(2).await;
    api.put_document(&document("notes", "bike", "My bike too."))
        .await
        .unwrap();
    server.wait_for_embeddings(4).await;
    let cookie = server.sign_in().await;

    let models = server
        .graphql(
            &cookie,
            "{ embeddingModels { name configuredDefault embedded chunks } }",
            json!({}),
        )
        .await;

    assert_eq!(
        models["data"]["embeddingModels"],
        json!([
            {"name": "alpha", "configuredDefault": true, "embedded": 2, "chunks": 2},
            {"name": "beta", "configuredDefault": false, "embedded": 2, "chunks": 2},
        ])
    );
}

#[sqlx::test(migrations = false)]
async fn a_default_model_removed_from_the_config_is_reset_at_startup(pool: PgPool) {
    let (server, _beta) = two_models(pool.clone()).await;
    let cookie = server.sign_in().await;
    set_default(&server, &cookie, json!("beta")).await;
    let alpha = server.tei.clone();
    drop(server);

    let config = EmbeddingConfig {
        default: "alpha".to_owned(),
        models: vec![model("alpha", &alpha)],
    };
    ferret_server::embeddings::prepare(&pool, config)
        .await
        .unwrap();

    let default: Option<String> =
        sqlx::query_scalar("select default_embedding_model from users where username = 'admin'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(default, None);
}
