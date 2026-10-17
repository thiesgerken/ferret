//! Parses carapace's memory mirror: the Markdown that `carapace/memory/render.py` writes for
//! sessions and digests. This couples the plugin to carapace's rendering, so anything that doesn't
//! parse is an error rather than skipped: a silently skipped fact would just be missing.

use chrono::{DateTime, NaiveDate, Utc};
use serde::Deserialize;
use serde_json::Value;

use crate::plugins::PluginError;

/// One session file, `memory/sessions/YYYY/MM/<session id>.md`.
#[derive(Debug)]
pub struct Session {
    pub session_id: String,
    pub week_key: String,
    pub month_key: String,
    /// When carapace extracted the session.
    pub created_at: DateTime<Utc>,
    pub provenance: Value,
    pub title: String,
    pub abstract_text: String,
    pub outcomes: Vec<String>,
    pub open_loops: Vec<String>,
    pub on_my_mind: Vec<String>,
    pub facts: Vec<Fact>,
    pub friction: Vec<String>,
    pub tags: Vec<String>,
}

/// One digest file, `memory/weeks/YYYY-Www.md` or `memory/months/YYYY-MM.md`.
#[derive(Debug)]
pub struct Digest {
    /// `week` or `month`.
    pub level: String,
    pub period_key: String,
    pub created_at: DateTime<Utc>,
    pub provenance: Value,
    pub coverage: Value,
    pub title: String,
    pub summary: String,
    pub on_my_mind: Vec<String>,
    pub highlights: Vec<String>,
    pub open_loops: Vec<String>,
    /// Repeats facts of the digest's sessions.
    pub learned: Vec<Fact>,
}

#[derive(Debug, PartialEq)]
pub struct Fact {
    /// `user`, `social` or `surroundings`.
    pub category: &'static str,
    pub subject: Option<String>,
    pub statement: String,
    /// `user_said` or `observed`.
    pub source_kind: String,
    /// `low`, `medium` or `high`.
    pub confidence: String,
    /// `durable` or `dated`.
    pub durability: String,
    /// Only for dated facts, and not for all of them.
    pub valid_until: Option<NaiveDate>,
}

#[derive(Deserialize)]
struct FrontMatter {
    kind: String,
    session_id: Option<String>,
    week_key: Option<String>,
    month_key: Option<String>,
    period_key: Option<String>,
    created_at: DateTime<Utc>,
    provenance: Value,
    #[serde(default)]
    coverage: Value,
}

pub fn parse_session(markdown: &str) -> Result<Session, PluginError> {
    let (front_matter, body) = front_matter(markdown)?;
    if front_matter.kind != "session" {
        return Err(format!("expected a session, got {:?}", front_matter.kind).into());
    }
    let mut session = Session {
        session_id: required(front_matter.session_id, "session_id")?,
        week_key: required(front_matter.week_key, "week_key")?,
        month_key: required(front_matter.month_key, "month_key")?,
        created_at: front_matter.created_at,
        provenance: front_matter.provenance,
        title: String::new(),
        abstract_text: String::new(),
        outcomes: Vec::new(),
        open_loops: Vec::new(),
        on_my_mind: Vec::new(),
        facts: Vec::new(),
        friction: Vec::new(),
        tags: Vec::new(),
    };
    let body = Body::parse(body)?;
    session.title = body.title;
    session.abstract_text = body.prose;
    for section in body.sections {
        match section.heading.as_str() {
            "Outcomes" => session.outcomes = section.items()?,
            "Open loops" => session.open_loops = section.items()?,
            "On my mind" => session.on_my_mind = section.items()?,
            "Facts" => session.facts = section.facts()?,
            "Friction" => session.friction = section.items()?,
            "Tags" => session.tags = section.tags()?,
            other => return Err(format!("unknown session section {other:?}").into()),
        }
    }
    Ok(session)
}

