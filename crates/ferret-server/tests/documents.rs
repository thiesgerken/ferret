mod common;

use common::{day, document, json_body, start_server};
use ferret_api::{ContentType, PutResponse};
use reqwest::{Method, StatusCode, header};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

async fn chunk_ids(pool: &PgPool) -> Vec<(Uuid, String)> {
    sqlx::query_as("select id, text from chunks order by ordinal")
        .fetch_all(pool)
        .await
        .unwrap()
}

#[sqlx::test(migrations = false)]
async fn a_put_creates_then_skips_unchanged_then_updates(pool: PgPool) {
    let server = start_server(pool.clone()).await;
    let api = server.admin_api().await;
    let mut note = document("notes", "a.md", "Some text.");

    let created = api.put_document(&note).await.unwrap();
    let seen_after_create: chrono::DateTime<chrono::Utc> =
        sqlx::query_scalar("select seen_at from documents")
            .fetch_one(&pool)
            .await
            .unwrap();
    let unchanged = api.put_document(&note).await.unwrap();
    note.title = Some("A new title".to_owned());
    let updated = api.put_document(&note).await.unwrap();

    let id = created.id().unwrap();
    assert_eq!(created, PutResponse::Created { id });
    assert_eq!(unchanged, PutResponse::Unchanged { id });
    assert_eq!(updated, PutResponse::Updated { id });
    let seen_after_unchanged: chrono::DateTime<chrono::Utc> =
        sqlx::query_scalar("select seen_at from documents")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(seen_after_unchanged > seen_after_create);
    assert_eq!(
        api.document(id).await.unwrap().title.unwrap(),
        "A new title"
    );
}

#[sqlx::test(migrations = false)]
async fn a_changed_document_keeps_the_chunks_whose_text_didnt_change(pool: PgPool) {
    let server = start_server(pool.clone()).await;
    let api = server.admin_api().await;
    let paragraph = |word: &str| format!("{word} ").repeat(300);
    let mut note = document(
        "notes",
        "long.md",
        &format!(
            "{}\n\n{}\n\n{}",
            paragraph("alpha"),
            paragraph("beta"),
            paragraph("gamma")
        ),
    );
    api.put_document(&note).await.unwrap();
    let before = chunk_ids(&pool).await;

    note.text = format!(
        "{}\n\n{}\n\n{}",
        paragraph("alpha"),
        paragraph("delta"),
        paragraph("gamma")
    );
    api.put_document(&note).await.unwrap();
    let after = chunk_ids(&pool).await;

    assert_eq!(before.len(), 3);
    assert_eq!(after.len(), 3);
    assert_eq!(after[0], before[0]);
    assert_eq!(after[2], before[2]);
    assert_ne!(after[1].0, before[1].0);
    assert!(after[1].1.starts_with("delta"));
}

#[sqlx::test(migrations = false)]
async fn a_document_can_be_read_back_whole(pool: PgPool) {
    let server = start_server(pool).await;
    let api = server.admin_api().await;
    let mut note = document("notes/work", "plan.md", "# Plan\n\nShip it.");
    note.content_type = ContentType::Markdown;
    note.url = Some("obsidian://open?vault=main&file=plan".to_owned());
    note.thread = Some("planning".to_owned());
    note.meta = json!({"tags": ["work"]}).as_object().unwrap().clone();
    note.modified_at = day(2);

    let id = api.put_document(&note).await.unwrap().id().unwrap();
    let stored = api.document(id).await.unwrap();

    let mut stored = serde_json::to_value(&stored).unwrap();
    let writer = stored.as_object_mut().unwrap().remove("writer").unwrap();
    assert_eq!(
        (&writer["kind"], &writer["name"]),
        (&json!("api_key"), &json!("test key"))
    );
    assert_eq!(
        stored,
        json!({
            "id": id,
            "source": "notes/work",
            "external_id": "plan.md",
            "content_type": "markdown",
            "title": "plan.md",
            "text": "# Plan\n\nShip it.",
            "url": "obsidian://open?vault=main&file=plan",
            "thread": "planning",
            "authors": ["someone"],
            "created_at": "2026-01-01T12:00:00Z",
            "modified_at": "2026-01-02T12:00:00Z",
            "meta": {"tags": ["work"]},
        })
    );
}

