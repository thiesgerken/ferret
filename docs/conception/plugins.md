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
| `remote`, `branch`, `token` | repository to clone over HTTPS. `token` is write-only and sent as HTTP basic auth with the user `x-access-token`, which GitHub, GitLab and Gitea accept with a token as password. Changing `remote` (or anything else but the schedule) needs it entered again |
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
- First run: full ingest inside a sync, commit at the end. Later runs:
  `git diff --name-status --no-renames <cursor> HEAD`, `PUT` changed files, `DELETE` removed ones
  (and files that can't be indexed any more), renames are both. The state stores the cursor
  (`HEAD` of the last successful run) and the indexing settings (remote, branch, globs,
  `url_template`).
- Full sync instead when the cursor is no longer in the branch's history (force push) or the
  indexing settings changed, since files the diff doesn't mention may now be in or out.
- With `{commit}` in `url_template`, an unchanged file keeps the URL of the commit it was last
  indexed at. That commit shows the same content, so the link stays right.

### Paperless

- One document per Paperless document in the connection's root (e.g. `paperless`),
  `external_id` = Paperless id, `content_type = plain` with the OCR text from `content`.
- `meta`: correspondent, document type, tags (by name), archive serial number. `authors` = the
  correspondent. `created_at` = Paperless' `created` date at noon UTC (Paperless keeps only the
  date; noon keeps it in every time zone from UTC-11 to UTC+11). `url` =
  `{public_url}/documents/{id}/details`, `public_url` being the address people open (`url` when
  left out).
- Every run: a sync, list all ids (`/api/documents/?fields=id,modified`), `seen` for all of them,
  fetch the ones modified after the cursor (`id__in`, 100 at a time) and `PUT` them, commit.
  Deleted documents disappear with the sweep. Cursor = the newest `modified`, minus 10 minutes on
  the next run: Paperless sets `modified` before its save commits, so a save may commit after a
  listing with an older timestamp.
- Every list is paged `ordering=id`. Paperless's default order (`-created`, a date) has ties, and
  paging over ties can skip a document, which the sweep would then delete for good. A listing
  whose count changes between pages, or with an id twice, fails the run.
- The state stores the cursor and `url`/`public_url`. When they change, every document is fetched
  again, so links follow.
- Renaming a tag, correspondent or document type doesn't change a document's `modified`, so its
  documents keep the old name until they change otherwise.
- Requires Paperless-ngx 3.0 or later: requests pin API version 10, the newest 3.3 accepts
  (checked against v3.3.2's source). In 3.x, versions of a document stay out of the list, its
  `content` is the latest version's, and a new version bumps its `modified`.
- Auth: a Paperless API token (write-only config field, `Authorization: Token …`). Objects the
  token's user can't see are left out of `meta` with a line in the run log.
- The URL must reach Paperless directly, e.g. the in-cluster service
  (`http://paperless.apps-paperless.svc:8000`). A public hostname behind a login proxy answers
  with a redirect (302), which fails the run with that hint instead of being followed.
- Unlike git remotes, `url` may be any plain-HTTP address in the cluster, checked for neither
  scheme nor host. A user can point the worker at another in-cluster service, and the run log then
  tells which statuses it answers (never its bodies). Accepted for a household in v1.

### carapace

Reads the user's carapace knowledge repo through its upstream remote. ferret never talks to
carapace itself.

**v1 parses the rendered Markdown** of the memory mirror (`src/carapace/memory/render.py`):

- YAML front matter: `kind`, `session_id`, `week_key`, `month_key`, `created_at`, `provenance`
  (digests: `period_key`, `coverage`).
- Body sections by heading: abstract (prose under the title), `## Outcomes`, `## Open loops`,
  `## On my mind`, `## Facts` with one `### <category>` per category, `## Friction`, `## Tags`.
- Fact lines: `- **<subject>**: <statement> _(<source_kind> · <confidence> confidence ·
  <durability or "dated until <date>"> · #<seq> ...)_`. render.py prints "dated until" whenever a
  fact has a `valid_until`, so a durable fact with one reads as dated. v2 fixes that.

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
- `created_at` (and `modified_at`) of sessions, facts and digests is when carapace extracted the
  record, the only timestamp the mirror has. A re-extraction moves it.
- A fact that appears twice in one session (under two categories, say) is indexed once.
- `memory/facts/*.md` is ignored: it repeats the facts of the session files.
- Facts have no id of their own in carapace (`#<seq>` refers to source messages, several facts
  share them). The hash of subject and statement is stable as long as the fact is, and a reworded
  fact is a different fact.
- Incremental like the git plugin. The state stores the indexed commit and `carapace_url`. A run
  diffs against that commit and puts the changed files' documents. A changed session file can lose
  facts, so the plugin also reads every changed or deleted file's version at the cursor
  (`git show <cursor>:<path>`) and deletes whatever that version held but the new one doesn't.
- Full sync (one sync over the root, every document put, the sweep deletes what's gone) on the
  first run, after a force push, when `carapace_url` changed (every link changes), and when an
  old version doesn't parse (a render format this plugin no longer reads).
- Config: `remote`, `branch`, `token` (write-only) like the git plugin, and `carapace_url`, the web
  UI that documents link to.
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
