use crate::common;

use std::time::Duration;

use async_imap::Session;
use common::{TestServer, start_server};
use ferret_api::ThreadQuery;
use ferret_plugins::{plugins::PluginOptions, worker::Worker};
use futures_util::TryStreamExt;
use serde_json::{Value, json};
use sqlx::PgPool;
use tokio::net::TcpStream;
use uuid::Uuid;

const PASSWORD: &str = "pass";

/// A mailbox of its own on the test Dovecot (see .cargo/config.toml), so tests don't share mail.
struct TestMailbox {
    server: String,
    username: String,
    session: Session<TcpStream>,
}

impl TestMailbox {
    async fn new() -> Self {
        let server = std::env::var("FERRET_TEST_IMAP").expect("FERRET_TEST_IMAP is set");
        let username = format!("test-{}", Uuid::now_v7());
        let mut client = async_imap::Client::new(TcpStream::connect(&server).await.unwrap());
        client.read_response().await.unwrap().unwrap();
        let session = client
            .login(&username, PASSWORD)
            .await
            .map_err(|(error, _)| error)
            .unwrap();
        Self {
            server,
            username,
            session,
        }
    }

    /// Appends a fixture, arrived at its `Date`, so new mails are indexed in the order they
    /// were written.
    async fn append(&mut self, folder: &str, fixture: &str, arrived: &str) {
        let raw = std::fs::read_to_string(format!(
            "{}/../ferret-plugins/fixtures/imap/{fixture}.eml",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap();
        self.append_raw(folder, &raw, arrived).await;
    }

    async fn append_raw(&mut self, folder: &str, raw: &str, arrived: &str) {
        // IMAP wants CRLF line ends.
        let raw = raw.replace("\r\n", "\n").replace('\n', "\r\n");
        self.session
            .append(folder, None, Some(&format!("\"{arrived}\"")), raw)
            .await
            .unwrap();
    }

    /// The UID of the mail with `message_id` in `folder`, which is selected afterwards.
    async fn uid(&mut self, folder: &str, message_id: &str) -> u32 {
        self.session.select(folder).await.unwrap();
        let uids = self
            .session
            .uid_search(format!("HEADER Message-ID \"{message_id}\""))
            .await
            .unwrap();
        *uids.iter().next().expect("the mail is in the folder")
    }

    async fn move_mail(&mut self, message_id: &str, from: &str, to: &str) {
        let uid = self.uid(from, message_id).await;
        self.session.uid_mv(uid.to_string(), to).await.unwrap();
    }

    async fn delete_mail(&mut self, message_id: &str, folder: &str) {
        let uid = self.uid(folder, message_id).await;
        self.session
            .uid_store(uid.to_string(), "+FLAGS (\\Deleted)")
            .await
            .unwrap()
            .try_collect::<Vec<_>>()
            .await
            .unwrap();
        self.session
            .expunge()
            .await
            .unwrap()
            .try_collect::<Vec<_>>()
            .await
            .unwrap();
    }

    fn config(&self, password: &str) -> Value {
        json!({
            "server": self.server,
            "tls": false,
            "username": self.username,
            "password": password,
            "exclude_folders": ["Trash", "Spam", "Junk", "Drafts", "Entwürfe"],
            "url_template": "mid:{message_id}",
        })
    }
}

struct Connection {
    server: TestServer,
    pool: PgPool,
    worker: Worker,
    worker_id: Uuid,
    cookie: String,
    id: Value,
}

impl Connection {
    async fn create(pool: PgPool, config: Value) -> Self {
        let server = start_server(pool.clone()).await;
        let worker = Worker {
            internal: server.internal(),
            public_url: server.public_url(),
            name: "test-worker".to_owned(),
            options: PluginOptions::default(),
        };
        let worker_id = worker.register().await;
        let cookie = server.sign_in().await;
        let created = server
            .graphql(
                &cookie,
                "mutation ($input: NewConnection!) { createConnection(input: $input) { id config } }",
                json!({"input": {
                    "name": "Mail", "icon": "mail", "plugin": "imap", "source": "mail/test",
                    "config": config.to_string(),
                    "schedule": "0 * * * *",
                }}),
            )
            .await;
        assert_eq!(created["errors"], Value::Null, "{created}");
        let id = created["data"]["createConnection"]["id"].clone();
        Self {
            server,
            pool,
            worker,
            worker_id,
            cookie,
            id,
        }
    }

    async fn update(&self, config: Value) -> Value {
        self.server
            .graphql(
                &self.cookie,
                "mutation ($id: ID!, $input: ConnectionUpdate!) {
                    updateConnection(id: $id, input: $input) { config }
                }",
                json!({"id": self.id, "input": {
                    "name": "Mail", "icon": "mail", "config": config.to_string(),
                    "schedule": "0 * * * *", "enabled": true,
                }}),
            )
            .await
    }

    /// Runs the connection and returns the run's status, error, stats and log.
    async fn run(&self) -> Value {
        self.server
            .graphql(
                &self.cookie,
                "mutation ($id: ID!) { runConnectionNow(id: $id) }",
                json!({"id": self.id}),
            )
            .await;
        assert!(
            self.worker
                .work_once(self.worker_id, Duration::ZERO)
                .await
                .unwrap()
        );
        let runs = self
            .server
            .graphql(
                &self.cookie,
                "{ connections { runs { status error stats { put unchanged deleted } log } } }",
                json!({}),
            )
            .await;
        runs["data"]["connections"][0]["runs"][0].clone()
    }

    /// The thread's documents as (Message-ID, text), oldest first.
    async fn thread(&self, thread: &str) -> Vec<(String, String)> {
        self.server
            .admin_api()
            .await
            .thread(&ThreadQuery {
                source: "mail/test".to_owned(),
                thread: thread.to_owned(),
            })
            .await
            .unwrap()
            .documents
            .into_iter()
            .map(|document| (document.external_id, document.text))
            .collect()
    }

    async fn external_ids(&self) -> Vec<String> {
        sqlx::query_scalar("select external_id from documents order by external_id")
            .fetch_all(&self.pool)
            .await
            .unwrap()
    }
}

fn stats(run: &Value) -> (i64, i64, i64) {
    assert_eq!(run["status"], "SUCCEEDED", "{run}");
    let stats = &run["stats"];
    (
        stats["put"].as_i64().unwrap(),
        stats["unchanged"].as_i64().unwrap(),
        stats["deleted"].as_i64().unwrap(),
    )
}

const SPAM: &str = "From: win@spam.example\nTo: alice@example.com\nSubject: Gewinn\nDate: Thu, 16 Jan 2025 03:00:00 +0100\nMessage-ID: <spam@spam.example>\n\nSie haben gewonnen.\n";
const DRAFT: &str = "From: alice@example.com\nTo: dave@example.org\nSubject: Entwurf\nDate: Thu, 16 Jan 2025 04:00:00 +0100\nMessage-ID: <draft@alice.example>\n\nNoch nicht fertig.\n";
const NEW_REPLY: &str = "From: Alice Example <alice@example.com>\nTo: Dave <dave@example.org>\nSubject: Re: RE: Termin\nDate: Sat, 18 Jan 2025 09:00:00 +0100\nMessage-ID: <alice-re-re-termin@alice.example>\nIn-Reply-To: <dave-re-termin@example.org>\nReferences: <termin@alice.example> <dave-re-termin@example.org>\n\nPrima.\n\nOn Fri, 17 Jan 2025 at 16:00, Dave <dave@example.org> wrote:\n> Passt so, bis Dienstag.\n";

#[sqlx::test(migrations = false)]
async fn presentation_and_folder_changes_reuse_password_but_login_changes_do_not(pool: PgPool) {
    let mut mailbox = TestMailbox::new().await;
    mailbox
        .append_raw("INBOX", SPAM, "16-Jan-2025 03:00:00 +0100")
        .await;
    let mut config = mailbox.config(PASSWORD);
    let connection = Connection::create(pool.clone(), config.clone()).await;
    assert_eq!(stats(&connection.run().await), (1, 0, 0));
    config.as_object_mut().unwrap().remove("password");

    config["url_template"] = json!("https://mail.example/message/{message_id}");
    let updated = connection.update(config.clone()).await;
    assert_eq!(updated["errors"], Value::Null, "{updated}");
    assert!(!updated.to_string().contains("password"));
    assert_eq!(stats(&connection.run().await), (1, 0, 0));
    let url: Option<String> = sqlx::query_scalar("select url from documents")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        url.as_deref(),
        Some("https://mail.example/message/spam@spam.example")
    );

