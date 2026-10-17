# Plugins

## TLDR

A plugin turns one kind of origin into documents. It decides what a document is, the server only
chunks and indexes. v1 ships the bare minimum: **git** (repositories), **Paperless** and **carapace**. Further
plugins follow continuously, see [Roadmap](#roadmap).

## Contract

- **A connection** (a plugin configured by a user) **owns a source root** and may write into any
  source below it. The carapace
  plugin with root `carapace` writes `carapace/sessions`, `carapace/digests` and `carapace/facts`.
  Source names are paths, see [README](README.md#sources).
- **Ingest goes through the public REST API** with the run's token, which is scoped to the root and
  everything below it ([auth.md](auth.md#plugin-run-tokens)).
- **Incremental state** (last commit, last `modified` timestamp) lives in the connection's `state` and
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

Indexes files from a git repository: notes repos, code docs. Obsidian vaults synced with Syncthing
are the files plugin's job ([Roadmap](#roadmap)).

| Config | Meaning |
|---|---|
| `remote`, `branch`, `token` | repository to clone over HTTPS (token write-only, [#11](https://github.com/thiesgerken/ferret/issues/11)) |
| `include`, `exclude` | globs, e.g. `**/*.md`, `.obsidian/**` |
| `markdown` | globs of Markdown files, default `**/*.md`; everything else is `plain` |
| `url_template` | deep link with `{path}` (percent-encoded per segment) and `{commit}`, e.g. `obsidian://open?vault=main&file={path}` or `https://git.example.com/notes/src/commit/{commit}/{path}` |

- One document per file, `external_id = {path}`, `title` from the first `# ` heading (deeper ones
  don't count) or the file name, `authors` from the last commit touching the file, `created_at`
  from the first.
- Only regular files: symlinks and submodules are skipped (a symlink could point at the worker's
  own files), so are files above 1 MiB and files that aren't UTF-8 text. Each skip is a line in
  the run log.
- Remotes are limited to HTTPS, so a connection can't reach the worker's filesystem or plain-HTTP
  services in the cluster. A git command that runs longer than 30 minutes fails the run.
- With `{commit}` in `url_template`, every new commit changes every file's URL, so a full sync
  puts (and re-chunks) every file again.
- First run: full ingest inside a sync, commit at the end. Later runs:
  `git diff --name-status <cursor>..HEAD`, `PUT` changed files, `DELETE` removed ones, renames are
  both. Cursor = `HEAD`.
- If the cursor is no longer reachable (force push), the plugin falls back to a full sync.

### Paperless

- One document per Paperless document in the connection's root (e.g. `paperless`),
  `external_id` = Paperless id, `content_type = plain` with the OCR text from `content`.
- `meta`: correspondent, document type, tags (by name), archive serial number. `authors` = the
  correspondent. `created_at` = Paperless' `created` date at noon UTC (Paperless keeps only the
  date; noon keeps it in every time zone from UTC-11 to UTC+11). `url` =
  `{paperless}/documents/{id}/details`.
- Every run: a sync, list all ids (`/api/documents/?fields=id,modified`), `seen` for all of them,
  fetch the ones modified after the cursor (`id__in`, 100 at a time) and `PUT` them, commit.
  Deleted documents disappear with the sweep. Cursor = the newest `modified`.
- Renaming a tag, correspondent or document type doesn't change a document's `modified`, so its
  documents keep the old name until they change otherwise.
- Requires Paperless-ngx 2.16 or later: requests pin API version 9, where `created` is a date.
- Auth: a Paperless API token (write-only config field, `Authorization: Token …`). Objects the
  token's user can't see are left out of `meta` with a line in the run log.

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
- `url` (relative to the configured carapace base URL): sessions
  `/memory/sessions?session={session_id}`, facts link to their session, digests
  `/memory/timeline?period={period_key}`, files `/knowledge?path={path}`.
- The git and carapace plugins share the repository handling (clone, fetch, diff) in one module.

## Roadmap

In rough order of value per effort:

1. **Files** (Obsidian vaults): the vaults are Syncthing folders, not git repos. A Syncthing node
   in the cluster syncs them into a PVC, mounted read-only into a dedicated worker. Every run is a
   full sync (`PUT` everything, the server skips unchanged files), excluding `.stfolder`,
   `.stversions` and `.obsidian`.
2. **Email (IMAP)**, starting with the private Zoho mailbox, see [Email](#email). Outlook (work)
   follows as its own plugin (Microsoft Graph rather than IMAP).
3. **Chats** (WhatsApp and Signal exports), see [Chats](#chats).
4. **AFFiNE** (self-hosted): there is no official Markdown export, documents are Yjs CRDTs. lumi
   already talks to AFFiNE through an MCP server, which shows a working API and auth path. Worst
   case, the MCP server is the API.
5. **Saved articles** (Karakeep or Wallabag). Only what I read or saved, no news firehose.

### Email

IMAP, MIME parsing with [`mail-parser`](https://crates.io/crates/mail-parser), which also turns HTML
into text.

**Identity**

- `external_id` = Message-ID, so a mail in several folders is one document and a move to another
  folder is invisible to ferret.
- Source = `mail/<account>`, e.g. `mail/zoho`. Folders are not recorded in v1 (a folder filter
  would require tracking moves).
- `thread` = the first Message-ID in `References`, else `In-Reply-To`, else the mail's own id.
- `title` = subject, `authors` = From, `meta`: To, Cc, attachment names. `created_at` =
  the `Date` header.
- `url` from an optional `url_template` with `{message_id}` (e.g. `mid:{message_id}`, RFC 2392).
  Without one, ferret's viewer is the origin.

**Sync and deletions**

- Per folder, `UIDVALIDITY` and the last UID are the cursor. New mails come in through
  `UID FETCH`. A changed `UIDVALIDITY` refetches that folder.
- Every run is a sync over the account: fetch the Message-IDs of all UIDs (`ENVELOPE`, a few MB for
  50k mails), `seen` for all of them, `PUT` the new ones, commit. Deleted mails disappear with the
  sweep.
- Trash, Spam and Drafts are excluded (configurable), so a mail moved to Trash counts as deleted.
- No noise filter: unwanted mails get deleted at the origin.

**Stripping quoted text**, in this order:

1. **HTML quote containers** before the HTML-to-text conversion: `blockquote[type=cite]` (Apple
   Mail, Thunderbird), `div.gmail_quote`, Outlook's `#divRplyFwdMsg` and reply header block.
2. **Text markers**, German and English: lines starting with `>`, `On ... wrote:` /
   `Am ... schrieb ...:`, `-----Original Message-----` / `-----Ursprüngliche Nachricht-----`,
   `From:`/`Sent:` and `Von:`/`Gesendet:` header blocks.
3. **Thread diff**: the plugin processes a thread's new mails in date order and loads the thread's
   earlier mails from ferret (`GET /api/documents?source=&thread=`, already stripped). From each new
   mail it removes every run of at least 3 consecutive lines that already appeared in an earlier
   mail of the thread, after normalizing (`>` removed, whitespace collapsed). Diffing against the
   whole thread, not just the parent, also catches the parent's quote of the grandparent. Runs of 3
   protect short lines like "Thanks!" that parent and reply happen to share. Top-posting and inline
   replies both work, only matching runs go.

Forwards (`Fwd:`, `WG:`) keep the forwarded content, it's new to me. An older mail that turns up
after its replies (moved in from elsewhere) doesn't retroactively strip them. Accepted, it's a
heuristic. Leftover duplicates are caught at search time by collapsing hits per thread
([README](README.md#search)).

**Wishlist**: attachment text (PDFs, Office documents).

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
