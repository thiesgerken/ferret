//! carapace: the user's knowledge repository, with carapace's memory mirror parsed into sessions,
//! facts and digests, and its session archives into the sessions' turns, each under its own source
//! below the connection's root.

mod archive;
mod mirror;
mod transcript;

use std::collections::{HashMap, HashSet};

use ferret_api::{ContentType, DocumentPut, PutResponse, internal::RunStats};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use sha2::{Digest as _, Sha256};

use super::{
    Log, Outcome, PluginError, Run, git,
    repo::{Change, Checkout, FileHistory},
};
use archive::Archive;
use mirror::{Digest, Fact, Session};

pub const NAME: &str = "carapace";

/// Indexes a carapace knowledge repository: its memory as sessions, facts and digests, its session
/// archives as the sessions' turns, and its other Markdown files.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "carapace")]
pub struct CarapaceConfig {
    /// The knowledge repository's upstream remote, e.g.
    /// `https://git.example.com/me/knowledge.git`.
    remote: String,
    /// The branch carapace pushes to. The repository's default branch when left out.
    #[serde(default)]
    #[schemars(extend("x-ferret-preserve-credentials" = true))]
    branch: Option<String>,
    /// A token that may read the repository, for a private one.
    #[serde(default)]
    #[schemars(extend("writeOnly" = true))]
    token: Option<String>,
    /// carapace's web UI, e.g. `https://carapace.example.com`, which documents link back to.
    #[schemars(extend("x-ferret-preserve-credentials" = true))]
    carapace_url: String,
}

