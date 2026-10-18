//! Source paths: `/`-separated namespaces like `mail/personal`, where a source includes every source
//! below it.

/// Whether `path` is a well-formed source path: non-empty segments, no leading, trailing or
/// doubled `/`, no surrounding whitespace.
pub fn is_valid(path: &str) -> bool {
    path == path.trim() && !path.is_empty() && path.split('/').all(|segment| !segment.is_empty())
}

/// Whether `root` is `path` or one of its ancestors, compared segment-wise: `carapace` covers
/// `carapace/facts` but not `carapace-old`.
pub fn covers(root: &str, path: &str) -> bool {
    path.strip_prefix(root)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

/// The roots a search covers: the `requested` ones (empty: all), narrowed to the `allowed`
/// ones (`None`: all). `Some(vec![])` means nothing is left to search, `None` means everything.
pub fn narrow(requested: &[String], allowed: Option<&[String]>) -> Option<Vec<String>> {
    let Some(allowed) = allowed else {
        return (!requested.is_empty()).then(|| requested.to_vec());
    };
    if requested.is_empty() {
        return Some(allowed.to_vec());
    }
    let mut roots: Vec<String> = Vec::new();
    for root in requested {
        if allowed.iter().any(|allowed| covers(allowed, root)) {
            roots.push(root.clone());
        } else {
            // Asking for `notes` with a key for `notes/work` searches `notes/work`.
            roots.extend(
                allowed
                    .iter()
                    .filter(|allowed| covers(root, allowed))
                    .cloned(),
            );
        }
    }
    Some(roots)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_paths() {
        for path in ["notes", "mail/personal", "carapace/facts/2026"] {
            assert!(is_valid(path), "{path}");
        }
        for path in ["", "/notes", "notes/", "mail//personal", " notes", "notes "] {
            assert!(!is_valid(path), "{path:?}");
        }
    }

    #[test]
    fn narrows_requested_roots_to_allowed_ones() {
        let strings = |values: &[&str]| values.iter().map(|v| v.to_string()).collect::<Vec<_>>();
        let narrow = |requested: &[&str], allowed: Option<&[&str]>| {
            narrow(&strings(requested), allowed.map(strings).as_deref())
        };

        assert_eq!(narrow(&[], None), None);
        assert_eq!(narrow(&["notes"], None), Some(strings(&["notes"])));
        assert_eq!(narrow(&[], Some(&["notes"])), Some(strings(&["notes"])));
        assert_eq!(
            narrow(&["notes/work", "mail"], Some(&["notes"])),
            Some(strings(&["notes/work"]))
        );
        assert_eq!(
            narrow(&["notes"], Some(&["notes/work", "notes/home", "mail"])),
            Some(strings(&["notes/work", "notes/home"]))
        );
        assert_eq!(narrow(&["mail"], Some(&["notes"])), Some(vec![]));
    }

    #[test]
    fn covers_by_segment() {
        assert!(covers("carapace", "carapace"));
        assert!(covers("carapace", "carapace/facts"));
        assert!(covers("mail/personal", "mail/personal/inbox"));
        assert!(!covers("carapace", "carapace-old"));
        assert!(!covers("carapace/facts", "carapace"));
        assert!(!covers("mail/personal", "mail/other"));
    }

    /// Search filters with the SQL twin of `covers`, which must not drift from it.
    #[sqlx::test(migrations = false)]
    async fn the_sql_function_agrees_with_covers(pool: sqlx::PgPool) {
        crate::migrate(&pool).await.unwrap();
        let roots = ["notes", "notes/work", "mail/personal", "carapace"];
        let paths = [
            "notes",
            "notes/work",
            "notes/work/a",
            "notes-old",
            "notesx/a",
            "mail/personal/inbox",
            "mail/other",
            "carapace-old",
            "carapace/facts",
        ];
        for root in roots {
            for path in paths {
                let in_sql: bool = sqlx::query_scalar("select source_covers($1, $2)")
                    .bind(root)
                    .bind(path)
                    .fetch_one(&pool)
                    .await
                    .unwrap();
                assert_eq!(in_sql, covers(root, path), "{root} covers {path}");
            }
        }
    }
}
