//! git: one document per file of a repository, a full sync on every run.

use std::{collections::HashMap, path::Path};

use base64::{Engine, engine::general_purpose::STANDARD};
use chrono::{DateTime, Utc};
use ferret_api::{ContentType, DocumentPut, PutOutcome, internal::RunStats};
use globset::{Glob, GlobSet, GlobSetBuilder};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;
use tokio::process::Command;

use super::{Outcome, PluginError, Run};

pub const NAME: &str = "git";

/// Indexes the files of a git repository.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "git")]
pub struct GitConfig {
    /// The repository to clone, e.g. `https://github.com/owner/notes.git`.
    remote: String,
    /// A token for a private repository over HTTPS, e.g. a GitHub fine-grained token that may read
    /// the repository's contents.
    #[serde(default)]
    #[schemars(extend("writeOnly" = true))]
    token: Option<String>,
    /// The branch to index. The repository's default branch when left out.
    #[serde(default)]
    branch: Option<String>,
    /// Globs of files to index, e.g. `**/*.md`. Everything when left out.
    #[serde(default)]
    include: Vec<String>,
    /// Globs of files to skip, e.g. `.obsidian/**`.
    #[serde(default)]
    exclude: Vec<String>,
    /// Globs of Markdown files. Everything else is plain text.
    #[serde(default = "markdown_by_default")]
    markdown: Vec<String>,
    /// Deep link to a file, with `{path}` and `{commit}`, e.g.
    /// `https://github.com/owner/notes/blob/{commit}/{path}`.
    #[serde(default)]
    url_template: Option<String>,
}

fn markdown_by_default() -> Vec<String> {
    vec!["**/*.md".to_owned()]
}

/// The latest and the first commit that touched a file.
struct FileHistory {
    author: String,
    modified_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
}

pub async fn run(run: &Run) -> Result<Outcome, PluginError> {
    let config: GitConfig = serde_json::from_value(run.config.clone())?;
    let include = glob_set(&config.include)?;
    let exclude = glob_set(&config.exclude)?;
    let markdown = glob_set(&config.markdown)?;

    let checkout = tempfile::tempdir()?;
    let dir = checkout.path();
    let mut clone = git_command(Path::new("."));
    clone.args(["clone", "--quiet", "--single-branch"]);
    if let Some(branch) = &config.branch {
        clone.args(["--branch", branch]);
    }
    clone.args(["--".as_ref(), config.remote.as_ref(), dir.as_os_str()]);
    if let Some(token) = &config.token {
        // Through the environment rather than `-c`, which would show the token in process
        // listings. The clone itself doesn't keep it.
        let credentials = STANDARD.encode(format!("x-access-token:{token}"));
        clone
            .env("GIT_CONFIG_COUNT", "1")
            .env("GIT_CONFIG_KEY_0", "http.extraHeader")
            .env(
                "GIT_CONFIG_VALUE_0",
                format!("Authorization: Basic {credentials}"),
            );
    }
    output(&mut clone, "git clone").await?;
    let commit = git(dir, &["rev-parse", "HEAD"]).await?.trim().to_owned();
    run.log
        .line(format!("cloned {} at {commit}", config.remote));

    let files: Vec<String> = git(dir, &["-c", "core.quotePath=false", "ls-files", "-z"])
        .await?
        .split('\0')
        .filter(|path| !path.is_empty())
        .filter(|path| {
            (config.include.is_empty() || include.is_match(path)) && !exclude.is_match(path)
        })
        .map(str::to_owned)
        .collect();
    let history = history(dir).await?;

    let sync = run.api.start_sync(&run.source).await?.sync_id;
    let mut stats = RunStats::default();
    let mut indexed = 0;
    for path in &files {
        let Some(file_history) = history.get(path) else {
            continue;
        };
        let bytes = tokio::fs::read(dir.join(path)).await?;
        let Some(text) = String::from_utf8(bytes)
            .ok()
            .filter(|text| !text.contains('\0'))
        else {
            run.log.line(format!("skipped {path}: not text"));
            continue;
        };
        let content_type = if markdown.is_match(path) {
            ContentType::Markdown
        } else {
            ContentType::Plain
        };
        let document = DocumentPut {
            source: run.source.clone(),
            external_id: path.clone(),
            content_type,
            title: Some(title(path, &text, content_type)),
            url: config.url_template.as_ref().map(|template| {
                template
                    .replace("{path}", path)
                    .replace("{commit}", &commit)
            }),
            text,
            thread: None,
            authors: vec![file_history.author.clone()],
            created_at: file_history.created_at,
            modified_at: file_history.modified_at,
            meta: Default::default(),
        };
        match run.api.put_document(&document).await?.outcome {
            PutOutcome::Unchanged => stats.unchanged += 1,
            PutOutcome::Created | PutOutcome::Updated => stats.put += 1,
        }
        indexed += 1;
    }
    stats.deleted = run.api.commit_sync(sync).await?.deleted;
    run.log.line(format!(
        "{indexed} files: {} put, {} unchanged, {} deleted",
        stats.put, stats.unchanged, stats.deleted
    ));
    Ok(Outcome {
        state: json!({ "commit": commit }),
        stats,
    })
}

