//! Vikunja REST v2: one document per task, including its complete comment discussion.

use std::{collections::HashSet, fmt::Write, time::Duration};

use chrono::{DateTime, Datelike, Utc};
use ferret_api::{ContentType, DocumentPut, PutResponse, internal::RunStats};
use mail_parser::decoders::html::html_to_text;
use schemars::JsonSchema;
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::json;

use super::{Outcome, PluginError, Run};

pub const NAME: &str = "vikunja";
const PAGE_SIZE: usize = 100;

/// Indexes all tasks visible to a Vikunja user, including archived projects and completed tasks.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "Vikunja")]
pub struct VikunjaConfig {
    /// The instance root as the worker reaches it, without `/api/v2`. Must reach Vikunja directly,
    /// not a login proxy. Checked against Vikunja 2.7.0.
    url: String,
    /// The frontend address people open. Uses Vikunja's configured frontend URL when omitted.
    #[serde(default)]
    public_url: Option<String>,
    /// A token from Settings > API Tokens with projects:read_all, tasks:read_all and
    /// tasks_comments:read_all permissions. The user's project access limits what is indexed.
    #[schemars(extend("writeOnly" = true))]
    token: String,
}

#[derive(Deserialize)]
struct Info {
    frontend_url: String,
}

#[derive(Deserialize)]
struct Page<T> {
    items: Option<Vec<T>>,
    total: usize,
    page: usize,
    per_page: usize,
    total_pages: usize,
}

#[derive(Deserialize)]
struct Project {
    id: i64,
    title: String,
    is_archived: bool,
    updated: DateTime<Utc>,
}

#[derive(Deserialize)]
struct User {
    username: String,
}

#[derive(Deserialize)]
struct Label {
    title: String,
    updated: DateTime<Utc>,
}

#[derive(Deserialize)]
struct Attachment {
    id: i64,
    file: File,
}

#[derive(Deserialize)]
struct File {
    name: String,
}

#[derive(Deserialize)]
struct Task {
    id: i64,
    project_id: i64,
    title: String,
    description: String,
    identifier: String,
    done: bool,
    done_at: DateTime<Utc>,
    due_date: DateTime<Utc>,
    start_date: DateTime<Utc>,
    end_date: DateTime<Utc>,
    priority: i64,
    percent_done: f64,
    labels: Option<Vec<Label>>,
    assignees: Option<Vec<User>>,
    attachments: Option<Vec<Attachment>>,
    created_by: User,
    created: DateTime<Utc>,
    updated: DateTime<Utc>,
}

#[derive(Deserialize)]
struct Comment {
    id: i64,
    comment: String,
    author: User,
    created: DateTime<Utc>,
    updated: DateTime<Utc>,
}

pub async fn run(run: &Run) -> Result<Outcome, PluginError> {
    let config: VikunjaConfig = serde_json::from_value(run.config.clone())?;
    let vikunja = Vikunja::new(&config)?;
    let info: Info = vikunja.get("/info", &[]).await?;
    let public_url = config.public_url.as_deref().unwrap_or(&info.frontend_url);
    let links = instance_url(public_url)?;
    // /tasks discovers only non-archived projects. Listing projects explicitly preserves history.
    let projects: Vec<Project> = vikunja
        .all(
            "/projects",
            &[("is_archived", "true")],
            |project: &Project| project.id,
        )
        .await?;
    run.log
        .line(format!("{} projects in Vikunja", projects.len()));

    let sync = run.api.start_sync(&run.source).await?.sync_id;
    let mut stats = RunStats::default();
    let mut task_ids = HashSet::new();
    for project in projects {
        let tasks: Vec<Task> = vikunja
            .all(
                &format!("/projects/{}/tasks", project.id),
                &[("sort_by", "id"), ("order_by", "asc")],
                |task: &Task| task.id,
            )
            .await?;
        for task in tasks {
            if task.project_id != project.id || !task_ids.insert(task.id) {
                return Err("Vikunja tasks moved while listing; the next run retries".into());
            }
            // Expanded comments are truncated; only the dedicated endpoint exposes every page.
            let mut comments: Vec<Comment> = vikunja
                .all(
                    &format!("/tasks/{}/comments", task.id),
                    &[("order_by", "asc")],
                    |comment: &Comment| comment.id,
                )
                .await?;
            comments.sort_unstable_by_key(|comment| (comment.created, comment.id));
            let document = to_document(task, &project, comments, &links, &run.source);
            match run.api.put_document(&document).await? {
                PutResponse::Created { .. } | PutResponse::Updated { .. } => stats.put += 1,
                PutResponse::Unchanged { .. } => stats.unchanged += 1,
                PutResponse::Excluded => {}
            }
        }
    }
    stats.deleted = run.api.commit_sync(sync).await?.deleted;
    run.log.line(format!(
        "{} put, {} unchanged, {} deleted",
        stats.put, stats.unchanged, stats.deleted
    ));
    Ok(Outcome {
        state: json!({}),
        stats,
    })
}

