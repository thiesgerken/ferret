use crate::common;

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use common::{TestServer, start_server};
use ferret_api::{ContentType, Document};
use ferret_plugins::{plugins::PluginOptions, worker::Worker};
use serde_json::{Value, json};
use sqlx::PgPool;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
    sync::mpsc,
};
use uuid::Uuid;

const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const FIXTURE: &str = include_str!("../../ferret-signal-archive/tests/fixtures/receive.jsonl");
const ME: &str = "00000000-0000-4000-8000-000000000001";
const ALICE: &str = "00000000-0000-4000-8000-000000000002";
const ALICE_NUMBER: &str = "+15555550101";
const BOB: &str = "00000000-0000-4000-8000-000000000003";
const GROUP: &str = "ZXhhbXBsZS1ncm91cC1pZC0wMDAwMDAwMDAwMDAwMDE=";
/// Messages the archive stores from the fixture; the other lines are ignored events.
const FIXTURE_MESSAGES: i64 = 9;

/// The real chat archive: its receiver fed by a fake `signal-cli` daemon, its router on a port.
struct ChatArchive {
    _dir: tempfile::TempDir,
    daemon: mpsc::UnboundedSender<String>,
    /// What the fake daemon's `listContacts` and `listGroups` return.
    names: Arc<Mutex<Value>>,
    url: String,
    store_id: String,
}

impl ChatArchive {
    async fn start() -> Self {
        Self::named(json!({"contacts": [], "groups": []})).await
    }

    async fn named(names: Value) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let store = ferret_signal_archive::Store::open(dir.path().join("archive.db"))
            .await
            .unwrap();
        let (daemon, lines) = mpsc::unbounded_channel();
        let names = Arc::new(Mutex::new(names));
        tokio::spawn(ferret_signal_archive::receive(
            store.clone(),
            fake_daemon(lines, names.clone()).await,
        ));
        let app = ferret_signal_archive::router(store, TOKEN).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let mut archive = Self {
            _dir: dir,
            daemon,
            names,
            url,
            store_id: String::new(),
        };
        archive.store_id = archive.identity().await.store_id;
        archive
    }

    async fn identity(&self) -> ferret_signal_archive::Archive {
        reqwest::Client::new()
            .get(format!("{}/v1/archive", self.url))
            .bearer_auth(TOKEN)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap()
    }

    /// Sends notification lines and waits until the journal holds `high` entries.
    async fn receive(&self, lines: &str, high: i64) {
        for line in lines.lines() {
            self.daemon.send(line.to_owned()).unwrap();
        }
        for _ in 0..200 {
            if self.identity().await.high >= high {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("the chat archive did not journal {high} messages");
    }

    fn config(&self, allow: &[&str], block: &[&str]) -> Value {
        json!({"url": self.url, "token": TOKEN, "expected_store_id": self.store_id,
            "allow_chats": allow, "block_chats": block})
    }
}

/// Answers `listAccounts` with the fixture account, `listContacts` and `listGroups` from `names`,
/// and each `receive` call with the next line it is sent, leaving the call pending while there is
/// none.
async fn fake_daemon(
    mut lines: mpsc::UnboundedReceiver<String>,
    names: Arc<Mutex<Value>>,
) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap().to_string();
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let (reader, mut writer) = stream.into_split();
        let mut requests = BufReader::new(reader).lines();
        while let Some(request) = requests.next_line().await.unwrap() {
            let request: Value = serde_json::from_str(&request).unwrap();
            let result = match request["method"].as_str().unwrap() {
                "listAccounts" => json!([{"number": "+15555550100", "aci": ME}]),
                "listContacts" => names.lock().unwrap()["contacts"].clone(),
                "listGroups" => names.lock().unwrap()["groups"].clone(),
                method => {
                    assert_eq!(method, "receive");
                    assert_eq!(request["params"]["account"], ME);
                    let Some(line) = lines.recv().await else {
                        break;
                    };
                    json!([serde_json::from_str::<Value>(&line).unwrap()])
                }
            };
            let response = json!({"jsonrpc": "2.0", "id": request["id"], "result": result});
            writer
                .write_all(format!("{response}\n").as_bytes())
                .await
                .unwrap();
        }
        std::future::pending::<()>().await;
    });
    address
}

/// A fixture line re-sent as a new message by `author` at `sent_at` with `text`.
fn variant(line: usize, author: (&str, &str), sent_at: i64, text: &str) -> String {
    let mut item: Value = serde_json::from_str(FIXTURE.lines().nth(line - 1).unwrap()).unwrap();
    let envelope = &mut item["envelope"];
    let (aci, number) = author;
    envelope["sourceUuid"] = json!(aci);
    envelope["source"] = json!(number);
    envelope["sourceNumber"] = json!(number);
    envelope["timestamp"] = json!(sent_at);
    envelope["dataMessage"]["timestamp"] = json!(sent_at);
    envelope["dataMessage"]["message"] = json!(text);
    item.to_string()
}

