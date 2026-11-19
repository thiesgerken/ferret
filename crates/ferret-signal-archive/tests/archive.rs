use std::time::Duration;

use ferret_signal_archive::{
    Archive, Attachment, ChangeOperation, ChangesPage, Chat, ChatKind, ChatsPage, MessageKey,
    MessagesPage, PAGE_SIZE, Store,
};
use serde::de::DeserializeOwned;
use std::sync::{Arc, Mutex};

use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
    sync::mpsc,
};

const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
/// Hand-built `receive` result items shaped like signal-cli v0.14.9's JSON records.
const RECEIVED: &str = include_str!("fixtures/receive.jsonl");
const ME: &str = "00000000-0000-4000-8000-000000000001";
const ALICE: &str = "00000000-0000-4000-8000-000000000002";
const BOB: &str = "00000000-0000-4000-8000-000000000003";
const GROUP: &str = "ZXhhbXBsZS1ncm91cC1pZC0wMDAwMDAwMDAwMDAwMDE=";

/// A fake multi-account `signal-cli daemon --tcp --receive-mode manual` serving `account`. It
/// answers `listAccounts`, `listContacts` and `listGroups` from `names`, which a test may change,
/// and each one-envelope `receive` call with the next line sent through `lines` (a JSON `receive`
/// result item); a `receive` call stays pending until a line arrives.
struct FakeDaemon {
    address: String,
    lines: mpsc::UnboundedSender<String>,
    names: Arc<Mutex<Names>>,
}

#[derive(Clone, Default)]
struct Names {
    contacts: Vec<serde_json::Value>,
    groups: Vec<serde_json::Value>,
}

impl FakeDaemon {
    async fn start(account: &'static str, names: Names) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let (lines, mut pending) = mpsc::unbounded_channel::<String>();
        let names = Arc::new(Mutex::new(names));
        let served = names.clone();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (reader, mut writer) = stream.into_split();
            let mut requests = BufReader::new(reader).lines();
            while let Some(request) = requests.next_line().await.unwrap() {
                let request: serde_json::Value = serde_json::from_str(&request).unwrap();
                let method = request["method"].as_str().unwrap();
                if method != "listAccounts" {
                    assert_eq!(request["params"]["account"], account, "{method}");
                }
                let result = match method {
                    "listAccounts" => {
                        serde_json::json!([{"number": "+15555550100", "aci": account}])
                    }
                    "listContacts" => {
                        assert_eq!(request["params"]["allRecipients"], true);
                        served.lock().unwrap().contacts.clone().into()
                    }
                    "listGroups" => served.lock().unwrap().groups.clone().into(),
                    "receive" => {
                        assert_eq!(request["params"]["maxMessages"], 1);
                        let Some(line) = pending.recv().await else {
                            break;
                        };
                        serde_json::json!([
                            serde_json::from_str::<serde_json::Value>(&line).unwrap()
                        ])
                    }
                    _ => panic!("unexpected call {method}"),
                };
                let response =
                    serde_json::json!({"jsonrpc": "2.0", "id": request["id"], "result": result});
                writer
                    .write_all(format!("{response}\n").as_bytes())
                    .await
                    .unwrap();
            }
            std::future::pending::<()>().await;
        });
        Self {
            address,
            lines,
            names,
        }
    }

    fn send(&self, lines: &str) {
        for line in lines.lines() {
            self.lines.send(line.to_owned()).unwrap();
        }
    }
}

/// A fake daemon without names that emits `lines`.
async fn fake_daemon(account: &'static str, lines: String) -> String {
    let daemon = FakeDaemon::start(account, Names::default()).await;
    daemon.send(&lines);
    // The daemon task holds the receiving end; the sender may go.
    std::mem::forget(daemon.lines);
    daemon.address
}

struct Api {
    url: String,
    http: reqwest::Client,
}

impl Api {
    async fn start(store: Store) -> Self {
        let app = ferret_signal_archive::router(store, TOKEN).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self {
            url,
            http: reqwest::Client::new(),
        }
    }

