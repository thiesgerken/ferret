# Gitea repositories and discussions

Use `gitea` to automatically index the repositories associated with one Gitea account, including repositories available through collaboration and organization/team membership. One connection discovers new repositories on its next scheduled run; it does not create individual Git connections. Unrelated public repositories are not crawled. For a single manually selected remote, use [git](git.md).

## Set up

Create a read-only Gitea token with `read:user`, `read:repository` and `read:issue` scopes and access to the repositories you want indexed. Account, organization/team and collaborator discovery was verified against Gitea 1.24.7. The worker needs Git and network access to the instance.

In **Connections**, create plugin `gitea`, choose a source root such as `gitea/work`, a name/icon and a schedule such as `0 * * * *`. Enter:

```json
{
  "url": "https://git.example.com",
  "token": "replace-with-read-only-token",
  "include_repos": ["acme/*", "alice/notes"],
  "exclude_repos": ["*/sandbox-*"],
  "include": ["**/*.md", "**/*.txt"],
  "exclude": ["vendor/**", "private/**"],
  "markdown": ["**/*.md"]
}
```

Only `url` and `token` are required. `url` is the instance address as the worker reaches it, including any installation subpath, not an `/api/v1` URL. It must be HTTP(S), without embedded credentials, query or fragment. The API must be reachable without a login proxy redirect. The token is write-only and encrypted by the server.

Repository patterns match current `owner/repo`; file patterns match repository-relative paths. Empty/omitted includes select everything in their respective scope; exclusions win. Forks and archived repositories are eligible unless a repository pattern excludes them. File patterns do not filter issue/PR text. `markdown` defaults to `**/*.md`. Unknown configuration fields are rejected.

Git transport defaults to HTTPS. An HTTP instance also needs the worker's `FERRET_GIT_PROTOCOLS` to include `http`, just as for the standalone Git plugin. API requests do not follow redirects or origin-supplied pagination URLs; clone URLs are built from the configured instance rather than trusting a different clone host returned by the API.

## Sources, documents and threads

For repository `acme/notes`, a connection rooted at `gitea/work` writes:

| Source | Documents | External IDs |
|---|---|---|
| `gitea/work/acme/notes/files` | Regular UTF-8 text files on the current default branch | Repository-relative file path |
| `gitea/work/acme/notes/issues` | Open issue bodies and ordinary comments | `body:<issue-id>`, `comment:<comment-id>` |
| `gitea/work/acme/notes/pulls` | Open PR bodies, ordinary comments, submitted review bodies and inline review comments | `body:<issue-id>`, `comment:<comment-id>`, `review:<review-id>`, `review-comment:<comment-id>` |

The file side reuses the Git plugin's 1 MiB limit, binary/symlink/submodule checks, Markdown titles, author history and timestamps. No other branches or Git history documents are indexed. File origin links point at the indexed commit.

Each issue/PR has one thread, identified by its repository-local number within its exact source. Discussion documents are Markdown, titled with `owner/repo #number: title`, with the origin author's login and creation/modification timestamps. Bodies without text use their issue/PR title so they remain searchable. Empty comments and empty review bodies are not indexed. Each document links to its body, comment or review in Gitea. Inline-comment metadata carries the path and current/original positions.

Open draft PRs are included. Pending/unsubmitted reviews and review-request events are not. Resolved or outdated inline comments on an open PR remain discussion content. No diffs, surrounding branch code, attachments, reactions or automatic timeline events are indexed; diff hunks supplied in review-comment API responses are discarded. Code blocks written in a body/comment remain part of that text.

## Synchronization and removal

Every run completely enumerates account-associated repositories and current open discussions/comments/reviews. Files use a successful commit/settings cursor per repository: unchanged selected paths are marked seen, changed files are read and put, and changed settings or missing ancestor commits cause a full file sync. Discussion edits are reflected even when the Git commit is unchanged.

One sync covers the entire connection root. Only after discovery and all selected repository/category reads and writes succeed does its sweep remove content that is no longer eligible:

- Deleted files, files no longer indexable and deleted/empty comments.
- Closed issues and closed/merged PRs, including their entire threads.
- Repositories no longer returned by complete account discovery or no longer matching the repository patterns.
- Old source paths after a repository rename or transfer.

Reopened discussions and newly/restored accessible repositories are indexed automatically. A complete successful discovery result is authoritative; the plugin makes no extra repository-ID confirmation requests. Renames/transfers are new namespaces, not migrations: document IDs can change and content can be re-embedded. Manual exclusions and saved source filters remain attached to the old source; they are not silently moved.

An error fails the run but independent repositories/categories are still attempted. Earlier puts can remain visible. No sweep is committed and no proposed cursor is saved after a discovery/read/write failure. One persistently failing repository can therefore delay removals elsewhere, including closed discussions or content whose access was revoked. The sweep itself is not a transaction with earlier puts.

## Verify and troubleshoot

Open **Activity** and inspect repository names, full-sync/diff reasons and put/unchanged/deleted counts. Search a known file title and an open discussion phrase in keyword mode under the connection root. Open a discussion in Ferret's viewer to navigate its thread, then check **Open in origin**. An unchanged repeat should report zero puts. Close a test issue and verify its thread disappears after a successful run.

A failed run retains stale content deliberately. Check token scopes/access, instance reachability, Git transport configuration and the contextual repository/category error before retrying. A failed or incomplete paginated listing is never interpreted as an empty origin. Review enumeration uses one API slot per page because Gitea counts hidden pending reviews but omits them from responses and does not provide review pagination links.

See [connection lifecycle](../connections.md). Implementation: [gitea.rs](../../crates/ferret-plugins/src/plugins/gitea.rs), [shared Git indexing](../../crates/ferret-plugins/src/plugins/git.rs), [repository transport](../../crates/ferret-plugins/src/plugins/repo.rs). Design and implementation plan: [#145](https://github.com/thiesgerken/ferret/issues/145).
