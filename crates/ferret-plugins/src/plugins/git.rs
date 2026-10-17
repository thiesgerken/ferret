//! git: one document per file of a repository. The first run is a full sync, later runs index
//! the diff since the last run's commit.

use std::collections::HashMap;

use ferret_api::{ContentType, DocumentPut, PutOutcome, internal::RunStats};
use globset::{Glob, GlobSet, GlobSetBuilder};
use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    Outcome, PluginError, Run,
    repo::{Change, Checkout, FileHistory},
};

pub const NAME: &str = "git";

/// Characters a path segment can't carry into a URL as they are.
const URL_SEGMENT: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'&')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'`')
    .add(b'{')
    .add(b'}');

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

impl GitConfig {
    /// The settings that decide which files become which documents. When they change between
    /// runs, the next run is a full sync, since files the diff doesn't mention may now be in or
    /// out. Holds no secrets: it is stored in the connection's state.
    fn indexing(&self) -> Value {
        json!({
            "remote": self.remote,
            "branch": self.branch,
            "include": self.include,
            "exclude": self.exclude,
            "markdown": self.markdown,
            "url_template": self.url_template,
        })
    }
}

fn markdown_by_default() -> Vec<String> {
    vec!["**/*.md".to_owned()]
}

pub async fn run(run: &Run) -> Result<Outcome, PluginError> {
    let config: GitConfig = serde_json::from_value(run.config.clone())?;
    let checkout = Checkout::clone(
        &config.remote,
        config.branch.as_deref(),
        config.token.as_deref(),
        &run.options.git_protocols,
    )
    .await?;
    run.log
        .line(format!("cloned {} at {}", config.remote, checkout.head));
    let indexer = Indexer {
        run,
        config: &config,
        include: glob_set(&config.include)?,
        exclude: glob_set(&config.exclude)?,
        markdown: glob_set(&config.markdown)?,
        modes: checkout.modes().await?,
        history: checkout.history().await?,
        checkout: &checkout,
    };

    let indexing = config.indexing();
    let stats = match changes(run, &checkout, &indexing).await? {
        Ok(changes) => indexer.apply(changes).await?,
        Err(reason) => {
            run.log.line(format!("full sync: {reason}"));
            indexer.full_sync().await?
        }
    };
    run.log.line(format!(
        "{} put, {} unchanged, {} deleted",
        stats.put, stats.unchanged, stats.deleted
    ));
    Ok(Outcome {
        state: json!({ "commit": checkout.head, "indexing": indexing }),
        stats,
    })
}

/// The changes since the commit the last run indexed, or why this run has to be a full sync.
async fn changes(
    run: &Run,
    checkout: &Checkout,
    indexing: &Value,
) -> Result<Result<Vec<Change>, String>, PluginError> {
    let state = &run.state;
    let Some(cursor) = state.get("commit").and_then(Value::as_str) else {
        return Ok(Err("no earlier run".to_owned()));
    };
    match state.get("indexing") {
        None => return Ok(Err("the last run stored no indexing settings".to_owned())),
        Some(stored) if stored != indexing => {
            return Ok(Err("which files to index or how changed".to_owned()));
        }
        Some(_) => {}
    }
    let changes = checkout.changes_since(cursor).await?;
    if changes.is_ok() {
        run.log.line(format!("indexing the changes since {cursor}"));
    }
    Ok(changes)
}

/// What a run needs to turn files of the checkout into documents.
struct Indexer<'a> {
    run: &'a Run,
    config: &'a GitConfig,
    include: GlobSet,
    exclude: GlobSet,
    markdown: GlobSet,
    checkout: &'a Checkout,
    modes: HashMap<String, String>,
    history: HashMap<String, FileHistory>,
}

