//! git: one document per file of a repository. The first run is a full sync, later runs index
//! the diff since the last run's commit.

use std::{collections::HashMap, path::Path, time::Duration};

use chrono::{DateTime, Utc};
use ferret_api::{ContentType, DocumentPut, PutOutcome, internal::RunStats};
use globset::{Glob, GlobSet, GlobSetBuilder};
use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::process::Command;

use super::{Outcome, PluginError, Run};

pub const NAME: &str = "git";

/// Larger files aren't indexed: far more text than search needs, and reading them could exhaust
/// the worker's memory.
const MAX_FILE_BYTES: u64 = 1024 * 1024;
/// A git command that takes longer has stalled, e.g. on a remote that stopped answering. The
/// heartbeats would otherwise keep the run alive forever.
const GIT_TIMEOUT: Duration = Duration::from_secs(30 * 60);

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
// ponytail: no `token` for private repositories yet, it needs encrypted credentials (#11).
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "git")]
pub struct GitConfig {
    /// The repository to clone, e.g. `https://github.com/owner/notes.git`.
    remote: String,
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

/// The latest and the first commit that touched a file.
#[derive(Debug, PartialEq)]
struct FileHistory {
    author: String,
    modified_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
}

pub async fn run(run: &Run) -> Result<Outcome, PluginError> {
    let config: GitConfig = serde_json::from_value(run.config.clone())?;
    let git = Git {
        protocols: &run.options.git_protocols,
    };

    let checkout = tempfile::tempdir()?;
    let dir = checkout.path();
    let mut clone = git.command(Path::new("."));
    clone.args(["clone", "--quiet", "--single-branch"]);
    if let Some(branch) = &config.branch {
        clone.args(["--branch", branch]);
    }
    clone.args(["--".as_ref(), config.remote.as_ref(), dir.as_os_str()]);
    output(&mut clone, "git clone").await?;
    let commit = git
        .run(dir, &["rev-parse", "HEAD"])
        .await?
        .trim()
        .to_owned();
    run.log
        .line(format!("cloned {} at {commit}", config.remote));

    let listing = git.run(dir, &["ls-files", "--stage", "-z"]).await?;
    let history = parse_history(
        &git.run(
            dir,
            &["log", "-z", "--format=%x1e%an%x1f%aI", "--name-only"],
        )
        .await?,
    )?;
    let indexer = Indexer {
        run,
        config: &config,
        include: glob_set(&config.include)?,
        exclude: glob_set(&config.exclude)?,
        markdown: glob_set(&config.markdown)?,
        dir,
        commit: &commit,
        modes: tracked_files(&listing)?
            .into_iter()
            .map(|(mode, path)| (path, mode))
            .collect(),
        history,
    };

    let indexing = config.indexing();
    let stats = match incremental_base(&git, dir, &run.state, &indexing).await? {
        Ok(base) => {
            run.log.line(format!("indexing the changes since {base}"));
            indexer.changes_since(&git, &base).await?
        }
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
        state: json!({ "commit": commit, "indexing": indexing }),
        stats,
    })
}

/// The commit the last run indexed, if this run can index just the changes since it, or why
/// it can't.
async fn incremental_base(
    git: &Git<'_>,
    dir: &Path,
    state: &Value,
    indexing: &Value,
) -> Result<Result<String, String>, PluginError> {
    let Some(base) = state.get("commit").and_then(Value::as_str) else {
        return Ok(Err("no earlier run".to_owned()));
    };
    if state.get("indexing") != Some(indexing) {
        return Ok(Err("which files to index or how changed".to_owned()));
    }
    // From the connection's own state, but it ends up in a git command line.
    if base.is_empty() || !base.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("the stored commit {base:?} isn't a commit hash").into());
    }
    // A force push leaves the old commit outside the branch's history.
    if !git
        .succeeds(dir, &["merge-base", "--is-ancestor", base, "HEAD"])
        .await?
    {
        return Ok(Err(format!("{base} is no longer in the branch's history")));
    }
    Ok(Ok(base.to_owned()))
}

