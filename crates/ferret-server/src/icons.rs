//! Icons of connections and API keys: lucide names the UI renders (`git-branch`, `key-round`).
//! The server checks the format only, the UI shows a neutral icon for a name it doesn't know.

const MAX_CHARS: usize = 64;

/// Kebab-case, as the database checks it too.
pub fn is_valid(name: &str) -> bool {
    name.len() <= MAX_CHARS
        && !name.is_empty()
        && name.split('-').all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takes_kebab_case_names_only() {
        for valid in ["brain", "git-branch", "key-round", "building-2"] {
            assert!(is_valid(valid), "{valid}");
        }
        for invalid in [
            "",
            "Brain",
            "git_branch",
            "-brain",
            "brain-",
            "git--branch",
            &"a".repeat(65),
        ] {
            assert!(!is_valid(invalid), "{invalid}");
        }
    }
}
