mod common;

use std::{collections::HashMap, sync::Arc};

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
};
use common::{TestServer, start_server};
use ferret_api::{Document, SearchQuery};
use ferret_plugins::plugins::{self, Log, PluginOptions, Run};
use parking_lot::Mutex;
use serde_json::{Value, json};
use sqlx::PgPool;

const TOKEN: &str = "vikunja-secret-token";
const CREATED: &str = "2026-01-01T12:00:00Z";
const UPDATED: &str = "2026-01-02T12:00:00Z";
const ZERO: &str = "0001-01-01T00:00:00Z";

#[derive(Clone, Copy, Debug)]
enum Failure {
    ChangedCount,
    DuplicateTask,
    TruncatedComments,
    ForbiddenComments,
    MalformedComments,
    RedirectedComments,
}

struct Origin {
    projects: Vec<Value>,
    tasks: Vec<Value>,
    comments: Vec<Value>,
    failure: Option<Failure>,
}

type FakeVikunja = Arc<Mutex<Origin>>;

fn task(id: u64, project: u64, title: &str) -> Value {
    json!({
        "id": id, "project_id": project, "title": title,
        "description": "<p>Review <strong>cobalt</strong> &amp; invoices.</p><p>Second paragraph.</p>",
        "identifier": format!("HOME-{id}"), "done": false, "done_at": ZERO,
        "due_date": "2026-02-01T12:00:00Z", "start_date": ZERO, "end_date": ZERO,
        "priority": 3, "percent_done": 0.5,
        "created": CREATED, "updated": UPDATED, "created_by": {"username": "alice"},
        "labels": [{"id": 1, "title": "finance", "updated": UPDATED}],
        "assignees": [{"username": "bob"}],
        "attachments": [{"id": 7, "file": {"name": "invoice.pdf"}}],
    })
}

fn origin() -> FakeVikunja {
    let mut archived = task(20, 2, "Archivedsentinel");
    archived["done"] = json!(true);
    archived["done_at"] = json!(UPDATED);
    Arc::new(Mutex::new(Origin {
        projects: vec![
            json!({"id": 1, "title": "Home", "is_archived": false, "updated": UPDATED}),
            json!({"id": 2, "title": "Archive", "is_archived": true, "updated": UPDATED}),
            json!({"id": 3, "title": "Empty", "is_archived": false, "updated": UPDATED}),
            json!({"id": -2, "title": "Saved filter", "is_archived": false, "updated": UPDATED}),
        ],
        tasks: vec![
            task(10, 1, "Mainsentinel"),
            task(11, 1, "Removalsentinel"),
            task(12, 1, "Unchangedsentinel"),
            archived,
        ],
        comments: (1..=53)
            .map(|id| {
                json!({
                    "id": id, "comment": format!("<p>Discussion{id} &amp; details.</p>"),
                    "author": {"username": "charlie"}, "created": CREATED,
                    "updated": "2026-01-03T12:00:00Z",
                })
            })
            .collect(),
        failure: None,
    }))
}

