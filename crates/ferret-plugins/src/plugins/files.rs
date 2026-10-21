//! Mounted filesystem discovery. File descriptors keep traversal beneath a verified mount.

use std::{
    collections::HashMap,
    fs::File,
    io::Read,
    path::{Component, Path},
    sync::Arc,
};

use base64::{Engine, engine::general_purpose::STANDARD};
use chrono::{DateTime, SecondsFormat, Utc};
use ferret_api::{ContentType, DocumentPut, InventoryEntry, PutResponse, internal::RunStats};
use globset::{Glob, GlobSet, GlobSetBuilder};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use rustix::fs::{AtFlags, Dir, FileType, Mode, OFlags, Stat, fstat, open, openat, statat};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::{Outcome, PluginError, Run};

pub const NAME: &str = "files";
const URL_SEGMENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');
const BATCH: usize = 512;
const MAX_LISTING_SIZE: usize = 1_048_576;
const RENDERING_VERSION: u32 = 1;
const EXCLUDED_DIRECTORIES: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    "node_modules",
    ".venv",
    "venv",
    "__pycache__",
    ".mypy_cache",
    ".pytest_cache",
    ".ruff_cache",
    ".next",
    ".nuxt",
    ".zfs",
    ".snapshot",
    ".stversions",
    ".Trash",
    ".Trashes",
    "$RECYCLE.BIN",
];
const EXCLUDED_FILES: &[&str] = &[".DS_Store", "Thumbs.db", "desktop.ini"];
const TEXT_EXTENSIONS: &[&str] = &[
    "tex",
    "md",
    "markdown",
    "txt",
    "rst",
    "org",
    "adoc",
    "py",
    "pyi",
    "ts",
    "tsx",
    "js",
    "jsx",
    "mjs",
    "cjs",
    "rs",
    "json",
    "jsonc",
    "yaml",
    "yml",
    "toml",
    "xml",
    "html",
    "htm",
    "css",
    "scss",
    "sass",
    "less",
    "sh",
    "bash",
    "zsh",
    "fish",
    "c",
    "h",
    "cc",
    "cpp",
    "hpp",
    "cs",
    "java",
    "kt",
    "kts",
    "go",
    "rb",
    "php",
    "pl",
    "pm",
    "lua",
    "r",
    "R",
    "jl",
    "swift",
    "m",
    "mm",
    "ex",
    "exs",
    "erl",
    "hrl",
    "hs",
    "lhs",
    "clj",
    "cljs",
    "scala",
    "sql",
    "graphql",
    "gql",
    "proto",
    "vue",
    "svelte",
    "ini",
    "cfg",
    "conf",
    "properties",
    "env",
    "nix",
    "tf",
    "tfvars",
    "cmake",
    "make",
    "mk",
    "dockerfile",
    "csv",
    "tsv",
    "log",
    "bib",
    "sty",
    "cls",
];
const TEXT_NAMES: &[&str] = &[
    "README",
    "LICENSE",
    "LICENCE",
    "COPYING",
    "NOTICE",
    "AUTHORS",
    "CHANGELOG",
    "CONTRIBUTING",
    "Makefile",
    "GNUmakefile",
    "Dockerfile",
    "Containerfile",
    "Justfile",
    "justfile",
    "Gemfile",
    "Rakefile",
    "Procfile",
    ".gitignore",
    ".gitattributes",
    ".editorconfig",
    ".env",
];

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "files")]
pub struct FilesConfig {
    /// Absolute mount point supplied read-only to every eligible worker. Symlinks are forbidden.
    pub root: String,
    /// Client-visible hierarchical URL prefix ending in /. Independent of the worker mount path.
    /// NFS URLs require a host, e.g. nfs://nas.example.com/export/.
    pub origin_url_prefix: String,
    /// Additional share-relative content globs, extending bundled prose/code/config selection.
    #[serde(default)]
    pub content_globs: Vec<String>,
    /// Additional share-relative exclusion globs. Mandatory names include .git, .hg, .svn,
    /// node_modules, .venv, venv, __pycache__, .mypy_cache, .pytest_cache, .ruff_cache, .next,
    /// .nuxt, .zfs, .snapshot, .stversions, .Trash, .Trashes, $RECYCLE.BIN, .DS_Store,
    /// Thumbs.db and desktop.ini. These rules cannot be disabled. Match a directory itself
    /// to prune it, e.g. **/private (not only **/private/**).
    #[serde(default)]
    pub exclude_globs: Vec<String>,
    /// Complete UTF-8 file contents only, in bytes. Directory listings have a separate 1 MiB cap.
    #[serde(default = "default_max_file_size")]
    pub max_file_size: u64,
}