/// What a run needs to turn files of the checkout into documents.
struct Indexer<'a> {
    run: &'a Run,
    config: &'a GitConfig,
    include: GlobSet,
    exclude: GlobSet,
    markdown: GlobSet,
    dir: &'a Path,
    commit: &'a str,
    /// Every tracked file's mode, by path.
    modes: HashMap<&'a str, &'a str>,
    history: HashMap<String, FileHistory>,
}

impl Indexer<'_> {
    /// Puts every file inside a sync, whose commit deletes the documents of files that are gone.
    async fn full_sync(&self) -> Result<RunStats, PluginError> {
        let run = self.run;
        let sync = run.api.start_sync(&run.source).await?.sync_id;
        let mut stats = RunStats::default();
        let mut paths: Vec<_> = self.modes.keys().copied().collect();
        paths.sort_unstable();
        for path in paths {
            if self.selected(path) {
                self.put(path, &mut stats).await?;
            }
        }
        stats.deleted = run.api.commit_sync(sync).await?.deleted;
        Ok(stats)
    }

    /// Puts the files that changed since `base` and deletes the ones that are gone. Renames
    /// count as both, `--no-renames` reports them that way.
    async fn changes_since(&self, git: &Git<'_>, base: &str) -> Result<RunStats, PluginError> {
        let diff = git
            .run(
                self.dir,
                &["diff", "--name-status", "--no-renames", "-z", base, "HEAD"],
            )
            .await?;
        let mut stats = RunStats::default();
        for (status, path) in name_status(&diff)? {
            if !self.selected(path) {
                continue;
            }
            if status == "D" {
                self.delete(path, &mut stats).await?;
            } else {
                self.put(path, &mut stats).await?;
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
        // Symlinks (120000) would let a repository read the worker's own files, submodules
        // (160000) have no content here. git never checks a regular file out below a symlink,
        // so a regular file's path stays inside the checkout.
        let mode = self.modes.get(path).copied().unwrap_or_default();
        if mode != "100644" && mode != "100755" {
            run.log.line(format!("skipped {path}: not a regular file"));
            return Ok(None);
        }
        let metadata = tokio::fs::symlink_metadata(self.dir.join(path)).await?;
        if !metadata.is_file() {
            return Err(format!("{path} is a regular file in git but not in the checkout").into());
        }
        if metadata.len() > MAX_FILE_BYTES {
            run.log.line(format!(
                "skipped {path}: larger than {MAX_FILE_BYTES} bytes"
            ));
            return Ok(None);
        }
        let file_history = self
            .history
            .get(path)
            .ok_or_else(|| format!("{path} is tracked but no commit in the log touched it"))?;
        let bytes = tokio::fs::read(self.dir.join(path)).await?;
        let Some(text) = String::from_utf8(bytes)
            .ok()
            .filter(|text| !text.contains('\0'))
        else {
            run.log.line(format!("skipped {path}: not text"));
            return Ok(None);
        };
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
                .map(|template| file_url(template, path, self.commit)),
            text,
            thread: None,
            authors: vec![file_history.author.clone()],
            created_at: file_history.created_at,
            modified_at: file_history.modified_at,
            meta: Default::default(),
        }))
    }
}

/// Status and path of every entry in `git diff --name-status --no-renames -z`.
fn name_status(diff: &str) -> Result<Vec<(&str, &str)>, PluginError> {
    let mut fields = diff.split('\0').filter(|field| !field.is_empty());
    let mut entries = Vec::new();
    while let Some(status) = fields.next() {
        let path = fields.next().ok_or("unexpected git diff output")?;
        entries.push((status, path));
    }
    Ok(entries)
}

/// Mode and path of every file in `git ls-files --stage -z`.
fn tracked_files(listing: &str) -> Result<Vec<(&str, &str)>, PluginError> {
    listing
        .split('\0')
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            let (meta, path) = entry
                .split_once('\t')
                .ok_or("unexpected git ls-files output")?;
            let mode = meta.split(' ').next().unwrap_or_default();
            Ok((mode, path))
        })
        .collect()
}

