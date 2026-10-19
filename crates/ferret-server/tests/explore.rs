mod common;

use chrono::{TimeZone, Utc};
use common::{TestServer, document, start_server};
use ferret_api::{Client, ContentType};
use serde_json::{Value, json};
use sqlx::PgPool;

const EXPLORE: &str = "query ($input: ExploreInput!) {
    explore(input: $input) {
        total
        documents { externalId }
        facets {
            sources { value count }
            contentTypes { contentType count }
            authors { value count }
            months { value count }
        }
    }
}";

/// Four documents over two sources, two content types, three authors and two months.
async fn ingest(api: &Client) {
    let docs = [
        (
            "notes/work",
            "a",
            ContentType::Markdown,
            "Ada",
            (2026, 1, 5),
        ),
        (
            "notes/work",
            "b",
            ContentType::Plain,
            "Grace",
            (2026, 1, 20),
        ),
        (
            "notes/home",
            "c",
            ContentType::Markdown,
            "Ada",
            (2026, 2, 1),
        ),
        (
            "mail/personal",
            "d",
            ContentType::Plain,
            "Linus",
            (2026, 2, 14),
        ),
    ];
    for (source, external_id, content_type, author, (year, month, day)) in docs {
        let mut put = document(source, external_id, "Some text.");
        put.content_type = content_type;
        put.authors = vec![author.to_owned()];
        put.created_at = Some(Utc.with_ymd_and_hms(year, month, day, 12, 0, 0).unwrap());
        api.put_document(&put).await.unwrap();
    }
}

async fn explore(server: &TestServer, input: Value) -> Value {
    let cookie = server.sign_in().await;
    let body = server
        .graphql(&cookie, EXPLORE, json!({"input": input}))
        .await;
    assert_eq!(body["errors"], Value::Null, "{body}");
    body["data"]["explore"].clone()
}

fn external_ids(exploration: &Value) -> Vec<&str> {
    exploration["documents"]
        .as_array()
        .unwrap()
        .iter()
        .map(|document| document["externalId"].as_str().unwrap())
        .collect()
}

#[sqlx::test(migrations = false)]
async fn lists_every_document_newest_first_with_facet_counts(pool: PgPool) {
    let server = start_server(pool.clone()).await;
    ingest(&server.admin_api().await).await;
    // Another user's document counts nowhere. No user management yet, so it comes from SQL.
    sqlx::query(
        "with bob as (
             insert into users (id, username, password_hash, created_at, password_changed_at)
             values (uuidv7(), 'bob', 'unused', now(), now())
             returning id
         )
         insert into documents (id, user_id, source, external_id, content_type, authors,
                                created_at, modified_at, meta, text, content_hash, seen_at)
         select uuidv7(), id, 'mail/personal', 'bobs', 'plain', '{Bob}', now(), now(), '{}', 'Bob.',
                '\\x00', now()
         from bob",
    )
    .execute(&pool)
    .await
    .unwrap();

    let exploration = explore(&server, json!({})).await;

    assert_eq!(exploration["total"], 4);
    assert_eq!(external_ids(&exploration), ["d", "c", "b", "a"]);
    assert_eq!(
        exploration["facets"],
        json!({
            "sources": [
                {"value": "notes/work", "count": 2},
                {"value": "mail/personal", "count": 1},
                {"value": "notes/home", "count": 1},
            ],
            "contentTypes": [
                {"contentType": "MARKDOWN", "count": 2},
                {"contentType": "PLAIN", "count": 2},
            ],
            "authors": [
                {"value": "Ada", "count": 2},
                {"value": "Grace", "count": 1},
                {"value": "Linus", "count": 1},
            ],
            "months": [
                {"value": "2026-02", "count": 2},
                {"value": "2026-01", "count": 2},
            ],
        })
    );
}

#[sqlx::test(migrations = false)]
async fn filters_like_search_and_counts_each_facet_without_its_own_filter(pool: PgPool) {
    let server = start_server(pool).await;
    ingest(&server.admin_api().await).await;

    let exploration = explore(
        &server,
        json!({"sources": ["notes"], "authors": ["Ada"], "orderBy": "TITLE", "descending": false}),
    )
    .await;

    assert_eq!(exploration["total"], 2);
    assert_eq!(external_ids(&exploration), ["a", "c"]);
    let facets = &exploration["facets"];
    // Sources ignore the source filter but keep the author one: Ada wrote in both notes sources.
    // The selected parent counts what it matches.
    assert_eq!(
        facets["sources"],
        json!([
            {"value": "notes", "count": 2},
            {"value": "notes/home", "count": 1},
            {"value": "notes/work", "count": 1},
        ])
    );
    // Authors ignore the author filter but keep the source one: Linus only wrote mail.
    assert_eq!(
        facets["authors"],
        json!([{"value": "Ada", "count": 2}, {"value": "Grace", "count": 1}])
    );
}

#[sqlx::test(migrations = false)]
async fn a_selected_author_counts_beyond_the_most_frequent_ones(pool: PgPool) {
    let server = start_server(pool).await;
    let api = server.admin_api().await;
    for author in 0..20 {
        for copy in 0..2 {
            let mut put = document("notes", &format!("{author}-{copy}"), "Some text.");
            put.authors = vec![format!("Author {author:02}")];
            api.put_document(&put).await.unwrap();
        }
    }
    let mut rare = document("notes", "rare", "Some text.");
    rare.authors = vec!["Zed".to_owned()];
    api.put_document(&rare).await.unwrap();

    let all = explore(&server, json!({})).await;
    let selected = explore(&server, json!({"authors": ["Zed"]})).await;

    let authors =
        |exploration: &Value| exploration["facets"]["authors"].as_array().unwrap().clone();
    assert_eq!(authors(&all).len(), 20);
    assert!(!authors(&all).contains(&json!({"value": "Zed", "count": 1})));
    assert_eq!(authors(&selected).len(), 21);
    assert_eq!(authors(&selected)[20], json!({"value": "Zed", "count": 1}));
}

#[sqlx::test(migrations = false)]
async fn pages_through_the_documents_and_filters_by_month(pool: PgPool) {
    let server = start_server(pool).await;
    ingest(&server.admin_api().await).await;

    let second_page = explore(
        &server,
        json!({"orderBy": "CREATED_AT", "descending": false, "offset": 2, "limit": 2}),
    )
    .await;
    let january = explore(
        &server,
        json!({"from": "2026-01-01T00:00:00Z", "to": "2026-02-01T00:00:00Z"}),
    )
    .await;

    assert_eq!(second_page["total"], 4);
    assert_eq!(external_ids(&second_page), ["c", "d"]);
    assert_eq!(external_ids(&january), ["b", "a"]);
    assert_eq!(
        january["facets"]["months"],
        json!([{"value": "2026-02", "count": 2}, {"value": "2026-01", "count": 2}]),
        "the months facet ignores the time filter"
    );
}

#[sqlx::test(migrations = false)]
async fn rejects_what_search_rejects(pool: PgPool) {
    let server = start_server(pool).await;
    let cookie = server.sign_in().await;

    for input in [json!({"sources": ["notes/"]}), json!({"limit": 101})] {
        let body = server
            .graphql(&cookie, EXPLORE, json!({"input": input}))
            .await;
        assert_eq!(
            body["errors"][0]["extensions"]["code"], "INVALID_INPUT",
            "{input}"
        );
    }
}