pub async fn run(run: &Run) -> Result<Outcome, PluginError> {
    let config: CarapaceConfig = serde_json::from_value(run.config.clone())?;
    let checkout = Checkout::clone(
        &config.remote,
        config.branch.as_deref(),
        config.token.as_deref(),
        &run.options.git_protocols,
    )
    .await?;
    run.log
        .line(format!("cloned {} at {}", config.remote, checkout.head));
    let repository = Repository {
        run,
        checkout: &checkout,
        modes: checkout.modes().await?,
        history: checkout.history().await?,
        target: Target {
            root: &run.source,
            carapace_url: config.carapace_url.trim_end_matches('/'),
        },
    };

    // Documents link to carapace, so a new URL needs every document put again. `files` names the
    // rule for which other files are indexed, `sessions` how a session becomes documents: a new
    // rule needs a full sync to sweep what the old one let in.
    let indexing = json!({
        "carapace_url": repository.target.carapace_url,
        "files": "markdown, skills/ apart",
        "sessions": "summary and own turns in one thread",
        "titles": git::TITLES,
    });
    let stats = match repository.changes(&indexing).await? {
        Ok(changes) => repository.apply(changes).await?,
        Err(reason) => {
            run.log.line(format!("full sync: {reason}"));
            repository.full_sync().await?
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

/// A document's key on the server.
type Key = (String, String);

/// The changes since the last run's commit, with the documents their files held back then.
///
/// This relies on a document coming from one path only: a key that a changed or deleted path held
/// and no changed path puts again is deleted, even if an unchanged path holds it too. carapace
/// keeps a session's summary in one file and its turns in another, and moving either shows up as a
/// deletion and an addition in one commit.
struct Changes {
    /// Added or modified, to put.
    changed: Vec<String>,
    /// What the changed and deleted files held at the last run. Whatever of it isn't put again
    /// is gone: a deleted file, or a fact a re-rendered session dropped.
    before: HashSet<Key>,
}

/// What a run needs to turn files of the checkout into documents.
struct Repository<'a> {
    run: &'a Run,
    checkout: &'a Checkout,
    modes: HashMap<String, String>,
    history: HashMap<String, FileHistory>,
    target: Target<'a>,
}

impl Repository<'_> {
    /// The changes since the commit the last run indexed, or why this run has to be a full sync.
    async fn changes(&self, indexing: &Value) -> Result<Result<Changes, String>, PluginError> {
        let state = &self.run.state;
        let Some(cursor) = state.get("commit").and_then(Value::as_str) else {
            return Ok(Err("no earlier run".to_owned()));
        };
        match state.get("indexing") {
            None => return Ok(Err("the last run stored no indexing settings".to_owned())),
            Some(stored) if stored != indexing => {
                return Ok(Err("the indexing settings changed".to_owned()));
            }
            Some(_) => {}
        }
        let diff = match self.checkout.changes_since(cursor).await? {
            Ok(diff) => diff,
            Err(reason) => return Ok(Err(reason)),
        };
        let mut changes = Changes {
            changed: Vec::new(),
            before: HashSet::new(),
        };
        for change in diff {
            let path = match change {
                Change::Changed(path) => {
                    changes.changed.push(path.clone());
                    path
                }
                Change::Deleted(path) => path,
            };
            match self.keys_at(cursor, &path).await? {
                Ok(keys) => changes.before.extend(keys),
                Err(reason) => return Ok(Err(reason)),
            }
        }
        self.run
            .log
            .line(format!("indexing the changes since {cursor}"));
        Ok(Ok(changes))
    }

    /// The keys of the documents the file at `path` held at `commit`, or why they can't be told:
    /// a session, digest or archive this plugin no longer parses.
    async fn keys_at(
        &self,
        commit: &str,
        path: &str,
    ) -> Result<Result<Vec<Key>, String>, PluginError> {
        let parse = |documents: Result<Vec<DocumentPut>, PluginError>| {
            Ok(documents
                .map(|documents| {
                    documents
                        .into_iter()
                        .map(|document| (document.source, document.external_id))
                        .collect()
                })
                .map_err(|error| format!("{path} at {commit}: {error}")))
        };
        match kind(path) {
            Kind::Ignored => Ok(Ok(Vec::new())),
            Kind::File(files) => Ok(Ok(vec![(
                self.target.source(files.sub_source()),
                path.to_owned(),
            )])),
            Kind::Session => match self.checkout.text_at(commit, path).await? {
                None => Ok(Ok(Vec::new())),
                // Repeated facts were logged when that version was indexed.
                Some(text) => parse(
                    mirror::parse_session(&text)
                        .map(|session| session_documents(&session, &self.target, &Log::default())),
                ),
            },
            Kind::Digest => match self.checkout.text_at(commit, path).await? {
                None => Ok(Ok(Vec::new())),
                Some(text) => parse(
                    mirror::parse_digest(&text)
                        .map(|digest| vec![digest_document(&digest, &self.target)]),
                ),
            },
            Kind::Archive => match self.checkout.text_at(commit, path).await? {
                None => Ok(Ok(Vec::new())),
                Some(text) => parse(
                    archive::parse(&text).map(|archive| turn_documents(&archive, &self.target)),
                ),
            },
        }
    }

    /// Puts every file inside a sync, whose commit deletes what's gone.
    async fn full_sync(&self) -> Result<RunStats, PluginError> {
        let run = self.run;
        let sync = run.api.start_sync(&run.source).await?.sync_id;
        let mut stats = RunStats::default();
        let mut paths: Vec<&String> = self.modes.keys().collect();
        paths.sort();
        for path in paths {
            for document in self.documents(path).await? {
                self.put(&document, &mut stats).await?;
            }
        }
        stats.deleted = run.api.commit_sync(sync).await?.deleted;
        Ok(stats)
    }

    /// Puts the changed files' documents and deletes what the changes dropped.
    async fn apply(&self, changes: Changes) -> Result<RunStats, PluginError> {
        let mut stats = RunStats::default();
        let mut put = HashSet::new();
        for path in &changes.changed {
            for document in self.documents(path).await? {
                self.put(&document, &mut stats).await?;
                put.insert((document.source, document.external_id));
            }
        }
        let mut dropped: Vec<&Key> = changes.before.difference(&put).collect();
        dropped.sort();
        for (source, external_id) in dropped {
            match self.run.api.delete_document(source, external_id).await {
                Ok(()) => stats.deleted += 1,
                // Never indexed, e.g. a file that was always too big.
                Err(error) if error.status() == Some(reqwest::StatusCode::NOT_FOUND) => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(stats)
    }

    /// The documents the file at `path` holds now. A session, digest or archive that can't be read
    /// fails the run, since what it held would be deleted. Other files that can't be indexed hold
    /// none.
    async fn documents(&self, path: &str) -> Result<Vec<DocumentPut>, PluginError> {
        let kind = kind(path);
        if kind == Kind::Ignored {
            return Ok(Vec::new());
        }
        let mode = self
            .modes
            .get(path)
            .ok_or_else(|| format!("{path} is in the diff but not tracked"))?;
        // An archive is mostly tool output, which no turn shows, so its size says little about
        // theirs.
        let text = match kind {
            Kind::Archive => self.checkout.text_of_any_size(path, mode).await?,
            _ => self.checkout.text(path, mode).await?,
        };
        let text = match text {
            Ok(text) => text,
            Err(skipped) if !matches!(kind, Kind::File(_)) => {
                return Err(format!("{path}: {skipped}").into());
            }
            Err(skipped) => {
                self.run.log.line(format!("skipped {path}: {skipped}"));
                return Ok(Vec::new());
            }
        };
        let in_file = |error: PluginError| format!("{path}: {error}");
        Ok(match kind {
            Kind::Session => session_documents(
                &mirror::parse_session(&text).map_err(in_file)?,
                &self.target,
                &self.run.log,
            ),
            Kind::Digest => vec![digest_document(
                &mirror::parse_digest(&text).map_err(in_file)?,
                &self.target,
            )],
            Kind::Archive => turn_documents(&archive::parse(&text).map_err(in_file)?, &self.target),
            Kind::File(files) => {
                let history = self.history.get(path).ok_or_else(|| {
                    format!("{path} is tracked but no commit in the log touched it")
                })?;
                vec![file_document(path, text, history, files, &self.target)]
            }
            Kind::Ignored => unreachable!("handled above"),
        })
    }

    async fn put(&self, document: &DocumentPut, stats: &mut RunStats) -> Result<(), PluginError> {
        match self.run.api.put_document(document).await? {
            PutResponse::Unchanged { .. } => stats.unchanged += 1,
            PutResponse::Created { .. } | PutResponse::Updated { .. } => stats.put += 1,
            // The user excluded it: as if the repository didn't have it.
            PutResponse::Excluded => {}
        }
        Ok(())
    }
}

/// What a repository path holds.
#[derive(Debug, PartialEq)]
enum Kind {
    Session,
    Digest,
    /// A session's `conversation.json`, wherever carapace's configurable prefix puts it.
    Archive,
    /// `memory/facts/*.md` repeats the facts of the session files. Other files than Markdown are
    /// skills' code, lockfiles and configuration.
    Ignored,
    File(Files),
}

/// The repository's other Markdown files: skills apart, everything else as files.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Files {
    Skill,
    Other,
}

impl Files {
    fn sub_source(self) -> &'static str {
        match self {
            Files::Skill => "skills",
            Files::Other => "files",
        }
    }
}

fn kind(path: &str) -> Kind {
    let markdown = path.ends_with(".md");
    if path.rsplit('/').next() == Some("conversation.json") {
        Kind::Archive
    } else if markdown && path.starts_with("memory/sessions/") {
        Kind::Session
    } else if markdown && (path.starts_with("memory/weeks/") || path.starts_with("memory/months/"))
    {
        Kind::Digest
    } else if path.starts_with("memory/facts/") || !markdown {
        Kind::Ignored
    } else if path.starts_with("skills/") {
        Kind::File(Files::Skill)
    } else {
        Kind::File(Files::Other)
    }
}

/// Where a connection's documents go and where they link to.
struct Target<'a> {
    /// The connection's root, e.g. `carapace`.
    root: &'a str,
    /// carapace's web UI, without a trailing slash.
    carapace_url: &'a str,
}