pub fn parse_digest(markdown: &str) -> Result<Digest, PluginError> {
    let (front_matter, body) = front_matter(markdown)?;
    if front_matter.kind != "week" && front_matter.kind != "month" {
        return Err(format!("expected a week or month digest, got {:?}", front_matter.kind).into());
    }
    let body = Body::parse(body)?;
    let mut digest = Digest {
        level: front_matter.kind,
        period_key: required(front_matter.period_key, "period_key")?,
        created_at: front_matter.created_at,
        provenance: front_matter.provenance,
        coverage: front_matter.coverage,
        title: body.title,
        summary: body.prose,
        on_my_mind: Vec::new(),
        highlights: Vec::new(),
        open_loops: Vec::new(),
        learned: Vec::new(),
    };
    for section in body.sections {
        match section.heading.as_str() {
            "On my mind" => digest.on_my_mind = section.items()?,
            "Highlights" => digest.highlights = section.items()?,
            "Open loops" => digest.open_loops = section.items()?,
            "Learned" => digest.learned = section.facts()?,
            other => return Err(format!("unknown digest section {other:?}").into()),
        }
    }
    Ok(digest)
}

fn required(value: Option<String>, key: &str) -> Result<String, PluginError> {
    value.ok_or_else(|| format!("the front matter has no {key}").into())
}

fn front_matter(markdown: &str) -> Result<(FrontMatter, &str), PluginError> {
    let rest = markdown
        .strip_prefix("---\n")
        .ok_or("the file doesn't start with front matter")?;
    let (yaml, body) = rest
        .split_once("\n---\n")
        .ok_or("the front matter doesn't end")?;
    Ok((serde_saphyr::from_str(yaml)?, body))
}

/// A file's body: the `# ` title, the prose under it, then `## ` sections.
struct Body {
    title: String,
    prose: String,
    sections: Vec<Section>,
}

struct Section {
    heading: String,
    /// The section's lines after the heading, blank ones included.
    lines: Vec<(usize, String)>,
}

impl Body {
    fn parse(body: &str) -> Result<Self, PluginError> {
        let mut lines = body.lines().enumerate().skip_while(|(_, line)| line.is_empty());
        let title = lines
            .next()
            .and_then(|(_, line)| line.strip_prefix("# "))
            .ok_or("the body doesn't start with a `# ` title")?
            .to_owned();
        let mut prose = Vec::new();
        let mut sections: Vec<Section> = Vec::new();
        for (number, line) in lines {
            if let Some(heading) = line.strip_prefix("## ") {
                sections.push(Section {
                    heading: heading.to_owned(),
                    lines: Vec::new(),
                });
            } else if let Some(section) = sections.last_mut() {
                section.lines.push((number, line.to_owned()));
            } else {
                prose.push(unescape(line));
            }
        }
        Ok(Self {
            title,
            prose: prose.join("\n").trim().to_owned(),
            sections,
        })
    }
}

impl Section {
    fn content(&self) -> impl Iterator<Item = &(usize, String)> {
        self.lines.iter().filter(|(_, line)| !line.is_empty())
    }

    /// The `- ` items of a list section.
    fn items(&self) -> Result<Vec<String>, PluginError> {
        self.content()
            .map(|(number, line)| {
                line.strip_prefix("- ")
                    .map(str::to_owned)
                    .ok_or_else(|| self.error(*number, line, "a `- ` list item"))
            })
            .collect()
    }

    /// The facts of `### <category>` sub-sections.
    fn facts(&self) -> Result<Vec<Fact>, PluginError> {
        let mut category = None;
        let mut facts = Vec::new();
        for (number, line) in self.content() {
            if let Some(heading) = line.strip_prefix("### ") {
                category = Some(match heading {
                    "You" => "user",
                    "People" => "social",
                    "Surroundings" => "surroundings",
                    _ => return Err(self.error(*number, line, "a known fact category")),
                });
            } else {
                let category = category.ok_or_else(|| self.error(*number, line, "a `### ` category"))?;
                facts.push(
                    parse_fact(line, category).ok_or_else(|| self.error(*number, line, "a fact"))?,
                );
            }
        }
        Ok(facts)
    }

