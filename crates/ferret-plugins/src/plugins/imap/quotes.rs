//! Strips what a mail quotes, so a thread's text is indexed once (plugins.md → Email): quote
//! containers in HTML, quote markers in text, and runs of lines an earlier mail of the thread
//! already has.

use std::collections::HashSet;

use lol_html::{RewriteStrSettings, element, html_content::ContentType, rewrite_str};
use mail_parser::decoders::html::html_to_text;

/// Stands in for Outlook's reply header. Outlook doesn't wrap the quoted mail, it just follows the
/// header, so the text is cut here.
const CUT: &str = "ferretquotedmailfollows";

/// A thread diff removes runs of at least this many lines, so short lines that a reply happens
/// to share with its parent ("Thanks!") stay.
const MIN_RUN: usize = 3;

/// Elements that start a line of their own. mail-parser's conversion only breaks lines at `<br>`,
/// so it would glue the text of two `<div>`s into one word.
const BLOCKS: &str = "address, blockquote, div, dl, dt, dd, h1, h2, h3, h4, h5, h6, hr, li, ol, p, pre, \
                      table, tr, ul";

/// The visible text of an HTML body, without the quote containers that mail clients wrap replies
/// in unless `keep_quotes`. Non-content elements are removed even when keeping forwarded content.
pub fn html_text(html: &str, keep_quotes: bool) -> String {
    let mut settings = RewriteStrSettings::new()
        // Ambiguous markup (e.g. stray tags inside `<select>`) is common in mail and harmless for
        // these selectors.
        .with_strict(false)
        // mail-parser only suppresses bare style/script tags, not tags with attributes.
        .append_element_content_handler(element!("style, script, template", |element| {
            element.remove();
            Ok(())
        }))
        .append_element_content_handler(element!(BLOCKS, |element| {
            element.before("<br>", ContentType::Html);
            element.after("<br>", ContentType::Html);
            Ok(())
        }));
    if !keep_quotes {
        let remove = |element: &mut lol_html::html_content::Element| {
            element.remove();
            Ok(())
        };
        settings = settings
            // Apple Mail and Thunderbird.
            .append_element_content_handler(element!("blockquote[type=cite]", remove))
            .append_element_content_handler(element!(".gmail_quote", remove))
            .append_element_content_handler(element!("#divRplyFwdMsg", |element| {
                element.replace(CUT, ContentType::Text);
                Ok(())
            }));
    }
    // ponytail: rewriting only fails past lol_html's memory limit. The mail is then indexed with
    // its quotes, and the thread diff still removes most of them.
    let rewritten = rewrite_str(html, settings).unwrap_or_else(|_| html.to_owned());
    let text = html_to_text(&rewritten);
    match text.find(CUT) {
        Some(cut) => text[..cut].to_owned(),
        None => text,
    }
}

/// `text` without quoted lines (`> …`), attribution lines (`On … wrote:`, `Am … schrieb …:`) and
/// everything from a quoted mail's header on (`-----Original Message-----`, `From:` followed by
/// `Sent:`), in English and German.
pub fn without_quote_markers(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut kept = Vec::new();
    let mut at = 0;
    while at < lines.len() {
        let rest = &lines[at..];
        if starts_quoted_mail(rest) {
            break;
        }
        if let Some(length) = attribution_length(rest) {
            at += length;
            continue;
        }
        if !rest[0].trim_start().starts_with('>') {
            kept.push(rest[0]);
        }
        at += 1;
    }
    tidy(&kept)
}

/// `text` without every run of at least `MIN_RUN` lines that also appears in one of the `earlier`
/// texts. Lines compare without quote markers and with whitespace collapsed, and blank lines
/// don't count, since quoting changes both.
pub fn without_repeated_runs(text: &str, earlier: &[&str]) -> String {
    let mut known = HashSet::new();
    for earlier in earlier {
        let lines: Vec<String> = earlier
            .lines()
            .map(normalized)
            .filter(|line| !line.is_empty())
            .collect();
        known.extend(lines.windows(MIN_RUN).map(|run| run.join("\n")));
    }
    let lines: Vec<&str> = text.lines().collect();
    let content: Vec<(usize, String)> = lines
        .iter()
        .enumerate()
        .map(|(at, line)| (at, normalized(line)))
        .filter(|(_, line)| !line.is_empty())
        .collect();
    let mut repeated = HashSet::new();
    for run in content.windows(MIN_RUN) {
        let joined: Vec<&str> = run.iter().map(|(_, line)| line.as_str()).collect();
        if known.contains(&joined.join("\n")) {
            repeated.extend(run.iter().map(|(at, _)| *at));
        }
    }
    let kept: Vec<&str> = lines
        .iter()
        .enumerate()
        .filter(|(at, _)| !repeated.contains(at))
        .map(|(_, line)| *line)
        .collect();
    tidy(&kept)
}

