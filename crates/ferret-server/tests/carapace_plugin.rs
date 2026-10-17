mod common;

use std::{collections::BTreeMap, path::Path, process::Command, time::Duration};

use common::{TestServer, start_server};
use ferret_plugins::{plugins::PluginOptions, worker::Worker};
use serde_json::{Value, json};
use sqlx::PgPool;

const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../ferret-plugins/fixtures/carapace"
);
const TALOS: &str = "memory/sessions/2026/09/s-talos.md";

fn git(repo: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(repo)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

fn commit(repo: &Path, message: &str) {
    git(repo, &["add", "-A"]);
    git(
        repo,
        &[
            "-c",
            "user.name=carapace",
            "-c",
            "user.email=c@example.com",
            "commit",
            "-qm",
            message,
        ],
    );
}

/// A knowledge repository: carapace's rendered mirror plus a skill.
fn repository(dir: &Path) {
    git(dir, &["init", "-q", "-b", "main"]);
    for entry in walk(&Path::new(FIXTURES).join("memory")) {
        let target = dir.join(entry.strip_prefix(FIXTURES).unwrap());
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::copy(&entry, target).unwrap();
    }
    std::fs::create_dir_all(dir.join("skills/brag")).unwrap();
    std::fs::write(dir.join("skills/brag/SKILL.md"), "# Brag\n\nCollects wins.").unwrap();
    commit(dir, "mirror");
}

fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
    std::fs::read_dir(dir)
        .unwrap()
        .flat_map(|entry| {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path)
            } else {
                vec![path]
            }
        })
        .collect()
}

async fn documents_per_source(pool: &PgPool) -> BTreeMap<String, i64> {
    sqlx::query_as::<_, (String, i64)>("select source, count(*) from documents group by source")
        .fetch_all(pool)
        .await
        .unwrap()
        .into_iter()
        .collect()
}

async fn run_now(server: &TestServer, cookie: &str, id: &Value) {
    server
        .graphql(
            cookie,
            "mutation ($id: ID!) { runConnectionNow(id: $id) }",
            json!({"id": id}),
        )
        .await;
}

async fn latest_run(server: &TestServer, cookie: &str) -> Value {
    let runs = server
        .graphql(
            cookie,
            "{ connections { runs(limit: 1) { status error } } }",
            json!({}),
        )
        .await;
    runs["data"]["connections"][0]["runs"][0].clone()
}

#[sqlx::test(migrations = false)]
async fn a_knowledge_repository_lands_in_four_sources_and_follows_its_changes(pool: PgPool) {
    let repo = tempfile::tempdir().unwrap();
    repository(repo.path());
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
                "plugin": "carapace",
                "source": "carapace",
                "config": json!({
                    "remote": format!("file://{}", repo.path().display()),
                    "carapace_url": "https://carapace.example/",
                }).to_string(),
                "schedule": "0 * * * *",
            }}),
        )
        .await;
    assert_eq!(created["errors"], Value::Null, "{created}");
    let id = created["data"]["createConnection"]["id"].clone();

    assert!(worker.work_once(worker_id, Duration::ZERO).await.unwrap());

    assert_eq!(latest_run(&server, &cookie).await["status"], "SUCCEEDED");
    // Two sessions with 4 + 1 distinct facts, a week and a month, the mirror's README and the skill. The
    // per-category facts files repeat the session facts and stay out.
    assert_eq!(
        documents_per_source(&pool).await,
        BTreeMap::from([
            ("carapace/digests".to_owned(), 2),
            ("carapace/facts".to_owned(), 5),
            ("carapace/files".to_owned(), 2),
            ("carapace/sessions".to_owned(), 2),
        ])
    );
    let skill_url: String =
        sqlx::query_scalar("select url from documents where external_id = 'skills/brag/SKILL.md'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        skill_url,
        "https://carapace.example/knowledge?path=skills%2Fbrag%2FSKILL.md"
    );

    // carapace re-renders the session without one of its facts (listed under two categories).
    let talos = repo.path().join(TALOS);
    let rendered = std::fs::read_to_string(&talos).unwrap();
    let without_anna: String = rendered
        .lines()
        .filter(|line| !line.contains("**Anna**: Anna helps"))
        .map(|line| format!("{line}\n"))
        .collect();
    std::fs::write(&talos, without_anna).unwrap();
    commit(repo.path(), "re-render");
    run_now(&server, &cookie, &id).await;
    assert!(worker.work_once(worker_id, Duration::ZERO).await.unwrap());

    assert_eq!(latest_run(&server, &cookie).await["status"], "SUCCEEDED");
    assert_eq!(documents_per_source(&pool).await["carapace/facts"], 4);
    let anna_helps: i64 =
        sqlx::query_scalar("select count(*) from documents where text like 'Anna helps%'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(anna_helps, 0);

    // A rendering this plugin doesn't understand fails the run before its sweep deletes anything.
    std::fs::write(
        &talos,
        rendered.replace("medium confidence", "fairly confident"),
    )
    .unwrap();
    commit(repo.path(), "new render format");
    run_now(&server, &cookie, &id).await;
    assert!(worker.work_once(worker_id, Duration::ZERO).await.unwrap());

    let failed = latest_run(&server, &cookie).await;
    assert_eq!(failed["status"], "FAILED");
    assert!(
        failed["error"]
            .as_str()
            .unwrap()
            .starts_with(&format!("{TALOS}: Facts: body line ")),
        "{failed}"
    );
    assert_eq!(documents_per_source(&pool).await["carapace/facts"], 4);

    // Nor does a session file the plugin can't read.
    std::fs::write(&talos, "binary\0").unwrap();
    commit(repo.path(), "corrupt");
    run_now(&server, &cookie, &id).await;
    assert!(worker.work_once(worker_id, Duration::ZERO).await.unwrap());

    assert_eq!(
        latest_run(&server, &cookie).await["error"],
        format!("{TALOS}: not text")
    );
    assert_eq!(documents_per_source(&pool).await["carapace/facts"], 4);
}