impl Indexer<'_> {
    /// Puts every file inside a sync, whose commit deletes the documents of files that are gone.
    async fn full_sync(&self) -> Result<RunStats, PluginError> {
        let run = self.run;
        let sync = run.api.start_sync(&run.source).await?.sync_id;
        let mut stats = RunStats::default();
        let mut paths: Vec<_> = self.modes.keys().map(String::as_str).collect();
        paths.sort_unstable();
        for path in paths {
            if self.selected(path) {
                self.put(path, &mut stats).await?;
            }
        }
        stats.deleted = run.api.commit_sync(sync).await?.deleted;
        Ok(stats)
    }

    /// Puts the changed files and deletes the documents of deleted ones.
    async fn apply(&self, changes: Vec<Change>) -> Result<RunStats, PluginError> {
        let mut stats = RunStats::default();
        for change in changes {
            match change {
                Change::Changed(path) if self.selected(&path) => {
                    self.put(&path, &mut stats).await?;
                }
                Change::Deleted(path) if self.selected(&path) => {
                    self.delete(&path, &mut stats).await?;
                }
                _ => {}
            }
        }
        Ok(stats)
    }

    /// Whether the config's globs select `path`.
    fn selected(&self, path: &str) -> bool {
        (self.config.include.is_empty() || self.include.is_match(path))
            && !self.exclude.is_match(path)
    }

    /// Puts the file at `path`, or deletes its document if the file can't be indexed (any more):
    /// a symlink, too big, not text.
    async fn put(&self, path: &str, stats: &mut RunStats) -> Result<(), PluginError> {
        match self.document(path).await? {
            Some(document) => match self.run.api.put_document(&document).await?.outcome {
                PutOutcome::Unchanged => stats.unchanged += 1,
                PutOutcome::Created | PutOutcome::Updated => stats.put += 1,
            },
            None => self.delete(path, stats).await?,
        }
        Ok(())
    }

    async fn delete(&self, path: &str, stats: &mut RunStats) -> Result<(), PluginError> {
        match self.run.api.delete_document(&self.run.source, path).await {
            Ok(()) => stats.deleted += 1,
            // Never indexed, e.g. a file that was always too big.
            Err(error) if error.status() == Some(reqwest::StatusCode::NOT_FOUND) => {}
            Err(error) => return Err(error.into()),
        }
        Ok(())
    }

    /// The document for the file at `path`, `None` (and a log line) if it can't be indexed.
    async fn document(&self, path: &str) -> Result<Option<DocumentPut>, PluginError> {
        let run = self.run;
        let mode = self
            .modes
            .get(path)
            .ok_or_else(|| format!("{path} is in the diff but not tracked"))?;
        let text = match self.checkout.text(path, mode).await? {
            Ok(text) => text,
            Err(reason) => {
                run.log.line(format!("skipped {path}: {reason}"));
                return Ok(None);
            }
        };
        let file_history = self
            .history
            .get(path)
            .ok_or_else(|| format!("{path} is tracked but no commit in the log touched it"))?;
        let content_type = if self.markdown.is_match(path) {
            ContentType::Markdown
        } else {
            ContentType::Plain
        };
        Ok(Some(DocumentPut {
            source: run.source.clone(),
            external_id: path.to_owned(),
            content_type,
            title: Some(title(path, &text, content_type)),
            url: self
                .config
                .url_template
                .as_ref()
                .map(|template| file_url(template, path, &self.checkout.head)),
            text,
            thread: None,
            authors: vec![file_history.author.clone()],
            created_at: file_history.created_at,
            modified_at: file_history.modified_at,
            meta: Default::default(),
        }))
    }
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

fn file_url(template: &str, path: &str, commit: &str) -> String {
    let path = path
        .split('/')
        .map(|segment| utf8_percent_encode(segment, URL_SEGMENT).to_string())
        .collect::<Vec<_>>()
        .join("/");
    template
        .replace("{path}", &path)
        .replace("{commit}", commit)
}

fn glob_set(globs: &[String]) -> Result<GlobSet, PluginError> {
    let mut set = GlobSetBuilder::new();
    for glob in globs {
        set.add(Glob::new(glob)?);
    }
    Ok(set.build()?)
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

    #[test]
    fn urls_encode_each_path_segment() {
        assert_eq!(
            file_url(
                "https://git.example/{commit}/{path}",
                "notes/a b#1?.md",
                "abc"
            ),
            "https://git.example/abc/notes/a%20b%231%3F.md"
        );
    }
}
