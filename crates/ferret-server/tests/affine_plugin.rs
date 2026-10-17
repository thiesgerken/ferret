mod common;

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::IntoResponse,
    routing::{get, post},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use chrono::{DateTime, Utc};
use common::{TestServer, start_server};
use ferret_api::SearchQuery;
use ferret_plugins::{plugins::PluginOptions, worker::Worker};
use serde_json::{Value, json};
use sqlx::PgPool;
use yrs::{Any, ArrayPrelim, Doc, In, Map, MapPrelim, ReadTxn, StateVector, TextPrelim, Transact};

const WORKSPACE: &str = "w1";
const EMAIL: &str = "ferret@affine.test";
const PASSWORD: &str = "affine-password";
const SESSION: &str = "affine_session=session-1";

#[derive(Clone)]
struct FakeDoc {
    id: &'static str,
    text: String,
    /// When the server last changed it. `None`: the root doc lists it, but no client has synced
    /// its content to the server.
    updated_at: Option<DateTime<Utc>>,
    trashed: bool,
}

fn fake_doc(id: &'static str, text: &str, updated_at: &str) -> FakeDoc {
    FakeDoc {
        id,
        text: text.to_owned(),
        updated_at: Some(updated_at.parse().unwrap()),
        trashed: false,
    }
}

/// An AFFiNE that serves docs the test can change between runs, built as Yjs states like the real
/// one's. `recentlyUpdatedDocs` pages at most `page_size` docs, so paging is exercised. It records
/// which docs were fetched.
#[derive(Clone)]
struct FakeAffine {
    docs: Arc<Mutex<Vec<FakeDoc>>>,
    page_size: usize,
    fetched: Arc<Mutex<Vec<String>>>,
}

impl FakeAffine {
    fn new(docs: Vec<FakeDoc>, page_size: usize) -> Self {
        Self {
            docs: Arc::new(Mutex::new(docs)),
            page_size,
            fetched: Arc::default(),
        }
    }

    fn update(&self, id: &str, change: impl FnOnce(&mut FakeDoc)) {
        let mut docs = self.docs.lock().unwrap();
        change(docs.iter_mut().find(|doc| doc.id == id).unwrap());
    }

    fn take_fetched(&self) -> Vec<String> {
        std::mem::take(&mut self.fetched.lock().unwrap())
    }
}

fn signed_in(headers: &HeaderMap) -> bool {
    headers
        .get(header::COOKIE)
        .and_then(|cookie| cookie.to_str().ok())
        .is_some_and(|cookie| cookie.split("; ").any(|pair| pair == SESSION))
}

async fn sign_in(Json(body): Json<Value>) -> impl IntoResponse {
    if body != json!({"email": EMAIL, "password": PASSWORD}) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let mut headers = HeaderMap::new();
    headers.append(
        header::SET_COOKIE,
        format!("{SESSION}; Path=/; HttpOnly").parse().unwrap(),
    );
    headers.append(
        header::SET_COOKIE,
        "affine_user_id=u1; Path=/".parse().unwrap(),
    );
    (headers, Json(json!({"id": "u1"}))).into_response()
}

