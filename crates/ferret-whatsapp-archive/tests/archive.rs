use ferret_whatsapp_archive::{Archive, ChangesPage, ChatsPage, MessagesPage};
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};

const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const SCHEMA: &str = include_str!("fixtures/wacli-v0.22.0.sql");

async fn database(path: &std::path::Path) -> SqliteConnection {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .pragma("journal_mode", "WAL");
    let mut db = SqliteConnection::connect_with(&options).await.unwrap();
    sqlx::raw_sql(SCHEMA).execute(&mut db).await.unwrap();
    sqlx::query("INSERT INTO chats(jid,kind,name) VALUES('1@s.whatsapp.net','dm','Alice')")
        .execute(&mut db)
        .await
        .unwrap();
    db
}
async fn serve(path: &std::path::Path) -> (String, tokio::task::JoinHandle<()>) {
    let app = ferret_whatsapp_archive::router(path, TOKEN).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    (
        url,
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        }),
    )
}

#[tokio::test]
async fn credentials_schema_and_allowlists_are_enforced_without_secrets() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("wacli.db");
    let mut db = database(&path).await;
    sqlx::query("INSERT INTO messages(chat_jid,msg_id,ts,from_me,text,direct_path,media_key,local_path) VALUES('1@s.whatsapp.net','secret',1,0,'Readable','privatepath',x'010203','/private/media')").execute(&mut db).await.unwrap();
    assert!(
        ferret_whatsapp_archive::router(&path, "short")
            .await
            .is_err()
    );
    let (url, task) = serve(&path).await;
    let http = reqwest::Client::new();
    assert_eq!(
        http.get(format!("{url}/healthz"))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    for route in ["archive", "chats", "messages", "changes"] {
        let response = http.get(format!("{url}/v1/{route}")).send().await.unwrap();
        assert_eq!(response.status(), 401);
        assert_eq!(response.text().await.unwrap(), r#"{"code":"unauthorized"}"#);
    }
    let archive: Archive = http
        .get(format!("{url}/v1/archive"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(archive.schema_version, 32);
    assert_eq!(archive.collector_status, "unknown");
    assert_eq!(archive.last_sync_at, None);
    let body = http
        .get(format!("{url}/v1/messages"))
        .bearer_auth(TOKEN)
        .query(&[
            ("store_id", &archive.store_id),
            ("chat", &"1@s.whatsapp.net".to_owned()),
        ])
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(body.contains("Readable"));
    for forbidden in [
        "privatepath",
        "media_key",
        "direct_path",
        "local_path",
        "/private/media",
    ] {
        assert!(!body.contains(forbidden));
    }
    let malformed = http
        .get(format!("{url}/v1/messages?token={TOKEN}"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert_eq!(malformed, r#"{"code":"invalid_query"}"#);
    std::fs::write(
        dir.path().join("SESSION_REVOKED"),
        "private reason not exposed",
    )
    .unwrap();
    let archive: Archive = http
        .get(format!("{url}/v1/archive"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(archive.collector_status, "logged_out");
    sqlx::query("INSERT INTO schema_migrations(version,name,applied_at) VALUES(34,'future',0)")
        .execute(&mut db)
        .await
        .unwrap();
    assert_eq!(
        http.get(format!("{url}/v1/archive"))
            .bearer_auth(TOKEN)
            .send()
            .await
            .unwrap()
            .status(),
        503
    );
    task.abort();
}

#[tokio::test]
async fn keysets_survive_wal_writes_and_journal_distinguishes_expiry_replacement_and_high_water() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("wacli.db");
    let mut db = database(&path).await;
    for n in 0..501 {
        sqlx::query("INSERT INTO messages(chat_jid,msg_id,ts,from_me,text) VALUES('1@s.whatsapp.net',?,100,0,'History')").bind(n.to_string()).execute(&mut db).await.unwrap();
    }
    let (url, task) = serve(&path).await;
    let http = reqwest::Client::new();
    let archive: Archive = http
        .get(format!("{url}/v1/archive"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let first: MessagesPage = http
        .get(format!("{url}/v1/messages"))
        .bearer_auth(TOKEN)
        .query(&[
            ("store_id", archive.store_id.clone()),
            ("chat", "1@s.whatsapp.net".into()),
        ])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(first.messages.len(), 500);
    let key = first.next.unwrap();
    sqlx::query("INSERT INTO messages(chat_jid,msg_id,ts,from_me,text) VALUES('1@s.whatsapp.net','late',1,1,'Backfill')").execute(&mut db).await.unwrap();
    sqlx::query("UPDATE messages SET text='Edit' WHERE msg_id='0'")
        .execute(&mut db)
        .await
        .unwrap();
    sqlx::query("DELETE FROM messages WHERE msg_id='1'")
        .execute(&mut db)
        .await
        .unwrap();
    let next: MessagesPage = http
        .get(format!("{url}/v1/messages"))
        .bearer_auth(TOKEN)
        .query(&[
            ("store_id", archive.store_id.clone()),
            ("chat", "1@s.whatsapp.net".into()),
            ("after_ts", key.ts.to_string()),
            ("after_rowid", key.rowid.to_string()),
        ])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(next.messages.len(), 1);
    assert_eq!(next.messages[0].msg_id, "500");
    assert!(next.next.is_none());
    let end: Archive = http
        .get(format!("{url}/v1/archive"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let changes: ChangesPage = http
        .get(format!("{url}/v1/changes"))
        .bearer_auth(TOKEN)
        .query(&[
            ("store_id", archive.store_id.clone()),
            ("after", archive.high.to_string()),
            ("through", end.high.to_string()),
        ])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(
        changes
            .changes
            .iter()
            .any(|change| change.msg_id == "late" && change.operation == "upsert")
    );
    assert!(
        changes
            .changes
            .iter()
            .any(|change| change.msg_id == "1" && change.operation == "delete")
    );
    sqlx::query("DELETE FROM message_changes WHERE sequence=?")
        .bind(end.high)
        .execute(&mut db)
        .await
        .unwrap();
    let high: Archive = http
        .get(format!("{url}/v1/archive"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(high.high, end.high);
    assert_eq!(
        http.get(format!("{url}/v1/changes"))
            .bearer_auth(TOKEN)
            .query(&[
                ("store_id", archive.store_id.clone()),
                ("after", archive.high.to_string()),
                ("through", end.high.to_string())
            ])
            .send()
            .await
            .unwrap()
            .status(),
        410
    );
    assert_eq!(
        http.get(format!("{url}/v1/chats"))
            .bearer_auth(TOKEN)
            .query(&[("store_id", "another-store")])
            .send()
            .await
            .unwrap()
            .status(),
        409
    );
    // Age alone invalidates replay even when no writable collector cleanup has run.
    sqlx::query("UPDATE message_changes SET recorded_at=1")
        .execute(&mut db)
        .await
        .unwrap();
    let aged: Archive = http
        .get(format!("{url}/v1/archive"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(aged.floor >= archive.high);
    let chats: ChatsPage = http
        .get(format!("{url}/v1/chats"))
        .bearer_auth(TOKEN)
        .query(&[("store_id", archive.store_id.clone())])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(chats.chats.len(), 1);
    task.abort();
}