/// Author and dates of every file, from `git log -z --format=%x1e%an%x1f%aI --name-only`: per
/// commit a header, NUL, a line break, then the paths, each ending in NUL. Newest commits come
/// first, so a file's first appearance is its latest commit and its last appearance its first.
fn parse_history(log: &str) -> Result<HashMap<String, FileHistory>, PluginError> {
    let mut files: HashMap<String, FileHistory> = HashMap::new();
    for commit in log.split('\x1e').filter(|commit| !commit.is_empty()) {
        let mut fields = commit.split('\0');
        let header = fields.next().unwrap_or_default();
        let (author, date) = header
            .split_once('\x1f')
            .ok_or("unexpected git log output")?;
        let date = DateTime::parse_from_rfc3339(date)?.with_timezone(&Utc);
        for (index, path) in fields.enumerate() {
            let path = if index == 0 {
                path.strip_prefix('\n').unwrap_or(path)
            } else {
                path
            };
            if path.is_empty() {
                continue;
            }
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

struct Git<'a> {
    /// What `GIT_ALLOW_PROTOCOL` gets.
    protocols: &'a str,
}

impl Git<'_> {
    /// Git in `dir`, limited to the allowed transports, so a remote can't point at the worker's
    /// files or at plain-HTTP services inside the cluster. It never prompts: a repository that
    /// needs credentials fails instead of hanging the run.
    fn command(&self, dir: &Path) -> Command {
        let mut command = Command::new("git");
        command
            .current_dir(dir)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_ALLOW_PROTOCOL", self.protocols)
            // Ends git when the run is dropped, e.g. because the server ended it.
            .kill_on_drop(true);
        command
    }

    /// Whether git in `dir` exits successfully, for commands that answer with their status.
    async fn succeeds(&self, dir: &Path, args: &[&str]) -> Result<bool, PluginError> {
        let status = tokio::time::timeout(GIT_TIMEOUT, self.command(dir).args(args).status())
            .await
            .map_err(|_| format!("git {} took too long", args.join(" ")))??;
        Ok(status.success())
    }

    /// Runs git in `dir` and returns its stdout.
    async fn run(&self, dir: &Path, args: &[&str]) -> Result<String, PluginError> {
        output(
            self.command(dir).args(args),
            &format!("git {}", args.join(" ")),
        )
        .await
    }
}

async fn output(command: &mut Command, name: &str) -> Result<String, PluginError> {
    let output = tokio::time::timeout(GIT_TIMEOUT, command.output())
        .await
        .map_err(|_| {
            format!(
                "{name} took longer than {} minutes",
                GIT_TIMEOUT.as_secs() / 60
            )
        })??;
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

    #[test]
    fn history_takes_the_latest_author_and_the_first_date() {
        // Two commits, newest first, the way `git log -z` prints them. One path has a tab and
        // quotes, which git would C-quote without -z.
        let log = "\x1eGrace\x1f2026-10-17T12:00:00+02:00\0\nnotes/a b.md\0c.md\0\
                   \x1eAda\x1f2026-10-16T09:30:00+02:00\0\nnotes/a b.md\0tab\t\"q\".md\0";

        let history = parse_history(log).unwrap();

        let at = |iso: &str| {
            DateTime::parse_from_rfc3339(iso)
                .unwrap()
                .with_timezone(&Utc)
        };
        assert_eq!(history.len(), 3);
        assert_eq!(
            history["notes/a b.md"],
            FileHistory {
                author: "Grace".to_owned(),
                modified_at: at("2026-10-17T12:00:00+02:00"),
                created_at: at("2026-10-16T09:30:00+02:00"),
            }
        );
        assert_eq!(history["tab\t\"q\".md"].author, "Ada");
        assert_eq!(history["c.md"].created_at, at("2026-10-17T12:00:00+02:00"));
    }

    #[test]
    fn tracked_files_carry_their_mode() {
        let listing = "100644 0123abcd 0\tnotes/a b.md\x00120000 4567ef01 0\tleak\0";

        assert_eq!(
            tracked_files(listing).unwrap(),
            [("100644", "notes/a b.md"), ("120000", "leak")]
        );
    }

    #[tokio::test]
    async fn remotes_are_limited_to_the_allowed_protocols() {
        let git = Git { protocols: "https" };

        let error = git
            .run(
                Path::new("."),
                &["ls-remote", "http://127.0.0.1:9/repo.git"],
            )
            .await
            .unwrap_err();

        assert!(error.to_string().contains("not allowed"), "{error}");
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