#[sqlx::test(migrations = false)]
async fn nul_content_is_normalized_before_storage_hashing_and_chunking(pool: PgPool) {
    let server = start_server(pool.clone()).await;
    let api = server.admin_api().await;
    let paragraph = "Grüße 🦦 aus Köln. ".repeat(80);
    let mut clean = document(
        "notes/work",
        "nul-é.md",
        &format!("{paragraph}\n\n{paragraph}\n\n{paragraph}"),
    );
    clean.title = Some("Grüße 🦦".to_owned());
    clean.authors = vec!["Zoë".to_owned(), "René 🦦".to_owned(), String::new()];
    clean.url = Some("https://notes.example.com/nul".to_owned());
    clean.thread = Some("thread-é".to_owned());
    clean.meta = json!({
        "label": "ab",
        "empty": "",
        "nested": {
            "array": ["Grüße 🦦", 42, true, null, ["xy", {"deep": "é🦦"}]],
            "object": {"value": "kept"}
        }
    })
    .as_object()
    .unwrap()
    .clone();
    let mut dirty = clean.clone();
    dirty.text = format!("\0{}\0", clean.text.replace('ü', "ü\0"));
    dirty.title = Some("\0Grü\0ße 🦦\0".to_owned());
    dirty.authors = vec![
        "\0Zoë".to_owned(),
        "Re\0né 🦦\0".to_owned(),
        "\0\0".to_owned(),
    ];
    dirty.meta = json!({
        "label": "\0a\0b\0",
        "empty": "\0\0",
        "nested": {
            "array": ["Grü\0ße 🦦", 42, true, null, ["x\0y", {"deep": "\0é🦦\0"}]],
            "object": {"value": "ke\0pt"}
        }
    })
    .as_object()
    .unwrap()
    .clone();

    let created = api.put_document(&dirty).await.unwrap();
    let id = created.id().unwrap();
    assert_eq!(created, PutResponse::Created { id });
    let stored = api.document(id).await.unwrap();
    let mut stored = serde_json::to_value(stored).unwrap();
    stored.as_object_mut().unwrap().remove("id");
    stored.as_object_mut().unwrap().remove("writer");
    assert_eq!(stored, serde_json::to_value(&clean).unwrap());

    let chunks: Vec<(Uuid, String, i32, i32, Option<String>)> = sqlx::query_as(
        "select id, text, start_offset, end_offset, title from chunks order by ordinal",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(chunks.len() > 1);
    for (_, text, start, end, title) in &chunks {
        assert!(!text.contains('\0'));
        assert_eq!(title, &clean.title);
        let slice: String = clean
            .text
            .chars()
            .skip(*start as usize)
            .take((end - start) as usize)
            .collect();
        assert_eq!(&slice, text);
    }
    for equivalent in [&dirty, &clean] {
        assert_eq!(
            api.put_document(equivalent).await.unwrap(),
            PutResponse::Unchanged { id }
        );
    }
    let after: Vec<(Uuid, String, i32, i32, Option<String>)> = sqlx::query_as(
        "select id, text, start_offset, end_offset, title from chunks order by ordinal",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(after, chunks);
}

#[sqlx::test(migrations = false)]
async fn nul_identities_urls_and_metadata_keys_are_rejected_without_changing_data(pool: PgPool) {
    let server = start_server(pool.clone()).await;
    let key = server
        .create_api_key(&["INGEST_WRITE", "SEARCH_READ"], None)
        .await;
    let api = server.api_with_key(&key);
    let mut note = document("notes/work", "a.md", "Original text.");
    note.thread = Some("thread".to_owned());
    note.url = Some("https://notes.example.com/a".to_owned());
    note.meta = json!({"key": "original"}).as_object().unwrap().clone();
    let id = api.put_document(&note).await.unwrap().id().unwrap();
    let before = serde_json::to_value(api.document(id).await.unwrap()).unwrap();
    let chunks = chunk_ids(&pool).await;

    for (field, value) in [
        ("source", json!("notes/\0work")),
        ("external_id", json!("a\0.md")),
        ("thread", json!("th\0read")),
        ("url", json!("https://notes.example.com/\0a")),
        (
            "meta",
            json!({"key": "changed", "ke\u{0000}y": "collision"}),
        ),
        ("meta", json!({"nested": {"bad\u{0000}key": "value"}})),
        (
            "meta",
            json!({"nested": [42, [{"bad\u{0000}key": "value"}]]}),
        ),
    ] {
        let mut invalid = serde_json::to_value(&note).unwrap();
        invalid["text"] = json!("Must not replace the original.");
        invalid[field] = value;
        let response = server
            .request(Method::PUT, "/api/documents")
            .header(header::AUTHORIZATION, format!("Bearer {key}"))
            .json(&invalid)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{field}");
        assert_eq!(
            serde_json::to_value(api.document(id).await.unwrap()).unwrap(),
            before,
            "{field}"
        );
        assert_eq!(chunk_ids(&pool).await, chunks, "{field}");
    }
    let count: i64 = sqlx::query_scalar("select count(*) from documents")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
    assert_eq!(
        api.put_document(&note).await.unwrap(),
        PutResponse::Unchanged { id }
    );
}

#[sqlx::test(migrations = false)]
async fn delete_removes_the_document_and_its_chunks(pool: PgPool) {
    let server = start_server(pool.clone()).await;
    let api = server.admin_api().await;
    let id = api
        .put_document(&document("notes", "a.md", "Some text."))
        .await
        .unwrap()
        .id()
        .unwrap();

    api.delete_document("notes", "a.md").await.unwrap();

    let error = api.document(id).await.unwrap_err();
    assert_eq!(error.status(), Some(StatusCode::NOT_FOUND));
    assert!(chunk_ids(&pool).await.is_empty());
    let again = api.delete_document("notes", "a.md").await.unwrap_err();
    assert_eq!(again.status(), Some(StatusCode::NOT_FOUND));
}

#[sqlx::test(migrations = false)]
async fn missing_required_fields_are_rejected_but_url_and_thread_are_optional(pool: PgPool) {
    let server = start_server(pool).await;
    let key = server
        .create_api_key(&["INGEST_WRITE", "SEARCH_READ"], None)
        .await;
    let put = |body: serde_json::Value| {
        server
            .request(Method::PUT, "/api/documents")
            .header(header::AUTHORIZATION, format!("Bearer {key}"))
            .json(&body)
            .send()
    };
    let minimal = json!({
        "source": "notes",
        "external_id": "a.md",
        "content_type": "plain",
        "text": "Some text.",
        "authors": [],
        "created_at": "2026-01-01T12:00:00Z",
        "modified_at": "2026-01-01T12:00:00Z",
    });

    assert_eq!(put(minimal.clone()).await.unwrap().status(), 200);
    for field in [
        "source",
        "external_id",
        "content_type",
        "text",
        "authors",
        "created_at",
        "modified_at",
    ] {
        let mut body = minimal.clone();
        body.as_object_mut().unwrap().remove(field);
        let response = put(body).await.unwrap();
        assert_eq!(response.status(), 422, "without {field}");
        assert!(
            json_body(response).await["error"]
                .as_str()
                .unwrap()
                .contains(field)
        );
    }
    for (field, value, status) in [
        ("content_type", json!("pdf"), 422),
        ("titel", json!("typo"), 422),
        ("source", json!("/notes"), 400),
        ("source", json!("notes//a"), 400),
        ("external_id", json!(""), 400),
    ] {
        let mut body = minimal.clone();
        body[field] = value;
        let response = put(body).await.unwrap();
        assert_eq!(response.status(), status, "{field}");
        assert!(json_body(response).await["error"].is_string());
    }
}

#[sqlx::test(migrations = false)]
async fn writing_needs_the_ingest_write_scope(pool: PgPool) {
    let server = start_server(pool).await;
    let search_only = server.api_with_key(&server.create_api_key(&["SEARCH_READ"], None).await);
    let no_key = server.api_without_key();

    let forbidden = search_only
        .put_document(&document("notes", "a.md", "Text."))
        .await
        .unwrap_err();
    let unauthorized = no_key
        .put_document(&document("notes", "a.md", "Text."))
        .await
        .unwrap_err();

    assert_eq!(forbidden.status(), Some(StatusCode::FORBIDDEN));
    assert_eq!(unauthorized.status(), Some(StatusCode::UNAUTHORIZED));
}

#[sqlx::test(migrations = false)]
async fn a_keys_sources_restrict_writing_and_reading(pool: PgPool) {
    let server = start_server(pool).await;
    let admin = server.admin_api().await;
    let note = admin
        .put_document(&document("notes", "a.md", "Text."))
        .await
        .unwrap()
        .id()
        .unwrap();
    let paperless = server.api_with_key(
        &server
            .create_api_key(&["INGEST_WRITE", "SEARCH_READ"], Some(&["paperless"]))
            .await,
    );

    let inside = paperless
        .put_document(&document("paperless/inbox", "1", "Text."))
        .await
        .unwrap();
    let write_outside = paperless
        .put_document(&document("paperless-old", "1", "Text."))
        .await
        .unwrap_err();
    let delete_outside = paperless
        .delete_document("notes", "a.md")
        .await
        .unwrap_err();
    let read_outside = paperless.document(note).await.unwrap_err();

    assert!(matches!(inside, PutResponse::Created { .. }));
    assert_eq!(
        paperless
            .document(inside.id().unwrap())
            .await
            .unwrap()
            .source,
        "paperless/inbox"
    );
    assert_eq!(write_outside.status(), Some(StatusCode::FORBIDDEN));
    assert_eq!(delete_outside.status(), Some(StatusCode::FORBIDDEN));
    assert_eq!(read_outside.status(), Some(StatusCode::NOT_FOUND));
}

#[sqlx::test(migrations = false)]
async fn urls_with_scripting_schemes_are_rejected(pool: PgPool) {
    let server = start_server(pool).await;
    let api = server.admin_api().await;

    for url in [
        " JavaScript:alert(1)",
        "data:text/html,x",
        "vbscript:msgbox",
    ] {
        let mut note = document("notes", "a.md", "Text.");
        note.url = Some(url.to_owned());
        let error = api.put_document(&note).await.unwrap_err();
        assert_eq!(error.status(), Some(StatusCode::BAD_REQUEST), "{url}");
    }
    let mut note = document("notes", "a.md", "Text.");
    note.url = Some("obsidian://open?vault=main&file=a".to_owned());
    assert!(matches!(
        api.put_document(&note).await.unwrap(),
        PutResponse::Created { .. }
    ));
}
