mod common;

use std::time::Duration;

use common::{TestServer, document, session_cookie, start_server};
use ferret_api::internal::{PluginRegistration, WorkerRegistration};
use serde_json::{Value, json};
use sqlx::PgPool;

const QUERY: &str = "{ writerStatistics {
    documents chunks storage { documents chunks embeddings indexes total }
    rows { writer { id name kind } sources documents chunks
           storage { documents chunks embeddings indexes total }
           models { model embedded rowBytes indexBytes } }
} explore(input: {}) { total } embeddingModels { chunks } }";

async fn statistics(server: &TestServer, cookie: &str) -> Value {
    let body = server.graphql(cookie, QUERY, json!({})).await;
    assert_eq!(body["errors"], Value::Null, "{body}");
    let result = &body["data"]["writerStatistics"];
    assert_eq!(result["documents"], body["data"]["explore"]["total"]);
    assert_eq!(
        result["chunks"],
        body["data"]["embeddingModels"][0]["chunks"]
    );
    let rows = result["rows"].as_array().unwrap();
    for field in ["documents", "chunks"] {
        assert_eq!(
            rows.iter()
                .map(|row| row[field].as_i64().unwrap())
                .sum::<i64>(),
            result[field].as_i64().unwrap()
        );
    }
    for field in ["documents", "chunks", "embeddings", "indexes", "total"] {
        let sum: f64 = rows
            .iter()
            .map(|row| row["storage"][field].as_f64().unwrap())
            .sum();
        assert!(
            (sum - result["storage"][field].as_f64().unwrap()).abs() < 0.001,
            "{field}"
        );
    }
    for row in rows {
        let storage = &row["storage"];
        let sum: f64 = ["documents", "chunks", "embeddings", "indexes"]
            .iter()
            .map(|field| storage[field].as_f64().unwrap())
            .sum();
        assert!((sum - storage["total"].as_f64().unwrap()).abs() < 0.001);
    }
    result.clone()
}

fn writer<'a>(statistics: &'a Value, name: Option<&str>) -> &'a Value {
    statistics["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["writer"]["name"].as_str() == name)
        .unwrap()
}

async fn notes_run(server: &TestServer, cookie: &str) -> ferret_api::Client {
    let worker = server
        .internal()
        .register(&WorkerRegistration {
            name: "stats-worker".to_owned(),
            plugins: vec![PluginRegistration {
                name: "fake".to_owned(),
                config_schema: json!({"type": "object"}),
            }],
        })
        .await
        .unwrap()
        .worker_id;
    let created = server.graphql(cookie,
        "mutation { createConnection(input: {name: \"Notes\", icon: \"notebook\", plugin: \"fake\", source: \"notes\", config: \"{}\", schedule: \"0 * * * *\"}) { id } }", json!({})).await;
    assert_eq!(created["errors"], Value::Null, "{created}");
    let run = server
        .internal()
        .claim(worker, Duration::ZERO)
        .await
        .unwrap()
        .unwrap();
    server.api_with_key(&run.token)
}

