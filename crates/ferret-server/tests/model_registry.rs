use crate::common;

use common::{
    MODEL, TestServer, document, embedding_config,
    fake_endpoint::{DIMENSION, FakeEndpoint},
    model, start_semantic_server, start_server, start_server_with,
};
use ferret_server::config::EmbeddingConfig;
use serde_json::{Value, json};
use sqlx::{AssertSqlSafe, PgPool};

const STORED: &str = "storedEmbeddingModels {
    id dimension configured embedded indexBytes rowBytesEstimate
}";

async fn registry(pool: &PgPool) -> Vec<(String, i32)> {
    sqlx::query_as("select id, dimension from embedding_models order by id")
        .fetch_all(pool)
        .await
        .unwrap()
}

async fn embeddings_of(pool: &PgPool, table: &str, model: &str) -> i64 {
    sqlx::query_scalar(AssertSqlSafe(format!(
        "select count(*) from {table} where model = $1"
    )))
    .bind(model)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn index_exists(pool: &PgPool, model: &str) -> bool {
    sqlx::query_scalar("select to_regclass('chunk_embeddings_hnsw_' || $1) is not null")
        .bind(model)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// `alpha` and `beta` embed two documents each, then the server restarts with `alpha` only:
/// `beta`'s embeddings stay behind.
async fn beta_left_the_config(pool: PgPool) -> TestServer {
    let alpha = FakeEndpoint::start().await;
    let beta = FakeEndpoint::start().await;
    let both = EmbeddingConfig {
        default: "alpha".to_owned(),
        models: vec![model("alpha", &alpha), model("beta", &beta)],
    };
    let server = start_server_with(pool.clone(), alpha.clone(), both).await;
    let api = server.admin_api().await;
    for (id, text) in [("car", "My car broke down."), ("bike", "The bike is fine.")] {
        api.put_document(&document("notes", id, text))
            .await
            .unwrap();
    }
    server.wait_for_embeddings(4).await;
    drop(server);
    let alpha_only = embedding_config(model("alpha", &alpha));
    start_server_with(pool, alpha, alpha_only).await
}

#[sqlx::test(migrations = false)]
async fn the_migration_registers_every_model_with_stored_embeddings(pool: PgPool) {
    let server = start_semantic_server(pool.clone()).await;
    server
        .admin_api()
        .await
        .put_document(&document("notes", "car", "My car broke down."))
        .await
        .unwrap();
    server.wait_for_embeddings(1).await;
    drop(server);
    // Back to the state before the registry, with the data the migration finds on prod.
    sqlx::query("drop table embedding_models cascade")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("delete from _sqlx_migrations where description = 'embedding models'")
        .execute(&pool)
        .await
        .unwrap();
    // A model that only cached a query embedding before it left the config, and one that only
    // ever failed.
    sqlx::query(
        "insert into embedding_cache (text_hash, model, embedding) values ('\\x00', 'retired', '[1,2,3]')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "insert into embedding_failures (chunk_id, model, error, at)
         select id, 'refused', 'too long', now() from chunks limit 1",
    )
    .execute(&pool)
    .await
    .unwrap();

    ferret_server::migrate(&pool).await.unwrap();

    assert_eq!(
        registry(&pool).await,
        [
            (MODEL.to_owned(), DIMENSION as i32),
            ("retired".to_owned(), 3)
        ]
    );
    assert_eq!(
        embeddings_of(&pool, "embedding_failures", "refused").await,
        0
    );
}

#[sqlx::test(migrations = false)]
async fn startup_registers_configured_models(pool: PgPool) {
    let _server = start_server(pool.clone()).await;

    assert_eq!(
        registry(&pool).await,
        [(MODEL.to_owned(), DIMENSION as i32)]
    );
}

#[sqlx::test(migrations = false)]
async fn a_dimension_that_contradicts_the_registry_stops_the_server_even_without_embeddings(
    pool: PgPool,
) {
    let server = start_server(pool.clone()).await;
    let endpoint = server.endpoint.clone();
    drop(server);

    let mut config = embedding_config(model(MODEL, &endpoint));
    config.models[0].dimension = 1024;
    let error = ferret_server::embeddings::prepare(&pool, config)
        .await
        .err()
        .unwrap();

    let message = error.to_string();
    assert!(
        message.contains(&format!("embedding model {MODEL}")),
        "{message}"
    );
    assert!(message.contains("256 dimensions"), "{message}");
}

#[sqlx::test(migrations = false)]
async fn a_model_whose_index_name_another_stored_model_has_stops_the_server(pool: PgPool) {
    let endpoint = FakeEndpoint::start().await;
    let server = start_server_with(
        pool.clone(),
        endpoint.clone(),
        embedding_config(model("a.b", &endpoint)),
    )
    .await;
    drop(server);

    let error =
        ferret_server::embeddings::prepare(&pool, embedding_config(model("a-b", &endpoint)))
            .await
            .err()
            .unwrap();

    assert!(error.to_string().contains("a.b"), "{error}");
    assert_eq!(
        registry(&pool).await,
        [("a.b".to_owned(), DIMENSION as i32)],
        "nothing recorded"
    );
}

#[sqlx::test(migrations = false)]
async fn admin_lists_every_stored_model_with_its_storage(pool: PgPool) {
    let server = beta_left_the_config(pool).await;
    let cookie = server.sign_in().await;

    let response = server
        .graphql(&cookie, &format!("{{ {STORED} }}"), json!({}))
        .await;

    let models = response["data"]["storedEmbeddingModels"]
        .as_array()
        .unwrap();
    let ids: Vec<&Value> = models.iter().map(|model| &model["id"]).collect();
    assert_eq!(ids, ["alpha", "beta"]);
    for (model, configured) in models.iter().zip([true, false]) {
        assert_eq!(model["configured"], configured);
        assert_eq!(model["dimension"], DIMENSION);
        assert_eq!(model["embedded"], 2);
        assert!(model["indexBytes"].as_f64().unwrap() > 0.0, "{model}");
        assert_eq!(
            model["rowBytesEstimate"],
            2.0 * (8.0 + 2.0 * DIMENSION as f64)
        );
    }
}

#[sqlx::test(migrations = false)]
async fn deleting_a_model_that_isnt_configured_removes_exactly_its_data(pool: PgPool) {
    let server = beta_left_the_config(pool.clone()).await;
    let cookie = server.sign_in().await;
    // A failure record of beta's, on one of the shared chunks.
    sqlx::query(
        "insert into embedding_failures (chunk_id, model, error, at)
         select id, 'beta', 'too long', now() from chunks limit 1",
    )
    .execute(&pool)
    .await
    .unwrap();
    let chunks: i64 = sqlx::query_scalar("select count(*) from chunks")
        .fetch_one(&pool)
        .await
        .unwrap();

    let before = server
        .graphql(
            &cookie,
            &format!("{{ {STORED} writerStatistics {{ rows {{ models {{ model }} }} }} }}"),
            json!({}),
        )
        .await;
    assert!(
        before["data"]["storedEmbeddingModels"]
            .as_array()
            .unwrap()
            .iter()
            .any(|model| model["id"] == "beta")
    );

    let response = server
        .graphql(
            &cookie,
            "mutation { deleteEmbeddingModel(id: \"beta\") }",
            json!({}),
        )
        .await;

    assert_eq!(
        response["data"]["deleteEmbeddingModel"], "beta",
        "{response}"
    );
    for table in ["chunk_embeddings", "embedding_cache", "embedding_failures"] {
        assert_eq!(embeddings_of(&pool, table, "beta").await, 0, "{table}");
    }
    assert!(!index_exists(&pool, "beta").await);
    assert_eq!(
        registry(&pool).await,
        [("alpha".to_owned(), DIMENSION as i32)]
    );
    assert_eq!(embeddings_of(&pool, "chunk_embeddings", "alpha").await, 2);
    assert!(embeddings_of(&pool, "embedding_cache", "alpha").await > 0);
    assert!(index_exists(&pool, "alpha").await);
    let chunks_after: i64 = sqlx::query_scalar("select count(*) from chunks")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(chunks_after, chunks);
    let after = server
        .graphql(
            &cookie,
            &format!("{{ {STORED} writerStatistics {{ rows {{ models {{ model }} }} }} }}"),
            json!({}),
        )
        .await;
    assert_eq!(after["errors"], Value::Null, "{after}");
    assert!(!after["data"].to_string().contains("beta"), "{after}");
}

/// A replica still configured for a deleted model (a rolling deploy) can't leave rows behind.
#[sqlx::test(migrations = false)]
async fn embeddings_of_a_model_that_isnt_registered_are_refused(pool: PgPool) {
    let server = beta_left_the_config(pool.clone()).await;
    let cookie = server.sign_in().await;
    server
        .graphql(
            &cookie,
            "mutation { deleteEmbeddingModel(id: \"beta\") }",
            json!({}),
        )
        .await;

    let late_write = sqlx::query(
        "insert into chunk_embeddings
             (chunk_id, model, document_id, user_id, source, content_type, ts, embedding)
         select chunk_id, 'beta', document_id, user_id, source, content_type, ts, embedding
         from chunk_embeddings where model = 'alpha' limit 1",
    )
    .execute(&pool)
    .await;

    let error = late_write.unwrap_err();
    assert!(
        error
            .as_database_error()
            .is_some_and(|error| error.is_foreign_key_violation()),
        "{error}"
    );
}

#[sqlx::test(migrations = false)]
async fn a_configured_model_cant_be_deleted(pool: PgPool) {
    let server = beta_left_the_config(pool.clone()).await;
    let cookie = server.sign_in().await;

    let response = server
        .graphql(
            &cookie,
            "mutation { deleteEmbeddingModel(id: \"alpha\") }",
            json!({}),
        )
        .await;

    assert_eq!(
        response["errors"][0]["extensions"]["code"], "INVALID_INPUT",
        "{response}"
    );
    assert_eq!(embeddings_of(&pool, "chunk_embeddings", "alpha").await, 2);
    assert!(index_exists(&pool, "alpha").await);
}
