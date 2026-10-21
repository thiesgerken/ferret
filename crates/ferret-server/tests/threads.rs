mod common;

use common::{TestServer, day, document, start_server};
use ferret_api::{Client, SearchQuery, ThreadQuery};
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

#[sqlx::test(migrations = false)]
async fn thread_filters_are_source_local_disjunctive_and_distinct_from_authors_across_apis(
    pool: PgPool,
) {
    use ferret_api::{SearchMode, ThreadIdentity};
    let server = common::start_semantic_server(pool).await;
    let api = server.admin_api().await;
    for (source, id, thread, authors, created, title) in [
        (
            "chats/personal",
            "incoming",
            "alice",
            vec!["Alice"],
            1,
            "Alice",
        ),
        (
            "chats/personal",
            "outgoing",
            "alice",
            vec!["Me"],
            2,
            "Alice",
        ),
        (
            "chats/personal",
            "mixed",
            "group",
            vec!["Alice", "Bob", "Me"],
            3,
            "Team",
        ),
        ("chats/personal", "bob", "group", vec!["Bob"], 4, "Team"),
        (
            "chats/work",
            "work",
            "alice",
            vec!["Alice"],
            5,
            "Private work Alice",
        ),
        (
            "chats/personal/child",
            "child",
            "alice",
            vec!["Alice"],
            6,
            "Child Alice",
        ),
    ] {
        let mut doc = document(source, id, "Car service details.");
        doc.thread = Some(thread.into());
        doc.authors = authors.into_iter().map(str::to_owned).collect();
        doc.title = Some(title.into());
        doc.created_at = Some(day(created));
        api.put_document(&doc).await.unwrap();
    }
    for index in 0..25 {
        let mut doc = document(
            "chats/personal",
            &format!("extra-{index}"),
            "Car service details.",
        );
        doc.thread = Some(format!("thread-{index}"));
        doc.title = Some(format!("Chat {index}"));
        api.put_document(&doc).await.unwrap();
    }
    server.wait_for_embeddings(31).await;
    let query = || SearchQuery {
        q: "car".into(),
        k: Some(100),
        collapse: Some(false),
        mode: SearchMode::Keyword,
        ..Default::default()
    };
    let alice = ThreadIdentity {
        source: "chats/personal".into(),
        thread: "alice".into(),
    };
    let work = ThreadIdentity {
        source: "chats/work".into(),
        thread: "alice".into(),
    };
    let group = ThreadIdentity {
        source: "chats/personal".into(),
        thread: "group".into(),
    };
    let all = api.search(&query()).await.unwrap();
    assert_eq!(
        all.facets.threads.len(),
        29,
        "thread discovery must not silently truncate low-volume chats"
    );
    let by_author = api
        .search(&SearchQuery {
            authors: vec!["Alice".into()],
            ..query()
        })
        .await
        .unwrap();
    assert_eq!(by_author.total, 4);
    assert!(
        by_author
            .hits
            .iter()
            .all(|hit| hit.external_id != "outgoing")
    );
    for mode in [
        SearchMode::Keyword,
        SearchMode::Semantic,
        SearchMode::Hybrid,
    ] {
        let by_thread = api
            .search(&SearchQuery {
                mode,
                threads: vec![alice.clone()],
                ..query()
            })
            .await
            .unwrap();
        assert_eq!(by_thread.total, 2, "{mode:?}");
        assert!(
            by_thread
                .hits
                .iter()
                .any(|hit| hit.external_id == "outgoing")
        );
        assert!(
            by_thread
                .hits
                .iter()
                .all(|hit| hit.source == "chats/personal")
        );
    }
    let alternative = api
        .search(&SearchQuery {
            threads: vec![alice.clone(), work.clone()],
            ..query()
        })
        .await
        .unwrap();
    assert_eq!(alternative.total, 3);
    let narrowed = api
        .search(&SearchQuery {
            sources: vec!["chats/personal".into()],
            threads: vec![alice.clone(), work.clone()],
            from: Some(day(2)),
            ..query()
        })
        .await
        .unwrap();
    assert_eq!(narrowed.total, 1);
    assert_eq!(narrowed.hits[0].external_id, "outgoing");
    let combined = api
        .search(&SearchQuery {
            threads: vec![alice.clone()],
            authors: vec!["Me".into()],
            ..query()
        })
        .await
        .unwrap();
    assert_eq!(combined.total, 1);
    assert_eq!(combined.hits[0].external_id, "outgoing");
    assert!(
        combined
            .facets
            .threads
            .iter()
            .any(|facet| facet.thread == "group" && facet.title.as_deref() == Some("Team"))
    );
    assert!(
        combined
            .facets
            .authors
            .iter()
            .any(|facet| facet.value == "Alice")
    );
    let in_group = api
        .search(&SearchQuery {
            threads: vec![group.clone()],
            ..query()
        })
        .await
        .unwrap();
    assert_eq!(in_group.total, 2);
    let group_author = api
        .search(&SearchQuery {
            threads: vec![group.clone()],
            authors: vec!["Alice".into()],
            ..query()
        })
        .await
        .unwrap();
    assert_eq!(group_author.total, 1);
    assert_eq!(group_author.hits[0].external_id, "mixed");
    let empty = api
        .search(&SearchQuery {
            q: "nomatchquasar".into(),
            threads: vec![group.clone()],
            ..query()
        })
        .await
        .unwrap();
    assert_eq!(empty.total, 0);
    assert_eq!(empty.facets.threads[0].title.as_deref(), Some("Team"));
    assert_eq!(empty.facets.threads[0].count, 0);
    let key = server
        .create_api_key(&["SEARCH_READ"], Some(&["chats/personal"]))
        .await;
    let restricted = server
        .api_with_key(&key)
        .search(&SearchQuery {
            threads: vec![work],
            ..query()
        })
        .await
        .unwrap();
    assert_eq!(restricted.total, 0);
    assert!(
        restricted
            .facets
            .threads
            .iter()
            .all(|facet| facet.source != "chats/work")
    );
    assert_eq!(
        server
            .request(reqwest::Method::GET, "/api/search")
            .bearer_auth(&key)
            .query(&[("q", "car"), ("thread", "{\"source\":\"chats/personal\"}")])
            .send()
            .await
            .unwrap()
            .status(),
        reqwest::StatusCode::BAD_REQUEST
    );
    let cookie = server.sign_in().await;
    let body = server.graphql(&cookie,
        "query ($search: SearchInput!, $explore: ExploreInput!) {
            search(input: $search) { total hits { externalId } facets { threads { source thread title count } } }
            explore(input: $explore) { total documents { externalId } facets { threads { source thread title count } } }
        }",
        json!({
            "search": {"q":"car","mode":"KEYWORD","collapse":false,"authors":["Me"],"threads":[alice]},
            "explore":{"authors":["Me"],"threads":[{"source":"chats/personal","thread":"alice"}]},
        }),
    ).await;
    assert_eq!(body["errors"], Value::Null, "{body}");
    for name in ["search", "explore"] {
        assert_eq!(body["data"][name]["total"], 1);
        assert!(
            body["data"][name]["facets"]["threads"]
                .as_array()
                .unwrap()
                .iter()
                .any(|facet| facet["title"] == "Team")
        );
    }
    assert_eq!(body["data"]["search"]["hits"][0]["externalId"], "outgoing");
    assert_eq!(
        body["data"]["explore"]["documents"][0]["externalId"],
        "outgoing"
    );
    let explored = server
        .graphql(
            &cookie,
            "{ explore(input: {}) { facets { threads { source thread title count } } } }",
            json!({}),
        )
        .await;
    assert_eq!(
        explored["data"]["explore"]["facets"]["threads"]
            .as_array()
            .unwrap()
            .len(),
        29
    );
}