#[sqlx::test(migrations = false)]
async fn counts_current_documents_once_and_tracks_attribution_updates_and_deletions(pool: PgPool) {
    let server = start_server(pool.clone()).await;
    let cookie = server.sign_in().await;
    let empty = statistics(&server, &cookie).await;
    assert_eq!(empty["documents"], 0);
    assert_eq!(empty["storage"]["total"], 0.0);
    assert_eq!(empty["rows"], json!([]));

    let run = notes_run(&server, &cookie).await;
    for source in ["notes", "notes/nested", "notes/nested/deeper"] {
        run.put_document(&document(source, source, "One short passage."))
            .await
            .unwrap();
    }
    let key = server.create_api_key(&["INGEST_WRITE"], None).await;
    let api = server.api_with_key(&key);
    let original = document("inbox", "a", "A complete document.");
    api.put_document(&original).await.unwrap();
    api.put_document(&document("inbox", "empty", ""))
        .await
        .unwrap();
    api.put_document(&document("legacy", "unknown", "Retained content."))
        .await
        .unwrap();
    sqlx::query("update documents set api_key_id = null where source = 'legacy'")
        .execute(&pool)
        .await
        .unwrap();
    let before = statistics(&server, &cookie).await;
    assert_eq!(before["documents"], 6);
    assert_eq!(before["chunks"], 5);
    let notes = writer(&before, Some("Notes"));
    assert_eq!(notes["documents"], 3);
    assert_eq!(notes["chunks"], 3);
    assert_eq!(
        notes["sources"],
        json!(["notes", "notes/nested", "notes/nested/deeper"])
    );
    assert_eq!(writer(&before, None)["documents"], 1);
    let key_row = writer(&before, Some("test key"));
    assert_eq!(key_row["documents"], 2);
    assert_eq!(key_row["chunks"], 1);
    assert!(
        key_row["storage"]["documents"].as_f64().unwrap()
            > key_row["storage"]["chunks"].as_f64().unwrap()
    );

    let second_key = server.create_api_key(&["INGEST_WRITE"], None).await;
    let second = server.api_with_key(&second_key);
    // An unchanged put still moves current attribution, without rechunking.
    second.put_document(&original).await.unwrap();
    let keys = server
        .graphql(&cookie, "{ apiKeys { id } }", json!({}))
        .await;
    let ids = keys["data"]["apiKeys"].as_array().unwrap();
    for (index, id) in ids.iter().enumerate() {
        let renamed = server.graphql(&cookie, "mutation ($id: ID!, $name: String!) { updateApiKey(id: $id, name: $name, icon: \"key-round\") { id } }", json!({"id": id["id"], "name": format!("Key {index}")})).await;
        assert_eq!(renamed["errors"], Value::Null, "{renamed}");
    }
    let moved = statistics(&server, &cookie).await;
    assert_eq!(moved["rows"].as_array().unwrap().len(), 4);
    assert_eq!(moved["documents"], before["documents"]);
    assert_eq!(moved["chunks"], before["chunks"]);
    let moved_key = moved["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["sources"] == json!(["inbox"]) && row["chunks"] == 1)
        .unwrap();
    assert_eq!(moved_key["documents"], 1);
    let revoked = server
        .graphql(
            &cookie,
            "mutation ($id: ID!) { revokeApiKey(id: $id) }",
            json!({"id": moved_key["writer"]["id"]}),
        )
        .await;
    assert_eq!(revoked["errors"], Value::Null, "{revoked}");
    let retained = statistics(&server, &cookie).await;
    assert_eq!(
        retained["rows"], moved["rows"],
        "revoked metadata remains attribution"
    );

    let mut changed = original.clone();
    changed.text = "A substantially longer new body. ".repeat(250);
    api.put_document(&changed).await.unwrap();
    let updated = statistics(&server, &cookie).await;
    assert!(updated["chunks"].as_i64().unwrap() > before["chunks"].as_i64().unwrap());
    assert!(
        updated["storage"]["documents"].as_f64().unwrap()
            > before["storage"]["documents"].as_f64().unwrap()
    );
    api.delete_document("inbox", "a").await.unwrap();
    let deleted = statistics(&server, &cookie).await;
    assert_eq!(deleted["documents"], 5);
    assert_eq!(deleted["chunks"], 4);
    let empty_row = deleted["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["sources"] == json!(["inbox"]))
        .unwrap();
    assert_eq!(empty_row["chunks"], 0);
    assert!(empty_row["storage"]["documents"].as_f64().unwrap() > 0.0);
    assert_eq!(empty_row["storage"]["chunks"], 0.0);
    let mut metadata_only = document("inbox", "empty", "");
    metadata_only.title = Some("A stored title ".repeat(100));
    metadata_only.meta.insert(
        "description".to_owned(),
        json!("Stored metadata ".repeat(100)),
    );
    api.put_document(&metadata_only).await.unwrap();
    let metadata_updated = statistics(&server, &cookie).await;
    assert_eq!(metadata_updated["chunks"], deleted["chunks"]);
    assert!(
        metadata_updated["storage"]["documents"].as_f64().unwrap()
            > deleted["storage"]["documents"].as_f64().unwrap() + 2000.0
    );
}

#[sqlx::test(migrations = false)]
async fn all_models_and_shared_indexes_are_allocated_without_cross_user_content_or_cache(
    pool: PgPool,
) {
    let server = start_server(pool.clone()).await;
    let cookie = server.sign_in().await;
    let api = server.admin_api().await;
    api.put_document(&document("inbox", "a", "Shared words."))
        .await
        .unwrap();
    let created = server.graphql(&cookie, "mutation { createUser(input: {username: \"bob\", password: \"a long user password\", isAdmin: false}) { id } }", json!({})).await;
    assert_eq!(created["errors"], Value::Null, "{created}");
    let bob = session_cookie(&server.login("bob", "a long user password").await);
    let key = server.graphql(&bob, "mutation { createApiKey(input: {name: \"Private writer\", icon: \"key-round\", scopes: [INGEST_WRITE]}) { key } }", json!({})).await;
    assert_eq!(key["errors"], Value::Null, "{key}");
    server
        .api_with_key(key["data"]["createApiKey"]["key"].as_str().unwrap())
        .put_document(&document("private", "b", "Shared words."))
        .await
        .unwrap();
    let before = statistics(&server, &cookie).await;
    sqlx::query("insert into embedding_models (id, dimension) values ('retired', 3), ('other', 3)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("insert into chunk_embeddings (chunk_id, model, document_id, user_id, source, content_type, ts, embedding)
        select c.id, m.id, c.document_id, c.user_id, c.source, c.content_type, c.ts, '[1,2,3]'::halfvec
        from chunks c cross join embedding_models m where m.id in ('retired', 'other')").execute(&pool).await.unwrap();
    sqlx::query("create index chunk_embeddings_hnsw_retired on chunk_embeddings using hnsw ((embedding::halfvec(3)) halfvec_cosine_ops) where model = 'retired'").execute(&pool).await.unwrap();
    sqlx::query("create index chunk_embeddings_hnsw_other on chunk_embeddings using hnsw ((embedding::halfvec(3)) halfvec_cosine_ops) where model = 'other'").execute(&pool).await.unwrap();
    let after = statistics(&server, &cookie).await;
    assert_eq!(after["documents"], before["documents"]);
    assert_eq!(after["chunks"], before["chunks"]);
    let row = writer(&after, Some("test key"));
    assert_eq!(row["models"].as_array().unwrap().len(), 2);
    let index_bytes: i64 =
        sqlx::query_scalar("select pg_relation_size('chunk_embeddings_hnsw_retired')")
            .fetch_one(&pool)
            .await
            .unwrap();
    let retired = row["models"]
        .as_array()
        .unwrap()
        .iter()
        .find(|model| model["model"] == "retired")
        .unwrap();
    assert_eq!(retired["embedded"], 1);
    assert_eq!(
        retired["indexBytes"].as_f64().unwrap(),
        index_bytes as f64 / 2.0
    );
    assert!(after["storage"]["embeddings"].as_f64().unwrap() > 0.0);
    assert!(after["storage"]["indexes"].as_f64().unwrap() > 0.0);
    let bobs = statistics(&server, &bob).await;
    assert_eq!(bobs["documents"], 1);
    assert_eq!(bobs["rows"][0]["writer"]["name"], "Private writer");
    assert_eq!(bobs["rows"][0]["sources"], json!(["private"]));
    assert!(!after.to_string().contains("Private writer"));
    assert!(!bobs.to_string().contains("test key"));

    sqlx::query("insert into embedding_cache (text_hash, model, embedding) values ('\\x00', 'retired', '[1,2,3]')").execute(&pool).await.unwrap();
    let cached = statistics(&server, &cookie).await;
    assert_eq!(
        cached, after,
        "shared/query cache contributes no writer bytes"
    );
    api.delete_document("inbox", "a").await.unwrap();
    let deleted = statistics(&server, &cookie).await;
    assert_eq!(deleted["documents"], 0);
    assert_eq!(deleted["storage"]["total"], 0.0);
    assert_eq!(deleted["rows"], json!([]));
    assert_eq!(
        sqlx::query_scalar::<_, i64>("select count(*) from embedding_cache")
            .fetch_one(&pool)
            .await
            .unwrap(),
        1
    );
}

#[sqlx::test(migrations = false)]
async fn the_size_migration_backfills_existing_documents_chunks_and_embeddings(pool: PgPool) {
    let server = start_server(pool.clone()).await;
    server
        .admin_api()
        .await
        .put_document(&document("notes", "a", "Existing text."))
        .await
        .unwrap();
    sqlx::query("insert into chunk_embeddings (chunk_id, model, document_id, user_id, source, content_type, ts, embedding)
        select id, 'fake', document_id, user_id, source, content_type, ts, array_fill(1::real, array[256])::halfvec from chunks")
        .execute(&pool).await.unwrap();
    // Recreate the previous schema, retaining its real corpus, before applying the new migration.
    for statement in [
        "drop trigger documents_storage_bytes on documents",
        "drop trigger chunks_storage_bytes on chunks",
        "drop trigger embeddings_storage_bytes on chunk_embeddings",
        "alter table documents drop column storage_bytes",
        "alter table chunks drop column storage_bytes, drop column indexed_text_bytes",
        "alter table chunk_embeddings drop column storage_bytes",
        "drop function estimate_content_row_bytes()",
        "delete from _sqlx_migrations where description = 'writer storage sizes'",
    ] {
        sqlx::query(statement).execute(&pool).await.unwrap();
    }
    ferret_server::migrate(&pool).await.unwrap();
    let cookie = server.sign_in().await;
    let result = statistics(&server, &cookie).await;
    assert_eq!(result["documents"], 1);
    assert_eq!(result["chunks"], 1);
    for part in ["documents", "chunks", "embeddings", "indexes"] {
        assert!(
            result["storage"][part].as_f64().unwrap() > 0.0,
            "{part}: {result}"
        );
    }
}
