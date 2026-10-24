use crate::common;

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
const TRIP: &str = "memory/sessions/2026/09/s-trip.md";
const ARCHIVE: &str = "sessions/2026/09/s-trip/conversation.json";

fn git(repo: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(repo)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

fn head(repo: &Path) -> String {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo)
        .output()
        .unwrap();
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
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

/// A knowledge repository: carapace's rendered mirror, a session archive, and a skill with its
/// lockfile.
fn repository(dir: &Path) {
    git(dir, &["init", "-q", "-b", "main"]);
    for entry in [
        walk(&Path::new(FIXTURES).join("memory")),
        walk(&Path::new(FIXTURES).join("sessions")),
    ]
    .concat()
    {
        let target = dir.join(entry.strip_prefix(FIXTURES).unwrap());
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::copy(&entry, target).unwrap();
    }
    std::fs::create_dir_all(dir.join("skills/brag")).unwrap();
    std::fs::write(
        dir.join("skills/brag/SKILL.md"),
        "---\nname: brag\ndescription: Collects wins.\n---\n# Brag\n\nCollects wins.",
    )
    .unwrap();
    std::fs::write(dir.join("skills/brag/uv.lock"), "version = 1\n").unwrap();
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

/// The external ids and parents of a session's thread.
async fn thread(pool: &PgPool, session_id: &str) -> Vec<(String, Option<String>)> {
    sqlx::query_as(
        "select external_id, parent from documents
         where source = 'carapace/sessions' and thread = $1 order by external_id",
    )
    .bind(session_id)
    .fetch_all(pool)
    .await
    .unwrap()
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
            "{ connections { runs(limit: 1) { status error log stats { put unchanged deleted } } } }",
            json!({}),
        )
        .await;
    runs["data"]["connections"][0]["runs"][0].clone()
}

#[sqlx::test(migrations = false)]
async fn a_knowledge_repository_lands_in_five_sources_and_follows_its_changes(pool: PgPool) {
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
                "name": "test", "icon": "plug", "plugin": "carapace",
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
    // Two session summaries with 4 + 1 distinct facts, the trip's three turns, a week and a month,
    // the mirror's README and the skill. The per-category facts files repeat the session facts and
    // stay out, so do the skill's lockfile and other files than Markdown.
    assert_eq!(
        documents_per_source(&pool).await,
        BTreeMap::from([
            ("carapace/digests".to_owned(), 2),
            ("carapace/facts".to_owned(), 5),
            ("carapace/files".to_owned(), 1),
            ("carapace/sessions".to_owned(), 5),
            ("carapace/skills".to_owned(), 1),
        ])
    );
    // The summary shares the thread with the turns, which go on from each other.
    let expected_thread = |ids: &[(&str, Option<&str>)]| -> Vec<(String, Option<String>)> {
        ids.iter()
            .map(|(id, parent)| (id.to_string(), parent.map(str::to_owned)))
            .collect()
    };
    assert_eq!(
        thread(&pool, "s-trip").await,
        expected_thread(&[
            ("summary:s-trip", None),
            ("turn:s-trip:1", None),
            ("turn:s-trip:15", Some("turn:s-trip:1")),
            ("turn:s-trip:19", Some("turn:s-trip:15")),
        ])
    );
    let (skill_title, skill_url): (String, String) = sqlx::query_as(
        "select title, url from documents where external_id = 'skills/brag/SKILL.md'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(skill_title, "brag", "the skill's front matter names it");
    assert_eq!(
        skill_url,
        "https://carapace.example/knowledge?path=skills%2Fbrag%2FSKILL.md"
    );

    // carapace rewrites the archive without the last turn, e.g. after a reset.
    let archive = repo.path().join(ARCHIVE);
    let full_archive = std::fs::read_to_string(&archive).unwrap();
    let mut shortened: Value = serde_json::from_str(&full_archive).unwrap();
    shortened["history"].as_array_mut().unwrap().pop();
    let shortened = serde_json::to_string_pretty(&shortened).unwrap();
    std::fs::write(&archive, &shortened).unwrap();
    commit(repo.path(), "reset the last turn");
    run_now(&server, &cookie, &id).await;
    assert!(worker.work_once(worker_id, Duration::ZERO).await.unwrap());

    assert_eq!(
        latest_run(&server, &cookie).await["stats"],
        json!({"put": 0, "unchanged": 2, "deleted": 1})
    );
    assert_eq!(
        thread(&pool, "s-trip").await,
        expected_thread(&[
            ("summary:s-trip", None),
            ("turn:s-trip:1", None),
            ("turn:s-trip:15", Some("turn:s-trip:1")),
        ])
    );

    // carapace re-renders the session without one of its facts (listed under two categories).
    let talos = repo.path().join(TALOS);
    let rendered = std::fs::read_to_string(&talos).unwrap();
    let without_anna: String = rendered
        .lines()
        .filter(|line| !line.contains("**Anna**: Anna helps"))
        .map(|line| format!("{line}\n"))
        .collect();
    std::fs::write(&talos, &without_anna).unwrap();
    commit(repo.path(), "re-render");
    run_now(&server, &cookie, &id).await;
    assert!(worker.work_once(worker_id, Duration::ZERO).await.unwrap());

    let incremental = latest_run(&server, &cookie).await;
    assert_eq!(incremental["status"], "SUCCEEDED");
    assert!(
        incremental["log"]
            .as_str()
            .unwrap()
            .contains("indexing the changes since"),
        "{incremental}"
    );
    // Only the Talos session's documents: unchanged ones and the dropped fact.
    assert_eq!(
        incremental["stats"],
        json!({"put": 0, "unchanged": 4, "deleted": 1})
    );
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
    let unparsable = head(repo.path());
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

    // Nor does an archive the plugin can't parse.
    std::fs::write(&talos, &without_anna).unwrap();
    std::fs::write(&archive, &shortened[..shortened.len() / 2]).unwrap();
    commit(repo.path(), "repair the session, tear the archive");
    run_now(&server, &cookie, &id).await;
    assert!(worker.work_once(worker_id, Duration::ZERO).await.unwrap());

    let torn = latest_run(&server, &cookie).await;
    assert_eq!(torn["status"], "FAILED");
    assert!(
        torn["error"]
            .as_str()
            .unwrap()
            .starts_with(&format!("{ARCHIVE}: ")),
        "{torn}"
    );
    assert_eq!(documents_per_source(&pool).await["carapace/sessions"], 4);

    std::fs::write(&archive, &shortened).unwrap();
    std::fs::remove_file(repo.path().join(TRIP)).unwrap();
    commit(repo.path(), "repair the archive, forget the trip's summary");
    run_now(&server, &cookie, &id).await;
    assert!(worker.work_once(worker_id, Duration::ZERO).await.unwrap());

    let documents = documents_per_source(&pool).await;
    assert_eq!(
        (documents["carapace/sessions"], documents["carapace/facts"]),
        (3, 3),
        "the Talos summary and the trip's two turns"
    );

    // A run under other indexing settings makes the next run a full sync, which puts everything
    // again and sweeps what the old settings let in: one that still indexed every file, or one
    // that keyed a session's summary by the session id alone. Inserted directly: a connection's
    // source takes documents from its runs only.
    for (source, external_id) in [
        ("carapace/files", ARCHIVE),
        ("carapace/sessions", "s-talos"),
    ] {
        sqlx::query(
            "insert into documents
                 (id, user_id, source, external_id, content_type, authors, created_at, modified_at,
                  meta, text, content_hash, seen_at)
             select gen_random_uuid(), user_id, $1, $2, 'plain', '{}', now(), now(),
                    '{}', 'stale', '\\x00', now()
             from connections",
        )
        .bind(source)
        .bind(external_id)
        .execute(&pool)
        .await
        .unwrap();
    }
    sqlx::query(
        "update connections set state = jsonb_set(state, '{indexing}', '{\"carapace_url\": \"https://carapace.example\"}')",
    )
    .execute(&pool)
    .await
    .unwrap();
    run_now(&server, &cookie, &id).await;
    assert!(worker.work_once(worker_id, Duration::ZERO).await.unwrap());

    let full = latest_run(&server, &cookie).await;
    assert!(
        full["log"]
            .as_str()
            .unwrap()
            .contains("full sync: the indexing settings changed"),
        "{full}"
    );
    let stale: i64 = sqlx::query_scalar("select count(*) from documents where text = 'stale'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(stale, 0);
    assert_eq!(
        full["stats"]["unchanged"], 10,
        "1 session, 3 facts, 2 digests, 2 files, 2 turns"
    );

    // carapace moves the session to another month's directory. The file's documents are put
    // under the new path before the old path's are deleted, so none of them go.
    let moved = repo.path().join("memory/sessions/2026/10/s-talos.md");
    std::fs::create_dir_all(moved.parent().unwrap()).unwrap();
    std::fs::rename(&talos, &moved).unwrap();
    commit(repo.path(), "move");
    run_now(&server, &cookie, &id).await;
    assert!(worker.work_once(worker_id, Duration::ZERO).await.unwrap());

    assert_eq!(
        latest_run(&server, &cookie).await["stats"],
        json!({"put": 0, "unchanged": 4, "deleted": 0})
    );
    assert_eq!(documents_per_source(&pool).await["carapace/facts"], 3);

    // A cursor whose version of a file this plugin can't parse falls back to a full sync.
    sqlx::query("update connections set state = jsonb_set(state, '{commit}', to_jsonb($1::text))")
        .bind(&unparsable)
        .execute(&pool)
        .await
        .unwrap();
    run_now(&server, &cookie, &id).await;
    assert!(worker.work_once(worker_id, Duration::ZERO).await.unwrap());

    let fallback = latest_run(&server, &cookie).await;
    assert_eq!(fallback["status"], "SUCCEEDED");
    assert!(
        fallback["log"].as_str().unwrap().contains(&format!(
            "full sync: {TALOS} at {unparsable}: Facts: body line "
        )),
        "{fallback}"
    );

    // So does a force push, after which the cursor isn't in the history anymore.
    git(
        repo.path(),
        &[
            "-c",
            "user.name=carapace",
            "-c",
            "user.email=c@example.com",
            "commit",
            "--amend",
            "-qm",
            "rewritten",
        ],
    );
    run_now(&server, &cookie, &id).await;
    assert!(worker.work_once(worker_id, Duration::ZERO).await.unwrap());

    let rewritten = latest_run(&server, &cookie).await;
    assert!(
        rewritten["log"]
            .as_str()
            .unwrap()
            .contains("is no longer in the branch's history"),
        "{rewritten}"
    );
    assert_eq!(documents_per_source(&pool).await["carapace/facts"], 3);
}
