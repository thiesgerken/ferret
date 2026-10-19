mod common;

use std::{
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
    time::Duration,
};

use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use common::{TestServer, start_server};
use ferret_api::{
    Client, Document, SearchQuery,
    internal::{RunFinish, RunResult},
};
use ferret_plugins::{
    plugins::{self, PluginOptions},
    worker::Worker,
};
use serde_json::{Value, json};
use sqlx::{Connection, PgPool, SqliteConnection, sqlite::SqliteConnectOptions};
use tokio::sync::Mutex;
use uuid::Uuid;

const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const ALICE: &str = "111@s.whatsapp.net";
const GROUP: &str = "222@g.us";
const SCHEMA: &str = include_str!("../../ferret-whatsapp-archive/tests/fixtures/wacli-v0.22.0.sql");

struct ArchiveServer {
    _dir: tempfile::TempDir,
    db: Arc<Mutex<SqliteConnection>>,
    mode: Arc<AtomicU8>,
    url: String,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for ArchiveServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[derive(Clone)]
struct Hook {
    db: Arc<Mutex<SqliteConnection>>,
    mode: Arc<AtomicU8>,
}
async fn archive_hook(State(hook): State<Hook>, request: Request, next: Next) -> Response {
    let messages = request.uri().path() == "/v1/messages";
    let continued = request
        .uri()
        .query()
        .is_some_and(|query| query.contains("after_rowid="));
    if messages
        && (hook.mode.load(Ordering::SeqCst) == 1
            || (hook.mode.load(Ordering::SeqCst) == 3 && continued))
    {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "synthetic private failure body",
        )
            .into_response();
    }
    let response = next.run(request).await;
    if messages
        && !continued
        && hook
            .mode
            .compare_exchange(2, 0, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    {
        let mut db = hook.db.lock().await;
        // These commits happen after the first page's read transaction and behind its keyset.
        sqlx::raw_sql("INSERT INTO messages(chat_jid,msg_id,ts,from_me,text) VALUES('111@s.whatsapp.net','backfill',1,0,'Backfillsentinel'); UPDATE messages SET text='Editsentinel' WHERE msg_id='m0'; DELETE FROM messages WHERE msg_id='m1'; INSERT INTO chats(jid,kind,name) VALUES('000@s.whatsapp.net','dm','Behind inventory'); INSERT INTO messages(chat_jid,msg_id,ts,from_me,text) VALUES('000@s.whatsapp.net','behind',2,0,'Behindinventorysentinel');").execute(&mut *db).await.unwrap();
    }
    response
}

impl ArchiveServer {
    async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wacli.db");
        let mut db = SqliteConnection::connect_with(
            &SqliteConnectOptions::new()
                .filename(&path)
                .create_if_missing(true)
                .pragma("journal_mode", "WAL"),
        )
        .await
        .unwrap();
        sqlx::raw_sql(SCHEMA).execute(&mut db).await.unwrap();
        sqlx::raw_sql("INSERT INTO chats(jid,kind,name) VALUES('111@s.whatsapp.net','dm','Alice'),('222@g.us','group','Team'),('status@broadcast','broadcast','Status'),('333@newsletter','newsletter','Channel');").execute(&mut db).await.unwrap();
        let db = Arc::new(Mutex::new(db));
        let mode = Arc::new(AtomicU8::new(0));
        let app = ferret_whatsapp_archive::router(&path, TOKEN)
            .await
            .unwrap()
            .layer(middleware::from_fn_with_state(
                Hook {
                    db: db.clone(),
                    mode: mode.clone(),
                },
                archive_hook,
            ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self {
            _dir: dir,
            db,
            mode,
            url,
            task,
        }
    }
    async fn sql(&self, sql: &'static str) {
        sqlx::raw_sql(sql)
            .execute(&mut *self.db.lock().await)
            .await
            .unwrap();
    }
    async fn message(&self, chat: &str, id: &str, ts: i64, text: &str, from_me: bool) {
        sqlx::query("INSERT INTO messages(chat_jid,msg_id,ts,from_me,text,sender_jid,sender_name) VALUES(?,?,?,?,?,'444@s.whatsapp.net','Sender')").bind(chat).bind(id).bind(ts).bind(from_me).bind(text).execute(&mut *self.db.lock().await).await.unwrap();
    }
    fn config(&self, allow: &[&str], block: &[&str]) -> Value {
        json!({"url": self.url, "token": TOKEN, "allow_chats": allow, "block_chats": block})
    }
}

async fn setup(server: &TestServer, config: Value) -> (Worker, Uuid, String) {
    let worker = Worker {
        internal: server.internal(),
        public_url: server.public_url(),
        name: "whatsapp-test-worker".into(),
        options: PluginOptions::default(),
    };
    let worker_id = worker.register().await;
    let cookie = server.sign_in().await;
    let created = server.graphql(&cookie, "mutation ($input: NewConnection!) { createConnection(input: $input) { id } }", json!({"input": {"name":"WhatsApp archive", "icon":"message-circle", "plugin":"whatsapp", "source":"chat/archive", "config":config.to_string(), "schedule":"0 * * * *"}})).await;
    assert!(created["errors"].is_null(), "{created}");
    (
        worker,
        worker_id,
        created["data"]["createConnection"]["id"]
            .as_str()
            .unwrap()
            .to_owned(),
    )
}
async fn due(server: &TestServer, id: &str) {
    let cookie = server.sign_in().await;
    let response = server
        .graphql(
            &cookie,
            "mutation ($id: ID!) { runConnectionNow(id: $id) }",
            json!({"id":id}),
        )
        .await;
    assert!(response["errors"].is_null(), "{response}");
}
async fn update(server: &TestServer, id: &str, config: Value) {
    let cookie = server.sign_in().await;
    let response = server.graphql(&cookie, "mutation ($id: ID!, $input: ConnectionUpdate!) { updateConnection(id: $id, input: $input) { id } }", json!({"id":id, "input": {"name":"WhatsApp archive", "icon":"message-circle", "config":config.to_string(), "schedule":"0 * * * *", "enabled":true}})).await;
    assert!(response["errors"].is_null(), "{response}");
    due(server, id).await;
}
async fn execute(
    worker: &Worker,
    worker_id: Uuid,
    pool: &PgPool,
) -> (String, Option<String>, Value) {
    assert!(worker.work_once(worker_id, Duration::ZERO).await.unwrap());
    sqlx::query_as("select status,error,jsonb_build_object('put',put,'unchanged',unchanged,'deleted',deleted) from plugin_runs order by started_at desc limit 1").fetch_one(pool).await.unwrap()
}
async fn documents(server: &TestServer) -> Vec<Document> {
    let api = server.admin_api().await;
    let hits = api
        .search(&SearchQuery {
            q: "WhatsApp".into(),
            collapse: Some(false),
            k: Some(100),
            ..Default::default()
        })
        .await
        .unwrap()
        .hits;
    let mut docs = Vec::new();
    for hit in hits {
        docs.push(api.document(hit.document_id).await.unwrap());
    }
    docs.sort_by(|a, b| {
        a.source
            .cmp(&b.source)
            .then(a.created_at.cmp(&b.created_at))
            .then(a.external_id.cmp(&b.external_id))
    });
    docs
}

#[sqlx::test(migrations = false)]
async fn registered_worker_imports_segments_reconciles_mutations_filters_and_retained_history(
    pool: PgPool,
) {
    let archive = ArchiveServer::new().await;
    std::fs::write(
        archive._dir.path().join("SESSION_REVOKED"),
        "synthetic logout",
    )
    .unwrap();
    archive
        .sql("UPDATE retrieval_state SET last_sync_at=1")
        .await;
    archive
        .message(ALICE, "received", 100, "Receivedsentinel", false)
        .await;
    archive
        .message(ALICE, "sent", 200, "Sentsentinel", true)
        .await;
    archive
        .message(ALICE, "pause", 4_000, "Pausesentinel", false)
        .await;
    archive
        .message(GROUP, "file", 300, "Captionsentinel", false)
        .await;
    archive.sql("UPDATE messages SET media_type='document',media_caption='Captionmetadata',filename='agenda.pdf',mime_type='application/pdf',file_length=12345,direct_path='not-for-response',local_path='/secret/path',media_key=x'010203' WHERE msg_id='file'").await;
    archive
        .message(
            "status@broadcast",
            "status",
            100,
            "Statusmustnotappear",
            false,
        )
        .await;
    archive
        .message(
            "333@newsletter",
            "news",
            100,
            "Newslettermustnotappear",
            false,
        )
        .await;
    let server = start_server(pool.clone()).await;
    let (worker, wid, id) = setup(&server, archive.config(&[], &[])).await;
    assert_eq!(execute(&worker, wid, &pool).await.0, "succeeded");
    let log: String =
        sqlx::query_scalar("select log from plugin_runs order by started_at desc limit 1")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(log.contains("logged_out") && log.contains("Some(1)"));
    let docs = documents(&server).await;
    assert_eq!(docs.len(), 3);
    let alice: Vec<_> = docs
        .iter()
        .filter(|doc| doc.thread.as_deref() == Some(ALICE))
        .collect();
    assert_eq!(alice.len(), 2);
    assert!(alice[0].text.contains("Sender (444@s.whatsapp.net)"));
    assert!(alice[0].text.contains("Me (444@s.whatsapp.net)"));
    assert!(alice[0].text.contains("Receivedsentinel"));
    assert!(alice[0].text.contains("Sentsentinel"));
    assert_eq!(alice[0].created_at.unwrap().timestamp(), 100);
    assert!(alice[0].modified_at.is_none());
    assert!(docs.iter().all(|doc| doc.url.is_none()));
    let attachment = docs
        .iter()
        .find(|doc| doc.thread.as_deref() == Some(GROUP))
        .unwrap();
    for expected in [
        "Captionmetadata",
        "agenda.pdf",
        "application/pdf",
        "12345 bytes",
    ] {
        assert!(attachment.text.contains(expected));
    }
    for forbidden in [
        "not-for-response",
        "/secret/path",
        "media_key",
        "Statusmustnotappear",
        "Newslettermustnotappear",
    ] {
        assert!(docs.iter().all(|doc| !doc.text.contains(forbidden)));
    }
    let identities: Vec<_> = docs.iter().map(|doc| doc.id).collect();
    due(&server, &id).await;
    let unchanged = execute(&worker, wid, &pool).await;
    assert_eq!(unchanged.0, "succeeded");
    assert_eq!(unchanged.2["put"], 0);
    assert_eq!(
        identities,
        documents(&server)
            .await
            .iter()
            .map(|doc| doc.id)
            .collect::<Vec<_>>()
    );
    archive.sql("UPDATE messages SET text='Editedsentinel',edited=1 WHERE msg_id='received'; DELETE FROM messages WHERE msg_id='pause'; UPDATE chats SET name='Renamed team' WHERE jid='222@g.us';").await;
    archive
        .message(ALICE, "backfill", 50, "Oldbackfillsentinel", false)
        .await;
    due(&server, &id).await;
    assert_eq!(execute(&worker, wid, &pool).await.0, "succeeded");
    let changed = documents(&server).await;
    assert!(
        changed
            .iter()
            .any(|doc| doc.text.contains("Editedsentinel")
                && doc.text.contains("Oldbackfillsentinel"))
    );
    assert!(
        !changed
            .iter()
            .any(|doc| doc.text.contains("Pausesentinel") || doc.text.contains("Receivedsentinel"))
    );
    assert!(
        changed
            .iter()
            .any(|doc| doc.title.as_deref().unwrap().contains("Renamed team"))
    );
    update(&server, &id, archive.config(&[ALICE, GROUP], &[ALICE])).await;
    assert_eq!(execute(&worker, wid, &pool).await.0, "succeeded");
    assert!(
        documents(&server)
            .await
            .iter()
            .all(|doc| doc.thread.as_deref() == Some(GROUP))
    );
    update(&server, &id, archive.config(&[ALICE], &[])).await;
    assert_eq!(execute(&worker, wid, &pool).await.0, "succeeded");
    let restored = documents(&server).await;
    assert!(!restored.is_empty());
    assert!(
        restored
            .iter()
            .all(|doc| doc.thread.as_deref() == Some(ALICE))
    );
    assert!(
        restored
            .iter()
            .any(|doc| doc.text.contains("Oldbackfillsentinel"))
    );
    // An identity move rebuilds and sweeps both source namespaces without duplicating content.
    update(&server, &id, archive.config(&[], &[])).await;
    assert_eq!(execute(&worker, wid, &pool).await.0, "succeeded");
    archive.sql("INSERT INTO chats(jid,kind,name) VALUES('555@s.whatsapp.net','dm','Alice canonical'); UPDATE messages SET chat_jid='555@s.whatsapp.net' WHERE chat_jid='111@s.whatsapp.net'; DELETE FROM chats WHERE jid='111@s.whatsapp.net';").await;
    due(&server, &id).await;
    assert_eq!(execute(&worker, wid, &pool).await.0, "succeeded");
    let moved = documents(&server).await;
    assert!(!moved.iter().any(|doc| doc.thread.as_deref() == Some(ALICE)));
    assert_eq!(
        moved
            .iter()
            .filter(|doc| doc.text.contains("Oldbackfillsentinel"))
            .count(),
        1
    );
    update(&server, &id, archive.config(&[], &[ALICE])).await;
    let failed = execute(&worker, wid, &pool).await;
    assert_eq!(failed.0, "failed");
    assert!(failed.1.unwrap().contains("missing chat ID"));
    assert_eq!(moved.len(), documents(&server).await.len());
}

#[sqlx::test(migrations = false)]
async fn concurrent_wal_pages_replay_backfill_and_failed_pages_never_authorize_sweep(pool: PgPool) {
    let archive = ArchiveServer::new().await;
    for n in 0..501 {
        archive
            .message(
                ALICE,
                &format!("m{n}"),
                100,
                &format!("Contentsentinel number {n}"),
                false,
            )
            .await;
    }
    let server = start_server(pool.clone()).await;
    let (worker, wid, id) = setup(&server, archive.config(&[], &[])).await;
    archive.mode.store(2, Ordering::SeqCst);
    assert_eq!(execute(&worker, wid, &pool).await.0, "succeeded");
    let docs = documents(&server).await;
    assert!(docs.iter().any(|doc| doc.text.contains("Backfillsentinel")));
    assert!(docs.iter().any(|doc| doc.text.contains("Editsentinel")));
    assert!(
        docs.iter()
            .any(|doc| doc.text.contains("Behindinventorysentinel"))
    );
    assert!(docs.iter().all(|doc| doc.text.chars().count() <= 8_000));
    let before: Vec<_> = docs.iter().map(|doc| doc.id).collect();
    let committed_before: i64 =
        sqlx::query_scalar("select count(*) from syncs where committed_at is not null")
            .fetch_one(&pool)
            .await
            .unwrap();
    // Force a full reconciliation and fail its second page after the first page succeeded.
    sqlx::query("update connections set state='{}'")
        .execute(&pool)
        .await
        .unwrap();
    archive.mode.store(3, Ordering::SeqCst);
    due(&server, &id).await;
    let failed = execute(&worker, wid, &pool).await;
    assert_eq!(failed.0, "failed");
    assert!(
        failed.2["unchanged"].as_u64().unwrap() > 1,
        "completed first-page segments must be put before requesting the failing page"
    );
    let after = documents(&server).await;
    assert!(
        before
            .iter()
            .all(|id| after.iter().any(|doc| &doc.id == id))
    );
    let committed: i64 =
        sqlx::query_scalar("select count(*) from syncs where committed_at is not null")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(committed, committed_before);
    archive.mode.store(0, Ordering::SeqCst);
    due(&server, &id).await;
    assert_eq!(execute(&worker, wid, &pool).await.0, "succeeded");
    let recovered: i64 =
        sqlx::query_scalar("select count(*) from syncs where committed_at is not null")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(recovered > committed);
    // Remove journal references: expiry recovers with an authoritative local full pass.
    archive
        .sql("DELETE FROM messages WHERE msg_id='m500'; DELETE FROM message_changes;")
        .await;
    due(&server, &id).await;
    assert_eq!(execute(&worker, wid, &pool).await.0, "succeeded");
    let log: String =
        sqlx::query_scalar("select log from plugin_runs order by started_at desc limit 1")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(log.contains("lost replay range"));
    assert!(
        documents(&server)
            .await
            .iter()
            .all(|doc| !doc.text.contains("number 500"))
    );
}

#[sqlx::test(migrations = false)]
async fn first_failed_ingest_keeps_binding_and_replacement_cannot_adopt_or_sweep(pool: PgPool) {
    let archive = ArchiveServer::new().await;
    archive
        .message(ALICE, "m0", 100, "Contentsentinel", false)
        .await;
    let server = start_server(pool.clone()).await;
    let (worker, wid, id) = setup(&server, archive.config(&[], &[])).await;
    // Ordinary ingest API keys cannot bind arbitrary connections.
    let rejected = server
        .admin_api()
        .await
        .bind_archive("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        .await
        .unwrap_err();
    assert_eq!(rejected.status(), Some(reqwest::StatusCode::FORBIDDEN));
    archive.mode.store(1, Ordering::SeqCst);
    assert_eq!(execute(&worker, wid, &pool).await.0, "failed");
    let binding: String = sqlx::query_scalar("select archive_store_id from connections")
        .fetch_one(&pool)
        .await
        .unwrap();
    let state: Value = sqlx::query_scalar("select state from connections")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state, json!({}));
    archive.mode.store(0, Ordering::SeqCst);
    due(&server, &id).await;
    assert_eq!(execute(&worker, wid, &pool).await.0, "succeeded");
    let before = documents(&server).await;
    archive.sql("UPDATE retrieval_state SET store_id='aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'; DELETE FROM messages;").await;
    due(&server, &id).await;
    assert_eq!(execute(&worker, wid, &pool).await.0, "failed");
    assert_eq!(documents(&server).await.len(), before.len());
    let unchanged_binding: String = sqlx::query_scalar("select archive_store_id from connections")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(binding, unchanged_binding);
    // A token that finished its run cannot use the compare-or-initialize endpoint.
    due(&server, &id).await;
    let claimed = worker
        .internal
        .claim(wid, Duration::ZERO)
        .await
        .unwrap()
        .unwrap();
    let token = claimed.token.clone();
    worker
        .internal
        .finish(
            claimed.run_id,
            &RunFinish {
                result: RunResult::Failed {
                    error: "intentional test failure".into(),
                },
                log_lines: vec![],
            },
        )
        .await
        .unwrap();
    assert_eq!(
        Client::new(server.public_url())
            .with_api_key(token)
            .bind_archive(&binding)
            .await
            .unwrap_err()
            .status(),
        Some(reqwest::StatusCode::UNAUTHORIZED)
    );
    // Registration advertises the required encrypted, write-only credential.
    let registration = plugins::registrations()
        .into_iter()
        .find(|plugin| plugin.name == "whatsapp")
        .unwrap();
    assert_eq!(
        registration.config_schema["properties"]["token"]["writeOnly"],
        true
    );
}
