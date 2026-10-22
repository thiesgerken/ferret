use crate::common;

use common::{TestServer, day, document, start_server};
use ferret_api::{Client, SearchQuery, ThreadQuery};
use reqwest::StatusCode;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

/// A mail thread of three invoices in `mail/personal`, written out of order, a lone invoice mail, and a
/// thread of the same name in another source. Returns the thread's ids, oldest first.
async fn ingest(api: &Client) -> Vec<Uuid> {
    let mut thread = Vec::new();
    for (external_id, created) in [("reply-2", 12), ("start", 10), ("reply-1", 11)] {
        let mut mail = document("mail/personal", external_id, "About the invoice for March.");
        mail.thread = Some("invoices".to_owned());
        mail.created_at = Some(day(created));
        thread.push((
            created,
            api.put_document(&mail).await.unwrap().id().unwrap(),
        ));
    }
    api.put_document(&document("mail/personal", "lone", "Another invoice."))
        .await
        .unwrap();
    let mut elsewhere = document("mail/other", "other", "An invoice somewhere else.");
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
        source: "mail/personal".to_owned(),
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
            ("mail/other", Some("invoices"), 0),
            ("mail/personal", None, 0),
            ("mail/personal", Some("invoices"), 2),
        ]
    );
    assert_eq!(all.len(), 5);
    assert!(all.iter().all(|hit| hit.collapsed == 0));
}

#[sqlx::test(migrations = false)]
async fn k_counts_threads_not_documents(pool: PgPool) {
    let server = start_server(pool).await;
    let api = server.admin_api().await;
    ingest(&api).await;

    let hits = api
        .search(&SearchQuery {
            sources: vec!["mail/personal".to_owned()],
            k: Some(2),
            ..invoices()
        })
        .await
        .unwrap()
        .hits;

    let mut groups: Vec<_> = hits
        .iter()
        .map(|hit| (hit.thread.as_deref(), hit.collapsed))
        .collect();
    groups.sort();
    assert_eq!(groups, [(None, 0), (Some("invoices"), 2)]);
}

#[sqlx::test(migrations = false)]
async fn the_viewer_outlines_a_branching_thread(pool: PgPool) {
    let server = start_server(pool).await;
    let api = server.admin_api().await;
    let mut ids = Vec::new();
    for (external_id, parent, created) in [
        ("start", None, 10),
        ("reply-1", Some("start"), 11),
        ("reply-2", Some("start"), 12),
        ("reply-1-1", Some("reply-1"), 13),
    ] {
        let mut mail = document("mail/personal", external_id, "About the invoice for March.");
        mail.thread = Some("invoices".to_owned());
        mail.parent = parent.map(str::to_owned);
        mail.created_at = Some(day(created));
        ids.push(api.put_document(&mail).await.unwrap().id().unwrap());
    }
    let cookie = server.sign_in().await;

    let outline = outline(&server, &cookie, ids[3]).await;

    assert_eq!(outline["total"], 4);
    let entries: Vec<_> = outline["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            (
                entry["externalId"].as_str().unwrap(),
                entry["parent"].as_str(),
            )
        })
        .collect();
    assert_eq!(
        entries,
        [
            ("start", None),
            ("reply-1", Some("start")),
            ("reply-2", Some("start")),
            ("reply-1-1", Some("reply-1")),
        ]
    );
}

#[sqlx::test(migrations = false)]
async fn a_long_thread_outlines_the_window_around_the_document(pool: PgPool) {
    const LIMIT: usize = 1000;
    let server = start_server(pool).await;
    let api = server.admin_api().await;
    let mut ids = Vec::new();
    for n in 0..LIMIT + 3 {
        let mut segment = document("chats/family", &format!("segment-{n:04}"), "Good morning");
        segment.thread = Some("family".to_owned());
        segment.created_at = Some(day(1) + chrono::Duration::minutes(n as i64));
        ids.push(api.put_document(&segment).await.unwrap().id().unwrap());
    }
    let cookie = server.sign_in().await;
    let window = async |id: Uuid| {
        let outline = outline(&server, &cookie, id).await;
        assert_eq!(outline["total"], LIMIT + 3);
        let entries = outline["entries"].as_array().unwrap().clone();
        assert_eq!(entries.len(), LIMIT);
        let id_of = |entry: &Value| entry["id"].as_str().unwrap().parse::<Uuid>().unwrap();
        (id_of(&entries[0]), id_of(&entries[LIMIT - 1]))
    };

    assert_eq!(window(ids[0]).await, (ids[0], ids[LIMIT - 1]));
    assert_eq!(window(ids[LIMIT + 2]).await, (ids[3], ids[LIMIT + 2]));
    assert_eq!(window(ids[2]).await, (ids[0], ids[LIMIT - 1]));
}

#[sqlx::test(migrations = false)]
async fn a_parent_needs_a_thread_and_another_document(pool: PgPool) {
    let server = start_server(pool).await;
    let api = server.admin_api().await;
    let mut orphan = document("mail/personal", "reply", "Re: invoice");
    orphan.parent = Some("start".to_owned());
    let mut own = orphan.clone();
    own.thread = Some("invoices".to_owned());
    own.parent = Some("reply".to_owned());

    for invalid in [orphan, own] {
        let error = api.put_document(&invalid).await.unwrap_err();
        assert_eq!(error.status(), Some(StatusCode::BAD_REQUEST));
    }
}

async fn outline(server: &TestServer, cookie: &str, id: Uuid) -> Value {
    let body = server
        .graphql(
            cookie,
            "query ($id: ID!) { document(id: $id) {
                threadOutline { total entries { id externalId parent } }
            } }",
            json!({"id": id}),
        )
        .await;
    assert_eq!(body["errors"], Value::Null, "{body}");
    body["data"]["document"]["threadOutline"].clone()
}