fn default_max_file_size() -> u64 {
    1_048_576
}

fn glob_set(patterns: &[String]) -> Result<GlobSet, PluginError> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        builder.add(Glob::new(pattern)?);
    }
    Ok(builder.build()?)
}

fn origin_prefix(prefix: &str) -> Result<reqwest::Url, PluginError> {
    if prefix.chars().any(char::is_control) {
        return Err("origin_url_prefix may not contain control characters".into());
    }
    let url = reqwest::Url::parse(prefix)?;
    if url.cannot_be_a_base()
        || !url.path().ends_with('/')
        || url.query().is_some()
        || url.fragment().is_some()
        || (url.scheme() == "nfs" && url.host_str().is_none())
    {
        return Err("origin_url_prefix must be a hierarchical URL ending in /, without query/fragment; NFS requires a host".into());
    }
    Ok(url)
}

fn origin_url(prefix: &reqwest::Url, relative: &str, directory: bool) -> String {
    if relative.is_empty() {
        return prefix.to_string();
    }
    // URL parsers discard literal tabs/newlines even in appended path segments. Encode before
    // concatenating so every filename byte, including those controls, remains part of the link.
    let mut url = prefix.to_string();
    for (index, segment) in relative.split('/').enumerate() {
        if index != 0 {
            url.push('/');
        }
        url.extend(utf8_percent_encode(segment, URL_SEGMENT));
    }
    if directory {
        url.push('/');
    }
    url
}

fn fingerprint(stat: &Stat) -> String {
    format!(
        "{}:{}:{}:{}:{}:{}:{}:{}:{}",
        stat.st_dev,
        stat.st_ino,
        stat.st_mode,
        stat.st_size,
        stat.st_mtime,
        stat.st_mtime_nsec,
        stat.st_ctime,
        stat.st_ctime_nsec,
        stat.st_nlink
    )
}

fn timestamp(stat: &Stat) -> Result<DateTime<Utc>, PluginError> {
    DateTime::from_timestamp(stat.st_mtime, stat.st_mtime_nsec.try_into()?)
        .ok_or_else(|| "filesystem mtime is outside the supported timestamp range".into())
}

fn open_directory(root: &File, relative: &str) -> Result<File, PluginError> {
    let mut directory = root.try_clone()?;
    if !relative.is_empty() {
        for component in relative.split('/') {
            directory = File::from(openat(
                &directory,
                component,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )?);
        }
    }
    Ok(directory)
}

fn open_root(root: &str) -> Result<File, PluginError> {
    let path = Path::new(root);
    if !path.is_absolute() || path == Path::new("/") {
        return Err("root must be an absolute mounted directory other than /".into());
    }
    let mut directory = File::from(open(
        "/",
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
    )?);
    for component in path.components() {
        match component {
            Component::RootDir => {}
            Component::Normal(name) => {
                directory = File::from(openat(
                    &directory,
                    name,
                    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                    Mode::empty(),
                )?)
            }
            _ => return Err("root may not contain . or .. components".into()),
        }
    }
    verify_mount(root)?;
    Ok(directory)
}

