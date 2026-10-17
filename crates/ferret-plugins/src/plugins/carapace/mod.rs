//! carapace: the user's knowledge repository, with carapace's memory mirror parsed into sessions,
//! facts and digests, each under its own source below the connection's root.

mod mirror;

use ferret_api::{ContentType, DocumentPut};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde_json::{Map, Value, json};
use sha2::{Digest as _, Sha256};

use mirror::{Digest, Fact, Session};

/// What a repository path holds.
#[derive(Debug, PartialEq)]
enum Kind {
    Session,
    Digest,
    /// `memory/facts/*.md` repeats the facts of the session files.
    Ignored,
    File,
}

fn kind(path: &str) -> Kind {
    let markdown = path.ends_with(".md");
    if markdown && path.starts_with("memory/sessions/") {
        Kind::Session
    } else if markdown && (path.starts_with("memory/weeks/") || path.starts_with("memory/months/")) {
        Kind::Digest
    } else if path.starts_with("memory/facts/") {
        Kind::Ignored
    } else {
        Kind::File
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
            utf8_percent_encode(value, NON_ALPHANUMERIC)
        )
    }
}

/// The session's document and one document per fact.
fn session_documents(session: &Session, target: &Target) -> Vec<DocumentPut> {
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
        external_id: session.session_id.clone(),
        content_type: ContentType::Markdown,
        title: Some(session.title.clone()),
        text,
        url: Some(url.clone()),
        thread: None,
        authors: Vec::new(),
        created_at: session.created_at,
        modified_at: session.created_at,
        meta,
    }];
    documents.extend(session.facts.iter().map(|fact| DocumentPut {
        source: target.source("facts"),
        external_id: format!("{}/{}", session.session_id, fact_id(fact)),
        content_type: ContentType::Plain,
        title: Some(fact.subject.clone().unwrap_or_else(|| fact.statement.clone())),
        text: fact_text(fact),
        url: Some(url.clone()),
        thread: None,
        authors: Vec::new(),
        created_at: session.created_at,
        modified_at: session.created_at,
        meta: fact_meta(fact, &session.session_id),
    }));
    documents
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
        authors: Vec::new(),
        created_at: digest.created_at,
        modified_at: digest.created_at,
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

    const TALOS: &str = include_str!("../../../fixtures/carapace/memory/sessions/2026/09/s-talos.md");
    const WEEK: &str = include_str!("../../../fixtures/carapace/memory/weeks/2026-W36.md");

    const TARGET: Target = Target {
        root: "carapace",
        carapace_url: "https://carapace.example",
    };

    #[test]
    fn paths_sort_into_sessions_digests_ignored_facts_and_files() {
        assert_eq!(kind("memory/sessions/2026/09/s-talos.md"), Kind::Session);
        assert_eq!(kind("memory/weeks/2026-W36.md"), Kind::Digest);
        assert_eq!(kind("memory/months/2026-09.md"), Kind::Digest);
        assert_eq!(kind("memory/facts/user.md"), Kind::Ignored);
        assert_eq!(kind("memory/README.md"), Kind::File);
        assert_eq!(kind("skills/brag/SKILL.md"), Kind::File);
    }

    #[test]
    fn a_session_becomes_its_document_and_one_per_fact() {
        let documents = session_documents(&mirror::parse_session(TALOS).unwrap(), &TARGET);

        let session = &documents[0];
        assert_eq!(
            (session.source.as_str(), session.external_id.as_str()),
            ("carapace/sessions", "s-talos")
        );
        assert_eq!(
            session.url.as_deref(),
            Some("https://carapace.example/memory/sessions?session=s%2Dtalos")
        );
        assert!(session.text.starts_with("The user planned the Talos upgrade"));
        assert!(session.text.contains("## Open loops\n\n- Back up etcd before the upgrade."));
        assert!(!session.text.contains("Friction"), "friction stays in carapace");
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
            Some("https://carapace.example/memory/timeline?period=2026%2DW36")
        );
        assert_eq!(
            document.text,
            "Mostly homelab work, and a trip with Anna.\n\n## Highlights\n\n- Planned the Talos upgrade.\n\n## Open loops\n\n- Back up etcd."
        );
    }
}
