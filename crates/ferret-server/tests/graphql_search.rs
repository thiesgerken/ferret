mod common;

use chrono::{DateTime, Utc};
use common::{TestServer, day, document, start_server};
use ferret_api::{Client, ContentType, SearchQuery};
use serde_json::{Value, json};
use sqlx::PgPool;

const SEARCH: &str = "query ($input: SearchInput!) {
    search(input: $input) {
        hits {
            documentId externalId source contentType title url thread authors createdAt modifiedAt collapsed
            chunk { startOffset endOffset parts { text highlight } }
        }
        semanticUnavailable
    }
}";

/// Three documents with an invoice each, by different authors, in different sources and on
/// different days.
async fn ingest(api: &Client) {
    let mut paperless = document(
        "paperless/2026",
        "invoice-a",
        "Invoice for the car insurance.",
    );
    paperless.authors = vec!["Allianz".to_owned()];
    let mut mail = document("mail/personal", "invoice-b", "Your invoice is attached.");
    mail.authors = vec!["Stadtwerke".to_owned(), "billing".to_owned()];
    mail.created_at = Some(day(10));
    mail.content_type = ContentType::Email;
    mail.thread = Some("invoices".to_owned());
    let mut notes = document("notes", "invoice-c", "Pay the invoice before Friday.");
    notes.created_at = Some(day(20));
    notes.title = Some("Friday".to_owned());
    notes.url = Some("https://notes.example.test/invoice-c".to_owned());
    for document in [paperless, mail, notes] {
        api.put_document(&document).await.unwrap();
    }
}

/// The hits of a GraphQL search, in the REST response's JSON shape, to compare the two.
async fn graphql_hits(server: &TestServer, cookie: &str, input: Value) -> Vec<Value> {
    let body = server
        .graphql(cookie, SEARCH, json!({"input": input}))
        .await;
    assert_eq!(body["errors"], Value::Null, "{body}");
    assert_eq!(body["data"]["search"]["semanticUnavailable"], Value::Null);
    body["data"]["search"]["hits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|hit| {
            let chunk = &hit["chunk"];
            json!({
                "document_id": hit["documentId"],
                "external_id": hit["externalId"],
                "source": hit["source"],
                "content_type": hit["contentType"].as_str().unwrap().to_lowercase(),
                "title": hit["title"],
                "url": hit["url"],
                "thread": hit["thread"],
                "authors": hit["authors"],
                "created_at": timestamp(&hit["createdAt"]),
                "modified_at": timestamp(&hit["modifiedAt"]),
                "collapsed": hit["collapsed"],
                "chunk": {
                    "start_offset": chunk["startOffset"],
                    "end_offset": chunk["endOffset"],
                    "parts": chunk["parts"],
                },
            })
        })
        .collect()
}

/// GraphQL's and serde's RFC 3339 strings may differ in precision, the instant may not.
fn timestamp(value: &Value) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value.as_str().unwrap())
        .unwrap()
        .with_timezone(&Utc)
}

async fn rest_hits(api: &Client, query: SearchQuery) -> Vec<Value> {
    api.search(&query)
        .await
        .unwrap()
        .hits
        .into_iter()
        .map(|hit| {
            json!({
                "document_id": hit.document_id,
                "external_id": hit.external_id,
                "source": hit.source,
                "content_type": hit.content_type,
                "title": hit.title,
                "url": hit.url,
                "thread": hit.thread,
                "authors": hit.authors,
                "created_at": hit.created_at,
                "modified_at": hit.modified_at,
                "collapsed": hit.collapsed,
                "chunk": {
                    "start_offset": hit.chunk.start_offset,
                    "end_offset": hit.chunk.end_offset,
                    "parts": hit.chunk.parts,
                },
            })
        })
        .collect()
}