async fn setup(server: &TestServer, config: Value) -> (Worker, Uuid, String) {
    let worker = Worker {
        internal: server.internal(),
        public_url: server.public_url(),
        name: "signal-test-worker".into(),
        options: PluginOptions::default(),
    };
    let worker_id = worker.register().await;
    let cookie = server.sign_in().await;
    let created = server.graphql(&cookie, "mutation ($input: NewConnection!) { createConnection(input: $input) { id } }", json!({"input": {"name":"Signal", "icon":"message-circle", "plugin":"signal", "source":"chat/signal", "config":config.to_string(), "schedule":"0 * * * *"}})).await;
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
    let response = server.graphql(&cookie, "mutation ($id: ID!, $input: ConnectionUpdate!) { updateConnection(id: $id, input: $input) { id } }", json!({"id":id, "input": {"name":"Signal", "icon":"message-circle", "config":config.to_string(), "schedule":"0 * * * *", "enabled":true}})).await;
    assert!(response["errors"].is_null(), "{response}");
    due(server, id).await;
}

async fn execute(
    worker: &Worker,
    worker_id: Uuid,
    pool: &PgPool,
) -> (String, Option<String>, Value, String) {
    assert!(worker.work_once(worker_id, Duration::ZERO).await.unwrap());
    sqlx::query_as("select status,error,jsonb_build_object('put',put,'unchanged',unchanged,'deleted',deleted),log from plugin_runs order by started_at desc limit 1").fetch_one(pool).await.unwrap()
}

async fn documents(server: &TestServer, pool: &PgPool) -> Vec<Document> {
    let ids: Vec<Uuid> = sqlx::query_scalar("select id from documents")
        .fetch_all(pool)
        .await
        .unwrap();
    let api = server.admin_api().await;
    let mut docs = Vec::new();
    for id in ids {
        docs.push(api.document(id).await.unwrap());
    }
    docs.sort_by(|a, b| {
        a.created_at
            .cmp(&b.created_at)
            .then(a.external_id.cmp(&b.external_id))
    });
    docs
}

fn in_thread<'a>(docs: &'a [Document], chat: &str) -> Vec<&'a Document> {
    docs.iter()
        .filter(|doc| doc.thread.as_deref() == Some(chat))
        .collect()
}

#[sqlx::test(migrations = false)]
async fn registered_worker_imports_then_follows_the_journal_and_filters(pool: PgPool) {
    let archive = ChatArchive::start().await;
    archive.receive(FIXTURE, FIXTURE_MESSAGES).await;
    let server = start_server(pool.clone()).await;
    let (worker, wid, id) = setup(&server, archive.config(&[], &[])).await;

    // The first run's chat inventory request sends an empty `after=`.
    let (status, error, _, log) = execute(&worker, wid, &pool).await;
    assert_eq!(status, "succeeded", "{error:?}");
    assert!(log.contains("collector status unknown, last received envelope 20"));
    let docs = documents(&server, &pool).await;
    assert_eq!(docs.len(), 3);
    assert!(
        docs.iter()
            .all(|doc| doc.source == "chat/signal" && doc.content_type == ContentType::Markdown)
    );
    let alice = in_thread(&docs, ALICE);
    assert_eq!(alice.len(), 1);
    // Without a name from signal-cli, the chat archive titles a direct chat with the number.
    assert_eq!(alice[0].title.as_deref(), Some(ALICE_NUMBER));
    assert_eq!(alice[0].authors, [ALICE_NUMBER, "Me"]);
    assert!(alice[0].text.contains("Reply from the phone"));
    assert!(alice[0].text.contains("Disappearing"));
    assert!(alice[0].text.contains("Reply to a story"));
    assert!(alice[0].text.contains("Attachment: image/png"));
    let group = in_thread(&docs, GROUP);
    assert!(
        group[0]
            .text
            .contains("Attachment: photo.jpg · image/jpeg · 48.2 kB")
    );
    assert!(
        group[0]
            .text
            .contains("Attachment: voice note · audio/aac · 9.1 kB")
    );
    assert!(group[0].text.contains("A caption"));
    let note_to_self = in_thread(&docs, ME);
    assert_eq!(note_to_self[0].authors, ["Me"]);
    assert!(note_to_self[0].text.contains("Note to self"));

    // Alice writes two days later, and in the group at the same instant as the last message
    // there, so the keyset must order the two by author.
    let incremental = [
        variant(1, (ALICE, ALICE_NUMBER), 1_760_200_000_000, "Latersentinel"),
        variant(
            21,
            (ALICE, ALICE_NUMBER),
            1_760_000_019_000,
            "Sameinstantsentinel",
        ),
    ]
    .join("\n");
    archive.receive(&incremental, FIXTURE_MESSAGES + 2).await;
    due(&server, &id).await;
    let (status, error, stats, _) = execute(&worker, wid, &pool).await;
    assert_eq!(status, "succeeded", "{error:?}");
    assert_eq!(stats["put"], 2, "{stats}");
    assert_eq!(stats["deleted"], 0, "{stats}");
    let docs = documents(&server, &pool).await;
    let alice = in_thread(&docs, ALICE);
    assert_eq!(alice.len(), 2);
    assert!(alice[1].text.contains("Latersentinel"));
    let group = &in_thread(&docs, GROUP)[0].text;
    let same_instant = group.find("Sameinstantsentinel").unwrap();
    assert!(same_instant < group.find("Last line").unwrap());

    update(&server, &id, archive.config(&[], &[GROUP])).await;
    let (status, error, stats, _) = execute(&worker, wid, &pool).await;
    assert_eq!(status, "succeeded", "{error:?}");
    assert_eq!(stats["deleted"], 1, "{stats}");
    let docs = documents(&server, &pool).await;
    assert!(in_thread(&docs, GROUP).is_empty());
    assert_eq!(docs.len(), 3);

    update(&server, &id, archive.config(&["unknown-chat"], &[])).await;
    let (status, error, _, _) = execute(&worker, wid, &pool).await;
    assert_eq!(status, "failed");
    assert!(error.unwrap().contains("missing chat ID"));
    assert_eq!(documents(&server, &pool).await.len(), 3);
}

