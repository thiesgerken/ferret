#![cfg(target_os = "linux")]

mod common;

use common::{TestServer, start_server};
use ferret_api::{
    SearchMode, SearchQuery,
    internal::{ClaimedRun, RunFinish, RunResult, WorkerRegistration},
};
use ferret_plugins::plugins::{self, Log, PluginOptions, Run};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::{fs, os::unix::fs::symlink, path::Path, time::Duration};
use uuid::Uuid;

async fn execute(
    server: &TestServer,
    worker: Uuid,
) -> (
    ClaimedRun,
    Result<plugins::Outcome, plugins::PluginError>,
    Vec<String>,
) {
    let claimed = server
        .internal()
        .claim(worker, Duration::ZERO)
        .await
        .unwrap()
        .unwrap();
    let run = Run {
        api: server.api_with_key(&claimed.token),
        source: claimed.source.clone(),
        config: claimed.config.clone(),
        state: claimed.state.clone(),
        log: Log::default(),
        options: PluginOptions::default(),
    };
    let result = plugins::run("files", &run).await;
    let logs = run.log.take();
    server
        .internal()
        .finish(
            claimed.run_id,
            &RunFinish {
                result: match &result {
                    Ok(outcome) => RunResult::Succeeded {
                        state: outcome.state.clone(),
                        stats: outcome.stats,
                    },
                    Err(error) => RunResult::Failed {
                        error: error.to_string(),
                    },
                },
                log_lines: logs.clone(),
            },
        )
        .await
        .unwrap();
    (claimed, result, logs)
}

async fn due(pool: &PgPool, connection: Uuid, config: &Value) {
    sqlx::query("update connections set config = $2, next_run_at = now() where id = $1")
        .bind(connection)
        .bind(config)
        .execute(pool)
        .await
        .unwrap();
}

async fn documents(pool: &PgPool) -> Vec<(String, String, String, Option<String>)> {
    sqlx::query_as("select external_id, id::text, text, url from documents where source = 'files/fixture' order by external_id")
        .fetch_all(pool).await.unwrap()
}

