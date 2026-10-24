//! Splits a session's history into turns and renders each as Markdown, the way
//! ferret-ingest-agents renders agent sessions: who spoke when, the text as written, and each tool
//! call as what went in and how much came back. Never the tool output: it mostly repeats files that
//! live elsewhere, and the same file read in many sessions would match every search for it.

use std::{
    borrow::Cow,
    collections::{HashMap, VecDeque},
    ops::Range,
};

use chrono::{DateTime, Utc};
use serde_json::{Map, Value};

use super::archive::{Event, ToolCall, ToolResult, UserMessage};

const TITLE_CHARS: usize = 120;
/// Longer turns are cut. A document goes to the server as one request, bound by its 2 MB body
/// limit, and a turn whose put fails would fail every run until carapace rewrote the archive.
const MAX_TEXT_BYTES: usize = 1_000_000;
const DETAIL_CHARS: usize = 300;
/// The arguments that say what a tool call did, in the order they're looked for. Any other call
/// shows all its arguments.
const DETAIL_ARGS: [&str; 6] = ["command", "path", "file_path", "pattern", "url", "code"];
/// What carapace's security records as tool calls next to the agent's own (a domain a call
/// reached, a credential it used, a push it made), after the action kinds of
/// `carapace/security/context.py`. Older ones don't name the call they belong to, so their tool
/// tells them apart.
const SECURITY_CHECKS: [&str; 3] = ["proxy_domain", "credential_access", "git_push"];

/// A conversational user message and everything up to the next one.
#[derive(Debug)]
pub struct Turn {
    /// The user message's seq, which identifies the turn within its session.
    pub seq: usize,
    /// The user message on one line, `None` for one of attachments only.
    pub title: Option<String>,
    pub text: String,
    /// The models that answered, in order.
    pub models: Vec<String>,
    /// When the user sent the message.
    pub created_at: Option<DateTime<Utc>>,
    /// The time of the turn's latest event, slash commands within it apart.
    pub modified_at: Option<DateTime<Utc>>,
}

/// The turns of a session's history. What comes before the first user message belongs to none.
pub fn turns(history: &[Event]) -> Vec<Turn> {
    let results = tool_results(history);
    let starts: Vec<(usize, &UserMessage)> = (0..history.len())
        .filter_map(|seq| Some((seq, user_message(history, seq)?)))
        .collect();
    starts
        .iter()
        .enumerate()
        .map(|(index, &(seq, message))| {
            let end = starts
                .get(index + 1)
                .map_or(history.len(), |(next, _)| *next);
            turn(seq, message, history, seq + 1..end, &results)
        })
        .collect()
}

/// The result of each tool call, by the call's seq. Results name their call's `tool_id`. Events
/// from before carapace had one pair a result with the oldest open call of the same tool, as
/// carapace itself does.
fn tool_results(history: &[Event]) -> HashMap<usize, &ToolResult> {
    let mut calls_by_id: HashMap<&str, usize> = HashMap::new();
    let mut open_calls_by_tool: HashMap<&str, VecDeque<usize>> = HashMap::new();
    let mut results = HashMap::new();
    for (seq, event) in history.iter().enumerate() {
        match event {
            Event::ToolCall(call) => match &call.tool_id {
                Some(id) => {
                    calls_by_id.insert(id, seq);
                }
                None => open_calls_by_tool
                    .entry(&call.tool)
                    .or_default()
                    .push_back(seq),
            },
            Event::ToolResult(result) => {
                let call = match &result.tool_id {
                    Some(id) => calls_by_id.remove(id.as_str()),
                    None => open_calls_by_tool
                        .get_mut(result.tool.as_str())
                        .and_then(VecDeque::pop_front),
                };
                if let Some(call) = call {
                    results.insert(call, result);
                }
            }
            _ => {}
        }
    }
    results
}

/// The user message at `seq`, unless it's a slash command the server answered, which is what a
/// command event right after it says. Anything else starting with `/` (`/etc/hosts is broken`)
/// went to the agent.
fn user_message(history: &[Event], seq: usize) -> Option<&UserMessage> {
    let Event::User(message) = &history[seq] else {
        return None;
    };
    let answered_by_server = matches!(history.get(seq + 1), Some(Event::Command));
    (!(message.content.starts_with('/') && answered_by_server)).then_some(message)
}