/// Author and dates of every file, from one pass over the log: newest commits come first, so a
/// file's first appearance is its latest commit and its last appearance its first.
async fn history(dir: &Path) -> Result<HashMap<String, FileHistory>, PluginError> {
    let log = git(
        dir,
        &[
            "-c",
            "core.quotePath=false",
            "log",
            "--format=%x1e%an%x1f%aI",
            "--name-only",
        ],
    )
    .await?;
    let mut files: HashMap<String, FileHistory> = HashMap::new();
    for commit in log.split('\x1e').filter(|commit| !commit.is_empty()) {
        let mut lines = commit.lines();
        let header = lines.next().unwrap_or_default();
        let (author, date) = header
            .split_once('\x1f')
            .ok_or("unexpected git log output")?;
        let date = DateTime::parse_from_rfc3339(date)?.with_timezone(&Utc);
        for path in lines.filter(|line| !line.is_empty()) {
            files
                .entry(path.to_owned())
                .and_modify(|file| file.created_at = date)
                .or_insert_with(|| FileHistory {
                    author: author.to_owned(),
                    modified_at: date,
                    created_at: date,
                });
        }
    }
    Ok(files)
}

/// The first Markdown heading, or the file name.
fn title(path: &str, text: &str, content_type: ContentType) -> String {
    let heading = (content_type == ContentType::Markdown)
        .then(|| text.lines().find_map(|line| line.strip_prefix("# ")))
        .flatten()
        .map(str::trim)
        .filter(|heading| !heading.is_empty());
    heading
        .unwrap_or_else(|| path.rsplit('/').next().unwrap_or(path))
        .to_owned()
}

fn glob_set(globs: &[String]) -> Result<GlobSet, PluginError> {
    let mut set = GlobSetBuilder::new();
    for glob in globs {
        set.add(Glob::new(glob)?);
    }
    Ok(set.build()?)
}

/// Runs git in `dir` and returns its stdout.
async fn git(dir: &Path, args: &[&str]) -> Result<String, PluginError> {
    output(
        git_command(dir).args(args),
        &format!("git {}", args.join(" ")),
    )
    .await
}

/// Git in `dir` that never prompts: a repository that needs other credentials fails instead of
/// hanging the run.
fn git_command(dir: &Path) -> Command {
    let mut command = Command::new("git");
    command.current_dir(dir).env("GIT_TERMINAL_PROMPT", "0");
    command
}

async fn output(command: &mut Command, name: &str) -> Result<String, PluginError> {
    let output = command.output().await?;
    if !output.status.success() {
        return Err(format!(
            "{name} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }
    Ok(String::from_utf8(output.stdout)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_come_from_the_first_heading_or_the_file_name() {
        assert_eq!(
            title("notes/a.md", "intro\n# The Title\n", ContentType::Markdown),
            "The Title"
        );
        assert_eq!(
            title("notes/a.md", "no heading", ContentType::Markdown),
            "a.md"
        );
        assert_eq!(
            title("src/main.rs", "# not a heading in Rust", ContentType::Plain),
            "main.rs"
        );
    }
}
