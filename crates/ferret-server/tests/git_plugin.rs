mod common;

use std::{path::Path, process::Command, time::Duration};

use common::{TestServer, start_server};
use ferret_api::SearchQuery;
use ferret_plugins::{plugins::PluginOptions, worker::Worker};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

fn git(repo: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(repo)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

fn commit(repo: &Path, author: &str, message: &str) {
    git(repo, &["add", "-A"]);
    git(
        repo,
        &[
            "-c",
            &format!("user.name={author}"),
            "-c",
            "user.email=a@example.com",
            "commit",
            "-qm",
            message,
        ],
    );
}

/// A repository with a Markdown note, a plain file, an excluded settings file and a symlink to
/// `outside`, a file on the worker that mustn't get into the index.
fn repository(dir: &Path, outside: &Path) {
    git(dir, &["init", "-q", "-b", "main"]);
    std::fs::create_dir_all(dir.join("notes")).unwrap();
    std::fs::create_dir_all(dir.join(".obsidian")).unwrap();
    std::fs::write(dir.join("notes/alpha.md"), "# Alpha\n\nThe invoice is due.").unwrap();
    std::fs::write(dir.join("todo.txt"), "Pay the invoice.").unwrap();
    std::fs::write(dir.join(".obsidian/app.json"), "{\"invoice\": true}").unwrap();
    std::os::unix::fs::symlink(outside, dir.join("leak.txt")).unwrap();
    commit(dir, "Ada", "first");
    std::fs::write(
        dir.join("notes/alpha.md"),
        "# Alpha\n\nThe invoice is paid.",
    )
    .unwrap();
    commit(dir, "Grace", "second");
}

/// A git connection on its own worker, run on demand.
struct GitConnection<'a> {
    server: &'a TestServer,
    cookie: String,
    id: Value,
    worker: Worker,
    worker_id: Uuid,
}

impl<'a> GitConnection<'a> {
    async fn create(server: &'a TestServer, config: Value) -> Self {
        let worker = Worker {
            internal: server.internal(),
            public_url: server.public_url(),
            name: "test-worker".to_owned(),
            options: PluginOptions {
                git_protocols: "file".to_owned(),
            },
        };
        let worker_id = worker.register().await;
        let cookie = server.sign_in().await;
        let created = server
            .graphql(
                &cookie,
                "mutation ($input: NewConnection!) { createConnection(input: $input) { id } }",
                json!({"input": {
                    "plugin": "git",
                    "source": "repo",
                    "config": config.to_string(),
                    "schedule": "0 * * * *",
                }}),
            )
            .await;
        assert_eq!(created["errors"], Value::Null, "{created}");
        Self {
            server,
            cookie,
            id: created["data"]["createConnection"]["id"].clone(),
            worker,
            worker_id,
        }
    }

    async fn update(&self, config: Value) {
        let updated = self
            .server
            .graphql(
                &self.cookie,
                "mutation ($id: ID!, $input: ConnectionUpdate!) {
                    updateConnection(id: $id, input: $input) { id }
                }",
                json!({"id": self.id, "input": {
                    "config": config.to_string(),
                    "schedule": "0 * * * *",
                    "enabled": true,
                }}),
            )
            .await;
        assert_eq!(updated["errors"], Value::Null, "{updated}");
    }

    /// Runs the connection now and returns every run so far, newest first.
    async fn run(&self) -> Vec<Value> {
        let queued = self
            .server
            .graphql(
                &self.cookie,
                "mutation ($id: ID!) { runConnectionNow(id: $id) }",
                json!({"id": self.id}),
            )
            .await;
        assert_eq!(queued["errors"], Value::Null, "{queued}");
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
                "{ connections { runs { status log stats { put unchanged deleted } } } }",
                json!({}),
            )
            .await;
        runs["data"]["connections"][0]["runs"]
            .as_array()
            .unwrap()
            .clone()
    }
}

async fn external_ids(pool: &PgPool) -> Vec<String> {
    sqlx::query_scalar("select external_id from documents order by external_id")
        .fetch_all(pool)
        .await
        .unwrap()
}

async fn invoice_hits(server: &TestServer) -> Vec<(String, String, Vec<String>, Option<String>)> {
    let mut hits: Vec<_> = server
        .admin_api()
        .await
        .search(&SearchQuery {
            q: "invoice".to_owned(),
            ..Default::default()
        })
        .await
        .unwrap()
        .hits
        .into_iter()
        .map(|hit| {
            (
                hit.external_id,
                hit.title.unwrap_or_default(),
                hit.authors,
                hit.url,
            )
        })
        .collect();
    hits.sort();
    hits
}

fn head(repo: &Path) -> String {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo)
        .output()
        .unwrap();
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

