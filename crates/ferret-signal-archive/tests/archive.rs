use std::time::Duration;

use ferret_signal_archive::{
    Archive, Attachment, ChangeOperation, ChangesPage, Chat, ChatKind, ChatsPage, ImportSummary,
    MessageKey, MessagesPage, PAGE_SIZE, Store,
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
    /// Answer `listContacts` with a JSON-RPC error instead.
    contacts_fail: bool,
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
                if method == "listContacts" && served.lock().unwrap().contacts_fail {
                    let response = serde_json::json!({"jsonrpc": "2.0", "id": request["id"],
                        "error": {"code": -32602, "message": "Unrecognized field"}});
                    writer
                        .write_all(format!("{response}\n").as_bytes())
                        .await
                        .unwrap();
                    continue;
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
                name: Some("Note to Self".into())
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
        ..Names::default()
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
    assert_eq!(
        titles,
        [Some("Note to Self"), Some("Ally"), Some("Renamed group")]
    );
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

    // Alice gets a new nickname and Bob a new profile name; a group update made on the
    // account's own phone triggers the refresh before the next envelope.
    {
        let mut names = daemon.names.lock().unwrap();
        names.contacts[0] = contact(ALICE, Some("Alice A"), "Alice");
        names.contacts[1] = contact(BOB, None, "Robert");
    }
    let sync = serde_json::json!({"account": "+15555550100", "envelope": {
        "sourceUuid": ME, "sourceNumber": "+15555550100", "timestamp": 1760000030000_i64,
        "syncMessage": {"sentMessage": {"destinationUuid": null, "timestamp": 1760000030000_i64,
            "message": null, "groupInfo": {"groupId": GROUP, "groupName": "Renamed group",
            "revision": 5, "type": "UPDATE"}}}}});
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

/// A `receive` result item: an envelope from `author` with `content` (`dataMessage`,
/// `editMessage` or `syncMessage`).
fn envelope(author: &str, content: serde_json::Value) -> String {
    let mut envelope = serde_json::json!({"sourceUuid": author, "sourceDevice": 1});
    envelope
        .as_object_mut()
        .unwrap()
        .extend(content.as_object().unwrap().clone());
    serde_json::json!({"account": "+15555550100", "envelope": envelope}).to_string()
}

fn data(sent_at: i64, text: &str) -> serde_json::Value {
    serde_json::json!({"timestamp": sent_at, "message": text})
}

fn edit(target: i64, edited_at: i64, text: &str) -> serde_json::Value {
    serde_json::json!({"editMessage": {"targetSentTimestamp": target, "dataMessage": data(edited_at, text)}})
}

fn remote_delete(sent_at: i64, target: i64) -> serde_json::Value {
    serde_json::json!({"dataMessage": {"timestamp": sent_at,
        "remoteDelete": {"targetTimestamp": target, "timestamp": target}}})
}

#[tokio::test]
async fn edits_and_deletes_resolve_any_revision_and_keep_identities_occupied() {
    let group_info = serde_json::json!({"groupId": GROUP, "type": "DELIVER"});
    let mut spam = data(8_000, "Spam");
    spam["groupInfo"] = group_info.clone();
    let lines = [
        envelope(ALICE, serde_json::json!({"dataMessage": data(1_000, "Original")})),
        envelope(ALICE, edit(1_000, 2_000, "First edit")),
        // Signal clients address a second edit to the first edit's timestamp.
        envelope(ALICE, edit(2_000, 3_000, "Second edit")),
        // A redelivered edit is not a new revision.
        envelope(ALICE, edit(1_000, 2_000, "First edit")),
        envelope(
            ME,
            serde_json::json!({"syncMessage": {"sentMessage": {"destinationUuid": ALICE,
                "timestamp": 4_000, "message": "Mine"}}}),
        ),
        envelope(
            ME,
            serde_json::json!({"syncMessage": {"sentMessage": {"destinationUuid": ALICE,
                "editMessage": {"targetSentTimestamp": 4_000, "dataMessage": data(5_000, "Mine edited")}}}}),
        ),
        // An older revision arriving late does not overwrite the newer text.
        envelope(ALICE, edit(1_000, 1_500, "Late older edit")),
        envelope(ALICE, serde_json::json!({"dataMessage": data(6_000, "Doomed")})),
        envelope(ALICE, edit(6_000, 7_000, "Doomed edited")),
        // Deletes may name any revision.
        envelope(ALICE, remote_delete(7_500, 7_000)),
        // Neither a redelivery nor an edit brings a deleted message back.
        envelope(ALICE, serde_json::json!({"dataMessage": data(6_000, "Doomed")})),
        envelope(ALICE, edit(6_000, 10_000, "Revived")),
        envelope(BOB, serde_json::json!({"dataMessage": spam})),
        envelope(
            ALICE,
            serde_json::json!({"dataMessage": {"timestamp": 8_500, "groupInfo": group_info,
                "adminDelete": {"targetAuthorUuid": BOB, "targetTimestamp": 8_000}}}),
        ),
        // A delete ahead of its message leaves a tombstone the message cannot replace.
        envelope(ALICE, remote_delete(9_500, 9_000)),
        envelope(ALICE, serde_json::json!({"dataMessage": data(9_000, "Too late")})),
        // Nobody edits or deletes another author's message, outside a group not even an admin.
        envelope(ALICE, edit(4_000, 12_000, "Hijacked")),
        envelope(ALICE, remote_delete(12_500, 4_000)),
        envelope(
            ALICE,
            serde_json::json!({"dataMessage": {"timestamp": 13_000,
                "adminDelete": {"targetAuthorUuid": ME, "targetTimestamp": 4_000}}}),
        ),
        envelope(ALICE, serde_json::json!({"dataMessage": data(11_000, "Last")})),
    ]
    .join("\n");
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("signal.db")).await.unwrap();
    tokio::spawn(ferret_signal_archive::receive(
        store.clone(),
        fake_daemon(ME, lines).await,
    ));
    let api = Api::start(store).await;
    let archive = api.await_high(11).await;
    assert_eq!(archive.high, 11);
    let store_id = archive.store_id.as_str();

    let direct: MessagesPage = api
        .json("/v1/messages", &[("store_id", store_id), ("chat", ALICE)])
        .await;
    let shown: Vec<_> = direct
        .messages
        .iter()
        .map(|message| (message.key.sent_at, message.text.as_deref(), message.edited))
        .collect();
    assert_eq!(
        shown,
        [
            (1_000, Some("Second edit"), true),
            (4_000, Some("Mine edited"), true),
            (11_000, Some("Last"), false),
        ]
    );
    let group: MessagesPage = api
        .json("/v1/messages", &[("store_id", store_id), ("chat", GROUP)])
        .await;
    assert!(group.messages.is_empty());

    let changes: ChangesPage = api
        .json(
            "/v1/changes",
            &[("store_id", store_id), ("after", "0"), ("through", "11")],
        )
        .await;
    let journaled: Vec<_> = changes
        .changes
        .iter()
        .map(|change| {
            (
                change.operation,
                change.key.as_ref().unwrap().sent_at,
                change.key.as_ref().unwrap().author.as_str(),
            )
        })
        .collect();
    use ferret_signal_archive::ChangeOperation::{Delete, Edit, Insert};
    assert_eq!(
        journaled,
        [
            (Insert, 1_000, ALICE),
            (Edit, 1_000, ALICE),
            (Edit, 1_000, ALICE),
            (Insert, 4_000, ME),
            (Edit, 4_000, ME),
            (Insert, 6_000, ALICE),
            (Edit, 6_000, ALICE),
            (Delete, 6_000, ALICE),
            (Insert, 8_000, BOB),
            (Delete, 8_000, BOB),
            (Insert, 11_000, ALICE),
        ],
        "edits and deletes journal the original identity"
    );
}

fn export_fixture() -> std::path::PathBuf {
    // A synthetic Signal Desktop chat export, shaped like a real one's frames.
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/export.jsonl")
}

#[tokio::test]
async fn imports_history_next_to_live_messages() {
    let (dir, api, archive) = received_archive().await;
    // A second handle while the receiver holds the database, as `kubectl exec ... import` does.
    let importer = Store::open(dir.path().join("signal.db")).await.unwrap();
    let summary = ferret_signal_archive::import(&importer, &export_fixture())
        .await
        .unwrap();
    assert_eq!(
        summary,
        ImportSummary {
            inserted: 6,
            present: 2,
            skipped: 6,
            unkeyed: 1,
        }
    );
    assert_eq!(api.archive().await.high, archive.high + 6);

    let store_id = archive.store_id.as_str();
    let alice: MessagesPage = api
        .json("/v1/messages", &[("store_id", store_id), ("chat", ALICE)])
        .await;
    let history: Vec<_> = alice
        .messages
        .iter()
        .take_while(|message| message.key.sent_at < 1760000000000)
        .collect();
    assert_eq!(history.len(), 4);
    assert_eq!(history[3].text.as_deref(), Some("Nice story"));
    assert_eq!(history[0].text.as_deref(), Some("An old message"));
    assert_eq!(history[0].author_number.as_deref(), Some("+15555550101"));
    assert!(history[1].from_me);
    assert_eq!(
        history[1].attachments,
        [
            Attachment {
                mime_type: Some("application/pdf".into()),
                filename: Some("plan.pdf".into()),
                size: Some(48213),
                caption: Some("The plan".into()),
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
    // An edit keeps the identity of the original send, with the latest text.
    assert_eq!(history[2].key.sent_at, 1750000200000);
    assert_eq!(history[2].text.as_deref(), Some("Corrected text"));
    assert!(history[2].edited);

    let note: MessagesPage = api
        .json("/v1/messages", &[("store_id", store_id), ("chat", ME)])
        .await;
    assert_eq!(note.messages.len(), 2);
    let bob: MessagesPage = api
        .json("/v1/messages", &[("store_id", store_id), ("chat", BOB)])
        .await;
    assert_eq!(bob.messages.len(), 1);
    assert_eq!(bob.messages[0].text.as_deref(), Some("See this"));

    let again = ferret_signal_archive::import(&importer, &export_fixture())
        .await
        .unwrap();
    assert_eq!(
        again,
        ImportSummary {
            inserted: 0,
            present: 8,
            skipped: 6,
            unkeyed: 1,
        }
    );
    assert_eq!(api.archive().await.high, archive.high + 6);
}

#[tokio::test]
async fn import_requires_the_receivers_own_aci() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("signal.db")).await.unwrap();
    assert!(
        ferret_signal_archive::import(&store, &export_fixture())
            .await
            .is_err()
    );
    let archive = Api::start(store).await.archive().await;
    assert_eq!(archive.high, 0);
}

#[tokio::test]
async fn imports_into_an_empty_archive() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("signal.db");
    let store = Store::open(&path).await.unwrap();
    // What the receiver records on its first connect, without receiving anything.
    let db =
        sqlx::SqlitePool::connect_with(sqlx::sqlite::SqliteConnectOptions::new().filename(&path))
            .await
            .unwrap();
    sqlx::query("UPDATE archive SET self_aci = ?")
        .bind(ME)
        .execute(&db)
        .await
        .unwrap();
    let summary = ferret_signal_archive::import(&store, &export_fixture())
        .await
        .unwrap();
    assert_eq!(
        summary,
        ImportSummary {
            inserted: 8,
            present: 0,
            skipped: 6,
            unkeyed: 1,
        }
    );
    let api = Api::start(store).await;
    let archive = api.archive().await;
    assert_eq!(archive.high, 8);
    let store_id = archive.store_id.as_str();
    let chats: ChatsPage = api.json("/v1/chats", &[("store_id", store_id)]).await;
    let kinds: Vec<_> = chats
        .chats
        .iter()
        .map(|chat| (chat.id.as_str(), chat.kind))
        .collect();
    assert_eq!(
        kinds,
        [
            (ME, ChatKind::NoteToSelf),
            (ALICE, ChatKind::Direct),
            (BOB, ChatKind::Direct)
        ]
    );
    let changes: ChangesPage = api
        .json(
            "/v1/changes",
            &[("store_id", store_id), ("after", "0"), ("through", "8")],
        )
        .await;
    assert_eq!(changes.changes.len(), 8);
}

#[tokio::test]
async fn group_chats_are_rejected() {
    let (dir, _api, _archive) = received_archive().await;
    let store = Store::open(dir.path().join("signal.db")).await.unwrap();
    let group = r#"{"recipient":{"id":"7","group":{"masterKey":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=","whitelisted":true}}}"#;
    let export = dir.path().join("groups.jsonl");
    // A group recipient without a chat holds no history and does not block the import.
    std::fs::write(&export, format!("{group}\n")).unwrap();
    ferret_signal_archive::import(&store, &export)
        .await
        .unwrap();
    std::fs::write(
        &export,
        format!(
            "{group}\n{}\n",
            r#"{"chat":{"id":"20","recipientId":"7","expireTimerVersion":1}}"#
        ),
    )
    .unwrap();
    let error = ferret_signal_archive::import(&store, &export)
        .await
        .unwrap_err();
    assert_eq!(error.to_string(), "line 2: group chat import unsupported");
}

#[tokio::test]
async fn live_edits_and_deletes_resolve_imported_revisions() {
    let (dir, api, _) = received_archive().await;
    let store = Store::open(dir.path().join("signal.db")).await.unwrap();
    ferret_signal_archive::import(&store, &export_fixture())
        .await
        .unwrap();
    let imported = api.archive().await.high;
    // The export holds Alice's message sent at 1750000200000 and edited at 1750000300000; Signal
    // addresses later edits and deletes to the latest revision.
    let lines = [
        envelope(
            ALICE,
            edit(1_750_000_300_000, 1_750_000_600_000, "Edited live"),
        ),
        envelope(ALICE, remote_delete(1_750_000_700_000, 1_750_000_300_000)),
    ]
    .join("\n");
    tokio::spawn(ferret_signal_archive::receive(
        store,
        fake_daemon(ME, lines).await,
    ));
    let archive = api.await_high(imported + 2).await;
    let store_id = archive.store_id.as_str();
    let after = imported.to_string();
    let through = archive.high.to_string();
    let changes: ChangesPage = api
        .json(
            "/v1/changes",
            &[
                ("store_id", store_id),
                ("after", &after),
                ("through", &through),
            ],
        )
        .await;
    let journaled: Vec<_> = changes
        .changes
        .iter()
        .map(|change| {
            (
                change.operation,
                change.key.as_ref().unwrap().sent_at,
                change.key.as_ref().unwrap().author.as_str(),
            )
        })
        .collect();
    use ferret_signal_archive::ChangeOperation::{Delete, Edit};
    assert_eq!(
        journaled,
        [
            (Edit, 1_750_000_200_000, ALICE),
            (Delete, 1_750_000_200_000, ALICE)
        ]
    );
    let direct: MessagesPage = api
        .json("/v1/messages", &[("store_id", store_id), ("chat", ALICE)])
        .await;
    assert!(
        direct
            .messages
            .iter()
            .all(|message| message.key.sent_at != 1_750_000_200_000)
    );
}

#[tokio::test]
async fn a_failing_name_refresh_does_not_stop_receiving() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("signal.db")).await.unwrap();
    let names = Names {
        contacts_fail: true,
        ..Names::default()
    };
    let daemon = FakeDaemon::start(ME, names).await;
    daemon.send(RECEIVED);
    tokio::spawn(ferret_signal_archive::receive(
        store.clone(),
        daemon.address.clone(),
    ));
    let api = Api::start(store).await;
    assert_eq!(api.await_high(9).await.high, 9);
}