    async fn get(&self, path: &str, query: &[(&str, &str)]) -> reqwest::Response {
        self.http
            .get(format!("{}{path}", self.url))
            .bearer_auth(TOKEN)
            .query(query)
            .send()
            .await
            .unwrap()
    }

    async fn json<T: DeserializeOwned>(&self, path: &str, query: &[(&str, &str)]) -> T {
        let response = self.get(path, query).await;
        assert_eq!(response.status(), 200, "{path}");
        response.json().await.unwrap()
    }

    async fn archive(&self) -> Archive {
        self.json("/v1/archive", &[]).await
    }

    /// Waits until the receiver has journaled `high` changes.
    async fn await_high(&self, high: i64) -> Archive {
        for _ in 0..200 {
            let archive = self.archive().await;
            if archive.high >= high {
                return archive;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("receiver did not reach {high} changes");
    }
}

async fn received_archive() -> (tempfile::TempDir, Api, Archive) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("signal.db")).await.unwrap();
    let address = fake_daemon(ME, RECEIVED.to_owned()).await;
    tokio::spawn(ferret_signal_archive::receive(store.clone(), address));
    let api = Api::start(store).await;
    let archive = api.await_high(9).await;
    (dir, api, archive)
}

#[tokio::test]
async fn stores_messages_and_skips_events() {
    let (_dir, api, archive) = received_archive().await;
    // The last fixture line is stored, so all lines before it were handled.
    assert_eq!(archive.high, 9, "duplicates and events are not journaled");
    assert!(archive.last_received_at.is_some());
    assert_eq!(archive.store_id.len(), 32);

    let store_id = archive.store_id.as_str();
    let chats: ChatsPage = api.json("/v1/chats", &[("store_id", store_id)]).await;
    assert_eq!(
        chats.chats,
        [
            Chat {
                id: ME.into(),
                kind: ChatKind::NoteToSelf,
                name: None
            },
            // Without a name from signal-cli, a direct chat is titled with the number.
            Chat {
                id: ALICE.into(),
                kind: ChatKind::Direct,
                name: Some("+15555550101".into())
            },
            // The group's name as the creating envelope carried it.
            Chat {
                id: GROUP.into(),
                kind: ChatKind::Group,
                name: Some("Example group".into())
            },
        ]
    );
    assert_eq!(chats.next, None);
    let from_start: ChatsPage = api
        .json("/v1/chats", &[("store_id", store_id), ("after", "")])
        .await;
    assert_eq!(
        from_start.chats, chats.chats,
        "an empty `after` starts at the beginning"
    );

    let texts = |page: &MessagesPage| {
        page.messages
            .iter()
            .map(|message| {
                (
                    message.key.author.clone(),
                    message.from_me,
                    message.text.clone(),
                )
            })
            .collect::<Vec<_>>()
    };
    let direct: MessagesPage = api
        .json("/v1/messages", &[("store_id", store_id), ("chat", ALICE)])
        .await;
    assert_eq!(
        texts(&direct),
        [
            (ALICE.into(), false, Some("Hello from Alice".into())),
            (ME.into(), true, Some("Reply from the phone".into())),
            (ALICE.into(), false, Some("Disappearing".into())),
            (ALICE.into(), false, None),
            (ALICE.into(), false, Some("Reply to a story".into())),
        ]
    );
    assert_eq!(
        direct.messages[0].author_number.as_deref(),
        Some("+15555550101")
    );
    assert_eq!(
        direct.messages[1].author_number.as_deref(),
        Some("+15555550100")
    );
    assert_eq!(
        direct.messages[3].attachments[0].mime_type.as_deref(),
        Some("image/png")
    );
    assert_eq!(direct.next, None);

    let group: MessagesPage = api
        .json("/v1/messages", &[("store_id", store_id), ("chat", GROUP)])
        .await;
    assert_eq!(
        texts(&group),
        [
            (BOB.into(), false, None),
            (ME.into(), true, Some("To the group".into())),
            (BOB.into(), false, Some("Last line".into())),
        ]
    );
    assert_eq!(
        group.messages[0].attachments,
        [
            Attachment {
                mime_type: Some("image/jpeg".into()),
                filename: Some("photo.jpg".into()),
                size: Some(48213),
                caption: Some("A caption".into()),
                voice_note: false,
            },
            Attachment {
                mime_type: Some("audio/aac".into()),
                filename: None,
                size: Some(9120),
                caption: None,
                voice_note: true,
            },
        ]
    );

    let note: MessagesPage = api
        .json("/v1/messages", &[("store_id", store_id), ("chat", ME)])
        .await;
    assert_eq!(
        texts(&note),
        [(ME.into(), true, Some("Note to self".into()))]
    );

    let after: MessagesPage = api
        .json(
            "/v1/messages",
            &[
                ("store_id", store_id),
                ("chat", ALICE),
                ("after_sent_at", "1760000002000"),
                ("after_author", ME),
            ],
        )
        .await;
    assert_eq!(after.messages.len(), 3);

    let changes: ChangesPage = api
        .json(
            "/v1/changes",
            &[("store_id", store_id), ("after", "0"), ("through", "9")],
        )
        .await;
    assert_eq!(changes.next, 9);
    assert!(!changes.has_more);
    assert_eq!(
        changes
            .changes
            .iter()
            .map(|change| change.sequence)
            .collect::<Vec<_>>(),
        [1, 2, 3, 4, 5, 6, 7, 8, 9]
    );
    assert_eq!(changes.changes[0].chat, ALICE);
    assert_eq!(
        changes.changes[8].key.as_ref().unwrap().sent_at,
        1760000019000,
        "the last line is last"
    );
    assert_eq!(
        changes.changes[0].key,
        Some(MessageKey {
            sent_at: 1760000001000,
            author: ALICE.into()
        })
    );
}