#[sqlx::test(migrations = false)]
async fn graphql_search_returns_the_same_hits_as_rest(pool: PgPool) {
    let server = start_server(pool).await;
    let api = server.admin_api().await;
    ingest(&api).await;
    let cookie = server.sign_in().await;

    let cases = [
        (json!({"q": "invoice"}), SearchQuery::default()),
        (
            json!({"q": "invoice", "sources": ["paperless", "notes"]}),
            SearchQuery {
                sources: vec!["paperless".to_owned(), "notes".to_owned()],
                ..Default::default()
            },
        ),
        (
            json!({"q": "invoice", "authors": ["billing"]}),
            SearchQuery {
                authors: vec!["billing".to_owned()],
                ..Default::default()
            },
        ),
        (
            json!({"q": "invoice", "contentTypes": ["PLAIN"]}),
            SearchQuery {
                content_types: vec![ContentType::Plain],
                ..Default::default()
            },
        ),
        (
            json!({"q": "invoice", "from": day(5), "to": day(15)}),
            SearchQuery {
                from: Some(day(5)),
                to: Some(day(15)),
                ..Default::default()
            },
        ),
    ];
    for (input, query) in cases {
        let query = SearchQuery {
            q: "invoice".to_owned(),
            ..query
        };
        let graphql = graphql_hits(&server, &cookie, input.clone()).await;
        assert!(!graphql.is_empty(), "{input}");
        assert_eq!(graphql, rest_hits(&api, query).await, "{input}");
    }
}

#[sqlx::test(migrations = false)]
async fn the_author_filter_keeps_documents_with_any_of_the_authors(pool: PgPool) {
    let server = start_server(pool).await;
    let api = server.admin_api().await;
    ingest(&api).await;

    let hits = api
        .search(&SearchQuery {
            q: "invoice".to_owned(),
            authors: vec!["Allianz".to_owned(), "billing".to_owned()],
            ..Default::default()
        })
        .await
        .unwrap()
        .hits;

    let mut ids: Vec<_> = hits.iter().map(|hit| hit.external_id.as_str()).collect();
    ids.sort();
    assert_eq!(ids, ["invoice-a", "invoice-b"]);
}

#[sqlx::test(migrations = false)]
async fn graphql_rejects_a_search_like_rest_does(pool: PgPool) {
    let server = start_server(pool).await;
    let cookie = server.sign_in().await;

    let body = server
        .graphql(&cookie, SEARCH, json!({"input": {"q": "-only -excluded"}}))
        .await;

    assert_eq!(
        body["errors"][0]["message"],
        "the query needs at least one word that isn't excluded"
    );
    assert_eq!(body["errors"][0]["extensions"]["code"], "INVALID_INPUT");
}

#[sqlx::test(migrations = false)]
async fn graphql_returns_a_whole_document_or_null(pool: PgPool) {
    let server = start_server(pool).await;
    let api = server.admin_api().await;
    let mut note = document("notes", "projects/ferret.md", "# ferret\n\nA search index.");
    note.content_type = ContentType::Markdown;
    note.url = Some("obsidian://open?file=projects/ferret.md".to_owned());
    let id = api.put_document(&note).await.unwrap().id().unwrap();
    let cookie = server.sign_in().await;
    let query = "query ($id: ID!) {
        document(id: $id) { id contentType title text url authors createdAt }
    }";

    let found = server
        .graphql(&cookie, query, json!({"id": id.to_string()}))
        .await;
    let missing = server
        .graphql(
            &cookie,
            query,
            json!({"id": "00000000-0000-0000-0000-000000000000"}),
        )
        .await;

    assert_eq!(
        found["data"]["document"],
        json!({
            "id": id.to_string(),
            "contentType": "MARKDOWN",
            "title": "projects/ferret.md",
            "text": "# ferret\n\nA search index.",
            "url": "obsidian://open?file=projects/ferret.md",
            "authors": ["someone"],
            "createdAt": "2026-01-01T12:00:00Z",
        })
    );
    assert_eq!(missing["data"]["document"], Value::Null);
}

fn source_counts(facets: &[ferret_api::SourceCount]) -> Vec<(&str, u32)> {
    facets
        .iter()
        .map(|facet| (facet.value.as_str(), facet.count))
        .collect()
}

fn counts(facets: &[ferret_api::FacetCount]) -> Vec<(&str, u32)> {
    facets
        .iter()
        .map(|facet| (facet.value.as_str(), facet.count))
        .collect()
}

