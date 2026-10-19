mod common;

use std::{collections::HashMap, sync::Arc, time::Duration};

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
};
use common::{TestServer, start_server};
use ferret_api::{Document, SearchQuery};
use ferret_plugins::{
    plugins::{self, Log, PluginOptions, Run},
    worker::Worker,
};
use parking_lot::Mutex;
use serde_json::{Value, json};
use sqlx::PgPool;

const TOKEN: &str = "karakeep-secret-token";
const CREATED: &str = "2026-01-01T12:00:00Z";

#[derive(Clone, Copy, Debug)]
enum Failure {
    InventoryHttp,
    InventoryMissingCursor,
    InventoryLoop,
    InventoryDuplicate,
    InventoryEmptyContinuation,
    ContentHttp,
    ContentChangedHttp,
    ContentChangedVersion,
    ContentMissingCursor,
    ContentLoop,
    ContentGap,
    ContentTruncated,
    ContentWrongBookmark,
    HighlightsForbidden,
    HighlightsMalformed,
    HighlightsWrongBookmark,
    HighlightsRedirect,
}

struct Origin {
    bookmarks: Vec<Value>,
    bodies: HashMap<String, String>,
    highlights: HashMap<String, Vec<Value>>,
    failure: Option<Failure>,
}

type FakeKarakeep = Arc<Mutex<Origin>>;

fn bookmark(id: &str, title: &str, content: Value) -> Value {
    json!({
        "id": id, "title": title, "createdAt": CREATED, "modifiedAt": null,
        "archived": false, "favourited": false,
        "note": "Bookmarknotesentinel", "summary": "AIsummarysentinel",
        "tags": [{"name": "Manualtagsentinel", "attachedBy": "human"},
                 {"name": "AItagsentinel", "attachedBy": "ai"}],
        "content": content,
    })
}

fn origin() -> FakeKarakeep {
    let mut pdf = bookmark(
        "b3",
        "PDFsentinel",
        json!({
            "type": "asset", "assetType": "pdf", "assetId": "pdf-asset",
            "fileName": "archive.pdf", "sourceUrl": "https://source.example.test/archive.pdf",
        }),
    );
    pdf["archived"] = json!(true);
    Arc::new(Mutex::new(Origin {
        bookmarks: vec![
            bookmark(
                "b1",
                "Linksentinel",
                json!({
                    "type": "link", "url": "https://source.example.test/article", "author": "Alice",
                    "htmlContent": "Forbiddenhtmlsentinel", "title": "Extracted title",
                }),
            ),
            bookmark(
                "b2",
                "Textsentinel",
                json!({"type": "text", "text": "DoNotUseInlineTextsentinel"}),
            ),
            pdf,
            bookmark(
                "b4",
                "Imagesentinel",
                json!({
                    "type": "asset", "assetType": "image", "assetId": "image-asset", "fileName": "receipt.png",
                }),
            ),
            bookmark(
                "b5",
                "Sparsesentinel",
                json!({"type": "link", "url": "https://source.example.test/pending"}),
            ),
        ],
        bodies: HashMap::from([
            (
                "b1".to_owned(),
                format!(
                    "Bodysentinel {} Tailbeyondchunksentinel",
                    "🦦cobalt ".repeat(2_000)
                ),
            ),
            ("b2".to_owned(), "Textbodysentinel".to_owned()),
            (
                "b3".to_owned(),
                format!(
                    "PDFbodysentinel {} PDFtailsentinel",
                    "archive ".repeat(4_000)
                ),
            ),
            ("b4".to_owned(), "OCRimagesentinel".to_owned()),
            ("b5".to_owned(), String::new()),
        ]),
        highlights: HashMap::from([(
            "b3".to_owned(),
            vec![json!({
                "id": "h1", "bookmarkId": "b3", "createdAt": "2026-01-03T12:00:00Z",
                "text": "Highlightquotesentinel", "note": "Highlightnotesentinel",
            })],
        )]),
        failure: None,
    }))
}