fn verify_mount(root: &str) -> Result<(), PluginError> {
    #[cfg(target_os = "linux")]
    {
        let mountinfo = std::fs::read_to_string("/proc/self/mountinfo")?;
        let expected = Path::new(root);
        let mounted = mountinfo
            .lines()
            .filter_map(|line| line.split_whitespace().nth(4))
            .any(|mount| {
                Path::new(
                    &mount
                        .replace("\\040", " ")
                        .replace("\\011", "\t")
                        .replace("\\012", "\n")
                        .replace("\\134", "\\"),
                ) == expected
            });
        if mounted {
            return Ok(());
        }
        Err(format!("root {root:?} is not an available mount point in /proc/self/mountinfo").into())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = root;
        Err("files mount verification requires Linux /proc/self/mountinfo; run the worker in Linux with a read-only volume".into())
    }
}

struct Child {
    name: String,
    relative: String,
    kind: &'static str,
    stat: Stat,
}

fn children(root: &File, relative: &str, exclude: &GlobSet) -> Result<Vec<Child>, PluginError> {
    let directory = open_directory(root, relative)?;
    let mut children = Vec::new();
    for entry in Dir::read_from(&directory)? {
        let entry = entry?;
        let name = entry.file_name().to_str()?;
        if matches!(name, "." | "..") {
            continue;
        }
        let path = if relative.is_empty() {
            name.to_owned()
        } else {
            format!("{relative}/{name}")
        };
        if exclude.is_match(&path) {
            continue;
        }
        let stat = statat(&directory, entry.file_name(), AtFlags::SYMLINK_NOFOLLOW)?;
        let file_type = FileType::from_raw_mode(stat.st_mode);
        if EXCLUDED_FILES.contains(&name)
            || matches!(name, ".git" | ".hg" | ".svn")
            || (file_type == FileType::Directory && EXCLUDED_DIRECTORIES.contains(&name))
        {
            continue;
        }
        let kind = match file_type {
            FileType::Directory => "directory",
            FileType::RegularFile => "file",
            FileType::Symlink => "symlink",
            _ => "special",
        };
        children.push(Child {
            name: name.to_owned(),
            relative: path,
            kind,
            stat,
        });
    }
    children.sort_unstable_by(|a, b| a.name.cmp(&b.name));
    Ok(children)
}

fn listing(relative: &str, children: &[Child]) -> Result<String, PluginError> {
    let label = if relative.is_empty() {
        "/ (share root)"
    } else {
        relative
    };
    let mut text = format!(
        "Directory {}\nImmediate children (JSON lines):\n",
        serde_json::to_string(label)?
    );
    for (mut shown, child) in children.iter().enumerate() {
        let mut entry = json!({"name": child.name, "kind": child.kind,
            "modified_at": timestamp(&child.stat)?.to_rfc3339_opts(SecondsFormat::Nanos, true)});
        if child.kind == "file" {
            entry["size_bytes"] = json!(child.stat.st_size);
        }
        let line = serde_json::to_string(&entry)?;
        if text.len() + line.len() + 1 > MAX_LISTING_SIZE {
            loop {
                let footer = format!(
                    "Listing truncated: {shown} of {} immediate children shown.\n",
                    children.len()
                );
                if text.len() + footer.len() <= MAX_LISTING_SIZE {
                    text.push_str(&footer);
                    return Ok(text);
                }
                if shown == 0 {
                    return Err(
                        "directory path leaves no room for listing truncation notice".into(),
                    );
                }
                // Make room for the notice by removing only complete JSON lines.
                let end = text[..text.len() - 1]
                    .rfind('\n')
                    .expect("listing header ends with a newline");
                text.truncate(end + 1);
                shown -= 1;
            }
        }
        text.push_str(&line);
        text.push('\n');
    }
    Ok(text)
}

fn selected(path: &str, extra: &GlobSet) -> bool {
    let path = Path::new(path);
    extra.is_match(path)
        || path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| TEXT_NAMES.contains(&name))
        || path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| TEXT_EXTENSIONS.contains(&extension))
}

