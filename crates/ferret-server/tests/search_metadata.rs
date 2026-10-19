mod common;

use std::time::Duration;

use common::{document, session_cookie, start_server};
use ferret_api::{SearchMode, SearchQuery};
use serde_json::{Value, json};
use sqlx::PgPool;

fn query(sources: &[&str]) -> SearchQuery {
    SearchQuery {
        q: "invoice".to_owned(),
        mode: SearchMode::Keyword,
        sources: sources.iter().map(|source| (*source).to_owned()).collect(),
        ..Default::default()
    }
}

#[sqlx::test(migrations = false)]
async fn scope_counts_are_isolated_by_user_and_effective_sources_while_hits_stay_fresh(
    pool: PgPool,
) {
    let server = start_server(pool).await;
    let cookie = server.sign_in().await;
    let api = server.admin_api().await;
    for (source, id, text) in [
        ("notes/work", "a", "invoice work"),
        ("notes/home", "b", "invoice excludedword"),
        ("mail/personal", "c", "invoice secret"),
        ("other", "d", "unrelated content"),
    ] {
        api.put_document(&document(source, id, text)).await.unwrap();
    }
    let broad = server
        .graphql(
            &cookie,
            "{ search(input: {q: \"invoice\", mode: KEYWORD}) { total documentsInScope } }",
            json!({}),
        )
        .await;
    assert_eq!(broad["errors"], Value::Null, "{broad}");
    assert_eq!(broad["data"]["search"]["documentsInScope"], 4);
    let restricted_key = server
        .create_api_key(&["SEARCH_READ"], Some(&["notes/work"]))
        .await;
    let restricted = server.api_with_key(&restricted_key);
    let first = restricted.search(&query(&["notes"])).await.unwrap();
    assert_eq!(first.documents_in_scope, 1);
    assert_eq!(first.total, 1);
    let roots = api
        .search(&query(&["notes/home", "notes/work", "notes/work"]))
        .await
        .unwrap();
    assert_eq!(roots.documents_in_scope, 2);

    api.put_document(&document("notes/work", "new", "invoice fresh"))
        .await
        .unwrap();
    let fresh = restricted.search(&query(&[])).await.unwrap();
    assert_eq!(fresh.documents_in_scope, 1);
    assert_eq!(fresh.total, 2);
    assert!(fresh.hits.iter().any(|hit| hit.external_id == "new"));
    assert!(fresh.hits.iter().all(|hit| hit.source == "notes/work"));
    let reordered = api
        .search(&query(&["notes/work", "notes/home"]))
        .await
        .unwrap();
    assert_eq!(
        reordered.documents_in_scope, 2,
        "root order and duplicates do not create another snapshot"
    );
    assert_eq!(reordered.total, 3);
    let shared = server.graphql(&cookie,
        "{ search(input: {q: \"invoice\", mode: KEYWORD, sources: [\"notes/work\"]}) { total documentsInScope } }", json!({})).await;
    assert_eq!(shared["errors"], Value::Null, "{shared}");
    assert_eq!(shared["data"]["search"]["documentsInScope"], 1);
    assert_eq!(shared["data"]["search"]["total"], 2);
    let broad_again = server
        .graphql(
            &cookie,
            "{ search(input: {q: \"invoice\", mode: KEYWORD}) { total documentsInScope } }",
            json!({}),
        )
        .await;
    assert_eq!(broad_again["data"]["search"]["documentsInScope"], 4);
    assert_eq!(broad_again["data"]["search"]["total"], 4);
    let denied = restricted.search(&query(&["mail"])).await.unwrap();
    assert_eq!(denied.documents_in_scope, 0);
    assert_eq!(denied.total, 0);
    assert!(denied.hits.is_empty());
    let excluded = api
        .search(&SearchQuery {
            q: "invoice -secret".to_owned(),
            ..query(&["mail"])
        })
        .await
        .unwrap();
    assert_eq!(
        excluded.documents_in_scope, 1,
        "query exclusions do not narrow informational source scope"
    );
    assert_eq!(excluded.total, 0);

    let user = server.graphql(&cookie,
        "mutation { createUser(input: {username: \"bob\", password: \"a long user password\", isAdmin: false}) { id } }", json!({})).await;
    assert_eq!(user["errors"], Value::Null, "{user}");
    let bob_cookie = session_cookie(&server.login("bob", "a long user password").await);
    let bob_key = server.graphql(&bob_cookie,
        "mutation { createApiKey(input: {name: \"Bob key\", icon: \"key-round\", scopes: [INGEST_WRITE, SEARCH_READ]}) { key } }", json!({})).await;
    assert_eq!(bob_key["errors"], Value::Null, "{bob_key}");
    let bob = server.api_with_key(bob_key["data"]["createApiKey"]["key"].as_str().unwrap());
    assert_eq!(bob.search(&query(&[])).await.unwrap().documents_in_scope, 0);
    bob.put_document(&document("notes/work", "bob", "invoice private"))
        .await
        .unwrap();
    let bobs_fresh = bob.search(&query(&[])).await.unwrap();
    assert_eq!(bobs_fresh.documents_in_scope, 0);
    assert_eq!(bobs_fresh.total, 1);
    assert_eq!(bobs_fresh.hits[0].external_id, "bob");

    api.delete_document("notes/work", "a").await.unwrap();
    api.delete_document("notes/work", "new").await.unwrap();
    let deleted = restricted.search(&query(&[])).await.unwrap();
    assert_eq!(deleted.documents_in_scope, 1);
    assert_eq!(deleted.total, 0);
    assert!(deleted.hits.is_empty());
}

#[sqlx::test(migrations = false)]
async fn concurrent_searches_observe_a_refreshed_scope_after_sixty_seconds(pool: PgPool) {
    let server = start_server(pool).await;
    let api = server.admin_api().await;
    api.put_document(&document("notes", "first", "invoice first"))
        .await
        .unwrap();
    let initial = api.search(&query(&[])).await.unwrap();
    assert_eq!(initial.documents_in_scope, 1);
    api.put_document(&document("notes", "second", "invoice second"))
        .await
        .unwrap();
    let cached = api.search(&query(&[])).await.unwrap();
    assert_eq!(cached.documents_in_scope, 1);
    assert_eq!(cached.total, 2);

    tokio::time::sleep(Duration::from_secs(61)).await;
    let responses = futures_util::future::join_all(
        (0..16).map(|_| async { api.search(&query(&[])).await.unwrap() }),
    )
    .await;
    for response in responses {
        assert_eq!(response.documents_in_scope, 2);
        assert_eq!(response.total, 2);
        assert_eq!(response.hits.len(), 2);
    }
    api.delete_document("notes", "first").await.unwrap();
    api.delete_document("notes", "second").await.unwrap();
    let deleted = api.search(&query(&[])).await.unwrap();
    assert_eq!(deleted.documents_in_scope, 2);
    assert_eq!(deleted.total, 0);
}