async fn endpoint(
    State(fake): State<FakeVikunja>,
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
    if path == "info" {
        return Json(json!({"frontend_url": "https://vikunja.example.test/subpath/"}))
            .into_response();
    }
    let origin = fake.lock();
    let number: usize = query["page"].parse().unwrap();
    let mut values = if path == "projects" {
        origin
            .projects
            .iter()
            .filter(|project| {
                !project["is_archived"].as_bool().unwrap()
                    || query
                        .get("is_archived")
                        .is_some_and(|value| value == "true")
            })
            .cloned()
            .collect::<Vec<_>>()
    } else if path.starts_with("projects/") && path.ends_with("/tasks") {
        let project: u64 = path.split('/').nth(1).unwrap().parse().unwrap();
        let mut tasks: Vec<_> = origin
            .tasks
            .iter()
            .filter(|task| task["project_id"] == project)
            .cloned()
            .collect();
        if query.get("sort_by").is_some_and(|value| value == "id") {
            tasks.sort_by_key(|task| task["id"].as_u64().unwrap());
        } else {
            tasks.reverse();
        }
        tasks
    } else if path == "tasks/10/comments" {
        match origin.failure {
            Some(Failure::ForbiddenComments) => return StatusCode::FORBIDDEN.into_response(),
            Some(Failure::MalformedComments) => {
                return Json(json!({"error": TOKEN})).into_response();
            }
            Some(Failure::RedirectedComments) => {
                return (
                    StatusCode::FOUND,
                    [(
                        "location",
                        format!("https://login.example.test/?token={TOKEN}"),
                    )],
                )
                    .into_response();
            }
            _ => {}
        }
        origin.comments.clone()
    } else if path.starts_with("tasks/") && path.ends_with("/comments") {
        Vec::new()
    } else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if path == "projects" {
        values.retain(|project| project["id"].as_i64().unwrap() > 0);
    }
    // The server caps the requested page size. Empty API lists may use null for items.
    let mut total = values.len();
    let mut items: Vec<_> = values.drain(..).skip((number - 1) * 2).take(2).collect();
    // Vikunja appends saved-filter projects to every page, outside its pagination counts.
    if path == "projects" {
        items.extend(
            origin
                .projects
                .iter()
                .filter(|project| project["id"].as_i64().unwrap() < 0)
                .cloned(),
        );
    }
    match origin.failure {
        Some(Failure::ChangedCount) if path == "projects/1/tasks" && number == 2 => total += 1,
        Some(Failure::DuplicateTask) if path == "projects/1/tasks" && number == 2 => {
            items[0] = origin.tasks[0].clone();
        }
        Some(Failure::TruncatedComments) if path == "tasks/10/comments" && number == 2 => {
            items.clear()
        }
        _ => {}
    }
    Json(json!({
        "items": if total == 0 { Value::Null } else { json!(items) },
        "total": total, "page": number, "per_page": 2, "total_pages": total.div_ceil(2),
    }))
    .into_response()
}

async fn serve(fake: FakeVikunja) -> String {
    let app = Router::new()
        .route("/api/v2/{*path}", get(endpoint))
        .with_state(fake);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    url
}