#[sqlx::test(migrations = false)]
async fn a_different_chat_archive_fails_before_any_write(pool: PgPool) {
    let archive = ChatArchive::start().await;
    archive.receive(FIXTURE, FIXTURE_MESSAGES).await;
    let server = start_server(pool.clone()).await;
    let mut mismatched = archive.config(&[], &[]);
    mismatched["expected_store_id"] = json!("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");
    let (worker, wid, id) = setup(&server, mismatched).await;
    let (status, error, _, _) = execute(&worker, wid, &pool).await;
    assert_eq!(status, "failed");
    assert!(error.unwrap().contains("expected_store_id"));
    assert!(documents(&server, &pool).await.is_empty());
    let syncs: i64 = sqlx::query_scalar("select count(*) from syncs")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(syncs, 0);

    update(&server, &id, archive.config(&[], &[])).await;
    let (status, error, _, _) = execute(&worker, wid, &pool).await;
    assert_eq!(status, "succeeded", "{error:?}");
    assert_eq!(documents(&server, &pool).await.len(), 3);
}

#[sqlx::test(migrations = false)]
async fn names_come_from_signal_cli_and_a_rename_rerenders_only_its_chat(pool: PgPool) {
    let alice = |nick: &str| {
        json!({"uuid": ALICE, "number": ALICE_NUMBER, "name": "Alice Example", "nickName": null,
            "nickGivenName": nick, "nickFamilyName": null, "profile": {"givenName": "A", "familyName": null}})
    };
    let bob = json!({"uuid": BOB, "number": null, "name": "", "profile": {"givenName": "Bob", "familyName": null}});
    let archive = ChatArchive::named(json!({"contacts": [alice("Ally"), bob], "groups": []})).await;
    archive.receive(FIXTURE, FIXTURE_MESSAGES).await;
    let server = start_server(pool.clone()).await;
    let (worker, wid, id) = setup(&server, archive.config(&[], &[])).await;
    let (status, error, _, _) = execute(&worker, wid, &pool).await;
    assert_eq!(status, "succeeded", "{error:?}");
    let docs = documents(&server, &pool).await;
    let direct = in_thread(&docs, ALICE);
    assert_eq!(direct[0].title.as_deref(), Some("Ally"));
    assert_eq!(direct[0].authors, ["Ally", "Me"]);
    assert_eq!(in_thread(&docs, GROUP)[0].authors, ["Bob", "Me"]);

    // A new nickname; another device's contacts sync makes the chat archive refresh names and
    // journal the rename.
    archive.names.lock().unwrap()["contacts"][0] = alice("Alice A");
    let sync = json!({"account": "+15555550100", "envelope": {"sourceUuid": ME,
        "sourceNumber": "+15555550100", "timestamp": 1_760_000_030_000_i64,
        "syncMessage": {"type": "CONTACTS_SYNC"}}});
    archive
        .receive(&sync.to_string(), FIXTURE_MESSAGES + 1)
        .await;
    due(&server, &id).await;
    let (status, error, stats, _) = execute(&worker, wid, &pool).await;
    assert_eq!(status, "succeeded", "{error:?}");
    assert_eq!(stats["put"], 1, "only Alice's chat is re-rendered: {stats}");
    let docs = documents(&server, &pool).await;
    let direct = in_thread(&docs, ALICE);
    assert_eq!(direct[0].title.as_deref(), Some("Alice A"));
    assert_eq!(direct[0].authors, ["Alice A", "Me"]);
}