#[tokio::test]
async fn requests_fail_without_credentials_or_with_another_store() {
    let (_dir, api, archive) = received_archive().await;
    assert!(
        ferret_signal_archive::router(
            Store::open(_dir.path().join("other.db")).await.unwrap(),
            "short"
        )
        .is_err()
    );
    let anonymous = reqwest::Client::new();
    assert_eq!(
        anonymous
            .get(format!("{}/healthz", api.url))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    for route in ["archive", "chats", "messages", "changes"] {
        let response = anonymous
            .get(format!("{}/v1/{route}", api.url))
            .bearer_auth("f".repeat(64))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 401);
        assert_eq!(response.text().await.unwrap(), r#"{"code":"unauthorized"}"#);
    }

    let other = "f".repeat(32);
    for (path, query) in [
        ("/v1/chats", vec![("store_id", other.as_str())]),
        ("/v1/messages", vec![("store_id", &other), ("chat", ALICE)]),
        (
            "/v1/changes",
            vec![("store_id", &other), ("after", "0"), ("through", "0")],
        ),
    ] {
        let response = api.get(path, &query).await;
        assert_eq!(response.status(), 409, "{path}");
        assert_eq!(
            response.text().await.unwrap(),
            r#"{"code":"store_replaced"}"#
        );
    }

    let store_id = archive.store_id.as_str();
    for query in [
        vec![("chat", ALICE)],
        vec![
            ("store_id", store_id),
            ("chat", ALICE),
            ("after_sent_at", "1"),
        ],
        vec![("store_id", store_id), ("chat", ALICE), ("unknown", "1")],
    ] {
        let response = api.get("/v1/messages", &query).await;
        assert_eq!(response.status(), 400);
        assert_eq!(
            response.text().await.unwrap(),
            r#"{"code":"invalid_query"}"#
        );
    }
    for (after, through) in [("-1", "0"), ("3", "2"), ("0", "10")] {
        let response = api
            .get(
                "/v1/changes",
                &[
                    ("store_id", store_id),
                    ("after", after),
                    ("through", through),
                ],
            )
            .await;
        assert_eq!(response.status(), 400);
        assert_eq!(
            response.text().await.unwrap(),
            r#"{"code":"invalid_checkpoint"}"#
        );
    }
}

#[tokio::test]
async fn store_identity_survives_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("signal.db");
    let first = Store::open(&path).await.unwrap();
    let store_id = Api::start(first).await.archive().await.store_id;
    let reopened = Api::start(Store::open(&path).await.unwrap())
        .await
        .archive()
        .await;
    assert_eq!(reopened.store_id, store_id);
    assert!(
        store_id
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    );
    let fresh = Api::start(Store::open(dir.path().join("fresh.db")).await.unwrap())
        .await
        .archive()
        .await;
    assert_ne!(fresh.store_id, store_id);
    assert_eq!(fresh.high, 0);
    assert_eq!(fresh.last_received_at, None);
}

