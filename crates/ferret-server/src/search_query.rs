//! The search query as typed: words, `"exact phrases"` and `-excluded` words. Filters are not
//! part of it, they arrive as separate parameters.

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ParsedQuery {
    /// Every word a hit must match, the words of the phrases included.
    pub words: Vec<String>,
    /// Must appear in the hit's chunk as they are, ignoring case and whitespace differences.
    pub phrases: Vec<String>,
    /// Documents containing any of these are no hits.
    pub excluded: Vec<String>,
}

pub fn parse(query: &str) -> ParsedQuery {
    let mut parsed = ParsedQuery::default();
    let mut rest = query.trim_start();
    while !rest.is_empty() {
        if let Some(quoted) = rest.strip_prefix('"') {
            // An unclosed quote runs to the end of the query.
            let (phrase, after) = quoted.split_once('"').unwrap_or((quoted, ""));
            let words: Vec<_> = phrase.split_whitespace().map(str::to_owned).collect();
            if !words.is_empty() {
                parsed.phrases.push(words.join(" "));
                parsed.words.extend(words);
            }
            rest = after;
        } else {
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            let (token, after) = rest.split_at(end);
            match token.strip_prefix('-') {
                Some(excluded) if !excluded.is_empty() => parsed.excluded.push(excluded.to_owned()),
                Some(_) => {}
                None => parsed.words.push(token.to_owned()),
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
}