/// Opens every component relative to pinned directory handles, never following a symlink.
fn read_text(
    root: &File,
    child: &Child,
    limit: u64,
) -> Result<Result<String, &'static str>, PluginError> {
    if child.stat.st_size < 0 {
        return Err("negative file size".into());
    }
    if child.stat.st_size as u64 > limit {
        return Ok(Err("oversized"));
    }
    let (parent, name) = child
        .relative
        .rsplit_once('/')
        .unwrap_or(("", &child.relative));
    let directory = open_directory(root, parent)?;
    let mut file = File::from(openat(
        &directory,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )?);
    if fingerprint(&fstat(&file)?) != fingerprint(&child.stat) {
        return Err(format!("file changed before read: {:?}", child.relative).into());
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(
            limit
                .checked_add(1)
                .ok_or("maximum file size is too large")?,
        )
        .read_to_end(&mut bytes)?;
    if fingerprint(&fstat(&file)?) != fingerprint(&child.stat) {
        return Err(format!("file changed during read: {:?}", child.relative).into());
    }
    if bytes.len() as u64 > limit {
        return Ok(Err("oversized"));
    }
    if bytes.contains(&0) {
        return Ok(Err("NUL-containing/binary"));
    }
    Ok(String::from_utf8(bytes).map_err(|_| "invalid UTF-8"))
}

fn document(
    run: &Run,
    prefix: &reqwest::Url,
    relative: &str,
    directory: bool,
    text: String,
    modified_at: Option<DateTime<Utc>>,
) -> DocumentPut {
    let content_type = if !directory
        && matches!(
            Path::new(relative).extension().and_then(|s| s.to_str()),
            Some("md" | "markdown")
        ) {
        ContentType::Markdown
    } else {
        ContentType::Plain
    };
    DocumentPut {
        source: run.source.clone(),
        external_id: format!("{}:{relative}", if directory { "dir" } else { "file" }),
        content_type,
        title: Some(if relative.is_empty() {
            "/ (share root)".to_owned()
        } else {
            relative.to_owned()
        }),
        text,
        url: Some(origin_url(prefix, relative, directory)),
        thread: None,
        authors: Vec::new(),
        created_at: None,
        modified_at,
        meta: serde_json::from_value(
            json!({"kind": if directory {"directory"} else {"file"}, "path":relative}),
        )
        .expect("object metadata"),
    }
}

fn count_put(response: PutResponse, stats: &mut RunStats) {
    match response {
        PutResponse::Created { .. } | PutResponse::Updated { .. } => stats.put += 1,
        PutResponse::Unchanged { .. } => stats.unchanged += 1,
        PutResponse::Excluded => {}
    }
}

