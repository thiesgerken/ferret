//! Marks the words of a chunk that match the query, approximating what the BM25 fields matched:
//! exactly the same word, a German or English stem variant, or a compound containing it, else
//! fuzzily a word one typo away from a typo-tolerant query word.

use ferret_api::{HighlightKind, TextPart};
use rust_stemmers::{Algorithm, Stemmer};
use strsim::osa_distance;

/// Below this length, a term only highlights whole words: "an" shouldn't light up every "and".
const MIN_INFIX_CHARS: usize = 3;

pub fn highlight(text: &str, query_words: &[String], typo_tolerant: &[&str]) -> Vec<TextPart> {
    let stemmers = [
        Stemmer::create(Algorithm::German),
        Stemmer::create(Algorithm::English),
    ];
    let terms: Vec<String> = query_words
        .iter()
        .flat_map(|word| word_spans(word).map(|(start, end)| word[start..end].to_lowercase()))
        .collect();
    let typo_tolerant: Vec<String> = typo_tolerant
        .iter()
        .map(|word| word.to_lowercase())
        .collect();
    let kind = |word: &str| {
        let word = word.to_lowercase();
        if terms.iter().any(|term| {
            word == *term
                || (term.chars().count() >= MIN_INFIX_CHARS && word.contains(term.as_str()))
                || stemmers
                    .iter()
                    .any(|stemmer| stemmer.stem(&word) == stemmer.stem(term))
        }) {
            Some(HighlightKind::Exact)
        } else if typo_tolerant
            .iter()
            .any(|term| osa_distance(&word, term) <= 1)
        {
            Some(HighlightKind::Fuzzy)
        } else {
            None
        }
    };

    let mut parts: Vec<TextPart> = Vec::new();
    let mut push = |text: &str, highlight: Option<HighlightKind>| match parts.last_mut() {
        Some(last) if last.highlight == highlight => last.text.push_str(text),
        _ if !text.is_empty() => parts.push(TextPart {
            text: text.to_owned(),
            highlight,
        }),
        _ => {}
    };
    let mut position = 0;
    for (start, end) in word_spans(text) {
        push(&text[position..start], None);
        push(&text[start..end], kind(&text[start..end]));
        position = end;
    }
    push(&text[position..], None);
    parts
}

/// Byte ranges of the maximal alphanumeric runs, the words as the BM25 tokenizer sees them.
fn word_spans(text: &str) -> impl Iterator<Item = (usize, usize)> + '_ {
    let mut chars = text.char_indices().peekable();
    std::iter::from_fn(move || {
        let (start, _) = chars.find(|(_, c)| c.is_alphanumeric())?;
        let mut end = text.len();
        while let Some(&(index, c)) = chars.peek() {
            if !c.is_alphanumeric() {
                end = index;
                break;
            }
            chars.next();
        }
        Some((start, end))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn highlighted(text: &str, query: &[&str]) -> Vec<String> {
        let words: Vec<String> = query.iter().map(|word| word.to_string()).collect();
        highlight(text, &words, &[])
            .into_iter()
            .filter(|part| part.highlight.is_some())
            .map(|part| part.text)
            .collect()
    }

    #[test]
    fn same_word_stems_and_compounds() {
        assert_eq!(
            highlighted(
                "Ihr Kfz-Versicherungsbeitrag für 2026: die Versicherungen zahlen.",
                &["Versicherung", "2026"]
            ),
            ["Versicherungsbeitrag", "2026", "Versicherungen"]
        );
        assert_eq!(
            highlighted("He runs. The runner was running.", &["running"]),
            ["runs", "running"]
        );
    }

    #[test]
    fn short_terms_only_match_whole_words() {
        assert_eq!(highlighted("an and an", &["an"]), ["an", "an"]);
    }

    #[test]
    fn parts_cover_the_whole_text() {
        let text = "  Grüße, Köln!  ";
        let parts = highlight(text, &["köln".to_string()], &[]);

        assert_eq!(
            parts
                .iter()
                .map(|part| part.text.as_str())
                .collect::<String>(),
            text
        );
        assert_eq!(
            parts,
            [
                TextPart {
                    text: "  Grüße, ".to_string(),
                    highlight: None
                },
                TextPart {
                    text: "Köln".to_string(),
                    highlight: Some(HighlightKind::Exact)
                },
                TextPart {
                    text: "!  ".to_string(),
                    highlight: None
                },
            ]
        );
    }

    #[test]
    fn typo_tolerant_words_also_highlight_one_typo_away() {
        let text =
            "Ihre Rechnung und die Kündigung: we recieve the invoice, Rechnungen and Rechner.";
        let words = ["Rechnug", "Kündigng", "receive", "invoice"].map(str::to_owned);
        let kinds: Vec<(String, HighlightKind)> =
            highlight(text, &words, &["Rechnug", "Kündigng", "receive", "invoice"])
                .into_iter()
                .filter_map(|part| Some((part.text, part.highlight?)))
                .collect();

        assert_eq!(
            kinds,
            [
                ("Rechnung".to_owned(), HighlightKind::Fuzzy),
                ("Kündigung".to_owned(), HighlightKind::Fuzzy),
                ("recieve".to_owned(), HighlightKind::Fuzzy),
                ("invoice".to_owned(), HighlightKind::Exact),
            ]
        );
    }

    #[test]
    fn without_typo_tolerance_typos_dont_highlight() {
        assert!(highlighted("Ihre Rechnung", &["Rechnug"]).is_empty());
    }
}