async fn endpoint(
    State(fake): State<FakeKarakeep>,
    Path(path): Path<String>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    if headers
        .get("authorization")
        .is_none_or(|value| value != format!("Bearer {TOKEN}").as_str())
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let origin = fake.lock();
    if path == "bookmarks" {
        // Real Karakeep uses the first bookmark of the NEXT page, not the last returned one.
        let start = query.get("cursor").map_or(0, |cursor| {
            origin
                .bookmarks
                .iter()
                .position(|bookmark| bookmark["id"] == *cursor)
                .unwrap()
        });
        if start > 0 && matches!(origin.failure, Some(Failure::InventoryHttp)) {
            return (StatusCode::BAD_GATEWAY, TOKEN).into_response();
        }
        assert!(!query.contains_key("archived"));
        assert!(!query.contains_key("favourited"));
        assert!(!query.contains_key("listId"));
        assert!(!query.contains_key("tagId"));
        let mut bookmarks: Vec<_> = origin
            .bookmarks
            .iter()
            .skip(start)
            .take(2)
            .cloned()
            .collect();
        let next = origin
            .bookmarks
            .get(start + 2)
            .map(|bookmark| bookmark["id"].clone());
        let mut page = json!({"bookmarks": bookmarks, "nextCursor": next});
        if start > 0 {
            match origin.failure {
                Some(Failure::InventoryMissingCursor) => {
                    page.as_object_mut().unwrap().remove("nextCursor");
                }
                Some(Failure::InventoryLoop) => {
                    page["nextCursor"] = json!("b3");
                }
                Some(Failure::InventoryDuplicate) => {
                    bookmarks = vec![origin.bookmarks[0].clone()];
                    page["bookmarks"] = json!(bookmarks);
                }
                Some(Failure::InventoryEmptyContinuation) => {
                    page["bookmarks"] = json!([]);
                    page["nextCursor"] = json!("b3");
                }
                _ => {}
            }
        }
        return Json(page).into_response();
    }
    let parts: Vec<_> = path.split('/').collect();
    let ["bookmarks", id, operation] = parts.as_slice() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if *operation == "highlights" {
        if *id == "b3" {
            match origin.failure {
                Some(Failure::HighlightsForbidden) => return (StatusCode::FORBIDDEN, TOKEN).into_response(),
                Some(Failure::HighlightsMalformed) => return Json(json!({"error": TOKEN})).into_response(),
                Some(Failure::HighlightsWrongBookmark) => return Json(json!({"highlights": [{
                    "id": "h1", "bookmarkId": "other", "createdAt": CREATED, "text": TOKEN, "note": null,
                }]})).into_response(),
                Some(Failure::HighlightsRedirect) => return (
                    StatusCode::FOUND, [("location", format!("https://login.example.test/?token={TOKEN}"))],
                ).into_response(),
                _ => {}
            }
        }
        return Json(
            json!({"highlights": origin.highlights.get(*id).cloned().unwrap_or_default()}),
        )
        .into_response();
    }
    if *operation == "content" {
        assert_eq!(query.get("format").map(String::as_str), Some("text"));
        let start: usize = query
            .get("cursor")
            .map_or(0, |cursor| cursor.parse().unwrap());
        if *id == "b3" && start > 0 {
            match origin.failure {
                Some(Failure::ContentHttp) => {
                    return (StatusCode::SERVICE_UNAVAILABLE, TOKEN).into_response();
                }
                Some(Failure::ContentChangedHttp) => {
                    return (StatusCode::CONFLICT, TOKEN).into_response();
                }
                _ => {}
            }
        }
        let body = &origin.bodies[*id];
        let total = body.chars().count();
        let content: String = body.chars().skip(start).take(12_000).collect();
        let end = start + content.chars().count();
        let kind = origin
            .bookmarks
            .iter()
            .find(|bookmark| bookmark["id"] == *id)
            .unwrap()["content"]["type"]
            .clone();
        let mut page = json!({
            "bookmarkId": id, "bookmarkType": kind, "format": "text", "content": content,
            "contentVersion": "sha256:stable-version", "range": {"start": start, "end": end, "total": total},
            "nextCursor": (end < total).then(|| end.to_string()), "truncated": end < total,
        });
        if *id == "b3" && start > 0 {
            match origin.failure {
                Some(Failure::ContentChangedVersion) => {
                    page["contentVersion"] = json!("sha256:changed")
                }
                Some(Failure::ContentMissingCursor) => {
                    page.as_object_mut().unwrap().remove("nextCursor");
                }
                Some(Failure::ContentLoop) => {
                    page["nextCursor"] = json!(start.to_string());
                    page["truncated"] = json!(true);
                }
                Some(Failure::ContentGap) => page["range"]["start"] = json!(start + 1),
                Some(Failure::ContentTruncated) => {
                    page["content"] = json!("");
                    page["nextCursor"] = Value::Null;
                }
                Some(Failure::ContentWrongBookmark) => page["bookmarkId"] = json!("other"),
                _ => {}
            }
        }
        return Json(page).into_response();
    }
    StatusCode::NOT_FOUND.into_response()
}