async fn doc(
    State(fake): State<FakeAffine>,
    headers: HeaderMap,
    Path((workspace, id)): Path<(String, String)>,
) -> impl IntoResponse {
    if !signed_in(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if workspace != WORKSPACE {
        return StatusCode::FORBIDDEN.into_response();
    }
    let docs = fake.docs.lock().unwrap().clone();
    if id == WORKSPACE {
        return root_doc(&docs).into_response();
    }
    match docs
        .iter()
        .find(|doc| doc.id == id && doc.updated_at.is_some())
    {
        Some(doc) => {
            fake.fetched.lock().unwrap().push(id);
            page_doc(&doc.text).into_response()
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn graphql(
    State(fake): State<FakeAffine>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    if !signed_in(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let query = body["query"].as_str().unwrap();
    let variables = &body["variables"];
    assert_eq!(variables["workspace"], WORKSPACE);
    let docs = fake.docs.lock().unwrap().clone();
    if query.contains("recentlyUpdatedDocs") {
        // Like AFFiNE: newest first, `updatedAt < after`, at most its own page size.
        let after: Option<DateTime<Utc>> = variables["after"].as_str().map(|after| {
            String::from_utf8(STANDARD.decode(after).unwrap())
                .unwrap()
                .parse()
                .unwrap()
        });
        let mut listed: Vec<(DateTime<Utc>, &str)> = docs
            .iter()
            .filter_map(|doc| Some((doc.updated_at?, doc.id)))
            .filter(|(updated_at, _)| after.is_none_or(|after| *updated_at < after))
            .collect();
        listed.sort_by(|a, b| b.cmp(a));
        let first = (variables["first"].as_u64().unwrap() as usize).min(fake.page_size);
        listed.truncate(first);
        return Json(json!({"data": {"workspace": {"recentlyUpdatedDocs": {
            "edges": listed.iter().map(|(updated_at, id)| json!({"node": {"id": id, "updatedAt": updated_at}})).collect::<Vec<_>>(),
            "pageInfo": {"hasNextPage": listed.len() == first},
        }}}}))
        .into_response();
    }
    let updated_at = docs
        .iter()
        .find(|doc| doc.id == variables["doc"])
        .and_then(|doc| doc.updated_at);
    Json(json!({"data": {"workspace": {"doc": {"updatedAt": updated_at}}}})).into_response()
}

/// The workspace's root doc: `meta.pages` with every doc, trashed or not.
fn root_doc(docs: &[FakeDoc]) -> Vec<u8> {
    let pages: Vec<Value> = docs
        .iter()
        .map(|doc| json!({"id": doc.id, "title": doc.id, "createDate": 1792195200000_i64, "trash": doc.trashed}))
        .collect();
    let doc = Doc::new();
    let meta = doc.get_or_insert_map("meta");
    let mut txn = doc.transact_mut();
    let pages: Vec<Any> = pages
        .into_iter()
        .map(|page| serde_json::from_value(page).unwrap())
        .collect();
    meta.insert(&mut txn, "pages", ArrayPrelim::from(pages));
    txn.encode_state_as_update_v1(&StateVector::default())
}

/// A doc of one paragraph, under its page and a note.
fn page_doc(text: &str) -> Vec<u8> {
    let doc = Doc::new();
    let blocks = doc.get_or_insert_map("blocks");
    let mut txn = doc.transact_mut();
    let block = |props: Vec<(&str, In)>| {
        MapPrelim::from_iter(
            props
                .into_iter()
                .map(|(key, value)| (key.to_owned(), value)),
        )
    };
    let children = |ids: &[&str]| {
        In::Array(ArrayPrelim::from(
            ids.iter().map(|id| Any::from(*id)).collect::<Vec<_>>(),
        ))
    };
    blocks.insert(
        &mut txn,
        "page",
        block(vec![
            ("sys:id", In::Any("page".into())),
            ("sys:flavour", In::Any("affine:page".into())),
            ("prop:title", In::Text(TextPrelim::new("").into())),
            ("sys:children", children(&["note"])),
        ]),
    );
    blocks.insert(
        &mut txn,
        "note",
        block(vec![
            ("sys:flavour", In::Any("affine:note".into())),
            ("sys:children", children(&["p"])),
        ]),
    );
    blocks.insert(
        &mut txn,
        "p",
        block(vec![
            ("sys:flavour", In::Any("affine:paragraph".into())),
            ("prop:type", In::Any("text".into())),
            ("prop:text", In::Text(TextPrelim::new(text).into())),
        ]),
    );
    txn.encode_state_as_update_v1(&StateVector::default())
}

async fn serve(fake: FakeAffine) -> String {
    let app = Router::new()
        .route("/api/auth/sign-in", post(sign_in))
        .route("/api/workspaces/{workspace}/docs/{doc}", get(doc))
        .route("/graphql", post(graphql))
        .with_state(fake);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    url
}

async fn search(server: &TestServer, q: &str) -> Vec<(String, Option<String>)> {
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
        .map(|hit| (hit.external_id, hit.url))
        .collect()
}

/// A server, a worker and an `affine` connection to `url`.
async fn connect(pool: PgPool, url: &str) -> (TestServer, Worker, uuid::Uuid, String, Value) {
    let server = start_server(pool).await;
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
                "name": "Notes", "icon": "notebook-pen", "plugin": "affine",
                "source": "affine",
                "config": json!({
                    "url": url,
                    "public_url": "https://affine.example.test",
                    "workspace_id": WORKSPACE,
                    "email": EMAIL,
                    "password": PASSWORD,
                }).to_string(),
                "schedule": "0 * * * *",
            }}),
        )
        .await;
    assert_eq!(created["errors"], Value::Null, "{created}");
    assert!(!created.to_string().contains(PASSWORD));
    let id = created["data"]["createConnection"]["id"].clone();
    (server, worker, worker_id, cookie, id)
}

async fn run_again(
    server: &TestServer,
    worker: &Worker,
    worker_id: uuid::Uuid,
    cookie: &str,
    id: &Value,
) {
    server
        .graphql(
            cookie,
            "mutation ($id: ID!) { runConnectionNow(id: $id) }",
            json!({"id": id}),
        )
        .await;
    assert!(worker.work_once(worker_id, Duration::ZERO).await.unwrap());
}

async fn last_run(server: &TestServer, cookie: &str) -> Value {
    let runs = server
        .graphql(
            cookie,
            "{ connections { runs(limit: 1) { status error log stats { put unchanged deleted } } } }",
            json!({}),
        )
        .await;
    runs["data"]["connections"][0]["runs"][0].clone()
}

#[sqlx::test(migrations = false)]
async fn affine_docs_sync_incrementally_and_trashed_ones_disappear(pool: PgPool) {
    let fake = FakeAffine::new(
        vec![
            fake_doc(
                "d1",
                "The boiler needs a new valve.",
                "2026-10-17T08:00:05Z",
            ),
            fake_doc(
                "d2",
                "Gutters get cleaned in spring.",
                "2026-10-17T07:00:04Z",
            ),
            // A tie across the first page's end: d3 is its last doc, d4 starts the next.
            fake_doc(
                "d3",
                "The attic insulation is rock wool.",
                "2026-10-17T07:00:03Z",
            ),
            fake_doc("d4", "The cellar window sticks.", "2026-10-17T07:00:03Z"),
            fake_doc(
                "d5",
                "Brine pressure reads 1.8 bar.",
                "2026-10-17T07:00:01Z",
            ),
            FakeDoc {
                id: "d6",
                text: "Never synced.".to_owned(),
                updated_at: None,
                trashed: false,
            },
            FakeDoc {
                trashed: true,
                ..fake_doc(
                    "d7",
                    "The old router lived in the drawer.",
                    "2026-10-17T07:00:02Z",
                )
            },
        ],
        3,
    );
    let affine = serve(fake.clone()).await;
    let (server, worker, worker_id, cookie, id) = connect(pool.clone(), &affine).await;

    assert!(worker.work_once(worker_id, Duration::ZERO).await.unwrap());

    assert_eq!(fake.take_fetched(), ["d1", "d2", "d3", "d4", "d5"]);
    assert_eq!(
        search(&server, "cellar").await,
        [(
            "d4".to_owned(),
            Some("https://affine.example.test/workspace/w1/d4".to_owned())
        )]
    );
    assert!(search(&server, "router").await.is_empty(), "trashed");
    let run = last_run(&server, &cookie).await;
    assert_eq!(
        run["stats"],
        json!({"put": 5, "unchanged": 0, "deleted": 0})
    );
    assert!(
        run["log"]
            .as_str()
            .unwrap()
            .contains("1 docs have no content on the server yet and are skipped"),
        "{run}"
    );

    // In AFFiNE, d5 changes and d2 goes to the trash.
    fake.update("d5", |doc| {
        doc.text = "Brine pressure reads 1.2 bar, refill soon.".to_owned();
        doc.updated_at = Some("2026-10-17T12:00:00Z".parse().unwrap());
    });
    fake.update("d2", |doc| doc.trashed = true);
    run_again(&server, &worker, worker_id, &cookie, &id).await;

    // d1 is within the overlap before the cursor.
    assert_eq!(fake.take_fetched(), ["d1", "d5"]);
    assert_eq!(search(&server, "refill").await.len(), 1);
    assert!(search(&server, "gutters").await.is_empty());
    let run = last_run(&server, &cookie).await;
    assert_eq!(
        run["stats"],
        json!({"put": 1, "unchanged": 1, "deleted": 1})
    );

    // Back from the trash: only d2 is fetched besides the overlap, though its time is old.
    fake.update("d2", |doc| doc.trashed = false);
    run_again(&server, &worker, worker_id, &cookie, &id).await;

    assert_eq!(fake.take_fetched(), ["d5", "d2"]);
    assert_eq!(search(&server, "gutters").await.len(), 1);
    let state: Value = sqlx::query_scalar("select state from connections")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state["updated"], "2026-10-17T12:00:00Z");
    assert_eq!(state["trashed"], json!(["d7"]));
}