    config["exclude_folders"] = json!(["INBOX"]);
    let updated = connection.update(config.clone()).await;
    assert_eq!(updated["errors"], Value::Null, "{updated}");
    assert_eq!(stats(&connection.run().await), (0, 0, 1));
    assert!(connection.external_ids().await.is_empty());

    config.as_object_mut().unwrap().remove("url_template");
    config.as_object_mut().unwrap().remove("exclude_folders");
    let updated = connection.update(config.clone()).await;
    assert_eq!(updated["errors"], Value::Null, "{updated}");
    assert_eq!(stats(&connection.run().await), (1, 0, 0));
    let url: Option<String> = sqlx::query_scalar("select url from documents")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(url, None);

    for (field, value) in [
        ("server", json!("attacker.example:993")),
        (
            "server",
            json!(format!("{}:994", mailbox.server.split(':').next().unwrap())),
        ),
        ("tls", json!(true)),
        ("username", json!("another-account")),
    ] {
        let mut protected = config.clone();
        protected[field] = value;
        for mixed in [false, true] {
            if mixed {
                protected["url_template"] = json!("mid:{message_id}");
            }
            let refused = connection.update(protected.clone()).await;
            assert_eq!(
                refused["errors"][0]["message"],
                "a changed configuration needs its credentials again: enter password",
                "{field}, mixed={mixed}: {refused}"
            );
        }
    }
    assert_eq!(stats(&connection.run().await), (0, 0, 0));
}