async fn serve(fake: FakeKarakeep) -> String {
    let app = Router::new()
        .route("/api/v1/{*path}", get(endpoint))
        .with_state(fake);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    url
}

async fn run(server: &TestServer, url: &str) -> Run {
    Run {
        api: server.admin_api().await,
        source: "saved/karakeep".to_owned(),
        config: json!({"url": format!("{url}/"), "public_url": "https://saved.example.test/library/", "token": TOKEN}),
        state: json!({}),
        log: Log::default(),
        options: PluginOptions::default(),
    }
}

async fn find(server: &TestServer, query: &str) -> Vec<Document> {
    let api = server.admin_api().await;
    let hits = api
        .search(&SearchQuery {
            q: query.to_owned(),
            ..Default::default()
        })
        .await
        .unwrap()
        .hits;
    let mut documents = Vec::new();
    for hit in hits {
        documents.push(api.document(hit.document_id).await.unwrap());
    }
    documents
}

#[sqlx::test(migrations = false)]
async fn clearing_custom_titles_preserves_searchable_origin_titles_and_document_identity(
    pool: PgPool,
) {
    let fake = origin();
    {
        let mut origin = fake.lock();
        origin
            .bookmarks
            .retain(|bookmark| bookmark["id"] == "b1" || bookmark["id"] == "b3");
        origin.bookmarks[0]["title"] = json!("Customarticlesentinel");
        origin.bookmarks[0]["content"]["title"] = json!("Fallbackarticlesentinel");
        origin.bookmarks[1]["title"] = json!("Customuploadsentinel");
        origin.bookmarks[1]["content"]["fileName"] = json!("Fallbackuploadsentinel.pdf");
    }
    let url = serve(fake.clone()).await;
    let server = start_server(pool).await;
    let run = run(&server, &url).await;
    plugins::run("karakeep", &run).await.unwrap();
    let article = find(&server, "Customarticlesentinel").await.remove(0);
    let upload = find(&server, "Customuploadsentinel").await.remove(0);
    assert_eq!(article.title.as_deref(), Some("Customarticlesentinel"));
    assert_eq!(upload.title.as_deref(), Some("Customuploadsentinel"));
    {
        let mut origin = fake.lock();
        for bookmark in &mut origin.bookmarks {
            bookmark["title"] = json!("");
        }
    }
    let changed = plugins::run("karakeep", &run).await.unwrap();
    assert_eq!(changed.stats.put, 2);
    let articles = find(&server, "Fallbackarticlesentinel").await;
    assert_eq!(
        articles.len(),
        1,
        "the extracted title must remain searchable after clearing its override"
    );
    assert_eq!(articles[0].id, article.id);
    assert_eq!(
        articles[0].title.as_deref(),
        Some("Fallbackarticlesentinel")
    );
    let uploads = find(&server, "Fallbackuploadsentinel").await;
    assert_eq!(uploads.len(), 1);
    assert_eq!(uploads[0].id, upload.id);
    assert_eq!(
        uploads[0].title.as_deref(),
        Some("Fallbackuploadsentinel.pdf")
    );
    let unchanged = plugins::run("karakeep", &run).await.unwrap();
    assert_eq!((unchanged.stats.put, unchanged.stats.unchanged), (0, 2));
}