#[tokio::test]
async fn a_daemon_for_another_account_is_fatal() {
    let (dir, _api, _archive) = received_archive().await;
    let store = Store::open(dir.path().join("signal.db")).await.unwrap();
    let other = fake_daemon(ALICE, String::new()).await;
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        ferret_signal_archive::receive(store, other),
    )
    .await
    .expect("fails instead of retrying");
    assert!(result.is_err());
}

#[tokio::test]
async fn pages_cross_the_page_size() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("signal.db")).await.unwrap();
    // 501 people each write once to the account and once to one group: 501 direct chats plus
    // the group, 501 group messages and 1002 journal entries.
    let count = PAGE_SIZE + 1;
    let line = |aci: &str, sent_at: i64, group: bool| {
        let group_info = group.then(|| {
            serde_json::json!({"groupId": GROUP, "groupName": null, "revision": 1, "type": "DELIVER"})
        });
        let envelope = serde_json::json!({
            "sourceUuid": aci, "sourceNumber": null, "timestamp": sent_at,
            "dataMessage": {"timestamp": sent_at, "message": "Hello", "groupInfo": group_info},
        });
        serde_json::json!({"account": "+15555550100", "envelope": envelope}).to_string() + "\n"
    };
    let lines: String = (0..count)
        .flat_map(|index| {
            let aci = format!("00000000-0000-4000-8000-{index:012}");
            let sent_at = 1_760_000_000_000 + index;
            [line(&aci, sent_at, false), line(&aci, sent_at, true)]
        })
        .collect();
    let address = fake_daemon(ME, lines).await;
    tokio::spawn(ferret_signal_archive::receive(store.clone(), address));
    let api = Api::start(store).await;
    let archive = api.await_high(2 * count).await;
    let store_id = archive.store_id.as_str();

    let mut chats = 0;
    let mut after = String::new();
    loop {
        let page: ChatsPage = api
            .json("/v1/chats", &[("store_id", store_id), ("after", &after)])
            .await;
        chats += page.chats.len() as i64;
        let Some(next) = page.next else { break };
        assert_eq!(page.chats.len() as i64, PAGE_SIZE);
        after = next;
    }
    assert_eq!(chats, count + 1);

    let first: MessagesPage = api
        .json("/v1/messages", &[("store_id", store_id), ("chat", GROUP)])
        .await;
    let next = first.next.expect("a second page");
    assert_eq!(next, first.messages.last().unwrap().key);
    let sent_at = next.sent_at.to_string();
    let second: MessagesPage = api
        .json(
            "/v1/messages",
            &[
                ("store_id", store_id),
                ("chat", GROUP),
                ("after_sent_at", &sent_at),
                ("after_author", &next.author),
            ],
        )
        .await;
    assert_eq!((first.messages.len() + second.messages.len()) as i64, count);
    assert_eq!(second.next, None);

    let through = archive.high.to_string();
    let mut sequences = Vec::new();
    let mut after = "0".to_owned();
    loop {
        let page: ChangesPage = api
            .json(
                "/v1/changes",
                &[
                    ("store_id", store_id),
                    ("after", &after),
                    ("through", &through),
                ],
            )
            .await;
        sequences.extend(page.changes.iter().map(|change| change.sequence));
        if !page.has_more {
            assert_eq!(page.next, archive.high);
            break;
        }
        assert!(page.next < archive.high);
        after = page.next.to_string();
    }
    assert_eq!(sequences, (1..=archive.high).collect::<Vec<_>>());
}

