use crate::common;

use std::{collections::HashMap, sync::Arc, time::Duration};

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
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

const TOKEN: &str = "secret-mealie-token-never-expose";
const CREATED: &str = "2025-11-03T12:34:56Z";
const UPDATED: &str = "2026-04-05T16:17:18Z";
const COMMENT_UPDATED: &str = "2026-05-06T12:00:00Z";

#[derive(Clone, Copy, Debug)]
enum Failure {
    Auth,
    Json,
    Redirect,
    MissingTimestamp,
    InvalidTimestamp,
    MissingIngredients,
    Duplicate,
    OutOfOrder,
    Truncated,
    ChangedCount,
    WrongPage,
    WrongPages,
    PageError,
    DetailMissing,
    DetailAuth,
    WrongDetail,
    WrongGroup,
    WrongComment,
}
struct Origin {
    recipes: Vec<Value>,
    failure: Option<Failure>,
}
type FakeMealie = Arc<Mutex<Origin>>;

fn recipe(id: u128) -> Value {
    json!({
        "id": Uuid::from_u128(id), "groupId": Uuid::from_u128(500),
        "householdId": Uuid::from_u128(600 + id % 2), "userId": Uuid::from_u128(700 + id % 2),
        "name": format!("Recipe{id}"), "slug": format!("recipe-{id}"),
        "description": "Descriptionsentinel", "createdAt": CREATED, "updatedAt": UPDATED,
        "recipeServings": 4, "recipeYieldQuantity": 2, "recipeYield": "loaves",
        "totalTime": "1 hour", "prepTime": "10 minutes", "cookTime": null, "performTime": "50 minutes",
        "totalTimeSeconds": 3600, "prepTimeSeconds": 600, "performTimeSeconds": 3000,
        "orgURL": "https://external.example.test/never-fetch", "recipeIngredient": [
            {"title": "Doughsentinel", "display": "2 cups Cardamomsentinel", "referencedRecipe": null},
            {"title": null, "display": "Base sauce", "referencedRecipe": {
                "id": Uuid::from_u128(999), "groupId": Uuid::from_u128(500), "name": "Referencesentinel", "slug": "base-sauce"
            }}
        ],
        "recipeInstructions": [{"title": "Mixsentinel", "summary": "Summarysentinel", "text": "**Kneadsentinel** until smooth."}],
        "notes": [{"title": "Tips", "text": "Fermentationsentinel overnight."}],
        "comments": [{"id": Uuid::from_u128(800 + id), "recipeId": Uuid::from_u128(id),
            "text": "Commentsentinel from the cook", "createdAt": CREATED, "updatedAt": COMMENT_UPDATED,
            "userId": Uuid::from_u128(750), "user": {"id": Uuid::from_u128(750), "username": "cook", "fullName": "Chef Sentinel"}}],
        "tags": [{"name": "Zebra"}, {"name": "Tagssentinel"}],
        "recipeCategory": [{"name": "Categorysentinel"}], "tools": [{"name": "Toolssentinel"}],
        "nutrition": {"calories": "350", "proteinContent": "8g"},
        "extras": {"not_indexed": "Extraomissionsentinel"}, "assets": [{"name": "Binaryomissionsentinel"}]
    })
}
fn origin() -> FakeMealie {
    let mut recipes: Vec<_> = (1..=101).map(recipe).collect();
    recipes[0]["name"] = json!("Mainrecipesentinel");
    recipes[1]["name"] = json!("Otherhouseholdsentinel");
    recipes[2]["name"] = json!("Removalsentinel");
    Arc::new(Mutex::new(Origin {
        recipes,
        failure: None,
    }))
}
fn authenticated(headers: &HeaderMap) {
    assert_eq!(headers["authorization"], format!("Bearer {TOKEN}"));
}
async fn user(State(fake): State<FakeMealie>, headers: HeaderMap) -> Response {
    authenticated(&headers);
    if matches!(fake.lock().failure, Some(Failure::Auth)) {
        return (StatusCode::UNAUTHORIZED, TOKEN).into_response();
    }
    Json(json!({"groupId": Uuid::from_u128(500), "groupSlug": "family & friends"})).into_response()
}
async fn listing(
    State(fake): State<FakeMealie>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    authenticated(&headers);
    assert_eq!(
        query.len(),
        4,
        "no household, owner, public or other visibility filters"
    );
    assert_eq!(query["orderBy"], "id");
    assert_eq!(query["orderDirection"], "asc");
    let page: usize = query["page"].parse().unwrap();
    let size: usize = query["perPage"].parse().unwrap();
    let origin = fake.lock();
    match origin.failure {
        Some(Failure::Json) => return format!("not JSON {TOKEN}").into_response(),
        Some(Failure::Redirect) => {
            return (
                StatusCode::FOUND,
                [("location", format!("https://login.invalid/?token={TOKEN}"))],
            )
                .into_response();
        }
        Some(Failure::PageError) if page > 1 => {
            return (StatusCode::FORBIDDEN, TOKEN).into_response();
        }
        _ => {}
    }
    let mut items: Vec<_> = origin
        .recipes
        .iter()
        .skip((page - 1) * size)
        .take(size)
        .map(|recipe| json!({"id": recipe["id"], "groupId": recipe["groupId"]}))
        .collect();
    let mut total = origin.recipes.len();
    match origin.failure {
        Some(Failure::Duplicate) if page > 1 => {
            items[0] = json!({"id": origin.recipes[0]["id"], "groupId": Uuid::from_u128(500)})
        }
        Some(Failure::OutOfOrder) if page == 1 => items.swap(0, 1),
        Some(Failure::Truncated) if page > 1 => {
            items.pop();
        }
        Some(Failure::ChangedCount) if page > 1 => total += 1,
        Some(Failure::WrongGroup) => items[0]["groupId"] = json!(Uuid::from_u128(501)),
        _ => {}
    }
    let pages = total.div_ceil(size);
    Json(json!({"page": if matches!(origin.failure, Some(Failure::WrongPage)) {page + 1} else {page},
        "per_page": size, "total": total,
        "total_pages": if matches!(origin.failure, Some(Failure::WrongPages)) {pages + 1} else {pages},
        "items": items, "next": (page < pages).then_some("https://do-not-follow.invalid/"),
        "previous": (page > 1).then_some("https://do-not-follow.invalid/")})).into_response()
}
async fn detail(
    State(fake): State<FakeMealie>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    authenticated(&headers);
    let origin = fake.lock();
    match origin.failure {
        Some(Failure::DetailMissing) => return (StatusCode::NOT_FOUND, TOKEN).into_response(),
        Some(Failure::DetailAuth) => return (StatusCode::UNAUTHORIZED, TOKEN).into_response(),
        _ => {}
    }
    let mut value = origin
        .recipes
        .iter()
        .find(|recipe| recipe["id"] == id)
        .unwrap()
        .clone();
    match origin.failure {
        Some(Failure::MissingTimestamp) => {
            value.as_object_mut().unwrap().remove("createdAt");
        }
        Some(Failure::InvalidTimestamp) => value["updatedAt"] = json!(TOKEN),
        Some(Failure::MissingIngredients) => {
            value.as_object_mut().unwrap().remove("recipeIngredient");
        }
        Some(Failure::WrongDetail) => value["id"] = json!(Uuid::from_u128(999)),
        Some(Failure::WrongComment) => {
            value["comments"][0]["recipeId"] = json!(Uuid::from_u128(999))
        }
        _ => {}
    }
    Json(value).into_response()
}
async fn serve(fake: FakeMealie) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/base", listener.local_addr().unwrap());
    let app = Router::new()
        .route("/base/api/users/self", get(user))
        .route("/base/api/recipes", get(listing))
        .route("/base/api/recipes/{id}", get(detail))
        .with_state(fake);
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    url
}
async fn run(server: &TestServer, url: &str) -> Run {
    Run {
        api: server.admin_api().await,
        source: "food/mealie".into(),
        config: json!({"url": format!("{url}/"), "public_url": "https://recipes.example.test/kitchen/", "token": TOKEN}),
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
async fn recipes_search_attached_content_preserve_uuid_and_reconcile_group_wide(pool: PgPool) {
    let fake = origin();
    let url = serve(fake.clone()).await;
    let server = start_server(pool).await;
    let run = run(&server, &url).await;
    run.api
        .put_document(&DocumentPut {
            source: "food/mealie/other".into(),
            external_id: "sibling".into(),
            content_type: ContentType::Plain,
            title: None,
            text: "Siblingsentinel".into(),
            url: None,
            thread: None,
            authors: vec![],
            created_at: Some(common::day(1)),
            modified_at: Some(common::day(1)),
            meta: Default::default(),
        })
        .await
        .unwrap();
    assert_eq!(plugins::run("mealie", &run).await.unwrap().stats.put, 101);
    let original = find(&server, "Mainrecipesentinel").await.remove(0);
    assert_eq!(original.source, "food/mealie/recipes");
    assert_eq!(original.external_id, Uuid::from_u128(1).to_string());
    assert_eq!(original.content_type, ContentType::Markdown);
    assert_eq!(
        original.url.as_deref(),
        Some("https://recipes.example.test/kitchen/g/family%20&%20friends/r/recipe-1/")
    );
    assert!(original.authors.is_empty());
    assert_eq!(
        original.created_at,
        Some(CREATED.parse::<chrono::DateTime<chrono::Utc>>().unwrap())
    );
    assert_eq!(
        original.modified_at,
        Some(
            COMMENT_UPDATED
                .parse::<chrono::DateTime<chrono::Utc>>()
                .unwrap()
        )
    );
    assert!(original.text.contains("**Kneadsentinel**"));
    assert!(original.text.contains("Author: Chef Sentinel"));
    assert!(original.text.contains("Total time: 3600 seconds"));
    assert!(original.text.contains("Protein: 8g"));
    assert!(original.text.contains("/r/base-sauce"));
    for term in [
        "Cardamomsentinel",
        "Kneadsentinel",
        "Fermentationsentinel",
        "Commentsentinel",
        "Tagssentinel",
        "Toolssentinel",
        "Categorysentinel",
        "Referencesentinel",
    ] {
        let hits = find(&server, &format!("Mainrecipesentinel {term}")).await;
        assert_eq!(hits.len(), 1, "{term}");
        assert_eq!(hits[0].id, original.id);
    }
    assert_eq!(find(&server, "Otherhouseholdsentinel").await.len(), 1);
    assert!(find(&server, "Extraomissionsentinel").await.is_empty());
    assert!(find(&server, "Binaryomissionsentinel").await.is_empty());
    let unchanged = plugins::run("mealie", &run).await.unwrap().stats;
    assert_eq!((unchanged.put, unchanged.unchanged), (0, 101));
    {
        let mut origin = fake.lock();
        let first = &mut origin.recipes[0];
        first["name"] = json!("Renamedsentinel");
        first["slug"] = json!("renamed / soup?#");
        first["notes"][0]["text"] = json!("Editednotesentinel");
        first["comments"][0]["text"] = json!("Editedcommentsentinel");
        first["comments"][0]["updatedAt"] = json!("2026-06-07T12:00:00Z");
        origin.recipes[1]["tags"][0]["name"] = json!("Editedtagssentinel");
        origin.recipes.remove(2);
    }
    let changed = plugins::run("mealie", &run).await.unwrap().stats;
    assert_eq!(
        (changed.put, changed.deleted, changed.unchanged),
        (2, 1, 98)
    );
    for term in [
        "Renamedsentinel",
        "Editednotesentinel",
        "Editedcommentsentinel",
    ] {
        assert_eq!(find(&server, term).await[0].id, original.id);
    }
    let renamed = find(&server, "Renamedsentinel").await.remove(0);
    assert_eq!(
        renamed.url.as_deref(),
        Some(
            "https://recipes.example.test/kitchen/g/family%20&%20friends/r/renamed%20%2F%20soup%3F%23/"
        )
    );
    assert_eq!(
        renamed.modified_at,
        Some(
            "2026-06-07T12:00:00Z"
                .parse::<chrono::DateTime<chrono::Utc>>()
                .unwrap()
        )
    );
    assert_eq!(
        find(&server, "Editedtagssentinel").await[0].modified_at,
        original.modified_at
    );
    assert!(find(&server, "Removalsentinel").await.is_empty());
    fake.lock().recipes.clear();
    assert_eq!(
        plugins::run("mealie", &run).await.unwrap().stats.deleted,
        100
    );
    assert_eq!(find(&server, "Siblingsentinel").await.len(), 1);
}

#[sqlx::test(migrations = false)]
async fn failed_inventory_or_detail_retains_unseen_documents_without_exposing_secrets(
    pool: PgPool,
) {
    let fake = origin();
    let url = serve(fake.clone()).await;
    let server = start_server(pool).await;
    let run = run(&server, &url).await;
    plugins::run("mealie", &run).await.unwrap();
    fake.lock().recipes.push(recipe(102));
    fake.lock().recipes.remove(2);
    for failure in [
        Failure::Auth,
        Failure::Json,
        Failure::Redirect,
        Failure::MissingTimestamp,
        Failure::InvalidTimestamp,
        Failure::MissingIngredients,
        Failure::Duplicate,
        Failure::OutOfOrder,
        Failure::Truncated,
        Failure::ChangedCount,
        Failure::WrongPage,
        Failure::WrongPages,
        Failure::PageError,
        Failure::DetailMissing,
        Failure::DetailAuth,
        Failure::WrongDetail,
        Failure::WrongGroup,
        Failure::WrongComment,
    ] {
        fake.lock().failure = Some(failure);
        let error = plugins::run("mealie", &run)
            .await
            .err()
            .unwrap()
            .to_string();
        assert!(!error.contains(TOKEN), "{failure:?}: {error}");
        assert!(run.log.take().iter().all(|line| !line.contains(TOKEN)));
        assert_eq!(
            find(&server, "Removalsentinel").await.len(),
            1,
            "{failure:?}"
        );
    }
    fake.lock().failure = None;
    assert_eq!(plugins::run("mealie", &run).await.unwrap().stats.deleted, 1);
    assert!(find(&server, "Removalsentinel").await.is_empty());
}

#[sqlx::test(migrations = false)]
async fn worker_hides_token_and_honors_recipe_exclusions(pool: PgPool) {
    let fake = origin();
    fake.lock().recipes.truncate(2);
    let url = serve(fake).await;
    let server = start_server(pool.clone()).await;
    let worker = Worker {
        internal: server.internal(),
        public_url: server.public_url(),
        name: "mealie-test-worker".into(),
        options: PluginOptions::default(),
    };
    let id = worker.register().await;
    let cookie = server.sign_in().await;
    let created = server.graphql(&cookie, "mutation ($input: NewConnection!) { createConnection(input: $input) { id config } }", json!({"input": {"name": "Mealie", "icon": "cooking-pot", "plugin": "mealie", "source": "food/mealie", "config": json!({"url": url, "token": TOKEN}).to_string(), "schedule": "0 * * * *"}})).await;
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
    let document = find(&server, "Mainrecipesentinel").await.remove(0);
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
    assert!(find(&server, "Mainrecipesentinel").await.is_empty());
    assert_eq!(find(&server, "Otherhouseholdsentinel").await.len(), 1);
}