#[sqlx::test(migrations = false)]
async fn each_facet_counts_the_matches_under_every_filter_but_its_own(pool: PgPool) {
    let server = start_server(pool).await;
    let api = server.admin_api().await;
    ingest(&api).await;

    let response = api
        .search(&SearchQuery {
            q: "invoice".to_owned(),
            sources: vec!["notes".to_owned()],
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(response.total, 1);
    assert_eq!(
        source_counts(&response.facets.sources),
        [("mail/personal", 1), ("notes", 1), ("paperless/2026", 1)]
    );
    assert_eq!(counts(&response.facets.authors), [("someone", 1)]);
    assert!(!response.facets.are_lower_bounds);

    let response = api
        .search(&SearchQuery {
            q: "invoice".to_owned(),
            authors: vec!["Allianz".to_owned()],
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(
        source_counts(&response.facets.sources),
        [("paperless/2026", 1)]
    );
    assert_eq!(
        counts(&response.facets.authors),
        [
            ("Allianz", 1),
            ("Stadtwerke", 1),
            ("billing", 1),
            ("someone", 1)
        ]
    );
}

#[sqlx::test(migrations = false)]
async fn pages_go_through_the_ranking_once_with_the_same_total(pool: PgPool) {
    let server = start_server(pool).await;
    let api = server.admin_api().await;
    ingest(&api).await;

    let mut seen = Vec::new();
    for offset in 0..4 {
        let response = api
            .search(&SearchQuery {
                q: "invoice".to_owned(),
                k: Some(1),
                offset: Some(offset),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(response.total, 3, "page {offset}");
        assert_eq!(response.facets.sources.len(), 3, "page {offset}");
        assert_eq!(response.has_more, offset < 2, "page {offset}");
        seen.extend(response.hits.into_iter().map(|hit| hit.external_id));
    }
    seen.sort();
    assert_eq!(seen, ["invoice-a", "invoice-b", "invoice-c"]);
}

#[sqlx::test(migrations = false)]
async fn graphql_has_paging_facets_and_the_models_cut_off(pool: PgPool) {
    let server = start_server(pool).await;
    let api = server.admin_api().await;
    ingest(&api).await;
    let cookie = server.sign_in().await;

    let response = server
        .graphql(
            &cookie,
            "query ($input: SearchInput!) {
                search(input: $input) {
                    hits { externalId }
                    hasMore
                    facets { sources { value count } authors { value count } areLowerBounds }
                }
                embeddingModels { minSimilarity }
            }",
            json!({"input": {"q": "invoice", "k": 2, "offset": 1}}),
        )
        .await;

    assert_eq!(response["errors"], Value::Null, "{response}");
    let search = &response["data"]["search"];
    assert_eq!(search["hits"].as_array().unwrap().len(), 2);
    assert_eq!(search["hasMore"], false);
    assert_eq!(search["facets"]["sources"].as_array().unwrap().len(), 3);
    assert_eq!(search["facets"]["areLowerBounds"], false);
    assert_eq!(response["data"]["embeddingModels"][0]["minSimilarity"], 0.0);

    let negative = server
        .graphql(
            &cookie,
            "query ($input: SearchInput!) { search(input: $input) { hasMore } }",
            json!({"input": {"q": "invoice", "offset": -1}}),
        )
        .await;
    assert!(
        negative["errors"][0]["message"]
            .as_str()
            .unwrap()
            .contains("offset")
    );
}

#[sqlx::test(migrations = false)]
async fn a_selected_parent_root_gets_its_covering_count(pool: PgPool) {
    let server = start_server(pool).await;
    let api = server.admin_api().await;
    ingest(&api).await;

    let response = api
        .search(&SearchQuery {
            q: "invoice".to_owned(),
            sources: vec!["mail".to_owned()],
            ..Default::default()
        })
        .await
        .unwrap();

    assert_eq!(
        source_counts(&response.facets.sources),
        [
            ("mail", 1),
            ("mail/personal", 1),
            ("notes", 1),
            ("paperless/2026", 1)
        ]
    );
}

#[sqlx::test(migrations = false)]
async fn a_collapsed_thread_counts_for_every_author_of_its_members(pool: PgPool) {
    let server = start_server(pool).await;
    let api = server.admin_api().await;
    ingest(&api).await;
    let mut reply = document("mail/personal", "invoice-reply", "Got the invoice, thanks.");
    reply.thread = Some("invoices".to_owned());
    reply.authors = vec!["Anna".to_owned()];
    api.put_document(&reply).await.unwrap();

    let response = api
        .search(&SearchQuery {
            q: "invoice".to_owned(),
            ..Default::default()
        })
        .await
        .unwrap();

    assert_eq!(response.total, 3, "the thread collapses into one hit");
    let authors = counts(&response.facets.authors);
    assert!(authors.contains(&("Anna", 1)), "{authors:?}");
    assert!(authors.contains(&("Stadtwerke", 1)), "{authors:?}");

    let anna = api
        .search(&SearchQuery {
            q: "invoice".to_owned(),
            authors: vec!["Anna".to_owned()],
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(anna.total, 1, "the facet's count is what the filter gives");
}
