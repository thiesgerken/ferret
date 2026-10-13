# Plugins

## TLDR

A plugin turns one kind of origin into documents. It decides what a document is, the server only
chunks and indexes. v1 ships the bare minimum: **git**, **Paperless** and **carapace**. Further
plugins follow continuously, see [Roadmap](#roadmap).

## Contract

- **One plugin instance owns a source root** and may write into any source below it. The carapace
  plugin with root `carapace` writes `carapace/sessions`, `carapace/digests` and `carapace/facts`.
  Source names are paths, see [README](README.md#sources).
- **Ingest goes through the public REST API** with the run's token, which is scoped to the root and
  everything below it ([auth.md](auth.md#plugin-run-tokens)).
- **Incremental state** (last commit, last `modified` timestamp) lives in the instance's `state` and
  is only stored on success ([architecture.md](architecture.md#run-lifecycle)).
- **Deletions**, depending on what the origin offers:
  - The origin reports them (git diff): explicit `DELETE`.
  - The origin can list what exists (Paperless ids, IMAP UIDs): a sync, `seen` for every existing
    id, `PUT` for the changed ones, commit.
  - The origin is append-only (chat exports): no deletions.
- **External ids are stable** and derived from the origin's own ids, never from positions or
  timestamps of a run.

## v1

### git

Indexes files from a git repository: Obsidian vaults, notes repos, code docs.

| Config | Meaning |
|---|---|
| `remote`, `branch`, `token` | repository to clone (token write-only) |
| `include`, `exclude` | globs, e.g. `**/*.md`, `.obsidian/**` |
| `content_types` | glob to content type, default `*.md` → `markdown`, others `plain` |
| `url_template` | deep link, e.g. `obsidian://open?vault=main&file={path}` or `https://git.example.com/notes/src/commit/{commit}/{path}` |

- One document per file, `external_id = {path}`, `title` from the first heading or the file name,
  `authors` from the last commit touching the file, `created_at` from the first.
- First run: full ingest inside a sync, commit at the end. Later runs:
  `git diff --name-status <cursor>..HEAD`, `PUT` changed files, `DELETE` removed ones, renames are
  both. Cursor = `HEAD`.
- If the cursor is no longer reachable (force push), the plugin falls back to a full sync.

### Paperless

- One document per Paperless document, source `paperless`, `content_type = plain` with the OCR
  text from `content`.
- `meta`: correspondent, document type, tags, archive serial number. `created_at` = Paperless'
  `created` date. `url` = `{paperless}/documents/{id}/details`.
- Every run: a sync, list all ids (`/api/documents/?fields=id,modified`), `seen` for all of them,
  `PUT` the ones modified after the cursor, commit. Deleted documents disappear with the sweep.
- Auth: a Paperless API token (write-only config field).

### carapace

Reads the user's carapace knowledge repo through its upstream remote. ferret never talks to
carapace itself.

**v1 parses the rendered Markdown** of the memory mirror (`src/carapace/memory/render.py`):

- YAML front matter: `kind`, `session_id`, `week_key`, `month_key`, `created_at`, `provenance`
  (digests: `period_key`, `coverage`).
- Body sections by heading: abstract (prose under the title), `## Outcomes`, `## Open loops`,
  `## On my mind`, `## Facts` with one `### <category>` per category, `## Friction`, `## Tags`.
- Fact lines: `- **<subject>**: <statement> _(<source_kind> · <confidence> confidence ·
  <durability or "dated until <date>"> · #<seq> ...)_`.

This couples the plugin to carapace's render format. Parser tests run against fixtures rendered
by carapace, and a line that doesn't parse fails the run instead of being skipped.

**v2: JSON sidecars.** The mirror writes `ExtractionRecord` and `DigestRecord` as
`model_dump_json()` next to every Markdown file, and the plugin switches to those. carapace can then
change its rendering freely.

| Source | Documents | `external_id` | Text |
|---|---|---|---|
| `carapace/sessions` | one per session | `{session_id}` | abstract, outcomes, open loops, on my mind |
| `carapace/facts` | one per fact | `{session_id}/{hash of subject and statement}` | the fact, plus subject and category |
| `carapace/digests` | one per week and month | `{level}:{period_key}` | summary, highlights, open loops |
| `carapace/files` | everything else in the repo (skills, workspace notes) | `{path}` | as in the git plugin |

- Fact `meta`: `category`, `subject`, `source_kind`, `confidence`, `durability`, `valid_until`,
  `session_id`. Session and digest `meta`: week and month keys, provenance.
- `memory/facts/*.md` is ignored: it repeats the facts of the session files.
- Facts have no id of their own in carapace (`#<seq>` refers to source messages, several facts
  share them). The hash of subject and statement is stable as long as the fact is, and a reworded
  fact is a different fact.
- Incremental like the git plugin. A changed session file can lose facts: the plugin compares the
  fact ids of the old version (`git show <cursor>:<path>`) with the new one and deletes the missing
  ones.
- `url`: carapace's memory view of the session or period (route to be checked), files link to
  carapace's knowledge view.
- The git and carapace plugins share the repository handling (clone, fetch, diff) in one module.

## Roadmap

In rough order of value per effort:

1. **Email (IMAP)**: one document per mail, `thread` = root Message-ID, quoted replies stripped.
   Sources per folder, e.g. `mail/inbox`, `mail/sent`. Deletions via UID listing.
2. **Chats** (WhatsApp and Signal exports), see [Chats](#chats).
3. **AFFiNE** (self-hosted): there is no official Markdown export, documents are Yjs CRDTs. lumi
   already talks to AFFiNE through an MCP server, which shows a working API and auth path. Worst
   case, the MCP server is the API.
4. **Saved articles** (Karakeep or Wallabag). Only what I read or saved, no news firehose.

### Chats

A chat plugin cuts a conversation into documents, one per **segment**:

1. `external_id = {channel}:{first message id}`, `thread = {channel}`, `content_type = plain`.
2. One open segment per channel. A message within the pause (~1 h) of the previous one is appended,
   and the plugin re-sends the whole segment.
3. A segment closes after a pause or at a size cap (~8k characters, a few chunks). A closed segment
   never changes again, so late messages can't shift boundaries and produce duplicates (chats are
   append-only, no sync sweep would clean them up).
4. A segment under ~200 characters stays open across a pause, so a lone "ok" never becomes its own
   document.
5. The plugin state holds the cursor (last message id, open segment). Messages at or before the
   cursor are skipped, so overlapping exports are harmless.
6. The plugin renders the text: a header line (channel, participants, date), optionally the last
   message of the previous segment as context, then one line per message (`[22:14] Alice: ...`).

The parameters are guesses. They are plugin config, tunable once there are relevance judgments.
The viewer navigates to the previous and next segment through `thread`.

Exports as the only copy: keep the raw zips until the importer has proven itself on real data
([README](README.md#backups)).
