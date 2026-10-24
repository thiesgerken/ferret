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
| `memory/sessions/*.md` | `carapace/sessions` | `summary:<session id>`; `/memory/sessions?session=<id>` |
| each `conversation.json` (session archive), one document per turn | `carapace/sessions` | `turn:<session id>:<seq>`; `/?session=<id>` |
| facts extracted from those sessions | `carapace/facts` | session id plus subject/statement hash; same session link |
| `memory/weeks/*.md`, `memory/months/*.md` | `carapace/digests` | level plus period key; `/memory/timeline?period=<key>` |
| Markdown below `skills/` | `carapace/skills` | repository path; `/knowledge?path=<path>` |
| other Markdown | `carapace/files` | repository path; `/knowledge?path=<path>` |

A session's summary contains abstract, outcomes, open loops and on-my-mind sections. Facts carry category, subject and provenance metadata; duplicate identical facts within one session are deduplicated. Summaries/facts/digests have no authors and use their mirror timestamps, so a summary sorts after its session's turns.

A turn is a user message and everything up to the next one, rendered as Markdown: the user's message (with a line per attachment), then the agent's messages and tool calls with a timestamp and model header. A tool call shows its command, path or arguments and whether it succeeded with how many lines/bytes came back, never its output. Thinking, approvals, carapace's security checks and slash commands the server answered are left out, as is anything before the first user message. A turn is titled by the user message, its authors are the models that answered in it, `created_at` is when the message was sent (empty for events carapace stored before it stamped them) and `modified_at` the turn's latest event. Its `meta` has `session`, `session_title`, `session_started`, `channel_type` and the session's `models`.

Summary and turns of a session share its thread (the session id), so hits collapse to one per session; each turn's parent is the turn before it. Archives are found by the file name `conversation.json`, wherever carapace's `path_prefix` puts them.

Other Markdown uses Git history, and skill titles prefer the front matter `name`. `memory/facts/` is ignored because it repeats session facts. Other non-Markdown files are not indexed.

## Runs, deletions and limits

The initial run fully enumerates and sweeps the root. Incremental runs compare commits, put current documents from changed files, and delete identities those files used to contain but no longer do. This removes deleted sessions, turns a rewritten archive no longer has and reworded facts as well as removed files. A missing ancestor, changed indexing rules or changed carapace URL forces a full sync.

The plugin expects carapace's supported Markdown mirror format and archive `schema_version` 1. An unreadable session/digest/archive, or an archive of another schema version, fails the run rather than silently deleting its old documents. Ordinary files have the git plugin's UTF-8/regular-file/1 MiB limits; archives are exempt from the size limit, but a turn's text is cut at 1,000,000 bytes (marked `…(truncated)`) so that one oversized message can't fail every run. Only pushed commits are visible; a carapace session not yet mirrored (summary) or archived (turns) and pushed cannot appear. carapace never archives private sessions. Transport restrictions match [git](git.md).

After creation inspect Activity for a successful clone and counts, then search a known session title under `carapace/sessions`. Confirm its origin opens the correct session rather than only carapace's home page. If counts are unexpectedly low, inspect the repository's committed mirror before changing Ferret filters.

See [connections](../connections.md). Source: [mapping and sync](../../crates/ferret-plugins/src/plugins/carapace/mod.rs), [mirror parser](../../crates/ferret-plugins/src/plugins/carapace/mirror.rs), [archive parser](../../crates/ferret-plugins/src/plugins/carapace/archive.rs), [turn rendering](../../crates/ferret-plugins/src/plugins/carapace/transcript.rs).
