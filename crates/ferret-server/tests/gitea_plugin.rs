use crate::common;

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::Duration,
};

use axum::{
    Json, Router,
    extract::{Query, State},
    http::{HeaderMap, StatusCode, Uri, header},
    response::{IntoResponse, Response},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use common::{TestServer, start_server};
use ferret_api::{Document, SearchQuery, ThreadQuery};
use ferret_plugins::{plugins::PluginOptions, worker::Worker};
use parking_lot::Mutex;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

const TOKEN: &str = "private-gitea-token";
const CREATED: &str = "2026-01-02T10:00:00Z";
const UPDATED: &str = "2026-01-03T10:00:00Z";

#[derive(Default)]
struct Origin {
    lists: HashMap<String, Vec<Value>>,
    failure: Option<(String, usize)>,
    requested: Vec<String>,
    duplicate_repository_page: bool,
}

#[derive(Clone)]
struct GiteaOrigin {
    data: Arc<Mutex<Origin>>,
    files: PathBuf,
}

async fn origin(
    State(origin): State<GiteaOrigin>,
    headers: HeaderMap,
    uri: Uri,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let path = uri.path().strip_prefix("/gitea/").unwrap();
    if let Some(path) = path.strip_prefix("api/v1/") {
        if headers
            .get(header::AUTHORIZATION)
            .is_none_or(|auth| auth != format!("token {TOKEN}").as_str())
        {
            return StatusCode::UNAUTHORIZED.into_response();
        }
        let path = if path.ends_with("/issues") {
            assert_eq!(query["state"], "open");
            format!("{path}:{}", query["type"])
        } else {
            path.to_owned()
        };
        let page = query["page"].parse::<usize>().unwrap();
        let mut data = origin.data.lock();
        data.requested.push(format!("{path}:{page}"));
        if data
            .failure
            .as_ref()
            .is_some_and(|failure| failure == &(path.clone(), page))
        {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
        let Some(rows) = data.lists.get(&path) else {
            return StatusCode::NOT_FOUND.into_response();
        };
        let reviews = path.ends_with("/reviews");
        let unpaged = path.contains("/reviews/") && path.ends_with("/comments");
        let offset = if data.duplicate_repository_page && path == "user/repos" && page == 2 {
            0
        } else {
            page - 1
        };
        let body: Vec<Value> = if unpaged {
            rows.clone()
        } else {
            rows.get(offset)
                .filter(|row| !row.is_null())
                .cloned()
                .into_iter()
                .collect()
        };
        let mut response = Json(body).into_response();
        if !unpaged {
            response
                .headers_mut()
                .insert("x-total-count", rows.len().to_string().parse().unwrap());
            if !reviews && page < rows.len() {
                // The worker must page the configured instance, not follow an origin-supplied URL.
                response.headers_mut().insert(
                    header::LINK,
                    "<http://unrelated.invalid/steal-token>; rel=\"next\""
                        .parse()
                        .unwrap(),
                );
            }
        }
        return response;
    }
    let credentials = format!(
        "Basic {}",
        STANDARD.encode(format!("x-access-token:{TOKEN}"))
    );
    if headers
        .get(header::AUTHORIZATION)
        .is_none_or(|auth| auth != credentials.as_str())
    {
        return (
            StatusCode::UNAUTHORIZED,
            [(header::WWW_AUTHENTICATE, "Basic realm=\"git\"")],
        )
            .into_response();
    }
    match std::fs::read(origin.files.join(path)) {
        Ok(bytes) => bytes.into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn serve(files: &Path) -> (String, Arc<Mutex<Origin>>) {
    let data = Arc::new(Mutex::new(Origin::default()));
    let app = Router::new().fallback(origin).with_state(GiteaOrigin {
        data: data.clone(),
        files: files.to_owned(),
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/gitea", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (url, data)
}

fn repository(id: u64, name: &str, url: &str, empty: bool) -> Value {
    json!({"id": id, "full_name": name, "html_url": format!("{url}/{name}"),
        "default_branch": "main", "empty": empty, "has_issues": true, "has_pull_requests": true})
}

fn discussion(id: u64, number: u64, title: &str, pull: bool) -> Value {
    json!({"id": id, "number": number, "title": title, "body": format!("Cobalt {title} body."),
        "user": {"login": "alice"}, "html_url": format!("https://git.example/acme/notes/{}/{number}", if pull {"pulls"} else {"issues"}),
        "state": "open", "pull_request": if pull {json!({"merged": false})} else {Value::Null},
        "created_at": CREATED, "updated_at": CREATED})
}

fn comment(id: u64, body: &str) -> Value {
    json!({"id": id, "body": body, "user": {"login": "bob"},
        "html_url": format!("https://git.example/acme/notes/issues/1#issuecomment-{id}"),
        "created_at": CREATED, "updated_at": CREATED})
}

fn add_repository(data: &mut Origin, repo: Value) {
    let name = repo["full_name"].as_str().unwrap();
    data.lists
        .insert(format!("repos/{name}/issues:issues"), vec![]);
    data.lists
        .insert(format!("repos/{name}/issues:pulls"), vec![]);
    data.lists
        .entry("user/repos".to_owned())
        .or_default()
        .push(repo);
}

fn git(path: &Path, args: &[&str]) {
    let result = Command::new("git")
        .current_dir(path)
        .args(args)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&result.stderr)
    );
}

fn commit(path: &Path) {
    git(path, &["add", "-A"]);
    git(
        path,
        &[
            "-c",
            "user.name=Ada",
            "-c",
            "user.email=ada@example.invalid",
            "commit",
            "-qm",
            "Update notes",
        ],
    );
}

fn publish(path: &Path, bare: &Path) {
    git(path, &["push", "-q", bare.to_str().unwrap(), "main"]);
    git(bare, &["update-server-info"]);
}

struct Connection<'a> {
    server: &'a TestServer,
    cookie: String,
    id: Value,
    worker: Worker,
    worker_id: Uuid,
}

impl<'a> Connection<'a> {
    async fn create(server: &'a TestServer, url: &str, extra: Value) -> Self {
        let worker = Worker {
            internal: server.internal(),
            public_url: server.public_url(),
            name: "gitea-test-worker".to_owned(),
            options: PluginOptions {
                git_protocols: "http".to_owned(),
            },
        };
        let worker_id = worker.register().await;
        let cookie = server.sign_in().await;
        let mut config = json!({"url": url, "token": TOKEN});
        config
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let result = server.graphql(&cookie,
            "mutation ($input: NewConnection!) { createConnection(input: $input) { id config } }",
            json!({"input": {"name": "Gitea", "icon": "git-branch", "plugin": "gitea", "source": "work",
                "schedule": "0 * * * *", "config": config.to_string()}})).await;
        assert_eq!(result["errors"], Value::Null, "{result}");
        assert!(!result.to_string().contains(TOKEN));
        Self {
            server,
            cookie,
            id: result["data"]["createConnection"]["id"].clone(),
            worker,
            worker_id,
        }
    }

    async fn run(&self) -> Value {
        let result = self
            .server
            .graphql(
                &self.cookie,
                "mutation ($id: ID!) { runConnectionNow(id: $id) }",
                json!({"id": self.id}),
            )
            .await;
        assert_eq!(result["errors"], Value::Null, "{result}");
        assert!(
            self.worker
                .work_once(self.worker_id, Duration::ZERO)
                .await
                .unwrap()
        );
        let result = self
            .server
            .graphql(
                &self.cookie,
                "{ connections { runs { status error log stats { put unchanged deleted } } } }",
                json!({}),
            )
            .await;
        assert!(!result.to_string().contains(TOKEN));
        result["data"]["connections"][0]["runs"][0].clone()
    }
}

async fn documents(server: &TestServer) -> Vec<Document> {
    let api = server.admin_api().await;
    let hits = api
        .search(&SearchQuery {
            q: "cobalt".to_owned(),
            collapse: Some(false),
            k: Some(100),
            ..Default::default()
        })
        .await
        .unwrap()
        .hits;
    let mut documents = Vec::new();
    for hit in hits {
        documents.push(api.document(hit.document_id).await.unwrap());
    }
    documents.sort_by(|a, b| (&a.source, &a.external_id).cmp(&(&b.source, &b.external_id)));
    documents
}

#[sqlx::test(migrations = false)]
async fn gitea_indexes_files_and_open_threaded_discussions_then_reconciles_changes(pool: PgPool) {
    let files = tempfile::tempdir().unwrap();
    let checkout = tempfile::tempdir().unwrap();
    git(checkout.path(), &["init", "-q", "-b", "main"]);
    std::fs::write(
        checkout.path().join("note.md"),
        "# Cobalt manual\n\nCobalt default branch.",
    )
    .unwrap();
    std::fs::write(checkout.path().join("private.md"), "Cobalt excluded file.").unwrap();
    std::fs::write(checkout.path().join("binary.md"), [0, 1, 2]).unwrap();
    std::os::unix::fs::symlink("/etc/passwd", checkout.path().join("leak.md")).unwrap();
    commit(checkout.path());
    git(checkout.path(), &["checkout", "-qb", "feature"]);
    std::fs::write(
        checkout.path().join("feature.md"),
        "Cobalt branchonly secret.",
    )
    .unwrap();
    commit(checkout.path());
    git(checkout.path(), &["checkout", "-q", "main"]);
    std::fs::create_dir_all(files.path().join("acme")).unwrap();
    let bare = files.path().join("acme/notes.git");
    git(
        checkout.path(),
        &["clone", "-q", "--bare", ".", bare.to_str().unwrap()],
    );
    git(&bare, &["update-server-info"]);
    let (url, data) = serve(files.path()).await;
    {
        let mut data = data.lock();
        add_repository(&mut data, repository(1, "acme/notes", &url, false));
        add_repository(&mut data, repository(2, "acme/excluded", &url, false));
        add_repository(&mut data, repository(3, "other/unselected", &url, false));
        data.lists.insert(
            "repos/acme/notes/issues:issues".to_owned(),
            vec![discussion(11, 1, "Invoice", false)],
        );
        data.lists.insert(
            "repos/acme/notes/issues:pulls".to_owned(),
            vec![discussion(12, 2, "Draft change", true)],
        );
        data.lists.insert(
            "repos/acme/notes/issues/1/comments".to_owned(),
            vec![
                comment(21, "Cobalt issue comment."),
                comment(22, "Cobalt later comment."),
            ],
        );
        data.lists.insert(
            "repos/acme/notes/issues/2/comments".to_owned(),
            vec![comment(23, "Cobalt PR comment.")],
        );
        data.lists.insert("repos/acme/notes/pulls/2/reviews".to_owned(), vec![
            Value::Null,
            json!({"id": 31, "body": "Cobalt submitted review.", "user": {"login": "reviewer"},
                "state": "COMMENT", "html_url": "https://git.example/review/31", "submitted_at": CREATED, "updated_at": CREATED}),
            json!({"id": 32, "body": "Cobalt private draft.", "user": {"login": "alice"},
                "state": "PENDING", "html_url": "https://git.example/review/32", "submitted_at": CREATED, "updated_at": CREATED}),
        ]);
        let mut inline = comment(41, "Cobalt inline feedback.");
        inline.as_object_mut().unwrap().extend(
            json!({"path": "feature.md", "position": 3,
            "original_position": 0, "diff_hunk": "Cobalt forbidden diff text."})
            .as_object()
            .unwrap()
            .clone(),
        );
        data.lists.insert(
            "repos/acme/notes/pulls/2/reviews/31/comments".to_owned(),
            vec![inline],
        );
    }
    let server = start_server(pool.clone()).await;
    let connection = Connection::create(
        &server,
        &url,
        json!({"include_repos": ["acme/*"],
        "exclude_repos": ["*/excluded"], "include": ["**/*.md"], "exclude": ["private.md"]}),
    )
    .await;
    let result = connection.run().await;
    assert_eq!(result["status"], "SUCCEEDED", "{result}");
    let stored = documents(&server).await;
    let identities: Vec<_> = stored
        .iter()
        .map(|document| (document.source.as_str(), document.external_id.as_str()))
        .collect();
    assert_eq!(
        identities,
        [
            ("work/acme/notes/files", "note.md"),
            ("work/acme/notes/issues", "body:11"),
            ("work/acme/notes/issues", "comment:21"),
            ("work/acme/notes/issues", "comment:22"),
            ("work/acme/notes/pulls", "body:12"),
            ("work/acme/notes/pulls", "comment:23"),
            ("work/acme/notes/pulls", "review-comment:41"),
            ("work/acme/notes/pulls", "review:31"),
        ]
    );
    let file = &stored[0];
    assert_eq!(file.authors, ["Ada"]);
    assert!(
        file.url
            .as_ref()
            .unwrap()
            .starts_with(&format!("{url}/acme/notes/src/commit/"))
    );
    assert!(file.url.as_ref().unwrap().ends_with("/note.md"));
    let inline = stored
        .iter()
        .find(|document| document.external_id == "review-comment:41")
        .unwrap();
    assert_eq!(inline.text, "Cobalt inline feedback.");
    assert_eq!(inline.meta["path"], "feature.md");
    assert_eq!(inline.meta["position"], 3);
    assert_eq!(inline.authors, ["bob"]);
    assert!(!inline.meta.contains_key("diff_hunk"));
    let thread = server
        .admin_api()
        .await
        .thread(&ThreadQuery {
            source: "work/acme/notes/issues".to_owned(),
            thread: "1".to_owned(),
        })
        .await
        .unwrap();
    assert_eq!(
        thread
            .documents
            .iter()
            .map(|doc| doc.external_id.as_str())
            .collect::<Vec<_>>(),
        ["body:11", "comment:21", "comment:22"]
    );
    let ids: Vec<_> = stored.iter().map(|document| document.id).collect();
    let result = connection.run().await;
    assert_eq!(result["status"], "SUCCEEDED", "{result}");
    assert_eq!(result["stats"]["put"], 0, "{result}");
    assert_eq!(
        documents(&server)
            .await
            .iter()
            .map(|document| document.id)
            .collect::<Vec<_>>(),
        ids
    );

    std::fs::write(checkout.path().join("note.md"), [0, 1, 2]).unwrap();
    std::fs::write(
        checkout.path().join("new.md"),
        "# Cobalt replacement\n\nCobalt updated file.",
    )
    .unwrap();
    commit(checkout.path());
    publish(checkout.path(), &bare);
    {
        let mut data = data.lock();
        data.lists
            .get_mut("repos/acme/notes/issues:issues")
            .unwrap()[0]["body"] = json!("Cobalt edited issue body.");
        data.lists
            .get_mut("repos/acme/notes/issues:issues")
            .unwrap()[0]["updated_at"] = json!(UPDATED);
        data.lists
            .get_mut("repos/acme/notes/issues/1/comments")
            .unwrap()
            .remove(1);
        data.lists
            .get_mut("repos/acme/notes/issues/1/comments")
            .unwrap()[0]["body"] = json!("Cobalt edited comment.");
        data.lists
            .get_mut("repos/acme/notes/issues/1/comments")
            .unwrap()[0]["updated_at"] = json!(UPDATED);
        data.lists
            .insert("repos/acme/notes/issues:pulls".to_owned(), vec![]);
        add_repository(&mut data, repository(4, "acme/newly-shared", &url, true));
    }
    let result = connection.run().await;
    assert_eq!(result["status"], "SUCCEEDED", "{result}");
    let stored = documents(&server).await;
    assert_eq!(
        stored
            .iter()
            .map(|document| document.external_id.as_str())
            .collect::<Vec<_>>(),
        ["new.md", "body:11", "comment:21"]
    );
    assert_eq!(stored[1].text, "Cobalt edited issue body.");
    assert_eq!(stored[2].text, "Cobalt edited comment.");
    assert!(
        data.lock()
            .requested
            .iter()
            .any(|path| path.starts_with("repos/acme/newly-shared/issues:issues"))
    );

    data.lock().lists.insert(
        "repos/acme/notes/issues:pulls".to_owned(),
        vec![discussion(12, 2, "Reopened change", true)],
    );
    let result = connection.run().await;
    assert_eq!(result["status"], "SUCCEEDED", "{result}");
    assert!(
        documents(&server)
            .await
            .iter()
            .any(|document| document.external_id == "review:31")
    );
    {
        let mut data = data.lock();
        let mut title_only = discussion(14, 4, "Cobalt title-only issue", false);
        title_only["body"] = json!("");
        data.lists
            .get_mut("repos/acme/notes/issues:issues")
            .unwrap()
            .push(title_only);
        data.lists
            .insert("repos/acme/notes/issues/4/comments".to_owned(), vec![]);
    }
    let result = connection.run().await;
    assert_eq!(result["status"], "SUCCEEDED", "{result}");
    let stored = documents(&server).await;
    assert_eq!(
        stored
            .iter()
            .find(|document| document.external_id == "body:14")
            .unwrap()
            .text,
        "Cobalt title-only issue"
    );
}

#[sqlx::test(migrations = false)]
async fn any_failed_repository_or_discovery_page_prevents_all_deletions_and_cursor_changes(
    pool: PgPool,
) {
    let files = tempfile::tempdir().unwrap();
    let checkout = tempfile::tempdir().unwrap();
    git(checkout.path(), &["init", "-q", "-b", "main"]);
    std::fs::write(
        checkout.path().join("note.md"),
        "# Cobalt independent file\n\nCobalt content.",
    )
    .unwrap();
    commit(checkout.path());
    std::fs::create_dir_all(files.path().join("acme")).unwrap();
    let bare = files.path().join("acme/third.git");
    git(
        checkout.path(),
        &["clone", "-q", "--bare", ".", bare.to_str().unwrap()],
    );
    git(&bare, &["update-server-info"]);
    let (url, data) = serve(files.path()).await;
    {
        let mut data = data.lock();
        for (id, name) in [(1, "acme/first"), (2, "acme/second")] {
            add_repository(&mut data, repository(id, name, &url, true));
            data.lists.insert(
                format!("repos/{name}/issues:issues"),
                vec![discussion(id, 1, "Original", false)],
            );
            data.lists
                .insert(format!("repos/{name}/issues/1/comments"), vec![]);
        }
    }
    let server = start_server(pool.clone()).await;
    let connection = Connection::create(&server, &url, json!({})).await;
    assert_eq!(connection.run().await["status"], "SUCCEEDED");
    let original_state: Value = sqlx::query_scalar("select state from connections")
        .fetch_one(&pool)
        .await
        .unwrap();
    {
        let mut data = data.lock();
        data.lists
            .insert("repos/acme/first/issues:issues".to_owned(), vec![]);
        data.failure = Some(("repos/acme/second/issues:issues".to_owned(), 1));
        let third = repository(3, "acme/third", &url, false);
        add_repository(&mut data, third);
        data.lists.insert(
            "repos/acme/third/issues:issues".to_owned(),
            vec![discussion(3, 1, "Independent update", false)],
        );
        data.lists
            .insert("repos/acme/third/issues/1/comments".to_owned(), vec![]);
    }
    let result = connection.run().await;
    assert_eq!(result["status"], "FAILED", "{result}");
    let stored = documents(&server).await;
    assert_eq!(
        stored
            .iter()
            .map(|doc| doc.source.as_str())
            .collect::<Vec<_>>(),
        [
            "work/acme/first/issues",
            "work/acme/second/issues",
            "work/acme/third/files",
            "work/acme/third/issues",
        ]
    );
    assert_eq!(
        sqlx::query_scalar::<_, Value>("select state from connections")
            .fetch_one(&pool)
            .await
            .unwrap(),
        original_state
    );
    {
        let mut data = data.lock();
        data.failure = Some(("user/repos".to_owned(), 2));
    }
    assert_eq!(connection.run().await["status"], "FAILED");
    assert_eq!(
        documents(&server)
            .await
            .iter()
            .map(|doc| doc.id)
            .collect::<Vec<_>>(),
        stored.iter().map(|doc| doc.id).collect::<Vec<_>>()
    );
    {
        let mut data = data.lock();
        data.failure = None;
        data.duplicate_repository_page = true;
    }
    assert_eq!(connection.run().await["status"], "FAILED");
    assert_eq!(
        documents(&server)
            .await
            .iter()
            .map(|doc| doc.id)
            .collect::<Vec<_>>(),
        stored.iter().map(|doc| doc.id).collect::<Vec<_>>()
    );
    {
        let mut data = data.lock();
        data.duplicate_repository_page = false;
        data.lists
            .get_mut("user/repos")
            .unwrap()
            .retain(|repo| repo["full_name"] != "acme/second");
    }
    let result = connection.run().await;
    assert_eq!(result["status"], "SUCCEEDED", "{result}");
    assert_eq!(
        documents(&server)
            .await
            .iter()
            .map(|doc| doc.source.as_str())
            .collect::<Vec<_>>(),
        ["work/acme/third/files", "work/acme/third/issues"]
    );
}

#[sqlx::test(migrations = false)]
async fn renamed_repositories_get_new_sources_without_migrating_manual_exclusions(pool: PgPool) {
    let files = tempfile::tempdir().unwrap();
    let (url, data) = serve(files.path()).await;
    {
        let mut data = data.lock();
        add_repository(&mut data, repository(1, "alice/old", &url, true));
        data.lists.insert(
            "repos/alice/old/issues:issues".to_owned(),
            vec![discussion(1, 1, "Renamed issue", false)],
        );
        data.lists
            .insert("repos/alice/old/issues/1/comments".to_owned(), vec![]);
    }
    let server = start_server(pool.clone()).await;
    let connection = Connection::create(&server, &url, json!({})).await;
    assert_eq!(connection.run().await["status"], "SUCCEEDED");
    let document = documents(&server).await.remove(0);
    let excluded = server
        .graphql(
            &connection.cookie,
            "mutation ($id: ID!) { excludeDocument(id: $id, wholeThread: true) { deleted } }",
            json!({"id": document.id}),
        )
        .await;
    assert_eq!(excluded["errors"], Value::Null, "{excluded}");
    assert_eq!(connection.run().await["status"], "SUCCEEDED");
    assert!(documents(&server).await.is_empty());
    {
        let mut data = data.lock();
        data.lists.insert("user/repos".to_owned(), vec![]);
        add_repository(&mut data, repository(1, "alice/new", &url, true));
        data.lists.insert(
            "repos/alice/new/issues:issues".to_owned(),
            vec![discussion(1, 1, "Renamed issue", false)],
        );
        data.lists
            .insert("repos/alice/new/issues/1/comments".to_owned(), vec![]);
    }
    let result = connection.run().await;
    assert_eq!(result["status"], "SUCCEEDED", "{result}");
    let stored = documents(&server).await;
    assert_eq!(
        stored
            .iter()
            .map(|doc| (doc.source.as_str(), doc.external_id.as_str()))
            .collect::<Vec<_>>(),
        [("work/alice/new/issues", "body:1")]
    );
    assert_ne!(stored[0].id, document.id);
}

#[sqlx::test(migrations = false)]
async fn team_review_requests_are_ignored_but_submitted_reviews_require_an_author(pool: PgPool) {
    let files = tempfile::tempdir().unwrap();
    let (url, data) = serve(files.path()).await;
    {
        let mut data = data.lock();
        add_repository(&mut data, repository(1, "acme/notes", &url, true));
        data.lists.insert(
            "repos/acme/notes/issues:pulls".to_owned(),
            vec![discussion(12, 2, "Review discussion", true)],
        );
        data.lists
            .insert("repos/acme/notes/issues/2/comments".to_owned(), vec![]);
        data.lists.insert(
            "repos/acme/notes/pulls/2/reviews".to_owned(),
            vec![
                json!({"id": 30, "body": "", "user": null, "team": {"id": 7, "name": "reviewers"},
                "state": "REQUEST_REVIEW", "html_url": "https://git.example/review/30",
                "submitted_at": CREATED, "updated_at": CREATED}),
                json!({"id": 31, "body": "Cobalt submitted review.", "user": {"login": "reviewer"},
                "state": "COMMENT", "html_url": "https://git.example/review/31",
                "submitted_at": CREATED, "updated_at": CREATED}),
            ],
        );
        data.lists.insert(
            "repos/acme/notes/pulls/2/reviews/31/comments".to_owned(),
            vec![],
        );
    }
    let server = start_server(pool).await;
    let connection = Connection::create(&server, &url, json!({})).await;
    let result = connection.run().await;
    assert_eq!(result["status"], "SUCCEEDED", "{result}");
    let stored = documents(&server).await;
    assert_eq!(
        stored
            .iter()
            .map(|document| document.external_id.as_str())
            .collect::<Vec<_>>(),
        ["body:12", "review:31"],
    );
    assert_eq!(stored[1].authors, ["reviewer"]);
    {
        let mut data = data.lock();
        data.lists
            .get_mut("repos/acme/notes/pulls/2/reviews")
            .unwrap()[1]["user"] = Value::Null;
    }
    let result = connection.run().await;
    assert_eq!(result["status"], "FAILED", "{result}");
    let retained = documents(&server).await;
    assert_eq!(
        retained
            .iter()
            .map(|document| document.id)
            .collect::<Vec<_>>(),
        stored
            .iter()
            .map(|document| document.id)
            .collect::<Vec<_>>(),
    );
    assert_eq!(retained[1].authors, ["reviewer"]);
}

#[sqlx::test(migrations = false)]
async fn recovery_reconciles_files_overwritten_by_a_failed_run_then_reverted(pool: PgPool) {
    let files = tempfile::tempdir().unwrap();
    let checkout = tempfile::tempdir().unwrap();
    git(checkout.path(), &["init", "-q", "-b", "main"]);
    let stable = "# Cobalt stable note\n\nCobalt original content.";
    std::fs::write(checkout.path().join("note.md"), stable).unwrap();
    commit(checkout.path());
    std::fs::create_dir_all(files.path().join("acme")).unwrap();
    let bare = files.path().join("acme/notes.git");
    git(
        checkout.path(),
        &["clone", "-q", "--bare", ".", bare.to_str().unwrap()],
    );
    git(&bare, &["update-server-info"]);
    let (url, data) = serve(files.path()).await;
    {
        let mut data = data.lock();
        add_repository(&mut data, repository(1, "acme/notes", &url, false));
    }
    let server = start_server(pool.clone()).await;
    let connection = Connection::create(&server, &url, json!({})).await;
    let result = connection.run().await;
    assert_eq!(result["status"], "SUCCEEDED", "{result}");
    let original = documents(&server).await.remove(0);
    let saved_state: Value = sqlx::query_scalar("select state from connections")
        .fetch_one(&pool)
        .await
        .unwrap();

    let transient = "# Cobalt transient note\n\nCobalt failed-run content.";
    std::fs::write(checkout.path().join("note.md"), transient).unwrap();
    std::fs::write(
        checkout.path().join("transient.md"),
        "Cobalt transient extra file.",
    )
    .unwrap();
    commit(checkout.path());
    publish(checkout.path(), &bare);
    data.lock().failure = Some(("repos/acme/notes/issues:issues".to_owned(), 1));
    let result = connection.run().await;
    assert_eq!(result["status"], "FAILED", "{result}");
    let partial = documents(&server).await;
    assert_eq!(
        partial
            .iter()
            .map(|document| (document.external_id.as_str(), document.text.as_str()))
            .collect::<Vec<_>>(),
        [
            ("note.md", transient),
            ("transient.md", "Cobalt transient extra file.")
        ],
    );
    assert_eq!(
        sqlx::query_scalar::<_, Value>("select state from connections")
            .fetch_one(&pool)
            .await
            .unwrap(),
        saved_state,
    );

    std::fs::write(checkout.path().join("note.md"), stable).unwrap();
    std::fs::remove_file(checkout.path().join("transient.md")).unwrap();
    commit(checkout.path());
    publish(checkout.path(), &bare);
    data.lock().failure = None;
    let result = connection.run().await;
    assert_eq!(result["status"], "SUCCEEDED", "{result}");
    let recovered = documents(&server).await;
    assert_eq!(
        recovered
            .iter()
            .map(|document| (document.external_id.as_str(), document.text.as_str()))
            .collect::<Vec<_>>(),
        [("note.md", stable)],
    );
    assert_eq!(recovered[0].id, original.id);
    let recovered_state: Value = sqlx::query_scalar("select state from connections")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_ne!(
        recovered_state["repositories"]["acme/notes"]["commit"],
        saved_state["repositories"]["acme/notes"]["commit"],
    );
    let result = connection.run().await;
    assert_eq!(result["status"], "SUCCEEDED", "{result}");
    assert_eq!(result["stats"]["put"], 0, "{result}");
    assert_eq!(documents(&server).await[0].text, stable);
}