#[sqlx::test(migrations = false)]
async fn readable_text_and_annotations_cover_all_types_and_reconcile_without_timestamp_caches(
    pool: PgPool,
) {
    let fake = origin();
    let url = serve(fake.clone()).await;
    let server = start_server(pool).await;
    let mut run = run(&server, &url).await;
    let first = plugins::run("karakeep", &run).await.unwrap();
    assert_eq!(first.stats.put, 5);
    for (word, id) in [
        ("Tailbeyondchunksentinel", "b1"),
        ("Textbodysentinel", "b2"),
        ("PDFtailsentinel", "b3"),
        ("OCRimagesentinel", "b4"),
        ("Sparsesentinel", "b5"),
        ("Highlightquotesentinel", "b3"),
        ("Highlightnotesentinel", "b3"),
    ] {
        assert_eq!(find(&server, word).await[0].external_id, id, "{word}");
    }
    assert_eq!(find(&server, "AItagsentinel").await.len(), 5);
    assert_eq!(find(&server, "Manualtagsentinel").await.len(), 5);
    assert_eq!(find(&server, "Bookmarknotesentinel").await.len(), 5);
    for excluded in [
        "AIsummarysentinel",
        "Forbiddenhtmlsentinel",
        "DoNotUseInlineTextsentinel",
    ] {
        assert!(find(&server, excluded).await.is_empty(), "{excluded}");
    }
    let original = find(&server, "PDFsentinel").await.remove(0);
    assert_eq!(original.source, "saved/karakeep");
    assert_eq!(
        original.url.as_deref(),
        Some("https://saved.example.test/library/dashboard/preview/b3")
    );
    assert_eq!(original.meta["archived"], true);
    assert_eq!(original.meta["favourited"], false);
    assert_eq!(original.meta["bookmark_type"], "asset");
    assert_eq!(original.meta["asset_type"], "pdf");
    assert_eq!(original.meta["file_name"], "archive.pdf");
    assert_eq!(original.created_at, Some(common::day(1)));
    assert_eq!(original.modified_at, Some(common::day(3)));
    let link = find(&server, "Linksentinel").await.remove(0);
    assert_eq!(link.authors, ["Alice"]);
    assert!(link.text.contains("🦦cobalt"));
    assert!(!link.text.contains("AIsummarysentinel"));
    assert!(link.meta.get("summary").is_none());
    let unchanged = plugins::run("karakeep", &run).await.unwrap();
    assert_eq!(unchanged.stats.put, 0);
    assert_eq!(unchanged.stats.unchanged, 5);
    {
        let mut origin = fake.lock();
        origin
            .bodies
            .insert("b5".to_owned(), "Completedextractionsentinel".to_owned());
        origin.bookmarks[2]["note"] = json!("Editedbookmarknotesentinel");
        origin.bookmarks[2]["tags"] = json!([{"name": "Renamedtagsentinel", "attachedBy": "ai"}]);
        origin.highlights.get_mut("b3").unwrap()[0]["note"] = json!("Revisedmarginalia");
        origin
            .bodies
            .insert("b3".to_owned(), "EditedPDFbodysentinel".to_owned());
        origin.bookmarks.retain(|bookmark| bookmark["id"] != "b4");
    }
    let changed = plugins::run("karakeep", &run).await.unwrap();
    assert_eq!(
        (
            changed.stats.put,
            changed.stats.unchanged,
            changed.stats.deleted
        ),
        (2, 2, 1)
    );
    for word in [
        "Editedbookmarknotesentinel",
        "Renamedtagsentinel",
        "Revisedmarginalia",
        "EditedPDFbodysentinel",
    ] {
        assert_eq!(find(&server, word).await[0].id, original.id, "{word}");
    }
    assert_eq!(
        find(&server, "Completedextractionsentinel").await[0].external_id,
        "b5"
    );
    assert!(find(&server, "Imagesentinel").await.is_empty());
    assert!(find(&server, "Highlightnotesentinel").await.is_empty());
    fake.lock().highlights.clear();
    plugins::run("karakeep", &run).await.unwrap();
    assert!(find(&server, "Highlightquotesentinel").await.is_empty());
    run.config.as_object_mut().unwrap().remove("public_url");
    plugins::run("karakeep", &run).await.unwrap();
    assert_eq!(
        find(&server, "PDFsentinel").await[0].url.as_deref(),
        Some(format!("{url}/dashboard/preview/b3").as_str())
    );
    fake.lock().bookmarks.clear();
    assert_eq!(
        plugins::run("karakeep", &run).await.unwrap().stats.deleted,
        4
    );
    assert!(find(&server, "PDFsentinel").await.is_empty());
}

#[sqlx::test(migrations = false)]
async fn late_failed_or_malformed_reads_never_sweep_and_never_replace_complete_text_with_empty(
    pool: PgPool,
) {
    let fake = origin();
    let url = serve(fake.clone()).await;
    let server = start_server(pool).await;
    let run = run(&server, &url).await;
    plugins::run("karakeep", &run).await.unwrap();
    fake.lock()
        .bookmarks
        .retain(|bookmark| bookmark["id"] != "b5");
    for failure in [
        Failure::InventoryHttp,
        Failure::InventoryMissingCursor,
        Failure::InventoryLoop,
        Failure::InventoryDuplicate,
        Failure::InventoryEmptyContinuation,
        Failure::ContentHttp,
        Failure::ContentChangedHttp,
        Failure::ContentChangedVersion,
        Failure::ContentMissingCursor,
        Failure::ContentLoop,
        Failure::ContentGap,
        Failure::ContentTruncated,
        Failure::ContentWrongBookmark,
        Failure::HighlightsForbidden,
        Failure::HighlightsMalformed,
        Failure::HighlightsWrongBookmark,
        Failure::HighlightsRedirect,
    ] {
        fake.lock().failure = Some(failure);
        let error = plugins::run("karakeep", &run)
            .await
            .err()
            .expect("must fail")
            .to_string();
        assert!(!error.contains(TOKEN), "{failure:?}: {error}");
        assert!(!run.log.take().join("\n").contains(TOKEN));
        assert_eq!(
            find(&server, "Sparsesentinel").await[0].external_id,
            "b5",
            "{failure:?}"
        );
        assert_eq!(
            find(&server, "PDFtailsentinel").await[0].external_id,
            "b3",
            "{failure:?}"
        );
        assert_eq!(
            find(&server, "Highlightnotesentinel").await[0].external_id,
            "b3",
            "{failure:?}"
        );
    }
    fake.lock().failure = None;
    assert_eq!(
        plugins::run("karakeep", &run).await.unwrap().stats.deleted,
        1
    );
    assert!(find(&server, "Sparsesentinel").await.is_empty());
}