    /// The tags line: `` `homelab` `talos` ``.
    fn tags(&self) -> Result<Vec<String>, PluginError> {
        let mut tags = Vec::new();
        for (number, line) in self.content() {
            for tag in line.split(' ') {
                let tag = tag
                    .strip_prefix('`')
                    .and_then(|tag| tag.strip_suffix('`'))
                    .ok_or_else(|| self.error(*number, line, "`tag` words"))?;
                tags.push(tag.to_owned());
            }
        }
        Ok(tags)
    }

    fn error(&self, number: usize, line: &str, expected: &str) -> PluginError {
        // Line numbers count from the body, which is what's left to point at without the
        // front matter's length.
        format!(
            "{}: body line {}: expected {expected}, got {line:?}",
            self.heading,
            number + 1
        )
        .into()
    }
}

/// `- **<subject>**: <statement> _(<source kind> · <confidence> confidence · <durability> ·
/// <refs>…)_`, the subject being optional. The meta is the last `_( … )_`, since a statement may
/// contain one too.
fn parse_fact(line: &str, category: &'static str) -> Option<Fact> {
    let item = line.strip_prefix("- ")?;
    let (text, meta) = item.rsplit_once(" _(")?;
    let mut meta = meta.strip_suffix(")_")?.split(" · ");
    let source_kind = meta.next().filter(|kind| ["user_said", "observed"].contains(kind))?;
    let confidence = meta
        .next()?
        .strip_suffix(" confidence")
        .filter(|confidence| ["low", "medium", "high"].contains(confidence))?;
    let (durability, valid_until) = match meta.next()? {
        "durable" => ("durable", None),
        "dated" => ("dated", None),
        dated => {
            let until = dated.strip_prefix("dated until ")?;
            ("dated", Some(NaiveDate::parse_from_str(until, "%Y-%m-%d").ok()?))
        }
    };
    // What's left are the refs (`#42` or links to sessions), which ferret doesn't keep.
    let (subject, statement) = match text.strip_prefix("**") {
        Some(rest) => {
            let (subject, statement) = rest.split_once("**: ")?;
            (Some(subject.to_owned()), statement)
        }
        None => (None, text),
    };
    Some(Fact {
        category,
        subject,
        statement: statement.to_owned(),
        source_kind: source_kind.to_owned(),
        confidence: confidence.to_owned(),
        durability: durability.to_owned(),
        valid_until,
    })
}

