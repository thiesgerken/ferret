//! Source paths: `/`-separated namespaces like `mail/zoho`, where a source includes every source
//! below it.

/// Whether `path` is a well-formed source path: non-empty segments, no leading, trailing or
/// doubled `/`, no surrounding whitespace.
pub fn is_valid(path: &str) -> bool {
    path == path.trim() && !path.is_empty() && path.split('/').all(|segment| !segment.is_empty())
}

/// Whether `root` is `path` or one of its ancestors, compared segment-wise: `carapace` covers
/// `carapace/facts` but not `carapace-old`.
#[allow(dead_code, reason = "first read by ingest and search (#4)")]
pub fn covers(root: &str, path: &str) -> bool {
    path.strip_prefix(root)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_paths() {
        for path in ["notes", "mail/zoho", "carapace/facts/2026"] {
            assert!(is_valid(path), "{path}");
        }
        for path in ["", "/notes", "notes/", "mail//zoho", " notes", "notes "] {
            assert!(!is_valid(path), "{path:?}");
        }
    }

    #[test]
    fn covers_by_segment() {
        assert!(covers("carapace", "carapace"));
        assert!(covers("carapace", "carapace/facts"));
        assert!(covers("mail/zoho", "mail/zoho/inbox"));
        assert!(!covers("carapace", "carapace-old"));
        assert!(!covers("carapace/facts", "carapace"));
        assert!(!covers("mail/zoho", "mail/gmail"));
    }
}
