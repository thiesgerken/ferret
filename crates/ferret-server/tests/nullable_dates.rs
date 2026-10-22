use crate::common;

use common::{day, document, start_semantic_server};
use ferret_api::{PutResponse, SearchMode, SearchQuery, ThreadQuery};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

#[sqlx::test(migrations = false)]
async fn unknown_origin_dates_remain_searchable_and_update_reused_embeddings(pool: PgPool) {
    let server = start_semantic_server(pool.clone()).await;
    let api = server.admin_api().await;
    let mut note = document("notes", "undated", "My cobalt car needs new tyres.");
    note.created_at = None;
    note.modified_at = None;
    let id = api.put_document(&note).await.unwrap().id().unwrap();
    assert_eq!(
        api.put_document(&note).await.unwrap(),
        PutResponse::Unchanged { id }
    );
    server.wait_for_embeddings(1).await;
    let chunk: Uuid = sqlx::query_scalar("select id from chunks where document_id = $1")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    let embedded_before = server.endpoint.embedded();
    let cookie = server.sign_in().await;
    for mode in [
        SearchMode::Keyword,
        SearchMode::Semantic,
        SearchMode::Hybrid,
    ] {
        let result = api
            .search(&SearchQuery {
                q: "cobalt".into(),
                mode,
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(result.hits.len(), 1);
        assert!(result.hits[0].created_at.is_none() && result.hits[0].modified_at.is_none());
        assert!(
            api.search(&SearchQuery {
                q: "cobalt".into(),
                mode,
                from: Some(day(1)),
                ..Default::default()
            })
            .await
            .unwrap()
            .hits
            .is_empty()
        );
    }
    let graphql = server.graphql(&cookie,
        "query ($id: ID!) { document(id: $id) { createdAt modifiedAt } search(input: {q: \"cobalt\"}) { hits { createdAt modifiedAt } } explore(input: {}) { documents { createdAt modifiedAt } facets { months { value } } } }",
        json!({"id":id})).await;
    assert_eq!(graphql["errors"], Value::Null, "{graphql}");
    assert_eq!(
        graphql["data"]["document"],
        json!({"createdAt":null,"modifiedAt":null})
    );
    assert_eq!(graphql["data"]["explore"]["facets"]["months"], json!([]));
    for (created, modified) in [
        (Some(day(1)), None),
        (None, Some(day(2))),
        (Some(day(3)), Some(day(4))),
        (None, None),
    ] {
        note.created_at = created;
        note.modified_at = modified;
        assert_eq!(
            api.put_document(&note).await.unwrap(),
            PutResponse::Updated { id }
        );
        let stored = api.document(id).await.unwrap();
        assert_eq!((stored.created_at, stored.modified_at), (created, modified));
        let rows: Vec<(Uuid, bool)> = sqlx::query_as(
            "select c.id, c.ts is not distinct from $2 and e.ts is not distinct from $2
             from chunks c join chunk_embeddings e on e.chunk_id=c.id where c.document_id=$1",
        )
        .bind(id)
        .bind(created)
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(rows, vec![(chunk, true)]);
        assert_eq!(
            api.put_document(&note).await.unwrap(),
            PutResponse::Unchanged { id }
        );
    }
    // Date-only metadata updates must not discard or recompute an unchanged embedded input.
    assert_eq!(
        server
            .endpoint
            .embedded()
            .iter()
            .filter(|s| s.starts_with(common::DOCUMENT_PREFIX))
            .count(),
        embedded_before
            .iter()
            .filter(|s| s.starts_with(common::DOCUMENT_PREFIX))
            .count()
    );
}

#[sqlx::test(migrations = false)]
async fn the_thread_outline_follows_the_listing_without_fake_dates(pool: PgPool) {
    let server = common::start_server(pool).await;
    let api = server.admin_api().await;
    let mut ids = Vec::new();
    for (external_id, date) in [
        ("dated", Some(day(1))),
        ("unknown-a", None),
        ("unknown-b", None),
    ] {
        let mut note = document("mail", external_id, "Cobalt correspondence");
        note.thread = Some("cobalt-thread".into());
        note.created_at = date;
        ids.push(api.put_document(&note).await.unwrap().id().unwrap());
    }
    let listing = api
        .thread(&ThreadQuery {
            source: "mail".into(),
            thread: "cobalt-thread".into(),
        })
        .await
        .unwrap();
    let ordered: Vec<_> = listing.documents.iter().map(|d| d.id).collect();
    assert_eq!(ordered[0], ids[0]);
    let cookie = server.sign_in().await;
    let result = server
        .graphql(
            &cookie,
            "query ($id: ID!) { document(id:$id) { threadOutline { entries { id createdAt } } } }",
            json!({"id": ordered[1]}),
        )
        .await;
    assert_eq!(result["errors"], Value::Null, "{result}");
    let entries = result["data"]["document"]["threadOutline"]["entries"]
        .as_array()
        .unwrap();
    let listed: Vec<Value> = entries.iter().map(|entry| entry["id"].clone()).collect();
    assert_eq!(
        listed,
        ordered.iter().map(|id| json!(id)).collect::<Vec<_>>()
    );
    assert_eq!(entries[1]["createdAt"], Value::Null);
}