#[sqlx::test(migrations = false)]
async fn mails_sync_incrementally_with_threads_moves_and_deletions(pool: PgPool) {
    let mut mailbox = TestMailbox::new().await;
    mailbox.session.create("Archiv").await.unwrap();
    mailbox.session.create("Leer").await.unwrap();
    // Excluded by its special use, by the last segment of its name, and by its decoded name.
    for folder in ["Papierkorb", "Archiv/Spam", "Entw&APw-rfe"] {
        mailbox.session.create(folder).await.unwrap();
    }
    for (folder, fixture, arrived) in [
        ("Sent", "01-original", "13-Jan-2025 09:12:00 +0100"),
        ("INBOX", "02-gmail-reply", "13-Jan-2025 10:30:00 +0100"),
        ("Sent", "03-apple-mail-reply", "13-Jan-2025 11:02:00 +0100"),
        ("INBOX", "04-outlook-reply", "13-Jan-2025 12:40:00 +0100"),
        (
            "INBOX",
            "05-thunderbird-reply",
            "13-Jan-2025 13:05:00 +0100",
        ),
        // Arrived before the mail it quotes (an import), but written after it.
        ("INBOX", "06-unmarked-quote", "12-Jan-2025 08:15:00 +0100"),
        ("INBOX", "07-html-only", "15-Jan-2025 06:00:00 +0100"),
        ("Sent", "08-forward", "15-Jan-2025 18:20:00 +0100"),
        ("INBOX", "09-no-message-id", "16-Jan-2025 07:45:00 +0100"),
        (
            "Archiv",
            "10-original-message",
            "17-Jan-2025 16:00:00 +0100",
        ),
        // A copy: still one document.
        ("Archiv", "02-gmail-reply", "13-Jan-2025 10:30:00 +0100"),
    ] {
        mailbox.append(folder, fixture, arrived).await;
    }
    mailbox
        .append_raw("Junk", SPAM, "16-Jan-2025 03:00:00 +0100")
        .await;
    for folder in ["Drafts", "Papierkorb", "Archiv/Spam", "Entw&APw-rfe"] {
        mailbox
            .append_raw(folder, DRAFT, "16-Jan-2025 04:00:00 +0100")
            .await;
    }
    let connection = Connection::create(pool, mailbox.config(PASSWORD)).await;

    let first = connection.run().await;

    assert_eq!(stats(&first), (10, 0, 0), "{first}");
    let ids = connection.external_ids().await;
    assert_eq!(ids.len(), 10, "{ids:?}");
    assert!(
        !ids.iter()
            .any(|id| id.contains("spam") || id.contains("draft"))
    );
    let thread = connection.thread("original@alice.example").await;
    assert_eq!(
        thread.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(),
        [
            "original@alice.example",
            "CAbob123@mail.gmail.example",
            "5F2C1A9E-7D3B-4C1E-9A6F-0B1D2E3F4A5B@example.com",
            "AM9PR01MB1234@AM9PR01MB1234.eurprd01.prod.outlook.example",
            "7b1c2d3e-4f5a-6b7c-8d9e-0f1a2b3c4d5e@gmail.example",
            "unmarked@gmail.example",
        ]
    );
    // Quoted without any marker, the original's lines go by the thread diff.
    assert_eq!(
        thread[5].1,
        "Erledigt, die Kündigung ist raus.\n\nGesendet von meinem Telefon"
    );

    // Moved, deleted, moved to Trash, and a new reply.
    mailbox
        .move_mail("CAbob123@mail.gmail.example", "INBOX", "Archiv")
        .await;
    mailbox
        .move_mail("invoice-2025-01@stadtwerke.example", "INBOX", "Trash")
        .await;
    mailbox.delete_mail("unmarked@gmail.example", "INBOX").await;
    mailbox
        .append_raw("INBOX", NEW_REPLY, "18-Jan-2025 09:00:00 +0100")
        .await;
    let second = connection.run().await;

    assert_eq!(stats(&second), (1, 0, 2), "{second}");
    let ids = connection.external_ids().await;
    assert!(ids.contains(&"CAbob123@mail.gmail.example".to_owned()));
    assert!(!ids.contains(&"invoice-2025-01@stadtwerke.example".to_owned()));
    assert!(!ids.contains(&"unmarked@gmail.example".to_owned()));
    assert_eq!(
        connection.thread("termin@alice.example").await,
        [
            (
                "dave-re-termin@example.org".to_owned(),
                "Passt so, bis Dienstag.".to_owned()
            ),
            (
                "alice-re-re-termin@alice.example".to_owned(),
                "Prima.".to_owned()
            ),
        ]
    );

    // A folder recreated under the same name gets a new UIDVALIDITY: its UIDs mean other mails.
    mailbox.session.select("INBOX").await.unwrap();
    mailbox.session.delete("Archiv").await.unwrap();
    mailbox.session.create("Archiv").await.unwrap();
    mailbox
        .append(
            "Archiv",
            "10-original-message",
            "17-Jan-2025 16:00:00 +0100",
        )
        .await;
    let third = connection.run().await;

    assert_eq!(stats(&third), (0, 1, 1), "{third}");
    assert!(
        third["log"]
            .as_str()
            .unwrap()
            .contains("Archiv has a new UIDVALIDITY"),
        "{third}"
    );
    let state: Value = sqlx::query_scalar("select state from connections")
        .fetch_one(&connection.pool)
        .await
        .unwrap();
    assert_eq!(
        state["folders"]
            .as_object()
            .unwrap()
            .keys()
            .collect::<Vec<_>>(),
        ["Archiv", "INBOX", "Leer", "Sent"]
    );
}

#[sqlx::test(migrations = false)]
async fn a_wrong_password_fails_the_run_without_repeating_it(pool: PgPool) {
    let mailbox = TestMailbox::new().await;
    let connection = Connection::create(pool, mailbox.config("wrong-password")).await;

    let run = connection.run().await;

    assert_eq!(run["status"], "FAILED", "{run}");
    let error = run["error"].as_str().unwrap();
    assert!(error.contains("logging in as test-"), "{error}");
    assert!(!run.to_string().contains("wrong-password"), "{run}");
}