#[sqlx::test(migrations = false)]
async fn worker_claims_encrypted_credentials_and_respects_exclusions_and_manual_runs(pool: PgPool) {
    let fake = origin();
    let url = serve(fake.clone()).await;
    let server = start_server(pool.clone()).await;
    let worker = Worker {
        internal: server.internal(),
        public_url: server.public_url(),
        name: "karakeep-test-worker".to_owned(),
        options: PluginOptions::default(),
    };
    let worker_id = worker.register().await;
    let cookie = server.sign_in().await;
    let created = server.graphql(&cookie,
        "mutation ($input: NewConnection!) { createConnection(input: $input) { id config } }",
        json!({"input": {
            "name": "Saved bookmarks", "icon": "bookmark", "plugin": "karakeep", "source": "saved/karakeep",
            "config": json!({"url": url, "token": TOKEN}).to_string(), "schedule": "*/15 * * * *",
        }}),
    ).await;
    assert_eq!(created["errors"], Value::Null, "{created}");
    assert!(!created.to_string().contains(TOKEN));
    let stored: (Value, Vec<u8>) = sqlx::query_as("select config, credentials from connections")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(stored.0.get("token").is_none());
    assert!(
        !stored
            .1
            .windows(TOKEN.len())
            .any(|bytes| bytes == TOKEN.as_bytes())
    );
    assert!(!stored.1.is_empty());
    let connection = created["data"]["createConnection"]["id"].clone();
    assert!(worker.work_once(worker_id, Duration::ZERO).await.unwrap());
    let document = find(&server, "PDFsentinel").await.remove(0);
    let excluded = server
        .graphql(
            &cookie,
            "mutation ($id: ID!) { excludeDocument(id: $id) { deleted remembered } }",
            json!({"id": document.id}),
        )
        .await;
    assert_eq!(
        excluded["data"]["excludeDocument"],
        json!({"deleted": 1, "remembered": true})
    );
    let due = server
        .graphql(
            &cookie,
            "mutation ($id: ID!) { runConnectionNow(id: $id) }",
            json!({"id": connection}),
        )
        .await;
    assert_eq!(due["errors"], Value::Null, "{due}");
    assert!(worker.work_once(worker_id, Duration::ZERO).await.unwrap());
    assert!(find(&server, "PDFsentinel").await.is_empty());
    let history = server
        .graphql(
            &cookie,
            "{ connections { runs { status stats { put unchanged deleted } error } } }",
            json!({}),
        )
        .await;
    assert_eq!(
        history["data"]["connections"][0]["runs"],
        json!([
            {"status": "SUCCEEDED", "stats": {"put": 0, "unchanged": 4, "deleted": 0}, "error": null},
            {"status": "SUCCEEDED", "stats": {"put": 5, "unchanged": 0, "deleted": 0}, "error": null},
        ])
    );
}

#[sqlx::test(migrations = false)]
async fn authentication_and_invalid_instance_urls_fail_without_leaking_secrets(pool: PgPool) {
    let url = serve(origin()).await;
    let server = start_server(pool).await;
    let mut run = run(&server, &url).await;
    plugins::run("karakeep", &run).await.unwrap();
    run.config["token"] = json!("invalid");
    let error = plugins::run("karakeep", &run)
        .await
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("401"));
    assert_eq!(find(&server, "Sparsesentinel").await.len(), 1);
    for url in [
        format!("http://user:{TOKEN}@example.test"),
        format!("https://example.test/?token={TOKEN}"),
        format!("https://example.test/#{TOKEN}"),
        format!("file:///tmp/{TOKEN}"),
    ] {
        run.config["url"] = json!(url);
        let error = plugins::run("karakeep", &run)
            .await
            .err()
            .unwrap()
            .to_string();
        assert!(!error.contains(TOKEN), "{error}");
    }
}