/// The turn opened by `message` at `seq`, with the events in `rest` up to the next user message.
fn turn(
    seq: usize,
    message: &UserMessage,
    history: &[Event],
    rest: Range<usize>,
    results: &HashMap<usize, &ToolResult>,
) -> Turn {
    let mut models: Vec<String> = Vec::new();
    for event in &history[rest.clone()] {
        if let Event::Assistant(answer) = event
            && let Some(model) = answer.usage.as_ref().and_then(|usage| usage.model.as_ref())
            && !models.contains(model)
        {
            models.push(model.clone());
        }
    }
    let mut text = format!(
        "{}\n\n{}",
        header(message.timestamp, "User"),
        user_text(message)
    );
    if let Some((at, agent)) = agent_part(history, rest.clone(), results) {
        let speaker = if models.is_empty() {
            "Agent".to_owned()
        } else {
            format!("Agent ({})", models.join(", "))
        };
        // A rule between the speakers, never above the first: ferret would read it as front matter.
        text.push_str(&format!("\n\n---\n{}\n\n{agent}", header(at, &speaker)));
    }
    let modified_at = history[rest]
        .iter()
        .filter(|event| !matches!(event, Event::User(_)))
        .filter_map(Event::timestamp)
        .chain(message.timestamp)
        .max();
    Turn {
        seq,
        title: Some(one_line(&message.content, TITLE_CHARS)).filter(|title| !title.is_empty()),
        text: cap(text),
        models,
        created_at: message.timestamp,
        modified_at,
    }
}

/// `text` cut to [`MAX_TEXT_BYTES`] at a character boundary, marked as cut.
fn cap(mut text: String) -> String {
    if text.len() <= MAX_TEXT_BYTES {
        return text;
    }
    let end = text.floor_char_boundary(MAX_TEXT_BYTES);
    text.truncate(end);
    text.push_str("\n\n…(truncated)");
    text
}

/// `**2026-09-06 18:00 UTC · User**`, without the time for an event that has none.
fn header(at: Option<DateTime<Utc>>, speaker: &str) -> String {
    match at {
        Some(at) => format!("**{} · {speaker}**", at.format("%Y-%m-%d %H:%M UTC")),
        None => format!("**{speaker}**"),
    }
}

/// The message with a line per attachment above it, as carapace shows it to its memory extraction.
fn user_text(message: &UserMessage) -> String {
    let mut lines: Vec<String> = message
        .attachments
        .iter()
        .map(|attachment| match &attachment.mime {
            Some(mime) => format!("[attachment: {mime}, {}]", attachment.name),
            None => format!("[attachment: {}]", attachment.name),
        })
        .collect();
    let content = message.content.trim();
    if !content.is_empty() {
        lines.push(content.to_owned());
    }
    lines.join("\n")
}

/// What the agent said and did in a turn, with the time it started, or `None` if it didn't answer.
/// Thinking, approvals and security checks are left out.
fn agent_part(
    history: &[Event],
    rest: Range<usize>,
    results: &HashMap<usize, &ToolResult>,
) -> Option<(Option<DateTime<Utc>>, String)> {
    let mut started_at = None;
    let mut blocks: Vec<String> = Vec::new();
    let mut listing_tools = false;
    for seq in rest {
        match &history[seq] {
            Event::Assistant(answer) if !answer.content.trim().is_empty() => {
                started_at = started_at.or(answer.timestamp);
                blocks.push(answer.content.trim().to_owned());
                listing_tools = false;
            }
            Event::ToolCall(call) if !SECURITY_CHECKS.contains(&call.tool.as_str()) => {
                started_at = started_at.or(call.timestamp);
                let entry = tool_call(call, results.get(&seq).copied());
                match blocks.last_mut() {
                    // Consecutive calls make one list, whatever happened between them.
                    Some(list) if listing_tools => {
                        list.push('\n');
                        list.push_str(&entry);
                    }
                    _ => {
                        blocks.push(entry);
                        listing_tools = true;
                    }
                }
            }
            _ => {}
        }
    }
    (!blocks.is_empty()).then(|| (started_at, blocks.join("\n\n")))
}