impl Target<'_> {
    fn source(&self, sub_source: &str) -> String {
        format!("{}/{sub_source}", self.root)
    }

    fn url(&self, path: &str, key: &str, value: &str) -> String {
        format!(
            "{}{path}?{key}={}",
            self.carapace_url,
            utf8_percent_encode(value, QUERY_VALUE)
        )
    }
}

/// RFC 3986's unreserved characters stay as they are, like in carapace's own URLs.
const QUERY_VALUE: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

/// The session's summary, which joins the session's thread after its turns, and one document per
/// fact. A fact carapace extracted twice (under different categories, say) has the same id, so
/// only its first occurrence counts.
fn session_documents(session: &Session, target: &Target, log: &Log) -> Vec<DocumentPut> {
    let url = target.url("/memory/sessions", "session", &session.session_id);
    let mut text = session.abstract_text.clone();
    push_section(&mut text, "Outcomes", &session.outcomes);
    push_section(&mut text, "Open loops", &session.open_loops);
    push_section(&mut text, "On my mind", &session.on_my_mind);
    let mut meta = Map::new();
    meta.insert("week_key".to_owned(), json!(session.week_key));
    meta.insert("month_key".to_owned(), json!(session.month_key));
    meta.insert("provenance".to_owned(), session.provenance.clone());
    let mut documents = vec![DocumentPut {
        source: target.source("sessions"),
        external_id: format!("summary:{}", session.session_id),
        content_type: ContentType::Markdown,
        title: Some(session.title.clone()),
        text,
        url: Some(url.clone()),
        thread: Some(session.session_id.clone()),
        parent: None,
        authors: Vec::new(),
        created_at: Some(session.created_at),
        modified_at: Some(session.created_at),
        meta,
    }];
    let mut seen = HashSet::new();
    for fact in &session.facts {
        let external_id = format!("{}/{}", session.session_id, fact_id(fact));
        if !seen.insert(external_id.clone()) {
            log.line(format!(
                "{}: skipped a repeated fact: {}",
                session.session_id, fact.statement
            ));
            continue;
        }
        documents.push(DocumentPut {
            source: target.source("facts"),
            external_id,
            content_type: ContentType::Plain,
            title: Some(
                fact.subject
                    .clone()
                    .unwrap_or_else(|| fact.statement.clone()),
            ),
            text: fact_text(fact),
            url: Some(url.clone()),
            thread: None,
            parent: None,
            authors: Vec::new(),
            created_at: Some(session.created_at),
            modified_at: Some(session.created_at),
            meta: fact_meta(fact, &session.session_id),
        });
    }
    documents
}