#[sqlx::test(migrations = false)]
async fn a_page_of_docs_with_one_time_fails_the_run_instead_of_skipping_some(pool: PgPool) {
    let fake = FakeAffine::new(
        vec![
            fake_doc("a", "First.", "2026-10-17T07:00:00Z"),
            fake_doc("b", "Second.", "2026-10-17T07:00:00Z"),
            fake_doc("c", "Third.", "2026-10-17T07:00:00Z"),
        ],
        2,
    );
    let affine = serve(fake.clone()).await;
    let (server, worker, worker_id, cookie, _) = connect(pool, &affine).await;

    assert!(worker.work_once(worker_id, Duration::ZERO).await.unwrap());

    let run = last_run(&server, &cookie).await;
    assert_eq!(run["status"], "FAILED");
    assert!(
        run["error"]
            .as_str()
            .unwrap()
            .contains("which paging can't get past"),
        "{run}"
    );
    assert!(fake.take_fetched().is_empty());
}

#[sqlx::test(migrations = false)]
async fn a_login_proxy_in_front_of_affine_fails_the_run_with_a_hint(pool: PgPool) {
    let app = Router::new().fallback(|| async {
        (
            StatusCode::FOUND,
            [("location", "https://auth.example.test/login")],
        )
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let (server, worker, worker_id, cookie, _) = connect(pool, &proxy).await;

    assert!(worker.work_once(worker_id, Duration::ZERO).await.unwrap());

    let run = last_run(&server, &cookie).await;
    assert_eq!(run["status"], "FAILED");
    assert!(
        run["error"]
            .as_str()
            .unwrap()
            .contains("reaches AFFiNE directly"),
        "{run}"
    );
}