/// Whether a quoted mail starts here: Outlook's separator line or its header block.
fn starts_quoted_mail(lines: &[&str]) -> bool {
    let first = lines[0].trim();
    if first.starts_with("-----") {
        let label = first.trim_matches('-').trim().to_lowercase();
        return label == "original message" || label == "ursprüngliche nachricht";
    }
    let starts_with_any = |line: &str, labels: &[&str]| {
        let line = line.trim();
        labels.iter().any(|label| line.starts_with(label))
    };
    starts_with_any(first, &["From:", "Von:"])
        && lines[1..]
            .iter()
            .filter(|line| !line.trim().is_empty())
            .take(2)
            .any(|line| starts_with_any(line, &["Sent:", "Date:", "Gesendet:", "Datum:"]))
}

/// How many lines the attribution line starting here takes, which clients wrap after the
/// sender's name at times.
fn attribution_length(lines: &[&str]) -> Option<usize> {
    let first = lines[0].trim();
    let is_attribution = |text: &str| {
        (text.starts_with("On ") && text.ends_with("wrote:"))
            || (text.starts_with("Am ") && text.contains(" schrieb ") && text.ends_with(':'))
    };
    if is_attribution(first) {
        return Some(1);
    }
    let second = lines.get(1)?.trim();
    is_attribution(&format!("{first} {second}")).then_some(2)
}

fn normalized(line: &str) -> String {
    line.trim_start_matches(|c: char| c == '>' || c.is_whitespace())
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// The lines as text, without leading and trailing blank lines (nor trailing separator lines
/// that a cut quote leaves), and with runs of blank lines collapsed.
fn tidy(lines: &[&str]) -> String {
    let is_separator = |line: &&&str| line.trim().chars().all(|c| c == '_' || c == '-');
    let end = lines.len() - lines.iter().rev().take_while(is_separator).count();
    let mut text = String::new();
    let mut blank = false;
    for line in &lines[..end] {
        if line.trim().is_empty() {
            blank = !text.is_empty();
            continue;
        }
        if blank {
            text.push('\n');
            blank = false;
        }
        text.push_str(line.trim_end());
        text.push('\n');
    }
    text.truncate(text.trim_end().len());
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoted_lines_and_attributions_go_and_inline_replies_stay() {
        let text = "Sounds good.\n\nOn Mon, 13 Jan 2025 at 09:12, Alice Example <alice@example.com>\nwrote:\n> Shall we meet on Friday?\n> At 10?\n\nFriday works.\n\n> Bring the papers.\nWill do.\n";

        assert_eq!(
            without_quote_markers(text),
            "Sounds good.\n\nFriday works.\n\nWill do."
        );
    }

    #[test]
    fn a_german_attribution_goes() {
        let text = "Passt.\n\nAm 13.01.25 um 09:12 schrieb Alice Example:\n> Freitag?\n";

        assert_eq!(without_quote_markers(text), "Passt.");
    }

    #[test]
    fn everything_from_an_outlook_header_block_goes() {
        for text in [
            "Done.\n\n________________________________\nFrom: Bob\nSent: Monday, 13 January 2025 09:12\nTo: Alice\nSubject: Report\n\nPlease send it.",
            "Erledigt.\n\nVon: Bob <bob@example.com>\nGesendet: Montag, 13. Januar 2025 09:12\nAn: Alice\n\nBitte schicken.",
            "Done.\n\n-----Original Message-----\nPlease send it.",
            "Erledigt.\n\n-----Ursprüngliche Nachricht-----\nBitte schicken.",
        ] {
            let stripped = without_quote_markers(text);
            assert!(
                stripped == "Done." || stripped == "Erledigt.",
                "{stripped:?}"
            );
        }
    }

    #[test]
    fn a_from_line_without_a_date_after_it_stays() {
        let text = "From: the desk of Alice\nGreetings.";

        assert_eq!(without_quote_markers(text), text);
    }

    #[test]
    fn runs_of_three_lines_an_earlier_mail_has_go() {
        let earlier = "Hi Bob,\n\nthe report is attached.\nIt covers January\nand February.\n\nThanks!\nAlice";
        let reply = "Great, thanks!\nI'll read it tonight.\n\nthe report is attached.\nIt  covers January\n> and February.\n\nCheers!\nBob";

        assert_eq!(
            without_repeated_runs(reply, &[earlier]),
            "Great, thanks!\nI'll read it tonight.\n\nCheers!\nBob"
        );
    }

    #[test]
    fn html_quote_containers_go() {
        let gmail = r#"<div dir="ltr">Friday works.</div><br><div class="gmail_quote"><div class="gmail_attr">On Mon, Alice wrote:</div><blockquote class="gmail_quote">Friday?</blockquote></div>"#;
        let apple = r#"<html><body>Friday works.<div><br><blockquote type="cite"><div>Friday?</div></blockquote></div></body></html>"#;
        let outlook = r#"<div>Friday works.</div><div id="appendonsend"></div><hr><div id="divRplyFwdMsg"><b>From:</b> Alice<br><b>Sent:</b> Monday</div><div>Friday?</div>"#;

        for html in [gmail, apple, outlook] {
            let text = html_text(html, false);
            assert_eq!(
                tidy(&text.lines().collect::<Vec<_>>()),
                "Friday works.",
                "{html}"
            );
        }
    }
}
