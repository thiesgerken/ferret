//! Gitea account repositories and open discussions, reconciled by one connection-root sweep.

use std::{collections::HashSet, time::Duration};

use chrono::{DateTime, Utc};
use ferret_api::{ContentType, DocumentPut, PutResponse, internal::RunStats};
use reqwest::{Url, header};
use schemars::JsonSchema;
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Map, json};

use super::{Outcome, PluginError, Run, git};

pub const NAME: &str = "gitea";
const PAGE_SIZE: usize = 50;

/// Indexes account-associated repositories and their open issues and pull requests.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "Gitea")]
pub struct GiteaConfig {
    /// The instance URL, including any subpath, e.g. `https://git.example.com`.
    url: String,
    /// A Gitea token with read access to the account, repositories and issues.
    #[schemars(extend("writeOnly" = true))]
    token: String,
    /// Repository globs matching `owner/repo`. Everything associated with the account when omitted.
    #[serde(default)]
    include_repos: Vec<String>,
    /// Repository globs to skip. Exclusions win over includes.
    #[serde(default)]
    exclude_repos: Vec<String>,
    /// Repository-relative file globs to index. All text files when omitted.
    #[serde(default)]
    include: Vec<String>,
    /// Repository-relative file globs to skip.
    #[serde(default)]
    exclude: Vec<String>,
    /// File globs rendered as Markdown; other selected files are plain text.
    #[serde(default = "git::markdown_by_default")]
    markdown: Vec<String>,
}

#[derive(Deserialize)]
struct Repository {
    id: u64,
    full_name: String,
    html_url: String,
    default_branch: String,
    empty: bool,
    has_issues: bool,
    has_pull_requests: bool,
}

#[derive(Deserialize)]
struct User {
    login: String,
}

#[derive(Deserialize)]
struct Discussion {
    id: u64,
    number: u64,
    title: String,
    body: String,
    user: User,
    html_url: String,
    state: String,
    pull_request: Option<PullRequest>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Deserialize)]
struct PullRequest {
    merged: bool,
}

#[derive(Deserialize)]
struct Comment {
    id: u64,
    body: String,
    user: User,
    html_url: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Deserialize)]
struct Review {
    id: u64,
    body: String,
    user: User,
    state: String,
    html_url: String,
    submitted_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Deserialize)]
struct ReviewComment {
    #[serde(flatten)]
    comment: Comment,
    path: String,
    position: u64,
    original_position: u64,
}

pub async fn run(run: &Run) -> Result<Outcome, PluginError> {
    let config: GiteaConfig = serde_json::from_value(run.config.clone())?;
    let gitea = Gitea::new(&config)?;
    let include = git::glob_set(&config.include_repos)?;
    let exclude = git::glob_set(&config.exclude_repos)?;
    git::glob_set(&config.include)?;
    git::glob_set(&config.exclude)?;
    git::glob_set(&config.markdown)?;

    let sync = run.api.start_sync(&run.source).await?.sync_id;
    let repositories = gitea
        .all::<Repository>("user/repos", |repo| repo.id)
        .await?;
    let mut states = Map::new();
    let mut stats = RunStats::default();
    let mut failures = Vec::new();
    for repo in repositories {
        validate_repository_name(&repo.full_name)?;
        if (!config.include_repos.is_empty() && !include.is_match(&repo.full_name))
            || exclude.is_match(&repo.full_name)
        {
            continue;
        }
        run.log.line(format!("indexing {}", repo.full_name));
        let source = format!("{}/{}", run.source, repo.full_name);
        if !repo.empty {
            let initial_state = json!({});
            let state = run
                .state
                .get("repositories")
                .and_then(|states| states.get(&repo.full_name))
                .unwrap_or(&initial_state);
            let result = async {
                if repo.default_branch.is_empty() {
                    return Err("a nonempty repository has no default branch".into());
                }
                let files = git::GitConfig {
                    remote: format!("{}{name}.git", gitea.base, name = repo.full_name),
                    token: Some(config.token.clone()),
                    branch: Some(repo.default_branch.clone()),
                    include: config.include.clone(),
                    exclude: config.exclude.clone(),
                    markdown: config.markdown.clone(),
                    url_template: Some(format!(
                        "{}/src/commit/{{commit}}/{{path}}",
                        repo.html_url.trim_end_matches('/')
                    )),
                };
                git::sync_files(run, &format!("{source}/files"), state, &files, sync).await
            }
            .await;
            match result {
                Ok(outcome) => {
                    stats.put += outcome.stats.put;
                    stats.unchanged += outcome.stats.unchanged;
                    states.insert(repo.full_name.clone(), outcome.state);
                }
                Err(error) => failure(run, &mut failures, &repo.full_name, "files", error),
            }
        }
        for (enabled, kind) in [
            (repo.has_issues, "issues"),
            (repo.has_pull_requests, "pulls"),
        ] {
            if enabled
                && let Err(error) = gitea
                    .discussions(run, &repo, &source, kind, &mut stats)
                    .await
            {
                failure(run, &mut failures, &repo.full_name, kind, error);
            }
        }
    }
    if !failures.is_empty() {
        return Err(format!(
            "Gitea run failed; no sweep committed: {}",
            failures.join("; ")
        )
        .into());
    }
    stats.deleted = run.api.commit_sync(sync).await?.deleted;
    run.log.line(format!(
        "{} put, {} unchanged, {} deleted",
        stats.put, stats.unchanged, stats.deleted
    ));
    Ok(Outcome {
        state: json!({"repositories": states}),
        stats,
    })
}