#[sqlx::test(migrations = false)]
async fn a_git_connection_indexes_the_files_and_deletes_removed_ones(pool: PgPool) {
    let worker_files = tempfile::tempdir().unwrap();
    let outside = worker_files.path().join("token");
    std::fs::write(&outside, "The invoice for the worker's own secret.").unwrap();
    let repo = tempfile::tempdir().unwrap();
    repository(repo.path(), &outside);
    let server = start_server(pool.clone()).await;
    let connection = GitConnection::create(
        &server,
        json!({
            "remote": format!("file://{}", repo.path().display()),
            "exclude": [".obsidian/**"],
            "url_template": "https://git.example.com/{commit}/{path}",
        }),
    )
    .await;

    let runs = connection.run().await;

    let first = head(repo.path());
    assert_eq!(
        invoice_hits(&server).await,
        [
            (
                "notes/alpha.md".to_owned(),
                "Alpha".to_owned(),
                vec!["Grace".to_owned()],
                Some(format!("https://git.example.com/{first}/notes/alpha.md")),
            ),
            (
                "todo.txt".to_owned(),
                "todo.txt".to_owned(),
                vec!["Ada".to_owned()],
                Some(format!("https://git.example.com/{first}/todo.txt")),
            ),
        ]
    );
    assert!(
        runs[0]["log"]
            .as_str()
            .unwrap()
            .contains("skipped leak.txt: not a regular file"),
        "{runs:?}"
    );
    let state: Value = sqlx::query_scalar("select state from connections")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state["commit"], json!(first));

    std::fs::remove_file(repo.path().join("todo.txt")).unwrap();
    commit(repo.path(), "Ada", "remove the todo");
    let runs = connection.run().await;

    // Only the diff: the note keeps its URL to the commit it was indexed at.
    assert_eq!(
        runs.iter()
            .map(|run| (run["status"].clone(), run["stats"].clone()))
            .collect::<Vec<_>>(),
        [
            (
                json!("SUCCEEDED"),
                json!({"put": 0, "unchanged": 0, "deleted": 1})
            ),
            (
                json!("SUCCEEDED"),
                json!({"put": 2, "unchanged": 0, "deleted": 0})
            ),
        ]
    );
    let hits = invoice_hits(&server).await;
    assert_eq!(hits.len(), 1);
    assert_eq!(
        hits[0].3,
        Some(format!("https://git.example.com/{first}/notes/alpha.md"))
    );
    let leaked = server
        .admin_api()
        .await
        .search(&SearchQuery {
            q: "secret".to_owned(),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(leaked.hits.is_empty(), "the symlink's target got indexed");

    std::fs::write(
        repo.path().join("notes/alpha.md"),
        "# Alpha\n\nThe invoice is overdue.",
    )
    .unwrap();
    commit(repo.path(), "Grace", "overdue");
    connection.run().await;

    let latest = head(repo.path());
    assert_eq!(
        invoice_hits(&server).await[0].3,
        Some(format!("https://git.example.com/{latest}/notes/alpha.md"))
    );
}

#[sqlx::test(migrations = false)]
async fn later_runs_index_the_diff_unless_history_or_config_changed(pool: PgPool) {
    let repo = tempfile::tempdir().unwrap();
    let dir = repo.path();
    git(dir, &["init", "-q", "-b", "main"]);
    std::fs::write(dir.join("a.md"), "# A\n\nThe invoice is due.").unwrap();
    std::fs::write(dir.join("b.txt"), "Pay the invoice.").unwrap();
    std::fs::write(dir.join("c.txt"), "The invoice stays.").unwrap();
    commit(dir, "Ada", "first");
    let remote = format!("file://{}", dir.display());
    let server = start_server(pool.clone()).await;
    let connection = GitConnection::create(&server, json!({"remote": remote})).await;
    let log = |runs: &[Value]| runs[0]["log"].as_str().unwrap().to_owned();

    let runs = connection.run().await;
    assert!(log(&runs).contains("full sync: no earlier run"), "{runs:?}");

    let base = head(dir);
    std::fs::write(dir.join("a.md"), "# A\n\nThe invoice is paid.").unwrap();
    git(dir, &["mv", "b.txt", "d.txt"]);
    commit(dir, "Grace", "pay and rename");
    let runs = connection.run().await;
    assert!(
        log(&runs).contains(&format!("indexing the changes since {base}")),
        "{runs:?}"
    );
    assert_eq!(
        runs[0]["stats"],
        json!({"put": 2, "unchanged": 0, "deleted": 1})
    );
    assert_eq!(external_ids(&pool).await, ["a.md", "c.txt", "d.txt"]);

    // A force push: the indexed commit is gone from the branch.
    let rewritten = head(dir);
    std::fs::write(dir.join("c.txt"), "The invoice moved.").unwrap();
    git(dir, &["add", "-A"]);
    git(
        dir,
        &[
            "-c",
            "user.name=Grace",
            "-c",
            "user.email=a@example.com",
            "commit",
            "-q",
            "--amend",
            "--no-edit",
        ],
    );
    let runs = connection.run().await;
    assert!(
        log(&runs).contains(&format!(
            "full sync: {rewritten} is no longer in the branch's history"
        )),
        "{runs:?}"
    );
    let moved: i64 =
        sqlx::query_scalar("select count(*) from chunks where text like '%invoice moved%'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(moved, 1);

    // Files the diff doesn't mention fall out of the selection.
    connection
        .update(json!({"remote": remote, "exclude": ["*.txt"]}))
        .await;
    let runs = connection.run().await;
    assert!(
        log(&runs).contains("full sync: which files to index or how changed"),
        "{runs:?}"
    );
    assert_eq!(external_ids(&pool).await, ["a.md"]);
}
