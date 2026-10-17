mod common;

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    Json, Router,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::get,
};
use common::{TestServer, start_server};
use ferret_api::SearchQuery;
use ferret_plugins::{plugins::PluginOptions, worker::Worker};
use serde_json::{Value, json};
use sqlx::PgPool;

const TOKEN: &str = "paperless-api-token";

/// A Paperless that serves the recorded fixtures, with documents the test can change between
/// runs. It records which documents were fetched in full.
#[derive(Clone)]
struct FakePaperless {
    documents: Arc<Mutex<Vec<Value>>>,
    fetched: Arc<Mutex<Vec<String>>>,
}

fn page(results: Vec<Value>) -> Json<Value> {
    Json(json!({"count": results.len(), "next": null, "previous": null, "results": results}))
}

fn authorized(headers: &HeaderMap) -> bool {
    headers
        .get("authorization")
        .is_some_and(|value| value == format!("Token {TOKEN}").as_str())
}

async fn documents(
    State(fake): State<FakePaperless>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    if !authorized(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let documents = fake.documents.lock().unwrap().clone();
    if query.get("fields").map(String::as_str) == Some("id,modified") {
        return page(
            documents
                .iter()
                .map(|document| json!({"id": document["id"], "modified": document["modified"]}))
                .collect(),
        )
        .into_response();
    }
    let ids = query["id__in"].clone();
    fake.fetched.lock().unwrap().push(ids.clone());
    let ids: Vec<&str> = ids.split(',').collect();
    page(
        documents
            .into_iter()
            .filter(|document| ids.contains(&document["id"].to_string().as_str()))
            .collect(),
    )
    .into_response()
}

fn fixture_results(json: &str) -> Vec<Value> {
    serde_json::from_str::<Value>(json).unwrap()["results"]
        .as_array()
        .unwrap()
        .clone()
}

async fn serve(fake: FakePaperless) -> String {
    let named = |json: &'static str| get(move || async move { page(fixture_results(json)) });
    let app = Router::new()
        .route("/api/documents/", get(documents))
        .route(
            "/api/tags/",
            named(include_str!(
                "../../ferret-plugins/fixtures/paperless/tags.json"
            )),
        )
        .route(
            "/api/correspondents/",
            named(include_str!(
                "../../ferret-plugins/fixtures/paperless/correspondents.json"
            )),
        )
        .route(
            "/api/document_types/",
            named(include_str!(
                "../../ferret-plugins/fixtures/paperless/document_types.json"
            )),
        )
        .with_state(fake);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    url
}

async fn search(server: &TestServer, q: &str) -> Vec<(String, Option<String>, Vec<String>)> {
    server
        .admin_api()
        .await
        .search(&SearchQuery {
            q: q.to_owned(),
            ..Default::default()
        })
        .await
        .unwrap()
        .hits
        .into_iter()
        .map(|hit| (hit.external_id, hit.url, hit.authors))
        .collect()
}

#[sqlx::test(migrations = false)]
async fn paperless_documents_sync_incrementally_and_deleted_ones_disappear(pool: PgPool) {
    let fake = FakePaperless {
        documents: Arc::new(Mutex::new(fixture_results(include_str!(
            "../../ferret-plugins/fixtures/paperless/documents.json"
        )))),
        fetched: Arc::default(),
    };
    let paperless = serve(fake.clone()).await;
    let server = start_server(pool.clone()).await;
    let worker = Worker {
        internal: server.internal(),
        public_url: server.public_url(),
        name: "test-worker".to_owned(),
        options: PluginOptions::default(),
    };
    let worker_id = worker.register().await;
    let cookie = server.sign_in().await;
    let created = server
        .graphql(
            &cookie,
            "mutation ($input: NewConnection!) { createConnection(input: $input) { id config } }",
            json!({"input": {
                "plugin": "paperless",
                "source": "paperless",
                "config": json!({"url": format!("{paperless}/"), "token": TOKEN}).to_string(),
                "schedule": "0 * * * *",
            }}),
        )
        .await;
    assert_eq!(created["errors"], Value::Null, "{created}");
    assert!(!created.to_string().contains(TOKEN));
    let id = created["data"]["createConnection"]["id"].clone();

    assert!(worker.work_once(worker_id, Duration::ZERO).await.unwrap());

    assert_eq!(
        search(&server, "Versicherung").await,
        [(
            "412".to_owned(),
            Some(format!("{paperless}/documents/412/details")),
            vec!["Allianz".to_owned()],
        )]
    );
    assert_eq!(search(&server, "Zählerstand").await.len(), 1);
    let document = server
        .admin_api()
        .await
        .search(&SearchQuery {
            q: "Versicherung".to_owned(),
            ..Default::default()
        })
        .await
        .unwrap()
        .hits[0]
        .document_id;
    let stored = server.admin_api().await.document(document).await.unwrap();
    assert_eq!(stored.meta["tags"], json!(["Versicherung", "Auto"]));
    assert_eq!(stored.meta["archive_serial_number"], 1041);

    // In Paperless, the note is deleted and the insurance letter gets a corrected OCR text.
    {
        let mut documents = fake.documents.lock().unwrap();
        documents.retain(|document| document["id"] != 977);
        documents[0]["content"] = json!("Beitragsrechnung: 398,10 EUR nach Rabatt.");
        documents[0]["modified"] = json!("2025-04-01T10:00:00+02:00");
    }
    server
        .graphql(
            &cookie,
            "mutation ($id: ID!) { runConnectionNow(id: $id) }",
            json!({"id": id}),
        )
        .await;
    assert!(worker.work_once(worker_id, Duration::ZERO).await.unwrap());

    assert_eq!(*fake.fetched.lock().unwrap(), ["412,977", "412"]);
    assert!(search(&server, "Zählerstand").await.is_empty());
    assert_eq!(search(&server, "Rabatt").await.len(), 1);
    let runs = server
        .graphql(
            &cookie,
            "{ connections { runs { status error stats { put unchanged deleted } } } }",
            json!({}),
        )
        .await;
    assert_eq!(
        runs["data"]["connections"][0]["runs"],
        json!([
            {"status": "SUCCEEDED", "error": null, "stats": {"put": 1, "unchanged": 0, "deleted": 1}},
            {"status": "SUCCEEDED", "error": null, "stats": {"put": 2, "unchanged": 0, "deleted": 0}},
        ])
    );
    let state: Value = sqlx::query_scalar("select state from connections")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state, json!({"modified": "2025-04-01T08:00:00Z"}));
}
