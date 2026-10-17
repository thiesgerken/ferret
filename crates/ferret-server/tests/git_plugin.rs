mod common;

use std::{path::Path, process::Command, time::Duration};

use common::{TestServer, start_server};
use ferret_api::SearchQuery;
use ferret_plugins::{plugins::PluginOptions, worker::Worker};
use serde_json::{Value, json};
use sqlx::PgPool;

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

#[sqlx::test(migrations = false)]
async fn a_git_connection_indexes_the_files_and_sweeps_removed_ones(pool: PgPool) {
    let worker_files = tempfile::tempdir().unwrap();
    let outside = worker_files.path().join("token");
    std::fs::write(&outside, "The invoice for the worker's own secret.").unwrap();
    let repo = tempfile::tempdir().unwrap();
    repository(repo.path(), &outside);
    let server = start_server(pool.clone()).await;
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
                "config": json!({
                    "remote": format!("file://{}", repo.path().display()),
                    "exclude": [".obsidian/**"],
                    "url_template": "https://git.example.com/{commit}/{path}",
                }).to_string(),
                "schedule": "0 * * * *",
            }}),
        )
        .await;
    assert_eq!(created["errors"], Value::Null, "{created}");
    let id = created["data"]["createConnection"]["id"].clone();

    assert!(worker.work_once(worker_id, Duration::ZERO).await.unwrap());

    let head = String::from_utf8(
        Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(repo.path())
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    let head = head.trim();
    assert_eq!(
        invoice_hits(&server).await,
        [
            (
                "notes/alpha.md".to_owned(),
                "Alpha".to_owned(),
                vec!["Grace".to_owned()],
                Some(format!("https://git.example.com/{head}/notes/alpha.md")),
            ),
            (
                "todo.txt".to_owned(),
                "todo.txt".to_owned(),
                vec!["Ada".to_owned()],
                Some(format!("https://git.example.com/{head}/todo.txt")),
            ),
        ]
    );
    let state: Value = sqlx::query_scalar("select state from connections")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state, json!({"commit": head}));

    std::fs::remove_file(repo.path().join("todo.txt")).unwrap();
    commit(repo.path(), "Ada", "remove the todo");
    server
        .graphql(
            &cookie,
            "mutation ($id: ID!) { runConnectionNow(id: $id) }",
            json!({"id": id}),
        )
        .await;
    assert!(worker.work_once(worker_id, Duration::ZERO).await.unwrap());

    let hits = invoice_hits(&server).await;
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].0, "notes/alpha.md");
    let runs = server
        .graphql(
            &cookie,
            "{ connections { runs { status log stats { put unchanged deleted } } } }",
            json!({}),
        )
        .await;
    let runs = runs["data"]["connections"][0]["runs"].as_array().unwrap();
    // The note is put again because its URL carries the new commit.
    assert_eq!(
        runs.iter()
            .map(|run| (run["status"].clone(), run["stats"].clone()))
            .collect::<Vec<_>>(),
        [
            (
                json!("SUCCEEDED"),
                json!({"put": 1, "unchanged": 0, "deleted": 1})
            ),
            (
                json!("SUCCEEDED"),
                json!({"put": 2, "unchanged": 0, "deleted": 0})
            ),
        ]
    );
    assert!(
        runs[1]["log"]
            .as_str()
            .unwrap()
            .contains("skipped leak.txt: not a regular file"),
        "{runs:?}"
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
}