fn to_document(
    task: Task,
    project: &Project,
    comments: Vec<Comment>,
    links: &str,
    source: &str,
) -> DocumentPut {
    let mut modified_at = task.updated.max(project.updated);
    let tags: Vec<String> = task
        .labels
        .into_iter()
        .flatten()
        .map(|label| {
            modified_at = modified_at.max(label.updated);
            label.title
        })
        .collect();
    let assignees: Vec<String> = task
        .assignees
        .into_iter()
        .flatten()
        .map(|user| user.username)
        .collect();
    let attachments: Vec<_> = task
        .attachments
        .into_iter()
        .flatten()
        .map(|attachment| json!({"id": attachment.id, "name": attachment.file.name}))
        .collect();
    let mut text = String::new();
    writeln!(text, "Project: {}", project.title).unwrap();
    writeln!(
        text,
        "Status: {}",
        if task.done { "completed" } else { "open" }
    )
    .unwrap();
    if !task.identifier.is_empty() {
        writeln!(text, "Task: {}", task.identifier).unwrap();
    }
    if !tags.is_empty() {
        writeln!(text, "Labels: {}", tags.join(", ")).unwrap();
    }
    if !assignees.is_empty() {
        writeln!(text, "Assignees: {}", assignees.join(", ")).unwrap();
    }
    writeln!(text, "Priority: {}", task.priority).unwrap();
    if let Some(due) = optional_date(task.due_date) {
        writeln!(text, "Due: {}", due.to_rfc3339()).unwrap();
    }
    let description = html_to_text(&task.description);
    if !description.trim().is_empty() {
        write!(text, "\n{}\n", description.trim()).unwrap();
    }
    let mut authors = vec![task.created_by.username];
    if !comments.is_empty() {
        text.push_str("\nComments:\n");
    }
    for comment in comments {
        modified_at = modified_at.max(comment.updated);
        write!(
            text,
            "\n{} ({})\n{}\n",
            comment.author.username,
            comment.created.to_rfc3339(),
            html_to_text(&comment.comment).trim()
        )
        .unwrap();
        if !authors.contains(&comment.author.username) {
            authors.push(comment.author.username);
        }
    }
    let serde_json::Value::Object(meta) = json!({
            "project_id": project.id,
            "project": project.title,
            "project_archived": project.is_archived,
            "identifier": task.identifier,
            "done": task.done,
            "done_at": optional_date(task.done_at),
            "due_date": optional_date(task.due_date),
            "start_date": optional_date(task.start_date),
            "end_date": optional_date(task.end_date),
            "priority": task.priority,
            "percent_done": task.percent_done,
            "tags": tags,
            "assignees": assignees,
            "attachments": attachments,
    }) else {
        unreachable!("metadata is an object");
    };
    DocumentPut {
        source: source.to_owned(),
        external_id: task.id.to_string(),
        content_type: ContentType::Plain,
        title: Some(task.title),
        text,
        url: Some(format!("{links}/tasks/{}", task.id)),
        thread: None,
        authors,
        created_at: task.created,
        modified_at,
        meta,
    }
}

// Vikunja serializes Go's zero time for dates the user has not set.
fn optional_date(date: DateTime<Utc>) -> Option<DateTime<Utc>> {
    (date.year() != 1).then_some(date)
}

fn instance_url(value: &str) -> Result<String, PluginError> {
    let url =
        reqwest::Url::parse(value).map_err(|_| "Vikunja needs an absolute HTTP(S) instance URL")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(
            "Vikunja needs an HTTP(S) instance URL without credentials, query or fragment".into(),
        );
    }
    Ok(url.as_str().trim_end_matches('/').to_owned())
}

struct Vikunja {
    http: reqwest::Client,
    base_url: String,
    token: String,
}

impl Vikunja {
    fn new(config: &VikunjaConfig) -> Result<Self, PluginError> {
        Ok(Self {
            http: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(10))
                .read_timeout(Duration::from_secs(120))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            base_url: instance_url(&config.url)?,
            token: config.token.clone(),
        })
    }

    async fn get<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<T, PluginError> {
        let response = self
            .http
            .get(format!("{}/api/v2{path}", self.base_url))
            .bearer_auth(&self.token)
            .header("Accept", "application/json")
            .query(query)
            .send()
            .await
            .map_err(reqwest::Error::without_url)?;
        let status = response.status();
        if status.is_redirection() {
            return Err(format!("Vikunja redirected {path} ({status}); use an address that reaches it directly, not a login proxy").into());
        }
        if !status.is_success() {
            return Err(format!("Vikunja answered {path} with {status}; REST API v2 and read permissions are required").into());
        }
        // Neither response bodies nor decoder errors belong in logs: an origin may echo secrets.
        response
            .json()
            .await
            .map_err(|_| format!("Vikunja answered {path} with an invalid REST v2 response").into())
    }

    async fn all<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, &str)],
        id: fn(&T) -> i64,
    ) -> Result<Vec<T>, PluginError> {
        let mut results = Vec::new();
        let mut ids = HashSet::new();
        let mut pagination = None;
        for number in 1.. {
            let number_string = number.to_string();
            let size = PAGE_SIZE.to_string();
            let mut query = query.to_vec();
            query.extend([
                ("page", number_string.as_str()),
                ("per_page", size.as_str()),
            ]);
            let page: Page<T> = self.get(path, &query).await?;
            let shape = (page.total, page.per_page, page.total_pages);
            if page.page != number
                || page.per_page == 0
                || page.total_pages != page.total.div_ceil(page.per_page)
                || *pagination.get_or_insert(shape) != shape
            {
                return Err(incomplete_listing(path));
            }
            let mut items = page.items.unwrap_or_default();
            // /projects appends every saved filter to every page without counting them in total.
            // They are virtual projects, not additional task owners.
            if path == "/projects" {
                items.retain(|item| id(item) > 0);
            }
            if items.len() > page.per_page || items.iter().any(|item| !ids.insert(id(item))) {
                return Err(incomplete_listing(path));
            }
            results.extend(items);
            if number >= page.total_pages {
                if results.len() != page.total {
                    return Err(incomplete_listing(path));
                }
                return Ok(results);
            }
            if results.len() != number * page.per_page {
                return Err(incomplete_listing(path));
            }
        }
        unreachable!("pagination returns or fails")
    }
}

fn incomplete_listing(path: &str) -> PluginError {
    format!("Vikunja changed or returned an incomplete listing at {path}; the next run retries")
        .into()
}