#[tokio::test]
async fn names_come_from_signal_cli_and_renames_are_journaled() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("signal.db")).await.unwrap();
    let contact = |aci: &str, nick: Option<&str>, profile: &str| {
        serde_json::json!({"uuid": aci, "number": null, "name": "", "nickName": null,
            "nickGivenName": nick, "nickFamilyName": null,
            "profile": {"givenName": profile, "familyName": null}})
    };
    let names = Names {
        contacts: vec![
            contact(ALICE, Some("Ally"), "Alice"),
            contact(BOB, None, "Bob"),
            contact(ME, None, "Myself"),
            serde_json::json!({"uuid": null, "number": "+15555550109", "name": "Number only"}),
        ],
        groups: vec![serde_json::json!({"id": GROUP, "name": "Renamed group"})],
    };
    let daemon = FakeDaemon::start(ME, names).await;
    daemon.send(RECEIVED);
    tokio::spawn(ferret_signal_archive::receive(
        store.clone(),
        daemon.address.clone(),
    ));
    let api = Api::start(store).await;
    // The fixture's group update makes the receiver refresh names mid-stream, which journals the
    // group's new name. The first refresh, before any message, has no chat to journal.
    let archive = api.await_high(10).await;
    assert_eq!(archive.high, 10);
    let store_id = archive.store_id.as_str();

    let chats: ChatsPage = api.json("/v1/chats", &[("store_id", store_id)]).await;
    let titles: Vec<_> = chats
        .chats
        .iter()
        .map(|chat| chat.name.as_deref())
        .collect();
    assert_eq!(titles, [None, Some("Ally"), Some("Renamed group")]);
    let group: MessagesPage = api
        .json("/v1/messages", &[("store_id", store_id), ("chat", GROUP)])
        .await;
    let authors: Vec<_> = group
        .messages
        .iter()
        .map(|message| message.author_name.as_deref())
        .collect();
    assert_eq!(
        authors,
        [Some("Bob"), None, Some("Bob")],
        "own name is not kept"
    );

    let renames = |changes: &ChangesPage| -> Vec<(String, bool)> {
        changes
            .changes
            .iter()
            .filter(|change| change.operation == ChangeOperation::Rename)
            .map(|change| (change.chat.clone(), change.key.is_some()))
            .collect()
    };
    let changes: ChangesPage = api
        .json(
            "/v1/changes",
            &[("store_id", store_id), ("after", "0"), ("through", "10")],
        )
        .await;
    assert_eq!(renames(&changes), [(GROUP.to_owned(), false)]);

    // Alice gets a new nickname and Bob a new profile name; another device's contacts sync
    // triggers the refresh before the next envelope.
    {
        let mut names = daemon.names.lock().unwrap();
        names.contacts[0] = contact(ALICE, Some("Alice A"), "Alice");
        names.contacts[1] = contact(BOB, None, "Robert");
    }
    let sync = serde_json::json!({"account": "+15555550100", "envelope": {
        "sourceUuid": ME, "sourceNumber": "+15555550100", "timestamp": 1760000030000_i64,
        "syncMessage": {"type": "CONTACTS_SYNC"}}});
    let last = serde_json::json!({"account": "+15555550100", "envelope": {
        "sourceUuid": BOB, "sourceNumber": null, "timestamp": 1760000031000_i64,
        "dataMessage": {"timestamp": 1760000031000_i64, "message": "After the rename",
            "groupInfo": {"groupId": GROUP, "groupName": "Renamed group", "revision": 4, "type": "DELIVER"}}}});
    daemon.send(&format!("{sync}\n{last}\n"));
    api.await_high(13).await;
    let changes: ChangesPage = api
        .json(
            "/v1/changes",
            &[("store_id", store_id), ("after", "10"), ("through", "13")],
        )
        .await;
    // Alice's direct chat, and the group Bob writes in: only the chats whose rendering changed.
    assert_eq!(
        renames(&changes),
        [(ALICE.to_owned(), false), (GROUP.to_owned(), false)]
    );
    assert_eq!(changes.changes[2].operation, ChangeOperation::Insert);
}
