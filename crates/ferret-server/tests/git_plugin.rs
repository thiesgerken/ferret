mod common;

use std::{
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use axum::{
    Router,
    http::{HeaderMap, StatusCode, Uri, header},
    response::IntoResponse,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use common::{TestServer, start_server};
use ferret_api::SearchQuery;
use ferret_plugins::worker::Worker;
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

/// A repository with a Markdown note, a plain file and an excluded settings file.
fn repository(dir: &Path) {
    git(dir, &["init", "-q", "-b", "main"]);
    std::fs::create_dir_all(dir.join("notes")).unwrap();
    std::fs::create_dir_all(dir.join(".obsidian")).unwrap();
    std::fs::write(dir.join("notes/alpha.md"), "# Alpha\n\nThe invoice is due.").unwrap();
    std::fs::write(dir.join("todo.txt"), "Pay the invoice.").unwrap();
    std::fs::write(dir.join(".obsidian/app.json"), "{\"invoice\": true}").unwrap();
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
    let repo = tempfile::tempdir().unwrap();
    repository(repo.path());
    let server = start_server(pool.clone()).await;
    let worker = Worker {
        internal: server.internal(),
        public_url: server.public_url(),
        name: "test-worker".to_owned(),
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
            "{ connections { runs { status stats { put unchanged deleted } } } }",
            json!({}),
        )
        .await;
    // The note is put again because its URL carries the new commit.
    assert_eq!(
        runs["data"]["connections"][0]["runs"],
        json!([
            {"status": "SUCCEEDED", "stats": {"put": 1, "unchanged": 0, "deleted": 1}},
            {"status": "SUCCEEDED", "stats": {"put": 2, "unchanged": 0, "deleted": 0}},
        ])
    );
}

/// Serves `repo` over git's dumb HTTP protocol, only to requests with the token, the way GitHub
/// takes one: basic auth with any user name.
async fn serve_private(repo: PathBuf, token: &str) -> String {
    let expected = format!(
        "Basic {}",
        STANDARD.encode(format!("x-access-token:{token}"))
    );
    let app = Router::new().fallback(move |headers: HeaderMap, uri: Uri| {
        let authorized = headers
            .get(header::AUTHORIZATION)
            .is_some_and(|value| value == expected.as_str());
        let path = repo.join(uri.path().trim_start_matches('/'));
        async move {
            if !authorized {
                return (
                    StatusCode::UNAUTHORIZED,
                    [(header::WWW_AUTHENTICATE, "Basic realm=\"git\"")],
                )
                    .into_response();
            }
            match std::fs::read(path) {
                Ok(bytes) => bytes.into_response(),
                Err(_) => StatusCode::NOT_FOUND.into_response(),
            }
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/repo.git", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    url
}

#[sqlx::test(migrations = false)]
async fn a_token_clones_a_private_repository_and_never_leaves_the_server(pool: PgPool) {
    let repo = tempfile::tempdir().unwrap();
    repository(repo.path());
    let served = tempfile::tempdir().unwrap();
    let bare = served.path().join("repo.git");
    git(
        repo.path(),
        &["clone", "-q", "--bare", ".", bare.to_str().unwrap()],
    );
    git(&bare, &["update-server-info"]);
    let remote = serve_private(served.path().to_owned(), "s3cret-token").await;
    let anonymous = reqwest::get(format!("{remote}/info/refs")).await.unwrap();
    assert_eq!(anonymous.status(), reqwest::StatusCode::UNAUTHORIZED);

    let server = start_server(pool.clone()).await;
    let worker = Worker {
        internal: server.internal(),
        public_url: server.public_url(),
        name: "test-worker".to_owned(),
    };
    let worker_id = worker.register().await;
    let cookie = server.sign_in().await;
    let created = server
        .graphql(
            &cookie,
            "mutation ($input: NewConnection!) { createConnection(input: $input) { id config } }",
            json!({"input": {
                "plugin": "git",
                "source": "private",
                "config": json!({"remote": remote, "token": "s3cret-token"}).to_string(),
                "schedule": "0 * * * *",
            }}),
        )
        .await;
    assert_eq!(created["errors"], Value::Null, "{created}");
    let id = created["data"]["createConnection"]["id"].clone();
    assert_eq!(
        created["data"]["createConnection"]["config"],
        json!({"remote": remote}).to_string()
    );

    assert!(worker.work_once(worker_id, Duration::ZERO).await.unwrap());
    // Nothing excluded: the note, the todo and the settings file.
    assert_eq!(invoice_hits(&server).await.len(), 3);

    // An update that leaves the token out keeps it.
    let updated = server
        .graphql(
            &cookie,
            "mutation ($id: ID!, $input: ConnectionUpdate!) {
                updateConnection(id: $id, input: $input) { config }
            }",
            json!({"id": id, "input": {
                "config": json!({"remote": remote, "include": ["notes/**"]}).to_string(),
                "schedule": "0 * * * *",
                "enabled": true,
            }}),
        )
        .await;
    assert_eq!(updated["errors"], Value::Null, "{updated}");
    server
        .graphql(
            &cookie,
            "mutation ($id: ID!) { runConnectionNow(id: $id) }",
            json!({"id": id}),
        )
        .await;
    assert!(worker.work_once(worker_id, Duration::ZERO).await.unwrap());
    assert_eq!(invoice_hits(&server).await.len(), 1);

    let runs = server
        .graphql(
            &cookie,
            "{ connections { config runs { status error } } }",
            json!({}),
        )
        .await;
    assert_eq!(
        runs["data"]["connections"][0]["runs"],
        json!([{"status": "SUCCEEDED", "error": null}, {"status": "SUCCEEDED", "error": null}])
    );
    assert!(!runs.to_string().contains("s3cret-token"));
    let (config, credentials): (String, Vec<u8>) =
        sqlx::query_as("select config::text, credentials from connections")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(!config.contains("s3cret-token"));
    assert!(
        !credentials
            .windows(12)
            .any(|window| window == b"s3cret-token")
    );
}
