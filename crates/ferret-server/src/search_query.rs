//! The search query as typed: words, `"exact phrases"` and `-excluded` words. Filters are not
//! part of it, they arrive as separate parameters.

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ParsedQuery {
    /// The positive words, the words of the phrases included, each once ignoring case: a
    /// repeated word would count twice towards the words a keyword hit needs.
    pub words: Vec<String>,
    /// Must appear in the hit's chunk as they are, ignoring case and whitespace differences.
    pub phrases: Vec<String>,
    /// Documents containing any of these are no hits.
    pub excluded: Vec<String>,
}

/// Shorter words have too many neighbours one typo away, and numbers or IDs are meant exactly.
const MIN_TYPO_TOLERANT_CHARS: usize = 5;

impl ParsedQuery {
    /// Whether `word` is part of a phrase, which already requires it in the hit.
    pub fn in_phrase(&self, word: &str) -> bool {
        let word = word.to_lowercase();
        self.phrases
            .iter()
            .any(|phrase| phrase.split(' ').any(|part| part.to_lowercase() == word))
    }

    /// The words that also match one typo away: those of 5 or more letters, outside phrases,
    /// which match exactly.
    pub fn typo_tolerant_words(&self) -> Vec<&str> {
        self.words
            .iter()
            .map(String::as_str)
            .filter(|word| {
                word.chars().count() >= MIN_TYPO_TOLERANT_CHARS
                    && word.chars().all(char::is_alphabetic)
                    && !self.in_phrase(word)
            })
            .collect()
    }

    fn push_word(&mut self, word: &str) {
        let lowercase = word.to_lowercase();
        if !self
            .words
            .iter()
            .any(|known| known.to_lowercase() == lowercase)
        {
            self.words.push(word.to_owned());
        }
    }
}

pub fn parse(query: &str) -> ParsedQuery {
    let mut parsed = ParsedQuery::default();
    let mut rest = query.trim_start();
    while !rest.is_empty() {
        if let Some(quoted) = rest.strip_prefix('"') {
            // An unclosed quote runs to the end of the query.
            let (phrase, after) = quoted.split_once('"').unwrap_or((quoted, ""));
            let words: Vec<_> = phrase.split_whitespace().collect();
            if !words.is_empty() {
                parsed.phrases.push(words.join(" "));
                for word in words {
                    parsed.push_word(word);
                }
            }
            rest = after;
        } else {
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            let (token, after) = rest.split_at(end);
            match token.strip_prefix('-') {
                Some(excluded) if !excluded.is_empty() => parsed.excluded.push(excluded.to_owned()),
                Some(_) => {}
                None => parsed.push_word(token),
            }
            rest = after;
        }
        rest = rest.trim_start();
    }
    parsed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn words_phrases_and_exclusions() {
        assert_eq!(
            parse(r#" Kfz-Versicherung  "running   shoes" -Haus 2026 "#),
            ParsedQuery {
                words: strings(&["Kfz-Versicherung", "running", "shoes", "2026"]),
                phrases: strings(&["running shoes"]),
                excluded: strings(&["Haus"]),
            }
        );
    }

    #[test]
    fn edge_cases() {
        assert_eq!(parse(""), ParsedQuery::default());
        assert_eq!(parse(r#"- "" "  ""#), ParsedQuery::default());
        assert_eq!(
            parse(r#"a "unclosed phrase"#),
            ParsedQuery {
                words: strings(&["a", "unclosed", "phrase"]),
                phrases: strings(&["unclosed phrase"]),
                excluded: vec![],
            }
        );
        assert_eq!(parse("-only -excluded").words, Vec::<String>::new());
    }

    #[test]
    fn each_word_once_ignoring_case() {
        assert_eq!(
            parse(r#"Rechnung rechnung "RECHNUNG März" märz"#).words,
            strings(&["Rechnung", "März"])
        );
    }

    #[test]
    fn typo_tolerant_words_are_long_letter_runs_outside_phrases() {
        assert_eq!(
            parse(
                r#"Rechnung Kündigung haus 2026 Kfz-Versicherung invoice42 -Steuer "running shoes""#
            )
            .typo_tolerant_words(),
            ["Rechnung", "Kündigung"]
        );
    }
}
