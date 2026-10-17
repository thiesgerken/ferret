mod common;

use common::{TestServer, day, document, start_server};
use ferret_api::{Client, SearchQuery, ThreadQuery};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

/// A mail thread of three invoices in `mail/zoho`, written out of order, a lone invoice mail, and a
/// thread of the same name in another source. Returns the thread's ids, oldest first.
async fn ingest(api: &Client) -> Vec<Uuid> {
    let mut thread = Vec::new();
    for (external_id, created) in [("reply-2", 12), ("start", 10), ("reply-1", 11)] {
        let mut mail = document("mail/zoho", external_id, "About the invoice for March.");
        mail.thread = Some("invoices".to_owned());
        mail.created_at = day(created);
        thread.push((created, api.put_document(&mail).await.unwrap().id));
    }
    api.put_document(&document("mail/zoho", "lone", "Another invoice."))
        .await
        .unwrap();
    let mut elsewhere = document("mail/gmail", "other", "An invoice somewhere else.");
    elsewhere.thread = Some("invoices".to_owned());
    api.put_document(&elsewhere).await.unwrap();
    thread.sort();
    thread.into_iter().map(|(_, id)| id).collect()
}

fn invoices() -> SearchQuery {
    SearchQuery {
        q: "invoice".to_owned(),
        ..Default::default()
    }
}

fn thread_query() -> ThreadQuery {
    ThreadQuery {
        source: "mail/zoho".to_owned(),
        thread: "invoices".to_owned(),
    }
}

#[sqlx::test(migrations = false)]
async fn a_thread_lists_its_documents_oldest_first(pool: PgPool) {
    let server = start_server(pool).await;
    let api = server.admin_api().await;
    let thread = ingest(&api).await;

    let listed = api.thread(&thread_query()).await.unwrap().documents;

    assert_eq!(
        listed
            .iter()
            .map(|document| document.id)
            .collect::<Vec<_>>(),
        thread
    );
    assert_eq!(listed[0].external_id, "start");
}

#[sqlx::test(migrations = false)]
async fn a_key_restricted_to_other_sources_sees_no_thread(pool: PgPool) {
    let server = start_server(pool).await;
    ingest(&server.admin_api().await).await;
    let key = server
        .create_api_key(&["SEARCH_READ"], Some(&["mail/gmail"]))
        .await;

    let listed = server
        .api_with_key(&key)
        .thread(&thread_query())
        .await
        .unwrap();

    assert!(listed.documents.is_empty());
}

#[sqlx::test(migrations = false)]
async fn search_collapses_a_thread_into_its_best_hit(pool: PgPool) {
    let server = start_server(pool).await;
    let api = server.admin_api().await;
    ingest(&api).await;

    let collapsed = api.search(&invoices()).await.unwrap().hits;
    let all = api
        .search(&SearchQuery {
            collapse: Some(false),
            ..invoices()
        })
        .await
        .unwrap()
        .hits;

    let mut groups: Vec<_> = collapsed
        .iter()
        .map(|hit| (hit.source.as_str(), hit.thread.as_deref(), hit.collapsed))
        .collect();
    groups.sort();
    assert_eq!(
        groups,
        [
            ("mail/gmail", Some("invoices"), 0),
            ("mail/zoho", None, 0),
            ("mail/zoho", Some("invoices"), 2),
        ]
    );
    assert_eq!(all.len(), 5);
    assert!(all.iter().all(|hit| hit.collapsed == 0));
}

#[sqlx::test(migrations = false)]
async fn the_viewer_steps_through_a_thread(pool: PgPool) {
    let server = start_server(pool).await;
    let thread = ingest(&server.admin_api().await).await;
    let cookie = server.sign_in().await;

    let around = |id: Uuid| neighbors(&server, &cookie, id);

    assert_eq!(around(thread[0]).await, (None, Some(thread[1])));
    assert_eq!(around(thread[1]).await, (Some(thread[0]), Some(thread[2])));
    assert_eq!(around(thread[2]).await, (Some(thread[1]), None));
}

async fn neighbors(server: &TestServer, cookie: &str, id: Uuid) -> (Option<Uuid>, Option<Uuid>) {
    let body = server
        .graphql(
            cookie,
            "query ($id: ID!) { document(id: $id) { previousInThread { id } nextInThread { id } } }",
            json!({"id": id}),
        )
        .await;
    assert_eq!(body["errors"], Value::Null, "{body}");
    let id_of = |neighbor: &Value| neighbor["id"].as_str().map(|id| id.parse().unwrap());
    let document = &body["data"]["document"];
    (
        id_of(&document["previousInThread"]),
        id_of(&document["nextInThread"]),
    )
}