/// `- **exec** `ls -la`` with `  - ok · 3 lines, 120 B` below it, from the call's result.
fn tool_call(call: &ToolCall, result: Option<&ToolResult>) -> String {
    let mut entry = format!("- **{}**", call.tool);
    if let Some(detail) = detail(&call.args) {
        entry.push(' ');
        entry.push_str(&inline_code(&one_line(&detail, DETAIL_CHARS)));
    }
    entry.push_str("\n  - ");
    entry.push_str(&result.map_or_else(|| "no result".to_owned(), outcome));
    entry
}

fn detail(args: &Map<String, Value>) -> Option<Cow<'_, str>> {
    DETAIL_ARGS
        .iter()
        .find_map(|key| args.get(*key)?.as_str().filter(|value| !value.is_empty()))
        .map(Cow::Borrowed)
        .or_else(|| (!args.is_empty()).then(|| Value::Object(args.clone()).to_string().into()))
}

/// Whether the call failed and how much came back.
fn outcome(result: &ToolResult) -> String {
    let output: Cow<str> = match &result.result {
        Value::String(output) => output.into(),
        Value::Null => "".into(),
        structured => structured.to_string().into(),
    };
    let lines = output.lines().count();
    let status = match result.exit_code {
        Some(code) if code != 0 => "failed",
        _ => "ok",
    };
    let plural = if lines == 1 { "" } else { "s" };
    format!("{status} · {lines} line{plural}, {}", size(output.len()))
}

/// `512 B`, `3.4 KB`, `1.2 MB`.
fn size(bytes: usize) -> String {
    match bytes {
        0..1_000 => format!("{bytes} B"),
        1_000..1_000_000 => format!("{:.1} KB", bytes as f64 / 1e3),
        _ => format!("{:.1} MB", bytes as f64 / 1e6),
    }
}

/// Inline code that holds any text: delimited by more backticks than any run inside, padded where
/// the text starts or ends with one.
fn inline_code(text: &str) -> String {
    let longest = text.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let ticks = "`".repeat(longest + 1);
    let pad = if text.starts_with('`') || text.ends_with('`') {
        " "
    } else {
        ""
    };
    format!("{ticks}{pad}{text}{pad}{ticks}")
}