fn failure(run: &Run, failures: &mut Vec<String>, repo: &str, kind: &str, error: PluginError) {
    let message = format!("{repo}/{kind}: {error}");
    run.log.line(&message);
    failures.push(message);
}

fn validate_repository_name(name: &str) -> Result<(), PluginError> {
    let Some((owner, repo)) = name.split_once('/') else {
        return Err("Gitea returned an invalid owner/repository name".into());
    };
    if [owner, repo].iter().any(|segment| {
        segment.is_empty()
            || *segment == "."
            || *segment == ".."
            || !segment
                .chars()
                .all(|c| c.is_alphanumeric() || matches!(c, '.' | '_' | '-'))
    }) {
        return Err("Gitea returned an invalid owner/repository name".into());
    }
    Ok(())
}

struct Gitea {
    http: reqwest::Client,
    base: Url,
    authorization: header::HeaderValue,
}

impl Gitea {
    fn new(config: &GiteaConfig) -> Result<Self, PluginError> {
        let mut base = Url::parse(&config.url)?;
        if !matches!(base.scheme(), "http" | "https")
            || base.host_str().is_none()
            || !base.username().is_empty()
            || base.password().is_some()
            || base.query().is_some()
            || base.fragment().is_some()
        {
            return Err(
                "Gitea URL must be an HTTP(S) instance URL without credentials, query or fragment"
                    .into(),
            );
        }
        base.set_path(&format!("{}/", base.path().trim_end_matches('/')));
        if config.token.trim().is_empty() {
            return Err("Gitea token must not be empty".into());
        }
        let mut authorization = header::HeaderValue::from_str(&format!("token {}", config.token))?;
        authorization.set_sensitive(true);
        Ok(Self {
            http: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(10))
                .read_timeout(Duration::from_secs(120))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            base,
            authorization,
        })
    }

    async fn get<T: DeserializeOwned>(
        &self,
        path: &str,
        page: usize,
        limit: usize,
    ) -> Result<(T, header::HeaderMap), PluginError> {
        let response = self
            .http
            .get(format!("{}api/v1/{path}", self.base))
            .header(header::AUTHORIZATION, &self.authorization)
            .query(&[("page", page), ("limit", limit)])
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(format!("Gitea answered {path} with {}", response.status()).into());
        }
        let headers = response.headers().clone();
        let body = response
            .json()
            .await
            .map_err(|_| format!("Gitea answered {path} with an invalid JSON response"))?;
        Ok((body, headers))
    }

    async fn all<T: DeserializeOwned>(
        &self,
        path: &str,
        id: fn(&T) -> u64,
    ) -> Result<Vec<T>, PluginError> {
        let mut results = Vec::new();
        let mut ids = HashSet::new();
        let mut total = None;
        for page in 1.. {
            let (rows, headers): (Vec<T>, _) = self.get(path, page, PAGE_SIZE).await?;
            let count =
                total_count(&headers)?.ok_or("Gitea paginated listing has no X-Total-Count")?;
            if *total.get_or_insert(count) != count {
                return Err(format!("Gitea listing changed during pagination: {path}").into());
            }
            for row in rows {
                if !ids.insert(id(&row)) {
                    return Err(
                        format!("Gitea repeated an identity during pagination: {path}").into(),
                    );
                }
                results.push(row);
            }
            let next = headers
                .get(header::LINK)
                .map(|value| value.to_str())
                .transpose()?
                .is_some_and(|links| links.split(',').any(|link| link.contains("rel=\"next\"")));
            if !next {
                if count != results.len() {
                    return Err(format!("Gitea returned an incomplete listing: {path}").into());
                }
                return Ok(results);
            }
        }
        unreachable!()
    }

    async fn reviews(&self, path: &str) -> Result<Vec<Review>, PluginError> {
        // Gitea counts other users' hidden pending reviews but omits them from pages, and this
        // endpoint has no Link header. One slot per page makes the total an unambiguous page count.
        let mut results = Vec::new();
        let mut ids = HashSet::new();
        let mut total = None;
        for page in 1.. {
            let (reviews, headers): (Vec<Review>, _) = self.get(path, page, 1).await?;
            let count =
                total_count(&headers)?.ok_or("Gitea review listing has no X-Total-Count")?;
            if *total.get_or_insert(count) != count {
                return Err(
                    format!("Gitea review listing changed during pagination: {path}").into(),
                );
            }
            for review in reviews {
                if !ids.insert(review.id) {
                    return Err(format!("Gitea repeated a review during pagination: {path}").into());
                }
                results.push(review);
            }
            if page >= count {
                return Ok(results);
            }
        }
        unreachable!()
    }

    async fn discussions(
        &self,
        run: &Run,
        repo: &Repository,
        source: &str,
        kind: &str,
        stats: &mut RunStats,
    ) -> Result<(), PluginError> {
        let path = format!("repos/{}/issues?state=open&type={kind}", repo.full_name);
        let discussions = self.all::<Discussion>(&path, |issue| issue.id).await?;
        let source = format!("{source}/{kind}");
        let mut failures = Vec::new();
        for discussion in discussions {
            if discussion.state != "open"
                || discussion
                    .pull_request
                    .as_ref()
                    .is_some_and(|pull| pull.merged)
            {
                continue;
            }
            let result = self
                .discussion(run, repo, &source, &discussion, kind, stats)
                .await;
            if let Err(error) = result {
                failures.push(format!("#{}: {error}", discussion.number));
            }
        }
        if !failures.is_empty() {
            return Err(failures.join("; ").into());
        }
        Ok(())
    }

    async fn discussion(
        &self,
        run: &Run,
        repo: &Repository,
        source: &str,
        discussion: &Discussion,
        kind: &str,
        stats: &mut RunStats,
    ) -> Result<(), PluginError> {
        let thread = discussion.number.to_string();
        let title = format!(
            "{} #{}: {}",
            repo.full_name, discussion.number, discussion.title
        );
        let mut meta = Map::new();
        meta.insert("repository".to_owned(), json!(repo.full_name));
        meta.insert("number".to_owned(), json!(discussion.number));
        meta.insert("kind".to_owned(), json!(kind));
        let body = DocumentPut {
            source: source.to_owned(),
            external_id: format!("body:{}", discussion.id),
            content_type: ContentType::Markdown,
            title: Some(title),
            // A title-only issue/PR still needs a text chunk to be searchable.
            text: if discussion.body.trim().is_empty() {
                discussion.title.clone()
            } else {
                discussion.body.clone()
            },
            url: Some(discussion.html_url.clone()),
            thread: Some(thread),
            authors: vec![discussion.user.login.clone()],
            created_at: discussion.created_at,
            modified_at: discussion.updated_at,
            meta,
        };
        put(run, &body, stats).await?;
        let path = format!(
            "repos/{}/issues/{}/comments",
            repo.full_name, discussion.number
        );
        let comments = self.all::<Comment>(&path, |comment| comment.id).await?;
        for comment in comments {
            put(run, &comment_document(&body, comment, "comment"), stats).await?;
        }
        if kind == "pulls" {
            let path = format!(
                "repos/{}/pulls/{}/reviews",
                repo.full_name, discussion.number
            );
            for review in self.reviews(&path).await? {
                if matches!(review.state.as_str(), "PENDING" | "REQUEST_REVIEW") {
                    continue;
                }
                if !review.body.trim().is_empty() {
                    let document = comment_document(
                        &body,
                        Comment {
                            id: review.id,
                            body: review.body,
                            user: review.user,
                            html_url: review.html_url,
                            created_at: review.submitted_at,
                            updated_at: review.updated_at,
                        },
                        "review",
                    );
                    put(run, &document, stats).await?;
                }
                // Gitea returns the entire review-comment list without pagination.
                let (comments, _) = self
                    .get::<Vec<ReviewComment>>(
                        &format!("{path}/{}/comments", review.id),
                        1,
                        PAGE_SIZE,
                    )
                    .await?;
                for comment in comments {
                    let mut document = comment_document(&body, comment.comment, "review-comment");
                    document.meta.insert("path".to_owned(), json!(comment.path));
                    document
                        .meta
                        .insert("position".to_owned(), json!(comment.position));
                    document.meta.insert(
                        "original_position".to_owned(),
                        json!(comment.original_position),
                    );
                    put(run, &document, stats).await?;
                }
            }
        }
        Ok(())
    }
}

fn total_count(headers: &header::HeaderMap) -> Result<Option<usize>, PluginError> {
    headers
        .get("x-total-count")
        .map(|value| Ok(value.to_str()?.parse()?))
        .transpose()
}

fn comment_document(body: &DocumentPut, comment: Comment, kind: &str) -> DocumentPut {
    DocumentPut {
        source: body.source.clone(),
        external_id: format!("{kind}:{}", comment.id),
        content_type: ContentType::Markdown,
        title: body.title.clone(),
        text: comment.body,
        url: Some(comment.html_url),
        thread: body.thread.clone(),
        authors: vec![comment.user.login],
        created_at: comment.created_at,
        modified_at: comment.updated_at,
        meta: body.meta.clone(),
    }
}

async fn put(run: &Run, document: &DocumentPut, stats: &mut RunStats) -> Result<(), PluginError> {
    if document.text.trim().is_empty() {
        return Ok(());
    }
    match run.api.put_document(document).await? {
        PutResponse::Created { .. } | PutResponse::Updated { .. } => stats.put += 1,
        PutResponse::Unchanged { .. } => stats.unchanged += 1,
        PutResponse::Excluded => {}
    }
    Ok(())
}