pub async fn run(run: &Run) -> Result<Outcome, PluginError> {
    let config: FilesConfig = serde_json::from_value(run.config.clone())?;
    if config.max_file_size == u64::MAX {
        return Err("max_file_size must be smaller than u64::MAX".into());
    }
    let prefix = origin_prefix(&config.origin_url_prefix)?;
    let include = glob_set(&config.content_globs)?;
    let exclude = Arc::new(glob_set(&config.exclude_globs)?);
    let root_name = config.root.clone();
    let root = Arc::new(tokio::task::spawn_blocking(move || open_root(&root_name)).await??);
    let root_identity = fingerprint(&fstat(&*root)?);
    let indexing = STANDARD.encode(Sha256::digest(serde_json::to_vec(
        &json!({"config":config, "rendering":RENDERING_VERSION}),
    )?));
    let previous: Option<Uuid> =
        if run.state.get("indexing").and_then(Value::as_str) == Some(&indexing) {
            run.state
                .get("inventory_generation")
                .and_then(Value::as_str)
                .map(str::parse)
                .transpose()?
        } else {
            None
        };
    if previous.is_none() {
        run.log
            .line("full reconciliation: no reusable inventory/settings");
    }
    let sync = run.api.start_sync(&run.source, None).await?.sync_id;
    let mut stats = RunStats::default();
    let mut pending = vec![String::new()];
    let mut generation = None;
    let mut directories = 0_u64;
    let mut files = 0_u64;
    let mut bodies_read = 0_u64;
    while let Some(relative) = pending.pop() {
        let scan_root = root.clone();
        let scan_relative = relative.clone();
        let scan_exclude = exclude.clone();
        let children = tokio::task::spawn_blocking(move || {
            children(&scan_root, &scan_relative, &scan_exclude)
        })
        .await?
        .map_err(|error| format!("enumerating directory {relative:?}: {error}"))?;
        directories += 1;
        // Child mtimes/sizes affect this representation. The directory's own mtime does not cover it.
        let doc = document(
            run,
            &prefix,
            &relative,
            true,
            listing(&relative, &children)?,
            None,
        );
        count_put(run.api.put_document(&doc).await?, &mut stats);
        for batch in children.chunks(BATCH) {
            let ids: Vec<_> = batch
                .iter()
                .map(|child| format!("{}:{}", child.kind, child.relative))
                .collect();
            let previous_entries = match previous {
                Some(previous) => run.api.inventory_lookup(previous, ids.clone()).await?,
                None => Vec::new(),
            };
            let previous_entries: HashMap<_, _> = previous_entries
                .into_iter()
                .map(|entry| (entry.external_id.clone(), entry))
                .collect();
            let mut entries = Vec::with_capacity(batch.len());
            let mut seen = Vec::new();
            for (child, id) in batch.iter().zip(ids) {
                let fingerprint = fingerprint(&child.stat);
                let mut indexed = false;
                if child.kind == "directory" {
                    pending.push(child.relative.clone());
                } else if child.kind == "file" {
                    files += 1;
                    if selected(&child.relative, &include) {
                        if let Some(old) = previous_entries
                            .get(&id)
                            .filter(|old| old.fingerprint == fingerprint)
                        {
                            indexed = old.indexed;
                            if indexed {
                                seen.push(id.clone());
                            } else {
                                run.log.line(format!(
                                    "skip {:?}: unchanged listing-only or remembered exclusion",
                                    child.relative
                                ));
                            }
                        } else {
                            let read_root = root.clone();
                            let read_child = Child {
                                name: child.name.clone(),
                                relative: child.relative.clone(),
                                kind: child.kind,
                                stat: child.stat,
                            };
                            let limit = config.max_file_size;
                            let content = tokio::task::spawn_blocking(move || {
                                read_text(&read_root, &read_child, limit)
                            })
                            .await?
                            .map_err(|error| {
                                format!("reading selected file {:?}: {error}", child.relative)
                            })?;
                            if child.stat.st_size as u64 <= limit {
                                bodies_read += 1;
                            }
                            match content {
                                Ok(text) => {
                                    let doc = document(
                                        run,
                                        &prefix,
                                        &child.relative,
                                        false,
                                        text,
                                        Some(timestamp(&child.stat)?),
                                    );
                                    let response = run.api.put_document(&doc).await?;
                                    // Remembered exclusions are stable until their removal resets working state.
                                    indexed = response.id().is_some();
                                    count_put(response, &mut stats);
                                }
                                Err(reason) => {
                                    run.log.line(format!("skip {:?}: {reason}", child.relative))
                                }
                            }
                        }
                    }
                }
                entries.push(InventoryEntry {
                    external_id: id,
                    fingerprint,
                    indexed,
                });
            }
            if !seen.is_empty() {
                stats.unchanged += run
                    .api
                    .sync_seen(
                        sync,
                        &run.source,
                        &seen.iter().map(String::as_str).collect::<Vec<_>>(),
                    )
                    .await?
                    .seen;
            }
            generation = Some(run.api.inventory_stage(&entries).await?.generation);
        }
    }
    // An empty share still needs a generation and root document.
    let generation = match generation {
        Some(id) => id,
        None => run.api.inventory_stage(&[]).await?.generation,
    };
    verify_mount(&config.root)?;
    // The pinned mount may have been replaced while scanning. An unavailable origin is not deletion.
    let current_root = open_root(&config.root)?;
    let current_stat = fstat(&current_root)?;
    let original_stat = fstat(&*root)?;
    if current_stat.st_dev != original_stat.st_dev || current_stat.st_ino != original_stat.st_ino {
        return Err(format!("root mount changed during scan ({root_identity})").into());
    }
    stats.deleted = run.api.commit_sync(sync).await?.deleted;
    run.log.line(format!("complete metadata walk: {directories} directories, {files} regular files; {bodies_read} file bodies read; {} put, {} unchanged, {} deleted", stats.put, stats.unchanged, stats.deleted));
    Ok(Outcome {
        state: json!({"inventory_generation": generation, "indexing":indexing}),
        stats,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::symlink};

    fn directory(path: &Path) -> File {
        File::open(path).unwrap()
    }

    #[test]
    fn bundled_selection_covers_prose_code_config_and_additional_globs() {
        let extra = glob_set(&["**/*.unusual".into()]).unwrap();
        for name in [
            "note.tex",
            "note.md",
            "note.markdown",
            "note.txt",
            "note.rst",
            "note.org",
            "note.adoc",
            "main.py",
            "main.ts",
            "main.tsx",
            "main.js",
            "main.rs",
            "data.json",
            "config.yaml",
            "config.yml",
            "Cargo.toml",
            "README",
            "src/Makefile",
            ".env",
            "nested/name.unusual",
        ] {
            assert!(selected(name, &extra), "{name}");
        }
        for name in [
            "document.pdf",
            "book.docx",
            "photo.jpg",
            "song.mp3",
            "film.mkv",
        ] {
            assert!(!selected(name, &extra), "{name}");
        }
    }

    #[test]
    fn exclusions_are_additive_and_pruned_before_enumeration() {
        let fixture = tempfile::tempdir().unwrap();
        for name in EXCLUDED_DIRECTORIES.iter().copied().chain([
            "Archive",
            "Backup",
            "build",
            "dist",
            "target",
            "vendor",
            ".ordinary",
            "private",
        ]) {
            fs::create_dir(fixture.path().join(name)).unwrap();
        }
        for name in EXCLUDED_FILES {
            fs::write(fixture.path().join(name), "hidden").unwrap();
        }
        let root = directory(fixture.path());
        let children = children(&root, "", &glob_set(&["**/private".into()]).unwrap()).unwrap();
        let names: Vec<_> = children.iter().map(|child| child.name.as_str()).collect();
        assert_eq!(
            names,
            [
                ".ordinary",
                "Archive",
                "Backup",
                "build",
                "dist",
                "target",
                "vendor"
            ]
        );
        fs::create_dir(fixture.path().join(".ordinary/.git")).unwrap();
        fs::write(fixture.path().join(".ordinary/.DS_Store"), "hidden").unwrap();
        assert!(
            super::children(&root, ".ordinary", &glob_set(&[]).unwrap())
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn listings_are_one_level_deterministic_escaped_and_never_follow_links() {
        let fixture = tempfile::tempdir().unwrap();
        fs::create_dir(fixture.path().join("nested")).unwrap();
        fs::write(
            fixture.path().join("nested/grandchild.txt"),
            "not in root listing",
        )
        .unwrap();
        fs::create_dir(fixture.path().join("empty")).unwrap();
        fs::write(
            fixture.path().join("a\n# [injection](bad).txt"),
            "child content absent",
        )
        .unwrap();
        symlink("/", fixture.path().join("outside")).unwrap();
        symlink(".", fixture.path().join("loop")).unwrap();
        let root = directory(fixture.path());
        let children = children(&root, "", &glob_set(&[]).unwrap()).unwrap();
        let text = listing("", &children).unwrap();
        assert!(!text.contains("grandchild"));
        assert!(!text.contains("child content absent"));
        assert!(text.contains("share root"));
        let rows: Vec<Value> = text
            .lines()
            .skip(2)
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(rows.len(), 5);
        assert_eq!(rows[0]["name"], "a\n# [injection](bad).txt");
        assert_eq!(rows[0]["size_bytes"], 20);
        assert!(rows[0]["modified_at"].as_str().unwrap().ends_with('Z'));
        assert_eq!(rows[2]["kind"], "symlink");
        assert!(open_directory(&root, "outside").is_err());
        assert!(open_directory(&root, "loop").is_err());
        assert!(listing("empty", &[]).unwrap().contains("empty"));
    }

    #[test]
    fn listing_cap_preserves_complete_escaped_entries_and_reports_omissions() {
        let fixture = tempfile::tempfile().unwrap();
        let stat = fstat(&fixture).unwrap();
        let children: Vec<_> = (0..12_000)
            .map(|i| Child {
                name: format!("{i:05}-ü\"\\\n{}", "a".repeat(100)),
                relative: String::new(),
                kind: "file",
                stat,
            })
            .collect();
        let text = listing("", &children).unwrap();
        assert!(text.len() <= MAX_LISTING_SIZE);
        let mut lines: Vec<_> = text.lines().skip(2).collect();
        let footer = lines.pop().unwrap();
        assert!(lines.len() < children.len());
        assert_eq!(
            footer,
            format!(
                "Listing truncated: {} of {} immediate children shown.",
                lines.len(),
                children.len()
            )
        );
        for (line, child) in lines.iter().zip(&children) {
            let entry: Value = serde_json::from_str(line).unwrap();
            assert_eq!(entry["name"], child.name);
            assert_eq!(entry["kind"], "file");
        }
        assert!(serde_json::to_vec(&json!({"text": text})).unwrap().len() < 2_097_152);
    }

    #[test]
    fn listing_cap_is_inclusive_and_reserves_room_for_the_truncation_notice() {
        let fixture = tempfile::tempfile().unwrap();
        let stat = fstat(&fixture).unwrap();
        let child = || Child {
            name: "file.txt".into(),
            relative: String::new(),
            kind: "file",
            stat,
        };
        let header_size = listing("", &[]).unwrap().len();
        let line_size = listing("", &[child()]).unwrap().len() - header_size;
        let mut children: Vec<_> = (0..(MAX_LISTING_SIZE - header_size) / line_size)
            .map(|_| child())
            .collect();
        let padding = (MAX_LISTING_SIZE - header_size) % line_size;
        children
            .last_mut()
            .unwrap()
            .name
            .push_str(&"x".repeat(padding));
        let text = listing("", &children).unwrap();
        assert_eq!(text.len(), MAX_LISTING_SIZE);
        assert_eq!(text.lines().skip(2).count(), children.len());
        assert!(!text.contains("Listing truncated:"));

        children.last_mut().unwrap().name.push('x');
        let text = listing("", &children).unwrap();
        assert!(text.len() <= MAX_LISTING_SIZE);
        let shown = text.lines().skip(2).count() - 1;
        assert!(shown < children.len());
        assert_eq!(
            text.lines().last().unwrap(),
            format!(
                "Listing truncated: {shown} of {} immediate children shown.",
                children.len()
            )
        );
        for line in text.lines().skip(2).take(shown) {
            let _: Value = serde_json::from_str(line).unwrap();
        }
    }

    #[test]
    fn file_limits_are_inclusive_and_binary_or_invalid_utf8_is_listing_only() {
        let fixture = tempfile::tempdir().unwrap();
        fs::write(fixture.path().join("ascii.txt"), "1234").unwrap();
        fs::write(fixture.path().join("unicode.txt"), "üö").unwrap();
        fs::write(fixture.path().join("nul.txt"), b"a\0b").unwrap();
        fs::write(fixture.path().join("invalid.txt"), [0xff, 0xfe]).unwrap();
        let root = directory(fixture.path());
        let children = children(&root, "", &glob_set(&[]).unwrap()).unwrap();
        let find = |name: &str| children.iter().find(|child| child.name == name).unwrap();
        assert_eq!(
            read_text(&root, find("ascii.txt"), 4).unwrap(),
            Ok("1234".into())
        );
        assert_eq!(
            read_text(&root, find("ascii.txt"), 3).unwrap(),
            Err("oversized")
        );
        assert_eq!(
            read_text(&root, find("unicode.txt"), 4).unwrap(),
            Ok("üö".into())
        );
        assert_eq!(
            read_text(&root, find("nul.txt"), 4).unwrap(),
            Err("NUL-containing/binary")
        );
        assert_eq!(
            read_text(&root, find("invalid.txt"), 4).unwrap(),
            Err("invalid UTF-8")
        );
        fs::write(fixture.path().join("ascii.txt"), "changed").unwrap();
        assert!(read_text(&root, find("ascii.txt"), 100).is_err());
    }

    #[test]
    fn swapping_selected_file_or_parent_to_symlink_fails_instead_of_reading_target() {
        let fixture = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("secret.txt"), "never read").unwrap();
        fs::create_dir(fixture.path().join("sub")).unwrap();
        fs::write(fixture.path().join("sub/secret.txt"), "initial").unwrap();
        let root = directory(fixture.path());
        let children = children(&root, "sub", &glob_set(&[]).unwrap()).unwrap();
        fs::remove_file(fixture.path().join("sub/secret.txt")).unwrap();
        symlink(
            outside.path().join("secret.txt"),
            fixture.path().join("sub/secret.txt"),
        )
        .unwrap();
        assert!(read_text(&root, &children[0], 100).is_err());
        fs::remove_file(fixture.path().join("sub/secret.txt")).unwrap();
        fs::remove_dir(fixture.path().join("sub")).unwrap();
        symlink(outside.path(), fixture.path().join("sub")).unwrap();
        assert!(read_text(&root, &children[0], 100).is_err());
    }

    #[test]
    fn origin_links_preserve_prefix_and_encode_each_segment() {
        let prefix = origin_prefix("nfs://nas.example.com/export%20name/").unwrap();
        assert_eq!(
            origin_url(&prefix, "", true),
            "nfs://nas.example.com/export%20name/"
        );
        assert_eq!(
            origin_url(&prefix, "a b/ü#?%.txt", false),
            "nfs://nas.example.com/export%20name/a%20b/%C3%BC%23%3F%25.txt"
        );
        assert_eq!(
            origin_url(&prefix, "directory", true),
            "nfs://nas.example.com/export%20name/directory/"
        );
        for invalid in [
            "nfs:/no-host/",
            "nfs://nas.example.com/no-slash",
            "nfs://nas.example.com/?q=x",
            "nfs://nas.example.com/#fragment",
            "mailto:test@example.com",
        ] {
            assert!(origin_prefix(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn changed_child_metadata_changes_listing_even_without_directory_mtime_change() {
        let fixture = tempfile::tempdir().unwrap();
        let path = fixture.path().join("file.txt");
        fs::write(&path, "before").unwrap();
        let root = directory(fixture.path());
        let before = listing("", &children(&root, "", &glob_set(&[]).unwrap()).unwrap()).unwrap();
        fs::write(&path, "after and longer").unwrap();
        let after = listing("", &children(&root, "", &glob_set(&[]).unwrap()).unwrap()).unwrap();
        assert_ne!(before, after);
    }

    #[test]
    fn missing_mount_and_placeholder_are_not_available_empty_shares() {
        let fixture = tempfile::tempdir().unwrap();
        assert!(open_root(fixture.path().to_str().unwrap()).is_err());
        assert!(open_root(fixture.path().join("missing").to_str().unwrap()).is_err());
        assert!(open_root("/").is_err());
        assert!(open_root("relative").is_err());
    }
}