/// One document per turn of the session, each going on from the one before. A copied session's
/// inherited turns belong to the session they were written in, which indexes them already.
fn turn_documents(archive: &Archive, target: &Target) -> Vec<DocumentPut> {
    let session = &archive.session;
    let inherited = session
        .forked_from
        .as_ref()
        .map_or(0, |fork| fork.event_count);
    let turns: Vec<_> = transcript::turns(&archive.history)
        .into_iter()
        .filter(|turn| turn.seq >= inherited)
        .collect();
    let mut models: Vec<&String> = Vec::new();
    for model in turns.iter().flat_map(|turn| &turn.models) {
        if !models.contains(&model) {
            models.push(model);
        }
    }
    let mut meta = Map::new();
    meta.insert("session".to_owned(), json!(session.session_id));
    meta.insert("session_title".to_owned(), json!(session.title));
    meta.insert("session_started".to_owned(), json!(session.created_at));
    meta.insert("channel_type".to_owned(), json!(session.channel_type));
    meta.insert("models".to_owned(), json!(models));
    let url = target.url("/", "session", &session.session_id);
    let mut documents: Vec<DocumentPut> = Vec::new();
    for turn in turns {
        let parent = documents
            .last()
            .map(|previous| previous.external_id.clone());
        documents.push(DocumentPut {
            source: target.source("sessions"),
            external_id: format!("turn:{}:{}", session.session_id, turn.seq),
            content_type: ContentType::Markdown,
            title: turn.title,
            text: turn.text,
            url: Some(url.clone()),
            thread: Some(session.session_id.clone()),
            parent,
            authors: turn.models,
            created_at: turn.created_at,
            modified_at: turn.modified_at,
            meta: meta.clone(),
        });
    }
    documents
}

