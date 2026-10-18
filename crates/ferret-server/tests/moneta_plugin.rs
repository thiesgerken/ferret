mod common;

use std::{sync::Arc, time::Duration};

use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
};
use common::{TestServer, start_server};
use ferret_api::{ContentType, Document, DocumentPut, SearchQuery};
use ferret_plugins::{
    plugins::{self, Log, PluginOptions, Run},
    worker::Worker,
};
use parking_lot::Mutex;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

const TOKEN: &str = "mk_synthetic.secret-that-must-not-appear-in-errors";
const CREATED: &str = "2025-11-03T12:34:56Z";
const UPDATED: &str = "2026-04-05T16:17:18Z";
const VALUE_DATE: &str = "2026-01-02T12:00:00Z";

#[derive(Clone, Copy, Debug)]
enum Failure {
    Auth,
    Graphql,
    Json,
    Redirect,
    MissingTimestamp,
    MissingNullable,
    Duplicate,
    Truncated,
    ChangedCount,
    PageError,
}
struct Origin {
    expenses: Vec<Value>,
    failure: Option<Failure>,
}
type FakeMoneta = Arc<Mutex<Origin>>;

fn expense(id: u128) -> Value {
    json!({
        "id": Uuid::from_u128(id).to_string(), "title": format!("Expense{id}"),
        "store": "Storesentinel", "description": "Descriptionsentinel", "comments": "Commentsentinel",
        "createdAt": CREATED, "updatedAt": UPDATED, "referenceAmount": -100,
        "effectiveAmount": -16.665, "accrualStart": "2026-02-01T12:00:00Z", "accrualEnd": "2026-03-01T12:00:00Z",
        "isTemplate": false, "isTaxRelevant": true, "isUnchecked": true, "isIncomplete": true, "isExtraordinary": true,
        "categories": [{"category": {"id": Uuid::from_u128(900).to_string(), "name": "Categorysentinel"}, "weight": 1.0}],
        "transactions": [
            {"account": {"id": Uuid::from_u128(901).to_string(), "name": "Sharedaccountsentinel"},
             "date": "2026-01-03T12:00:00Z", "bookingDate": "2026-01-04T12:00:00Z", "amount": null, "fraction": 0.3333,
             "calculatedAmount": -33.33, "effectiveAmount": -16.665, "comments": "Transactioncommentsentinel", "statement": "Banksentinel"},
            {"account": {"id": Uuid::from_u128(902).to_string(), "name": "Otheraccountsentinel"},
             "date": VALUE_DATE, "bookingDate": null, "amount": 100, "fraction": null,
             "calculatedAmount": 100.0, "effectiveAmount": 0.0, "comments": "", "statement": ""}
        ],
        "positions": [{"title": "Positiontitlesentinel", "quantity": 2.5, "unit": "kg", "amount": -125, "comments": "Positioncommentsentinel"}],
        "receipts": [{"uri": "https://receipts.example.test/never-fetch.pdf", "ocr": "Receipttailsentinel"}]
    })
}
fn origin() -> FakeMoneta {
    let mut expenses: Vec<_> = (1..=101).map(expense).collect();
    expenses[0]["title"] = json!("Mainexpensesentinel");
    expenses[1]["title"] = json!("Templatesentinel");
    expenses[1]["isTemplate"] = json!(true);
    expenses[2]["title"] = json!("Removalsentinel");
    expenses[3]["title"] = json!("Incomesentinel");
    expenses[3]["effectiveAmount"] = json!(250000.0);
    expenses[3]["transactions"] = json!([{
        "account": {"id": Uuid::from_u128(901).to_string(), "name": "Payroll"},
        "date": VALUE_DATE, "bookingDate": null, "amount": 250000, "fraction": null,
        "calculatedAmount": 250000.0, "effectiveAmount": 250000.0, "comments": "", "statement": "Salary"
    }]);
    expenses[4]["title"] = json!("Transfersentinel");
    expenses[4]["effectiveAmount"] = json!(0.0);
    expenses[4]["transactions"][0]["amount"] = json!(-100);
    expenses[4]["transactions"][0]["fraction"] = Value::Null;
    expenses[4]["transactions"][0]["calculatedAmount"] = json!(-100.0);
    expenses[4]["transactions"][0]["effectiveAmount"] = json!(-100.0);
    expenses[4]["transactions"][1]["effectiveAmount"] = json!(100.0);
    Arc::new(Mutex::new(Origin {
        expenses,
        failure: None,
    }))
}
async fn endpoint(
    State(fake): State<FakeMoneta>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    assert_eq!(headers["authorization"], format!("Bearer {TOKEN}"));
    let query = body["query"].as_str().unwrap();
    assert!(query.contains("sortBy: ACCRUAL_START, sortDir: ASC"));
    for flag in [
        "templates: true",
        "taxRelevant: true",
        "extraordinary: true",
        "unchecked: true",
        "incomplete: true",
        "normal: true",
    ] {
        assert!(query.contains(flag));
    }
    let skip = body["variables"]["skip"].as_u64().unwrap() as usize;
    let limit = body["variables"]["limit"].as_u64().unwrap() as usize;
    let origin = fake.lock();
    match origin.failure {
        Some(Failure::Auth) => return (StatusCode::UNAUTHORIZED, TOKEN).into_response(),
        Some(Failure::Graphql) => return Json(json!({"data": {"expenses": {"list": {"totalCount": 0, "edges": []}}}, "errors": [{"message": TOKEN, "extensions": {"code": "UNAUTHENTICATED"}}]})).into_response(),
        Some(Failure::Json) => return format!("not JSON {TOKEN}").into_response(),
        Some(Failure::Redirect) => return (StatusCode::FOUND, [("location", format!("https://login.example.test/?token={TOKEN}"))]).into_response(),
        Some(Failure::PageError) if skip > 0 => return (StatusCode::FORBIDDEN, TOKEN).into_response(),
        _ => {}
    }
    let mut nodes: Vec<_> = origin
        .expenses
        .iter()
        .skip(skip)
        .take(limit)
        .cloned()
        .collect();
    let mut count = origin.expenses.len();
    match origin.failure {
        Some(Failure::MissingTimestamp) => {
            nodes[0].as_object_mut().unwrap().remove("updatedAt");
        }
        Some(Failure::MissingNullable) => {
            nodes[0]["transactions"][0]
                .as_object_mut()
                .unwrap()
                .remove("bookingDate");
        }
        Some(Failure::Duplicate) if skip > 0 => nodes[0] = origin.expenses[0].clone(),
        Some(Failure::Truncated) if skip > 0 => {
            nodes.pop();
        }
        Some(Failure::ChangedCount) if skip > 0 => count += 1,
        _ => {}
    }
    Json(json!({"data": {"expenses": {"list": {"totalCount": count, "edges": nodes.into_iter().map(|node| json!({"node": node})).collect::<Vec<_>>()}}}})).into_response()
}
async fn serve(fake: FakeMoneta) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/base", listener.local_addr().unwrap());
    let app = Router::new()
        .route("/base/graphql", post(endpoint))
        .with_state(fake);
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    url
}
async fn run(server: &TestServer, url: &str) -> Run {
    Run {
        api: server.admin_api().await,
        source: "finance/moneta".into(),
        config: json!({"url": format!("{url}/"), "public_url": "https://money.example.test/household/", "token": TOKEN}),
        state: json!({}),
        log: Log::default(),
        options: PluginOptions::default(),
    }
}
async fn find(server: &TestServer, query: &str) -> Vec<Document> {
    let api = server.admin_api().await;
    let hits = api
        .search(&SearchQuery {
            q: query.into(),
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
async fn expenses_preserve_financial_semantics_dates_and_identity_and_reconcile_every_run(
    pool: PgPool,
) {
    let fake = origin();
    let url = serve(fake.clone()).await;
    let server = start_server(pool).await;
    let run = run(&server, &url).await;
    let sibling = DocumentPut {
        source: "finance/moneta/accounts".into(),
        external_id: "sibling".into(),
        content_type: ContentType::Plain,
        title: None,
        text: "Siblingsentinel".into(),
        url: None,
        thread: None,
        authors: vec![],
        created_at: common::day(1),
        modified_at: common::day(1),
        meta: Default::default(),
    };
    run.api.put_document(&sibling).await.unwrap();
    assert_eq!(plugins::run("moneta", &run).await.unwrap().stats.put, 101);
    let original = find(&server, "Mainexpensesentinel").await.remove(0);
    assert_eq!(original.source, "finance/moneta/expenses");
    assert_eq!(original.external_id, Uuid::from_u128(1).to_string());
    assert_eq!(
        original.url.as_deref(),
        Some(
            "https://money.example.test/household/expenses?expense=00000000-0000-0000-0000-000000000001"
        )
    );
    assert!(original.authors.is_empty());
    assert_eq!(
        original.created_at,
        VALUE_DATE.parse::<chrono::DateTime<chrono::Utc>>().unwrap()
    );
    assert_eq!(
        original.modified_at,
        UPDATED.parse::<chrono::DateTime<chrono::Utc>>().unwrap()
    );
    assert_eq!(original.meta["expense"]["createdAt"], CREATED);
    assert_eq!(original.meta["expense"]["updatedAt"], UPDATED);
    assert_eq!(
        original.meta["expense"]["transactions"][0]["calculatedAmount"],
        -33.33
    );
    assert_eq!(
        original.meta["expense"]["transactions"][0]["effectiveAmount"],
        -16.665
    );
    assert_eq!(
        original.meta["expense"]["transactions"][0]["fraction"],
        0.3333
    );
    assert_eq!(
        original.meta["expense"]["transactions"][0]["amount"],
        Value::Null
    );
    assert_eq!(original.meta["amount_unit"], "cents");
    for text in [
        "Account movement: -0.3333 EUR",
        "User's effective wealth change: -0.16665 EUR",
        "Booking date: 2026-01-04",
        "Accrual period: 2026-02-01",
        "to 2026-03-01",
        "Quantity: 2.5",
        "Unit: kg",
        "Amount: -1.25 EUR",
        "https://receipts.example.test/never-fetch.pdf",
    ] {
        assert!(original.text.contains(text), "missing {text}");
    }
    for term in [
        "Storesentinel",
        "Descriptionsentinel",
        "Commentsentinel",
        "Categorysentinel",
        "Sharedaccountsentinel",
        "Transactioncommentsentinel",
        "Banksentinel",
        "Positiontitlesentinel",
        "Positioncommentsentinel",
        "Receipttailsentinel",
    ] {
        assert!(
            !find(&server, term).await.is_empty(),
            "not searchable: {term}"
        );
    }
    let template = find(&server, "Templatesentinel").await.remove(0);
    assert!(
        template
            .text
            .starts_with("Template, not an actual payment\n")
    );
    assert_eq!(template.meta["is_template"], true);
    assert!(
        find(&server, "Incomesentinel").await[0]
            .text
            .contains("User's effective wealth change: 2500 EUR")
    );
    assert!(
        find(&server, "Transfersentinel").await[0]
            .text
            .contains("User's effective wealth change: 0 EUR")
    );
    let unchanged = plugins::run("moneta", &run).await.unwrap();
    assert_eq!((unchanged.stats.put, unchanged.stats.unchanged), (0, 101));
    {
        let mut origin = fake.lock();
        origin.expenses[0]["comments"] = json!("Editedexpensesentinel");
        origin.expenses[0]["updatedAt"] = json!("2026-05-06T12:00:00Z");
        origin.expenses[1]["categories"][0]["category"]["name"] = json!("Renamedcategorysentinel");
        origin.expenses[1]["transactions"][0]["account"]["name"] = json!("Renamedaccountsentinel");
        origin.expenses.remove(2);
    }
    let changed = plugins::run("moneta", &run).await.unwrap();
    assert_eq!((changed.stats.put, changed.stats.deleted), (2, 1));
    let edited = find(&server, "Editedexpensesentinel").await.remove(0);
    assert_eq!(edited.id, original.id);
    assert_eq!(
        edited.modified_at,
        "2026-05-06T12:00:00Z"
            .parse::<chrono::DateTime<chrono::Utc>>()
            .unwrap()
    );
    assert_eq!(
        find(&server, "Renamedcategorysentinel").await[0].id,
        template.id
    );
    assert_eq!(
        find(&server, "Renamedaccountsentinel").await[0].modified_at,
        template.modified_at
    );
    assert!(find(&server, "Removalsentinel").await.is_empty());
    fake.lock().expenses.clear();
    assert_eq!(
        plugins::run("moneta", &run).await.unwrap().stats.deleted,
        100
    );
    assert_eq!(find(&server, "Siblingsentinel").await.len(), 1);
}

#[sqlx::test(migrations = false)]
async fn failed_auth_graphql_or_inventory_never_authorizes_sweep_or_leaks_secrets(pool: PgPool) {
    let fake = origin();
    let url = serve(fake.clone()).await;
    let server = start_server(pool).await;
    let run = run(&server, &url).await;
    plugins::run("moneta", &run).await.unwrap();
    // Still two pages after losing visibility of one document.
    fake.lock().expenses.push(expense(102));
    fake.lock().expenses.remove(2);
    for failure in [
        Failure::Auth,
        Failure::Graphql,
        Failure::Json,
        Failure::Redirect,
        Failure::MissingTimestamp,
        Failure::MissingNullable,
        Failure::Duplicate,
        Failure::Truncated,
        Failure::ChangedCount,
        Failure::PageError,
    ] {
        fake.lock().failure = Some(failure);
        let error = plugins::run("moneta", &run)
            .await
            .err()
            .unwrap()
            .to_string();
        assert!(!error.contains(TOKEN), "{failure:?}: {error}");
        assert!(!error.contains("Mainexpensesentinel"));
        assert!(run.log.take().iter().all(|line| !line.contains(TOKEN)));
        assert_eq!(
            find(&server, "Removalsentinel").await.len(),
            1,
            "{failure:?}"
        );
    }
    fake.lock().failure = None;
    assert_eq!(plugins::run("moneta", &run).await.unwrap().stats.deleted, 1);
    assert!(find(&server, "Removalsentinel").await.is_empty());
}

#[sqlx::test(migrations = false)]
async fn claimed_worker_encrypts_read_key_and_respects_expense_exclusions(pool: PgPool) {
    let fake = origin();
    fake.lock().expenses.truncate(2);
    let url = serve(fake).await;
    let server = start_server(pool.clone()).await;
    let worker = Worker {
        internal: server.internal(),
        public_url: server.public_url(),
        name: "moneta-test-worker".into(),
        options: PluginOptions::default(),
    };
    let id = worker.register().await;
    let cookie = server.sign_in().await;
    let created = server.graphql(&cookie, "mutation ($input: NewConnection!) { createConnection(input: $input) { id config } }", json!({"input": {"name": "Moneta", "icon": "wallet", "plugin": "moneta", "source": "finance/moneta", "config": json!({"url": url, "token": TOKEN}).to_string(), "schedule": "0 * * * *"}})).await;
    assert_eq!(created["errors"], Value::Null, "{created}");
    assert!(!created.to_string().contains(TOKEN));
    let stored: (Value, Vec<u8>) = sqlx::query_as("select config, credentials from connections")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(stored.0.get("token").is_none());
    assert!(!stored.1.is_empty());
    assert!(
        !stored
            .1
            .windows(TOKEN.len())
            .any(|bytes| bytes == TOKEN.as_bytes())
    );
    assert!(worker.work_once(id, Duration::ZERO).await.unwrap());
    let document = find(&server, "Mainexpensesentinel").await.remove(0);
    assert!(document.url.as_deref().unwrap().starts_with(&url));
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
    server
        .graphql(
            &cookie,
            "mutation ($id: ID!) { runConnectionNow(id: $id) }",
            json!({"id": created["data"]["createConnection"]["id"]}),
        )
        .await;
    assert!(worker.work_once(id, Duration::ZERO).await.unwrap());
    assert!(find(&server, "Mainexpensesentinel").await.is_empty());
    assert_eq!(find(&server, "Templatesentinel").await.len(), 1);
}
