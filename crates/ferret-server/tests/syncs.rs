mod common;

use common::{document, start_server};
use ferret_api::Client;
use reqwest::StatusCode;
use sqlx::PgPool;
use uuid::Uuid;

async fn put(api: &Client, source: &str, external_id: &str) {
    api.put_document(&document(source, external_id, "Some text."))
        .await
        .unwrap();
}

/// The `source/external_id` of every stored document, sorted.
async fn stored(pool: &PgPool) -> Vec<String> {
    let mut stored: Vec<String> =
        sqlx::query_scalar("select source || '/' || external_id from documents")
            .fetch_all(pool)
            .await
            .unwrap();
    stored.sort();
    stored
}

fn status(error: reqwest::Error) -> StatusCode {
    error.status().unwrap()
}

#[sqlx::test(migrations = false)]
async fn a_commit_sweeps_what_was_neither_put_nor_seen_below_the_source(pool: PgPool) {
    let server = start_server(pool.clone()).await;
    let api = server.admin_api().await;
    for (source, id) in [
        ("paperless", "kept-by-put"),
        ("paperless", "kept-by-seen"),
        ("paperless", "gone"),
        ("paperless/inbox", "gone-below"),
        ("paperless-old", "outside"),
        ("notes", "outside"),
    ] {
        put(&api, source, id).await;
    }

    let sync = api.start_sync("paperless").await.unwrap().sync_id;
    // A no-op PUT counts as seen, just like `seen`.
    put(&api, "paperless", "kept-by-put").await;
    let seen = api
        .sync_seen(sync, "paperless", &["kept-by-seen", "never-existed"])
        .await
        .unwrap();
    let committed = api.commit_sync(sync).await.unwrap();

    assert_eq!(seen.seen, 1, "unknown external ids don't count");
    assert_eq!(committed.deleted, 2);
    assert_eq!(
        stored(&pool).await,
        [
            "notes/outside",
            "paperless-old/outside",
            "paperless/kept-by-put",
            "paperless/kept-by-seen",
        ]
    );
}

#[sqlx::test(migrations = false)]
async fn a_sync_that_is_never_committed_deletes_nothing(pool: PgPool) {
    let server = start_server(pool.clone()).await;
    let api = server.admin_api().await;
    put(&api, "paperless", "a").await;
    put(&api, "paperless", "b").await;

    let sync = api.start_sync("paperless").await.unwrap().sync_id;
    api.sync_seen(sync, "paperless", &["a"]).await.unwrap();
    assert_eq!(stored(&pool).await, ["paperless/a", "paperless/b"]);
    // A crashed run starts over with a new sync, which sees everything again.
    let next = api.start_sync("paperless").await.unwrap().sync_id;
    api.sync_seen(next, "paperless", &["a", "b"]).await.unwrap();
    api.commit_sync(next).await.unwrap();

    assert_eq!(stored(&pool).await, ["paperless/a", "paperless/b"]);
}

#[sqlx::test(migrations = false)]
async fn seen_names_the_source_because_external_ids_repeat_across_sources(pool: PgPool) {
    let server = start_server(pool.clone()).await;
    let api = server.admin_api().await;
    put(&api, "carapace/facts", "1").await;
    put(&api, "carapace/sessions", "1").await;

    let sync = api.start_sync("carapace").await.unwrap().sync_id;
    api.sync_seen(sync, "carapace/facts", &["1"]).await.unwrap();
    let outside = api.sync_seen(sync, "notes", &["1"]).await.unwrap_err();
    api.commit_sync(sync).await.unwrap();

    assert_eq!(status(outside), StatusCode::BAD_REQUEST);
    assert_eq!(stored(&pool).await, ["carapace/facts/1"]);
}

#[sqlx::test(migrations = false)]
async fn a_committed_sync_takes_no_more_reports(pool: PgPool) {
    let server = start_server(pool).await;
    let api = server.admin_api().await;
    let sync = api.start_sync("paperless").await.unwrap().sync_id;
    api.commit_sync(sync).await.unwrap();

    let seen = api.sync_seen(sync, "paperless", &["a"]).await.unwrap_err();
    let commit = api.commit_sync(sync).await.unwrap_err();
    let unknown = api.commit_sync(Uuid::nil()).await.unwrap_err();

    assert_eq!(status(seen), StatusCode::CONFLICT);
    assert_eq!(status(commit), StatusCode::CONFLICT);
    assert_eq!(status(unknown), StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = false)]
async fn syncs_need_ingest_write_within_the_keys_sources(pool: PgPool) {
    let server = start_server(pool.clone()).await;
    let admin = server.admin_api().await;
    put(&admin, "notes/work", "kept").await;
    let reader = server.api_with_key(&server.create_api_key(&["INGEST_READ"], None).await);
    let notes_key = server
        .create_api_key(&["INGEST_WRITE"], Some(&["notes"]))
        .await;
    let notes = server.api_with_key(&notes_key);

    assert_eq!(
        status(reader.start_sync("notes").await.unwrap_err()),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        status(notes.start_sync("paperless").await.unwrap_err()),
        StatusCode::FORBIDDEN
    );
    // A sync of another key stays out of reach of a key whose sources don't cover it.
    let foreign = admin.start_sync("paperless").await.unwrap().sync_id;
    assert_eq!(
        status(notes.commit_sync(foreign).await.unwrap_err()),
        StatusCode::FORBIDDEN
    );
    let sync = notes.start_sync("notes/work").await.unwrap().sync_id;
    notes
        .sync_seen(sync, "notes/work", &["kept"])
        .await
        .unwrap();
    assert_eq!(notes.commit_sync(sync).await.unwrap().deleted, 0);
    assert_eq!(stored(&pool).await, ["notes/work/kept"]);
}

#[sqlx::test(migrations = false)]
async fn a_sweep_leaves_other_users_documents_alone(pool: PgPool) {
    let server = start_server(pool.clone()).await;
    let api = server.admin_api().await;
    // No user management yet, so the second user and their document come straight from SQL.
    sqlx::query(
        "with bob as (
             insert into users (id, username, password_hash, created_at, password_changed_at)
             values (uuidv7(), 'bob', 'unused', now(), now())
             returning id
         )
         insert into documents (id, user_id, source, external_id, content_type, authors,
                                created_at, modified_at, meta, text, content_hash, seen_at)
         select uuidv7(), id, 'paperless', 'bobs', 'plain', '{}', now(), now(), '{}', 'Bob.',
                '\\x00', now() - interval '1 day'
         from bob",
    )
    .execute(&pool)
    .await
    .unwrap();

    let sync = api.start_sync("paperless").await.unwrap().sync_id;
    let committed = api.commit_sync(sync).await.unwrap();

    assert_eq!(committed.deleted, 0);
    assert_eq!(stored(&pool).await, ["paperless/bobs"]);
}