/// Any other Markdown file of the repository, like the git plugin indexes it. A skill's title is
/// the `name` of its front matter, as carapace lists it.
fn file_document(
    path: &str,
    text: String,
    history: &FileHistory,
    files: Files,
    target: &Target,
) -> DocumentPut {
    let skill_name = (files == Files::Skill)
        .then(|| front_matter_name(&text))
        .flatten();
    DocumentPut {
        source: target.source(files.sub_source()),
        external_id: path.to_owned(),
        content_type: ContentType::Markdown,
        title: Some(skill_name.unwrap_or_else(|| git::title(path, &text, ContentType::Markdown))),
        text,
        url: Some(target.url("/knowledge", "path", path)),
        thread: None,
        parent: None,
        authors: vec![history.author.clone()],
        created_at: Some(history.created_at),
        modified_at: Some(history.modified_at),
        meta: Map::new(),
    }
}

/// `name: …` in the `---` front matter at the top of `text`.
fn front_matter_name(text: &str) -> Option<String> {
    let mut lines = text.lines();
    if lines.next()? != "---" {
        return None;
    }
    lines
        .take_while(|line| *line != "---")
        .find_map(|line| line.strip_prefix("name:"))
        .map(|name| name.trim().trim_matches(['"', '\'']).to_owned())
        .filter(|name| !name.is_empty())
}

fn digest_document(digest: &Digest, target: &Target) -> DocumentPut {
    let mut text = digest.summary.clone();
    push_section(&mut text, "Highlights", &digest.highlights);
    push_section(&mut text, "Open loops", &digest.open_loops);
    let mut meta = Map::new();
    meta.insert("level".to_owned(), json!(digest.level));
    meta.insert("period_key".to_owned(), json!(digest.period_key));
    meta.insert("provenance".to_owned(), digest.provenance.clone());
    DocumentPut {
        source: target.source("digests"),
        external_id: format!("{}:{}", digest.level, digest.period_key),
        content_type: ContentType::Markdown,
        title: Some(digest.title.clone()),
        text,
        url: Some(target.url("/memory/timeline", "period", &digest.period_key)),
        thread: None,
        parent: None,
        authors: Vec::new(),
        created_at: Some(digest.created_at),
        modified_at: Some(digest.created_at),
        meta,
    }
}