/// Undoes carapace's escaping of prose line starts that Markdown would read as block structure
/// (`\#`, `\-`, `\1.`, …).
fn unescape(line: &str) -> String {
    let indent = line.len() - line.trim_start_matches([' ', '\t']).len();
    let (spaces, rest) = line.split_at(indent);
    match rest.strip_prefix('\\') {
        Some(escaped)
            if escaped.starts_with(['#', '>', '*', '+', '|', '`', '-'])
                || escaped.starts_with(|c: char| c.is_ascii_digit()) =>
        {
            format!("{spaces}{escaped}")
        }
        _ => line.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TALOS: &str = include_str!("../../../fixtures/carapace/memory/sessions/2026/09/s-talos.md");
    const TRIP: &str = include_str!("../../../fixtures/carapace/memory/sessions/2026/09/s-trip.md");
    const WEEK: &str = include_str!("../../../fixtures/carapace/memory/weeks/2026-W36.md");
    const MONTH: &str = include_str!("../../../fixtures/carapace/memory/months/2026-09.md");

    fn fact(category: &'static str, subject: Option<&str>, statement: &str) -> Fact {
        Fact {
            category,
            subject: subject.map(str::to_owned),
            statement: statement.to_owned(),
            source_kind: "user_said".to_owned(),
            confidence: "high".to_owned(),
            durability: "durable".to_owned(),
            valid_until: None,
        }
    }

    #[test]
    fn a_session_parses_into_its_sections_and_facts() {
        let session = parse_session(TALOS).unwrap();

        assert_eq!(session.session_id, "s-talos");
        assert_eq!(session.week_key, "2026-W36");
        assert_eq!(session.month_key, "2026-09");
        assert_eq!(session.created_at.to_rfc3339(), "2026-09-08T07:12:00+00:00");
        assert_eq!(session.provenance["model"], "anthropic:claude-haiku-4-5");
        assert_eq!(session.title, "Session s-talos");
        assert_eq!(
            session.abstract_text,
            "The user planned the Talos upgrade of the home cluster.\n\n\
             # not a heading, just a line the model started with a hash\n\
             - nor a list item"
        );
        assert_eq!(
            session.outcomes,
            ["Decided to upgrade the control plane first.", "Picked Saturday for it."]
        );
        assert_eq!(session.open_loops, ["Back up etcd before the upgrade."]);
        assert_eq!(session.on_my_mind, ["upgrading the home cluster to Talos 1.11"]);
        assert_eq!(session.friction, ["The upgrade docs were hard to find."]);
        assert_eq!(session.tags, ["homelab", "talos"]);
        assert_eq!(
            session.facts,
            [
                Fact {
                    confidence: "medium".to_owned(),
                    ..fact("user", None, "The user prefers upgrading on weekends.")
                },
                Fact {
                    confidence: "low".to_owned(),
                    ..fact(
                        "user",
                        None,
                        "The user writes notes like _(this · or that)_ inline."
                    )
                },
                fact("social", Some("Anna"), "Anna helps with the cluster now and then."),
                Fact {
                    source_kind: "observed".to_owned(),
                    durability: "dated".to_owned(),
                    valid_until: NaiveDate::from_ymd_opt(2026, 10, 1),
                    ..fact(
                        "surroundings",
                        Some("home cluster"),
                        "The home cluster runs Talos 1.10."
                    )
                },
            ]
        );
    }

    #[test]
    fn a_session_without_most_sections_parses_too() {
        let session = parse_session(TRIP).unwrap();

        assert_eq!(session.abstract_text, "The user planned a weekend trip with Anna.");
        assert!(session.outcomes.is_empty() && session.tags.is_empty());
        assert_eq!(
            session.facts,
            [fact("social", Some("Anna"), "Anna is the user's sister.")]
        );
    }

    #[test]
    fn digests_parse_with_their_period_and_learned_facts() {
        let week = parse_digest(WEEK).unwrap();
        let month = parse_digest(MONTH).unwrap();

        assert_eq!((week.level.as_str(), week.period_key.as_str()), ("week", "2026-W36"));
        assert_eq!(week.title, "Week 2026-W36 (2026-08-31 to 2026-09-06)");
        assert_eq!(week.summary, "Mostly homelab work, and a trip with Anna.");
        assert_eq!(week.highlights, ["Planned the Talos upgrade."]);
        assert_eq!(week.open_loops, ["Back up etcd."]);
        assert_eq!(week.coverage[1]["source_id"], "s-trip");
        assert_eq!(
            week.learned,
            [fact("social", Some("Anna"), "Anna is the user's sister.")]
        );
        assert_eq!((month.level.as_str(), month.period_key.as_str()), ("month", "2026-09"));
        assert_eq!(month.highlights, ["Upgraded the cluster."]);
        assert!(month.learned.is_empty());
    }

    #[test]
    fn a_line_that_doesnt_parse_fails_with_where_it_is() {
        let broken = TALOS.replace("medium confidence", "fairly confident");
        let unknown_section = TALOS.replace("## Friction", "## Grumbles");
        let stray_line = TALOS.replace("- Picked Saturday for it.", "Picked Saturday for it.");

        let error = parse_session(&broken).unwrap_err().to_string();
        assert!(error.starts_with("Facts: body line "), "{error}");
        assert!(error.contains("expected a fact"), "{error}");
        assert!(parse_session(&unknown_section).is_err());
        assert!(parse_session(&stray_line).is_err());
        assert!(parse_digest(TALOS).is_err(), "a session isn't a digest");
    }
}