#[sqlx::test(migrations = false)]
async fn mounted_files_reconcile_contents_filters_links_and_failed_runs(pool: PgPool) {
    let root = std::env::var("FERRET_TEST_FILES_ROOT")
        .expect("mounted files integration requires an empty isolated Linux mount at FERRET_TEST_FILES_ROOT");
    let root = Path::new(&root);
    assert_eq!(
        fs::read_dir(root).unwrap().count(),
        0,
        "fixture mount must start empty; never point at personal data"
    );
    fs::create_dir_all(root.join("nested/empty")).unwrap();
    fs::create_dir_all(root.join("nested/.git")).unwrap();
    fs::create_dir_all(root.join("Archive")).unwrap();
    fs::create_dir_all(root.join("build")).unwrap();
    fs::create_dir_all(root.join("private")).unwrap();
    fs::write(root.join("private/secret.txt"), "never disclose").unwrap();
    fs::write(root.join("nested/.git/config"), "never disclose").unwrap();
    fs::write(root.join(".hidden.txt"), "ordinary hidden text").unwrap();
    for extension in [
        "md", "tex", "py", "ts", "tsx", "rs", "json", "yaml", "yml", "toml",
    ] {
        fs::write(
            root.join(format!("nested/prose.{extension}")),
            format!("cobaltquasar content only {extension}"),
        )
        .unwrap();
    }
    fs::write(root.join("README"), "conventional extensionless prose").unwrap();
    fs::write(root.join("binary-movie.mp4"), [0_u8, 255, 1, 2]).unwrap();
    fs::write(root.join("invalid.txt"), [255_u8, 254]).unwrap();
    fs::write(root.join("nul.txt"), b"a\0b").unwrap();
    fs::write(root.join("boundary.txt"), "x".repeat(64)).unwrap();
    fs::write(root.join("oversize.txt"), "x".repeat(65)).unwrap();
    fs::write(root.join("odd.unusual"), "unusual extension text").unwrap();
    fs::write(root.join("quote\"\n# ü?.txt"), "special path text").unwrap();
    symlink("/", root.join("outside")).unwrap();
    symlink(".", root.join("loop")).unwrap();
    let server = start_server(pool.clone()).await;
    let worker = server
        .internal()
        .register(&WorkerRegistration {
            name: "files-fixture".into(),
            plugins: plugins::registrations(),
        })
        .await
        .unwrap()
        .worker_id;
    let mut config = json!({"root":root.to_str().unwrap(), "origin_url_prefix":"nfs://nas.example.com/export/",
        "content_globs":["**/*.unusual"], "exclude_globs":["**/private"], "max_file_size":64});
    let cookie = server.sign_in().await;
    let created = server.graphql(&cookie,
        "mutation ($input: NewConnection!) { createConnection(input: $input) { id } }",
        json!({"input":{"name":"Mounted fixture", "icon":"folder", "plugin":"files", "source":"files/fixture",
            "schedule":"0 2,14 * * *", "config":config.to_string()}})).await;
    assert_eq!(created["errors"], Value::Null, "{created}");
    let connection: Uuid = created["data"]["createConnection"]["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let (_, first, logs) = execute(&server, worker).await;
    first.unwrap();
    assert!(logs.iter().any(|line| line.contains("invalid UTF-8")));
    assert!(logs.iter().any(|line| line.contains("NUL-containing")));
    assert!(logs.iter().any(|line| line.contains("oversized")));
    let initial = documents(&pool).await;
    let ids: Vec<_> = initial.iter().map(|doc| doc.0.as_str()).collect();
    for expected in [
        "dir:",
        "dir:nested",
        "dir:nested/empty",
        "dir:Archive",
        "dir:build",
        "file:README",
        "file:.hidden.txt",
        "file:boundary.txt",
        "file:odd.unusual",
    ] {
        assert!(ids.contains(&expected), "missing {expected}: {ids:?}");
    }
    for missing in [
        "file:invalid.txt",
        "file:nul.txt",
        "file:oversize.txt",
        "file:binary-movie.mp4",
        "dir:private",
        "dir:nested/.git",
        "dir:outside",
        "dir:loop",
    ] {
        assert!(!ids.contains(&missing), "unexpected {missing}");
    }
    let listing = &initial.iter().find(|doc| doc.0 == "dir:").unwrap().2;
    assert!(listing.contains("binary-movie.mp4"));
    assert!(!listing.contains("private"));
    assert!(!listing.contains("prose.rs"));
    assert!(!listing.contains("cobaltquasar"));
    let hits = server
        .admin_api()
        .await
        .search(&SearchQuery {
            q: "cobaltquasar".into(),
            mode: SearchMode::Keyword,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(hits.total, 10);
    let hits = server
        .admin_api()
        .await
        .search(&SearchQuery {
            q: "binary-movie".into(),
            mode: SearchMode::Keyword,
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(hits.hits.iter().any(|hit| hit.external_id == "dir:"));
    let special = initial
        .iter()
        .find(|doc| doc.0 == "file:quote\"\n# ü?.txt")
        .unwrap();
    assert_eq!(
        special.3.as_deref(),
        Some("nfs://nas.example.com/export/quote%22%0A%23%20%C3%BC%3F.txt")
    );

    due(&pool, connection, &config).await;
    let (_, repeat, logs) = execute(&server, worker).await;
    assert_eq!(repeat.unwrap().stats.put, 0);
    assert!(logs.iter().any(|line| line.contains("0 file bodies read")));
    assert_eq!(initial, documents(&pool).await);

    fs::rename(root.join("README"), root.join("renamed.txt")).unwrap();
    fs::write(
        root.join("nested/prose.rs"),
        "cobaltquasar changed child content",
    )
    .unwrap();
    fs::write(root.join("boundary.txt"), [0_u8]).unwrap();
    fs::remove_file(root.join("odd.unusual")).unwrap();
    fs::remove_dir(root.join("nested/empty")).unwrap();
    fs::write(root.join("nested/empty"), "path now a regular file").unwrap();
    config["content_globs"] = json!(["**/*.unusual", "nested/empty"]);
    due(&pool, connection, &config).await;
    execute(&server, worker).await.1.unwrap();
    let changed = documents(&pool).await;
    for removed in [
        "file:README",
        "file:boundary.txt",
        "file:odd.unusual",
        "dir:nested/empty",
    ] {
        assert!(!changed.iter().any(|doc| doc.0 == removed), "{removed}");
    }
    assert!(changed.iter().any(|doc| doc.0 == "file:renamed.txt"));
    assert!(changed.iter().any(|doc| doc.0 == "file:nested/empty"));
    assert_ne!(
        initial.iter().find(|doc| doc.0 == "dir:nested").unwrap().2,
        changed.iter().find(|doc| doc.0 == "dir:nested").unwrap().2
    );

    config["origin_url_prefix"] = json!("nfs://other.example.com/new%20export/");
    config["exclude_globs"] = json!([]);
    config["max_file_size"] = json!(65);
    due(&pool, connection, &config).await;
    execute(&server, worker).await.1.unwrap();
    let relinked = documents(&pool).await;
    assert!(relinked.iter().all(|doc| {
        doc.3
            .as_ref()
            .unwrap()
            .starts_with("nfs://other.example.com/new%20export/")
    }));
    assert!(
        relinked
            .iter()
            .any(|doc| doc.0 == "file:private/secret.txt")
    );
    assert!(relinked.iter().any(|doc| doc.0 == "file:oversize.txt"));

    // Losing the mount configuration cannot sweep any previously indexed documents.
    let original_root = config["root"].clone();
    config["root"] = json!(root.join("unmounted-placeholder").to_str().unwrap());
    fs::create_dir(root.join("unmounted-placeholder")).unwrap();
    due(&pool, connection, &config).await;
    assert!(execute(&server, worker).await.1.is_err());
    assert_eq!(relinked, documents(&pool).await);
    config["root"] = original_root;
    fs::remove_file(root.join("renamed.txt")).unwrap();
    due(&pool, connection, &config).await;
    let (retry, recovered, logs) = execute(&server, worker).await;
    assert_eq!(retry.state, json!({}));
    recovered.unwrap();
    assert!(logs.iter().any(|line| line.contains("full reconciliation")));
    assert!(
        !documents(&pool)
            .await
            .iter()
            .any(|doc| doc.0 == "file:renamed.txt")
    );

    // A complete directory body above the HTTP hard limit must fail, not truncate or sweep.
    fs::create_dir(root.join("zz-large")).unwrap();
    for i in 0..12500 {
        fs::write(
            root.join("zz-large")
                .join(format!("{i:05}-{}.bin", "longname".repeat(16))),
            [],
        )
        .unwrap();
    }
    fs::remove_file(root.join("oversize.txt")).unwrap();
    due(&pool, connection, &config).await;
    let (_, failed, _) = execute(&server, worker).await;
    assert!(failed.err().unwrap().to_string().contains("413"));
    assert!(
        documents(&pool)
            .await
            .iter()
            .any(|doc| doc.0 == "file:oversize.txt")
    );
    fs::remove_dir_all(root.join("zz-large")).unwrap();
    due(&pool, connection, &config).await;
    execute(&server, worker).await.1.unwrap();
    assert!(
        !documents(&pool)
            .await
            .iter()
            .any(|doc| doc.0 == "file:oversize.txt")
    );

    // This mount started empty and every child was created by this test.
    for child in fs::read_dir(root).unwrap() {
        let child = child.unwrap();
        if child.file_type().unwrap().is_dir() {
            fs::remove_dir_all(child.path()).unwrap();
        } else {
            fs::remove_file(child.path()).unwrap();
        }
    }
    due(&pool, connection, &config).await;
    execute(&server, worker).await.1.unwrap();
    let empty = documents(&pool).await;
    assert_eq!(empty.len(), 1);
    assert_eq!(empty[0].0, "dir:");
    assert!(empty[0].2.contains("share root"));
}
