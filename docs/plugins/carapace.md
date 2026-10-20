# carapace knowledge repositories

Use `carapace` for the Git knowledge repository pushed by carapace, not its live database or a generic folder of session dumps. The worker needs read access to the repository and the human needs access to carapace's web UI. A public HTTPS repository needs no token; for a private one use a repository-scoped read token.

## Set up

Create plugin `carapace`, source `carapace`, a name/icon and schedule `0 * * * *`:

```json
{
  "remote": "https://git.example.com/me/knowledge.git",
  "branch": "main",
  "token": "replace-with-read-only-repository-token",
  "carapace_url": "https://carapace.example.com"
}
```

`remote` and `carapace_url` are required. Omit `branch` to use the remote default; omit `token` for a public repository. The token uses the same HTTPS transport as the [git plugin](git.md). The URL must be the UI address users can open, not an API credential endpoint.

Changing `branch` or the browser-only `carapace_url` preserves the stored token. Changing `remote` requires re-entering it, even alongside safe settings.

## What becomes searchable

For a connection root `carapace`:

| Repository content | Source | External identity and origin |
| --- | --- | --- |
| `memory/sessions/*.md` | `carapace/sessions` | session id; `/memory/sessions?session=<id>` |
| facts extracted from those sessions | `carapace/facts` | session id plus subject/statement hash; same session link |
| `memory/weeks/*.md`, `memory/months/*.md` | `carapace/digests` | level plus period key; `/memory/timeline?period=<key>` |
| Markdown below `skills/` | `carapace/skills` | repository path; `/knowledge?path=<path>` |
| other Markdown | `carapace/files` | repository path; `/knowledge?path=<path>` |

Session text contains abstract, outcomes, open loops and on-my-mind sections. Facts carry category, subject and provenance metadata; duplicate identical facts within one session are deduplicated. Sessions/facts/digests have no authors and use their mirror timestamps. Other Markdown uses Git history, and skill titles prefer the front matter `name`. No threads are created. `memory/facts/` is ignored because it repeats session facts. Non-Markdown files, including raw conversation JSON, are not indexed.

## Runs, deletions and limits

The initial run fully enumerates and sweeps the root. Incremental runs compare commits, put current documents from changed files, and delete identities those files used to contain but no longer do. This removes deleted sessions and reworded facts as well as removed files. A missing ancestor, changed indexing rules or changed carapace URL forces a full sync.

The plugin expects carapace's supported Markdown mirror format. An unreadable session/digest fails the run rather than silently deleting its old documents. Ordinary files have the git plugin's UTF-8/regular-file/1 MiB limits. Only pushed commits are visible; a carapace session not yet mirrored and pushed cannot appear. Transport restrictions match [git](git.md).

After creation inspect Activity for a successful clone and counts, then search a known session title under `carapace/sessions`. Confirm its origin opens the correct session rather than only carapace's home page. If counts are unexpectedly low, inspect the repository's committed mirror before changing Ferret filters.

See [connections](../connections.md). Source: [mapping and sync](../../crates/ferret-plugins/src/plugins/carapace/mod.rs), [mirror parser](../../crates/ferret-plugins/src/plugins/carapace/mirror.rs).