async fn run(server: &TestServer, url: &str) -> Run {
    Run {
        api: server.admin_api().await,
        source: "tasks/vikunja".to_owned(),
        config: json!({"url": format!("{url}/"), "token": TOKEN}),
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
async fn tasks_include_archived_history_and_complete_discussions_and_reconcile_changes(
    pool: PgPool,
) {
    let fake = origin();
    let url = serve(fake.clone()).await;
    let server = start_server(pool).await;
    let mut run = run(&server, &url).await;
    let first = plugins::run("vikunja", &run).await.unwrap();
    assert_eq!(first.stats.put, 4);
    let mut documents = find(&server, "Mainsentinel").await;
    let original = documents.remove(0);
    assert_eq!(original.external_id, "10");
    assert_eq!(
        original.url.as_deref(),
        Some("https://vikunja.example.test/subpath/tasks/10")
    );
    assert_eq!(original.authors, ["alice", "charlie"]);
    assert!(original.text.contains("Review cobalt & invoices."));
    assert!(original.text.contains("Second paragraph."));
    assert!(original.text.contains("Discussion53 & details."));
    assert!(
        original
            .text
            .contains("charlie (2026-01-01T12:00:00+00:00)")
    );
    assert!(!original.text.contains("<p>"));
    assert_eq!(original.meta["tags"], json!(["finance"]));
    assert_eq!(original.meta["assignees"], json!(["bob"]));
    assert_eq!(
        original.meta["attachments"],
        json!([{"id": 7, "name": "invoice.pdf"}])
    );
    assert_eq!(original.meta["start_date"], Value::Null);
    assert_eq!(original.created_at, Some(common::day(1)));
    assert_eq!(original.modified_at, Some(common::day(3)));
    let archived = find(&server, "Archivedsentinel").await.remove(0);
    assert_eq!(archived.meta["project_archived"], true);
    assert_eq!(archived.meta["done"], true);

    let unchanged = plugins::run("vikunja", &run).await.unwrap();
    assert_eq!(unchanged.stats.unchanged, 4);
    assert_eq!(unchanged.stats.put, 0);

    // Neither the task nor project timestamp changes for a comment-only edit.
    fake.lock().comments[52]["comment"] = json!("<p>Editeddiscussion</p>");
    fake.lock().comments[52]["updated"] = json!("2026-01-04T12:00:00Z");
    let edited = plugins::run("vikunja", &run).await.unwrap();
    assert_eq!(edited.stats.put, 1);
    let edit = find(&server, "Editeddiscussion").await.remove(0);
    assert_eq!(edit.id, original.id);
    assert_eq!(edit.modified_at, Some(common::day(4)));
    assert!(find(&server, "Discussion53").await.is_empty());

    // Comment deletion, a task move and a project rename must not depend on task.updated either.
    {
        let mut origin = fake.lock();
        origin.comments.pop();
        origin.tasks[0]["project_id"] = json!(2);
        origin.tasks.retain(|task| task["id"] != 11);
        origin.projects[1]["title"] = json!("Historicalprojects");
        origin.projects[1]["updated"] = json!("2026-01-05T12:00:00Z");
    }
    let reconciled = plugins::run("vikunja", &run).await.unwrap();
    assert_eq!(reconciled.stats.deleted, 1);
    assert!(find(&server, "Removalsentinel").await.is_empty());
    assert!(find(&server, "Editeddiscussion").await.is_empty());
    let moved = find(&server, "Mainsentinel").await.remove(0);
    assert_eq!(moved.id, original.id);
    assert_eq!(moved.meta["project_id"], 2);
    assert_eq!(moved.meta["project"], "Historicalprojects");
    assert_eq!(moved.modified_at, Some(common::day(5)));

    run.config["public_url"] = json!("https://other.example.test");
    plugins::run("vikunja", &run).await.unwrap();
    assert_eq!(
        find(&server, "Mainsentinel").await.remove(0).url.as_deref(),
        Some("https://other.example.test/tasks/10")
    );

    // Lost project visibility is reconciled just like deletion.
    fake.lock().projects.retain(|project| project["id"] != 2);
    let revoked = plugins::run("vikunja", &run).await.unwrap();
    assert_eq!(revoked.stats.deleted, 2);
    assert!(find(&server, "Mainsentinel").await.is_empty());
    assert_eq!(
        find(&server, "Unchangedsentinel").await[0].external_id,
        "12"
    );
}

#[sqlx::test(migrations = false)]
async fn incomplete_or_failed_listings_never_sweep_existing_documents_or_log_secrets(pool: PgPool) {
    let fake = origin();
    let url = serve(fake.clone()).await;
    let server = start_server(pool).await;
    let run = run(&server, &url).await;
    plugins::run("vikunja", &run).await.unwrap();
    // This disappears from the origin, but must remain indexed until a complete successful run.
    fake.lock().tasks.retain(|task| task["id"] != 20);
    for failure in [
        Failure::ChangedCount,
        Failure::DuplicateTask,
        Failure::TruncatedComments,
        Failure::ForbiddenComments,
        Failure::MalformedComments,
        Failure::RedirectedComments,
    ] {
        fake.lock().failure = Some(failure);
        let error = plugins::run("vikunja", &run)
            .await
            .err()
            .expect("the run must fail")
            .to_string();
        assert!(!error.contains(TOKEN), "{failure:?}: {error}");
        assert_eq!(
            find(&server, "Archivedsentinel").await[0].external_id,
            "20",
            "{failure:?}"
        );
        assert_eq!(
            find(&server, "Discussion53").await[0].external_id,
            "10",
            "{failure:?}"
        );
    }
    fake.lock().failure = None;
    let recovered = plugins::run("vikunja", &run).await.unwrap();
    assert_eq!(recovered.stats.deleted, 1);
    assert!(find(&server, "Archivedsentinel").await.is_empty());
}