/// Facts have no id in carapace. Subject and statement identify one as long as it stays the
/// same, and a reworded fact is a different fact.
fn fact_id(fact: &Fact) -> String {
    let mut hash = Sha256::new();
    hash.update(fact.subject.as_deref().unwrap_or_default());
    hash.update("\n");
    hash.update(&fact.statement);
    // 64 bits are plenty among one session's facts.
    hash.finalize()[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn fact_text(fact: &Fact) -> String {
    let mut text = fact.statement.clone();
    if let Some(subject) = &fact.subject {
        text.push_str(&format!("\n\nSubject: {subject}"));
    }
    text.push_str(&format!("\nCategory: {}", fact.category));
    text
}

fn fact_meta(fact: &Fact, session_id: &str) -> Map<String, Value> {
    let mut meta = Map::new();
    meta.insert("category".to_owned(), json!(fact.category));
    if let Some(subject) = &fact.subject {
        meta.insert("subject".to_owned(), json!(subject));
    }
    meta.insert("source_kind".to_owned(), json!(fact.source_kind));
    meta.insert("confidence".to_owned(), json!(fact.confidence));
    meta.insert("durability".to_owned(), json!(fact.durability));
    if let Some(valid_until) = fact.valid_until {
        meta.insert("valid_until".to_owned(), json!(valid_until));
    }
    meta.insert("session_id".to_owned(), json!(session_id));
    meta
}

fn push_section(text: &mut String, heading: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }
    text.push_str(&format!("\n\n## {heading}\n"));
    for item in items {
        text.push_str(&format!("\n- {item}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TALOS: &str =
        include_str!("../../../fixtures/carapace/memory/sessions/2026/09/s-talos.md");
    const WEEK: &str = include_str!("../../../fixtures/carapace/memory/weeks/2026-W36.md");
    const ARCHIVE: &str =
        include_str!("../../../fixtures/carapace/sessions/2026/09/s-trip/conversation.json");

    const TARGET: Target = Target {
        root: "carapace",
        carapace_url: "https://carapace.example",
    };

    #[test]
    fn paths_sort_into_sessions_digests_archives_ignored_facts_and_files() {
        assert_eq!(kind("memory/sessions/2026/09/s-talos.md"), Kind::Session);
        assert_eq!(kind("memory/weeks/2026-W36.md"), Kind::Digest);
        assert_eq!(kind("memory/months/2026-09.md"), Kind::Digest);
        assert_eq!(kind("memory/facts/user.md"), Kind::Ignored);
        assert_eq!(kind("memory/README.md"), Kind::File(Files::Other));
        assert_eq!(
            kind("sessions/2026/04/2026-04-11-13-43-0cebc9fa/conversation.json"),
            Kind::Archive
        );
        assert_eq!(
            kind("archive/2026/04/s-1/conversation.json"),
            Kind::Archive,
            "under another path prefix"
        );
        assert_eq!(kind("conversation.json.bak"), Kind::Ignored);
        assert_eq!(kind("sessions/2026/04/s-1/notes.json"), Kind::Ignored);
        assert_eq!(kind("skills/brag/uv.lock"), Kind::Ignored);
        assert_eq!(kind("skills/brag/src/cli.py"), Kind::Ignored);
        assert_eq!(kind("skills/brag/SKILL.md"), Kind::File(Files::Skill));
        assert_eq!(
            kind("skills/mermaid/references/flowchart.md"),
            Kind::File(Files::Skill)
        );
        assert_eq!(kind("research/klima/README.md"), Kind::File(Files::Other));
    }

    #[test]
    fn a_session_becomes_its_document_and_one_per_fact() {
        let documents = session_documents(
            &mirror::parse_session(TALOS).unwrap(),
            &TARGET,
            &Log::default(),
        );

        let session = &documents[0];
        assert_eq!(
            (session.source.as_str(), session.external_id.as_str()),
            ("carapace/sessions", "summary:s-talos")
        );
        assert_eq!(session.thread.as_deref(), Some("s-talos"));
        assert_eq!(session.parent, None);
        assert_eq!(
            session.url.as_deref(),
            Some("https://carapace.example/memory/sessions?session=s-talos")
        );
        assert!(
            session
                .text
                .starts_with("The user planned the Talos upgrade")
        );
        assert!(
            session
                .text
                .contains("## Open loops\n\n- Back up etcd before the upgrade.")
        );
        assert!(
            !session.text.contains("Friction"),
            "friction stays in carapace"
        );
        assert_eq!(session.meta["week_key"], "2026-W36");

        let facts = &documents[1..];
        assert_eq!(facts.len(), 4);
        let cluster = facts
            .iter()
            .find(|fact| fact.title.as_deref() == Some("home cluster"))
            .unwrap();
        assert_eq!(cluster.source, "carapace/facts");
        assert!(cluster.external_id.starts_with("s-talos/"));
        assert_eq!(cluster.url, session.url, "facts link to their session");
        assert_eq!(
            Value::Object(cluster.meta.clone()),
            json!({
                "category": "surroundings",
                "subject": "home cluster",
                "source_kind": "observed",
                "confidence": "high",
                "durability": "dated",
                "valid_until": "2026-10-01",
                "session_id": "s-talos",
            })
        );
        assert_eq!(
            cluster.text,
            "The home cluster runs Talos 1.10.\n\nSubject: home cluster\nCategory: surroundings"
        );
    }

    #[test]
    fn an_archive_becomes_a_thread_of_turns_next_to_the_summary() {
        let documents = turn_documents(&archive::parse(ARCHIVE).unwrap(), &TARGET);

        assert_eq!(
            documents
                .iter()
                .map(|turn| (turn.external_id.as_str(), turn.parent.as_deref()))
                .collect::<Vec<_>>(),
            [
                ("turn:s-trip:1", None),
                ("turn:s-trip:15", Some("turn:s-trip:1")),
                ("turn:s-trip:19", Some("turn:s-trip:15")),
            ]
        );
        let first = &documents[0];
        assert_eq!(first.source, "carapace/sessions");
        assert_eq!(first.thread.as_deref(), Some("s-trip"));
        assert_eq!(
            first.title.as_deref(),
            Some("Plan a weekend trip with Anna to the coast.")
        );
        assert_eq!(
            first.url.as_deref(),
            Some("https://carapace.example/?session=s-trip")
        );
        assert_eq!(first.authors, ["anthropic:claude-sonnet-4-5"]);
        assert_eq!(first.created_at, None, "no fallback to the session's start");
        assert_eq!(
            Value::Object(first.meta.clone()),
            json!({
                "session": "s-trip",
                "session_title": "Weekend trip",
                "session_started": "2026-09-06T17:57:00Z",
                "channel_type": "cli",
                "models": ["anthropic:claude-sonnet-4-5", "openai:gpt-5"],
            })
        );
        assert!(documents[2].authors.is_empty(), "the user isn't an author");
    }

    #[test]
    fn a_copied_session_leaves_its_inherited_turns_to_the_original() {
        let copy = ARCHIVE.replace(
            "\"session_id\": \"s-trip\",",
            "\"session_id\": \"s-trip\", \"forked_from\": {\"session_id\": \"s-origin\", \"event_count\": 15},",
        );

        let documents = turn_documents(&archive::parse(&copy).unwrap(), &TARGET);

        assert_eq!(
            documents
                .iter()
                .map(|turn| (turn.external_id.as_str(), turn.parent.as_deref()))
                .collect::<Vec<_>>(),
            [
                ("turn:s-trip:15", None),
                ("turn:s-trip:19", Some("turn:s-trip:15")),
            ]
        );
        assert_eq!(
            documents[0].meta["models"],
            json!(["openai:gpt-5"]),
            "only the models that answered in the copy"
        );
    }

    #[test]
    fn a_fact_repeated_in_a_session_becomes_one_document() {
        let session = mirror::parse_session(TALOS).unwrap();
        let log = Log::default();

        let documents = session_documents(&session, &TARGET, &log);

        assert_eq!(session.facts.len(), 5, "the fixture repeats a fact");
        let anna: Vec<_> = documents
            .iter()
            .filter(|document| document.text.starts_with("Anna helps"))
            .collect();
        assert_eq!(anna.len(), 1);
        assert_eq!(anna[0].meta["category"], "social", "the first one counts");
        assert_eq!(
            log.take(),
            ["s-talos: skipped a repeated fact: Anna helps with the cluster now and then."]
        );
    }

    #[test]
    fn a_fact_id_changes_with_its_wording_only() {
        let session = mirror::parse_session(TALOS).unwrap();
        let mut reworded = mirror::parse_session(TALOS).unwrap();
        reworded.facts[0].statement.push_str(" Mostly.");
        reworded.facts[1].confidence = "high".to_owned();

        assert_ne!(fact_id(&session.facts[0]), fact_id(&reworded.facts[0]));
        assert_eq!(fact_id(&session.facts[1]), fact_id(&reworded.facts[1]));
        assert_eq!(fact_id(&session.facts[1]).len(), 16);
    }

    #[test]
    fn a_digest_links_to_its_period_on_the_timeline() {
        let document = digest_document(&mirror::parse_digest(WEEK).unwrap(), &TARGET);

        assert_eq!(
            (document.source.as_str(), document.external_id.as_str()),
            ("carapace/digests", "week:2026-W36")
        );
        assert_eq!(
            document.url.as_deref(),
            Some("https://carapace.example/memory/timeline?period=2026-W36")
        );
        assert_eq!(
            document.text,
            "Mostly homelab work, and a trip with Anna.\n\n## Highlights\n\n- Planned the Talos upgrade.\n\n## Open loops\n\n- Back up etcd."
        );
    }

    #[test]
    fn a_skill_is_titled_by_its_front_matter_name() {
        assert_eq!(
            front_matter_name("---\nname: moneydb\ndescription: x\n---\n# MoneyDB skill\n"),
            Some("moneydb".to_owned())
        );
        assert_eq!(front_matter_name("# Brag\n\nname: not front matter"), None);
        assert_eq!(
            front_matter_name("---\ndescription: x\n---\nname: below"),
            None
        );
    }
}
