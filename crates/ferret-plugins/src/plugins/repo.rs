//! A git repository checked out for one run, shared by the plugins that index repositories.

use std::{collections::HashMap, path::Path, time::Duration};

use chrono::{DateTime, Utc};
use tempfile::TempDir;
use tokio::process::Command;

use super::PluginError;

/// Larger files aren't indexed: far more text than search needs, and reading them could exhaust
/// the worker's memory.
const MAX_FILE_BYTES: u64 = 1024 * 1024;
/// A git command that takes longer has stalled, e.g. on a remote that stopped answering. The
/// heartbeats would otherwise keep the run alive forever.
const GIT_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// The latest and the first commit that touched a file.
#[derive(Debug, PartialEq)]
pub struct FileHistory {
    pub author: String,
    pub modified_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

/// A change to a tracked file between two commits. A rename is a deletion plus an addition.
#[derive(Debug, PartialEq)]
pub enum Change {
    /// Added or modified.
    Changed(String),
    Deleted(String),
}

pub struct Checkout {
    dir: TempDir,
    /// What `GIT_ALLOW_PROTOCOL` gets.
    protocols: String,
    /// The checked out commit.
    pub head: String,
}

impl Checkout {
    /// Clones `branch` of `remote` (its default branch if `None`), with the whole history so that
    /// diffs against earlier runs' commits work.
    pub async fn clone(
        remote: &str,
        branch: Option<&str>,
        protocols: &str,
    ) -> Result<Self, PluginError> {
        let dir = tempfile::tempdir()?;
        let mut checkout = Self {
            dir,
            protocols: protocols.to_owned(),
            head: String::new(),
        };
        let mut clone = checkout.command(Path::new("."));
        clone.args(["clone", "--quiet", "--single-branch"]);
        if let Some(branch) = branch {
            clone.args(["--branch", branch]);
        }
        clone.args(["--".as_ref(), remote.as_ref(), checkout.path().as_os_str()]);
        output(&mut clone, "git clone").await?;
        checkout.head = checkout
            .git(&["rev-parse", "HEAD"])
            .await?
            .trim()
            .to_owned();
        Ok(checkout)
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    /// Every tracked file's mode (`100644`, `120000` for a symlink, ...), by path.
    pub async fn modes(&self) -> Result<HashMap<String, String>, PluginError> {
        let listing = self.git(&["ls-files", "--stage", "-z"]).await?;
        Ok(tracked_files(&listing)?
            .into_iter()
            .map(|(mode, path)| (path.to_owned(), mode.to_owned()))
            .collect())
    }

    /// Every file's author and dates, by path.
    pub async fn history(&self) -> Result<HashMap<String, FileHistory>, PluginError> {
        parse_history(
            &self
                .git(&["log", "-z", "--format=%x1e%an%x1f%aI", "--name-only"])
                .await?,
        )
    }

    /// The changes from `cursor` to HEAD, or why there is no such diff: `cursor` is no longer in
    /// the branch's history, e.g. after a force push.
    pub async fn changes_since(
        &self,
        cursor: &str,
    ) -> Result<Result<Vec<Change>, String>, PluginError> {
        // From a connection's state, but it ends up in a git command line.
        if cursor.is_empty() || !cursor.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(format!("the stored commit {cursor:?} isn't a commit hash").into());
        }
        if !self
            .succeeds(&["merge-base", "--is-ancestor", cursor, "HEAD"])
            .await?
        {
            return Ok(Err(format!(
                "{cursor} is no longer in the branch's history"
            )));
        }
        let diff = self
            .git(&[
                "diff",
                "--name-status",
                "--no-renames",
                "-z",
                cursor,
                "HEAD",
            ])
            .await?;
        Ok(Ok(name_status(&diff)?))
    }

    /// The text of the file at `path` with the given mode, or why it can't be indexed: not a
    /// regular file, too big, not text.
    pub async fn text(
        &self,
        path: &str,
        mode: &str,
    ) -> Result<Result<String, String>, PluginError> {
        // Symlinks (120000) would let a repository read the worker's own files, submodules
        // (160000) have no content here. git never checks a regular file out below a symlink,
        // so a regular file's path stays inside the checkout.
        if mode != "100644" && mode != "100755" {
            return Ok(Err("not a regular file".to_owned()));
        }
        let file = self.path().join(path);
        let metadata = tokio::fs::symlink_metadata(&file).await?;
        if !metadata.is_file() {
            return Err(format!("{path} is a regular file in git but not in the checkout").into());
        }
        if metadata.len() > MAX_FILE_BYTES {
            return Ok(Err(format!("larger than {MAX_FILE_BYTES} bytes")));
        }
        let bytes = tokio::fs::read(file).await?;
        Ok(String::from_utf8(bytes)
            .ok()
            .filter(|text| !text.contains('\0'))
            .ok_or_else(|| "not text".to_owned()))
    }

    /// Git in `dir`, limited to the allowed transports, so a remote can't point at the worker's
    /// files or at plain-HTTP services inside the cluster. It never prompts: a repository that
    /// needs credentials fails instead of hanging the run.
    fn command(&self, dir: &Path) -> Command {
        let mut command = Command::new("git");
        command
            .current_dir(dir)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_ALLOW_PROTOCOL", &self.protocols)
            // Ends git when the run is dropped, e.g. because the server ended it.
            .kill_on_drop(true);
        command
    }

    /// Runs git in the checkout and returns its stdout.
    async fn git(&self, args: &[&str]) -> Result<String, PluginError> {
        output(
            self.command(self.path()).args(args),
            &format!("git {}", args.join(" ")),
        )
        .await
    }

    /// Whether git in the checkout exits successfully, for commands that answer with their
    /// status.
    async fn succeeds(&self, args: &[&str]) -> Result<bool, PluginError> {
        let status =
            tokio::time::timeout(GIT_TIMEOUT, self.command(self.path()).args(args).status())
                .await
                .map_err(|_| format!("git {} took too long", args.join(" ")))??;
        Ok(status.success())
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

/// Every entry of `git diff --name-status --no-renames -z`.
fn name_status(diff: &str) -> Result<Vec<Change>, PluginError> {
    let mut fields = diff.split('\0').filter(|field| !field.is_empty());
    let mut changes = Vec::new();
    while let Some(status) = fields.next() {
        let path = fields
            .next()
            .ok_or("unexpected git diff output")?
            .to_owned();
        changes.push(if status == "D" {
            Change::Deleted(path)
        } else {
            Change::Changed(path)
        });
    }
    Ok(changes)
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

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn diffs_split_into_changes_and_deletions() {
        assert_eq!(
            name_status("M\0a.md\0D\0b c.txt\0A\0d.txt\0").unwrap(),
            [
                Change::Changed("a.md".to_owned()),
                Change::Deleted("b c.txt".to_owned()),
                Change::Changed("d.txt".to_owned()),
            ]
        );
    }

    #[tokio::test]
    async fn remotes_are_limited_to_the_allowed_protocols() {
        let error = Checkout::clone("http://127.0.0.1:9/repo.git", None, "https")
            .await
            .err()
            .unwrap();

        assert!(error.to_string().contains("not allowed"), "{error}");
    }
}