/// `text` with its whitespace collapsed to single spaces, cut to `limit` characters with an `…`.
fn one_line(text: &str, limit: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= limit {
        return flat;
    }
    let mut cut: String = flat.chars().take(limit - 1).collect();
    cut.push('…');
    cut
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::plugins::carapace::archive;

    const ARCHIVE: &str =
        include_str!("../../../fixtures/carapace/sessions/2026/09/s-trip/conversation.json");

    fn fixture_turns() -> Vec<Turn> {
        turns(&archive::parse(ARCHIVE).unwrap().history)
    }

    fn at(time: &str) -> Option<DateTime<Utc>> {
        Some(format!("2026-09-06T{time}Z").parse().unwrap())
    }

    #[test]
    fn a_turn_runs_from_a_user_message_to_the_next_and_what_came_before_is_dropped() {
        let turns = fixture_turns();

        assert_eq!(
            turns.iter().map(|turn| turn.seq).collect::<Vec<_>>(),
            [1, 15, 19]
        );
        assert!(
            turns.iter().all(|turn| !turn.text.contains("Good evening")),
            "the greeting before the first user message"
        );
    }

    #[test]
    fn a_turn_shows_messages_and_tool_calls_but_no_output_thinking_or_approvals() {
        let turns = fixture_turns();

        assert_eq!(
            turns[0].text,
            "**User**\n\n\
             [attachment: image/png, map.png]\n\
             Plan a weekend trip with Anna to the coast.\n\n\
             ---\n\
             **2026-09-06 18:00 UTC · Agent (anthropic:claude-sonnet-4-5)**\n\n\
             Let me check the weather first.\n\n\
             - **exec** `curl -s https://weather.example/coast`\n  - ok · 2 lines, 29 B\n\
             - **read** `/workspace/notes/trips.md`\n  - failed · 1 line, 21 B\n\
             - **search** `{\"limit\":5,\"query\":\"coast hotels\"}`\n  - no result\n\n\
             Saturday looks best: sunny at the coast. Shall I look for a place to stay?"
        );
        assert!(
            !turns[0].text.contains("proxy_domain"),
            "the security check of the curl call"
        );
    }

    #[test]
    fn a_result_without_a_tool_id_pairs_with_the_oldest_open_call_of_its_tool() {
        let turns = fixture_turns();

        assert_eq!(
            turns[1].text,
            "**2026-09-06 18:10 UTC · User**\n\n\
             /etc/hosts on the cabin's router looks broken, can you check it?\n\n\
             ---\n\
             **2026-09-06 18:10 UTC · Agent (openai:gpt-5)**\n\n\
             - **exec** `cat /etc/hosts`\n  - ok · 2 lines, 36 B\n\n\
             It maps the router's own name to a wrong address."
        );
    }

    #[test]
    fn an_oversized_turn_is_cut_at_a_character_boundary() {
        // Two-byte characters, so the limit falls inside one unless the cut moves to a boundary.
        let archive = json!({
            "schema_version": 1,
            "session": {"session_id": "s-1", "channel_type": "cli", "title": null,
                        "created_at": "2026-09-06T00:00:00Z"},
            "history": [{"role": "user", "content": "ä".repeat(MAX_TEXT_BYTES)}],
        });

        let turns = turns(&archive::parse(&archive.to_string()).unwrap().history);

        let text = turns[0].text.strip_suffix("\n\n…(truncated)").unwrap();
        assert!(text.len() <= MAX_TEXT_BYTES && text.len() > MAX_TEXT_BYTES - 2);
        assert!(text.starts_with("**User**\n\nää"));
    }

    #[test]
    fn a_slash_command_the_server_answered_is_left_out_but_a_path_is_a_message() {
        let turns = fixture_turns();

        assert!(turns.iter().all(|turn| !turn.text.contains("/model")));
        assert_eq!(
            turns[1].title.as_deref(),
            Some("/etc/hosts on the cabin's router looks broken, can you check it?")
        );
        assert_eq!(
            turns[0].modified_at,
            at("18:01:00"),
            "the slash command after the answer doesn't date the turn"
        );
    }

    #[test]
    fn a_turn_names_the_models_that_answered_in_it() {
        let turns = fixture_turns();

        assert_eq!(turns[0].models, ["anthropic:claude-sonnet-4-5"]);
        assert_eq!(turns[1].models, ["openai:gpt-5"]);
        assert!(turns[2].models.is_empty(), "nobody answered yet");
        assert_eq!(turns[2].text, "**2026-09-06 18:20 UTC · User**\n\nThanks!");
    }

    #[test]
    fn a_message_without_a_timestamp_leaves_the_turn_undated() {
        let turns = fixture_turns();

        assert_eq!(turns[0].created_at, None);
        assert!(turns[0].text.starts_with("**User**\n\n"));
        assert_eq!(turns[1].created_at, at("18:10:00"));
        assert_eq!(turns[1].modified_at, at("18:11:00"));
    }

    #[test]
    fn a_tool_call_shows_its_most_telling_argument_on_one_line() {
        let args = |value: Value| value.as_object().unwrap().clone();

        assert_eq!(
            detail(&args(json!({"title": "x", "path": "/a", "command": "ls"}))).as_deref(),
            Some("ls")
        );
        assert_eq!(
            detail(&args(json!({"command": "", "query": "q"}))).as_deref(),
            Some(r#"{"command":"","query":"q"}"#)
        );
        assert_eq!(detail(&Map::new()), None);
        assert_eq!(one_line("a\n  b", 300), "a b");
        assert_eq!(one_line(&"x".repeat(301), 300).chars().count(), 300);
        assert_eq!(inline_code("a `b` c"), "``a `b` c``");
        assert_eq!(size(1_234), "1.2 KB");
    }
}
