# WhatsApp archive

The `whatsapp` built-in plugin indexes a **retained local wacli archive**, not WhatsApp itself. A separate read-only HTTP container exposes one archive/account; ordinary ferret workers run scheduled connections against it. Pairing, collection and backfill remain the operator's responsibility. Neither component sends messages or starts a collector.

## Prerequisites and archive API

Use released **wacli v0.22.0 (schema 32)**, or upstream commit [`c45422b4f29eee225d06d326163845a88fe0d6ae`](https://github.com/openclaw/wacli/tree/c45422b4f29eee225d06d326163845a88fe0d6ae) (schema 33). The collector must open its archive writable at least once to apply native migrations. Versions 30–32 must carry their upstream retrieval/journal migration names; schema 33 additionally requires `ordered chat auto-unarchive`. Unknown future versions and prerelease swapped migration IDs are rejected. The API never migrates the archive. This is a deployment prerequisite, not a request to let the API write it.

Pull `ghcr.io/thiesgerken/ferret-whatsapp-archive:<version>` (x64), or build `whatsapp-archive-image` with Nix on Apple Silicon (`nix build .#packages.aarch64-linux.whatsapp-archive-image`, then `docker load -i result`). Use `ferret-whatsapp-archive:local` for local builds. It is not part of the Compose stack or Helm chart.

Create a mounted bearer Secret containing exactly 64 hexadecimal characters generated from 32 random bytes:

```sh
openssl rand -hex 32 > archive-token
chmod 600 archive-token
```

Run the container with the archive **directory**, including `wacli.db-wal` and `wacli.db-shm`, mounted read-only. Do not mount only `wacli.db`, copy it while the collector writes, or use SQLite `immutable=1`. Start the API while the collector has created/opened its readable WAL sidecars. The API retains one idle read-only handle, without a transaction or snapshot, so a later clean collector exit does not remove those sidecars; fresh per-request connections still detect store replacement.

**Cold-start prerequisite:** if a collector previously closed the archive before any API reader existed, SQLite may already have removed `-wal`/`-shm`. A read-only mount cannot recreate them. Reopen the archive with native writable wacli and start the API while that handle is open. The API fails explicitly rather than copying the archive, enabling writes or using unsafe immutable mode. Existing sidecars retained by the read-only API also permit an API restart while the collector is offline.

Native wacli puts `session.db` in the same store directory. The whole-directory example below therefore gives this **trusted container** filesystem visibility of session credentials, although the API never opens `session.db` or returns its contents. Trust the API image accordingly, or supply a deliberate filesystem projection exposing only the archive, sidecars and optional logout marker. There is no automatic credential-isolating projection or session rewriting.

```sh
docker run --rm --name whatsapp-archive \
  --user "$(id -u):$(id -g)" --read-only \
  --cap-drop=ALL --security-opt=no-new-privileges \
  --tmpfs /tmp:rw,nosuid,nodev,mode=1777 \
  --mount type=bind,src="$(pwd)/archive",dst=/archive,readonly \
  --mount type=bind,src="$(pwd)/archive-token",dst=/run/secrets/archive-token,readonly \
  -e WHATSAPP_ARCHIVE_DB=/archive/wacli.db \
  -e WHATSAPP_ARCHIVE_TOKEN_FILE=/run/secrets/archive-token \
  -p 127.0.0.1:8080:8080 ghcr.io/thiesgerken/ferret-whatsapp-archive:latest
```

Required environment variables are `WHATSAPP_ARCHIVE_DB` (existing database file) and `WHATSAPP_ARCHIVE_TOKEN_FILE` (mounted Secret file). The process refuses missing/invalid configuration and listens on port 8080. An optional final newline in the Secret file is stripped. The image defaults to UID/GID `65532:65532`; override its numeric user to match the archive owner when native wacli's private file permissions require that. Do not loosen archive/session permissions globally to make a container work.

In Kubernetes deploy this as an independent workload next to the collector's storage. Use a read-only volume mount and root filesystem, a Secret volume, a UID able to read the archive, an internal Service on 8080 and a NetworkPolicy permitting ingress only from the ferret plugin workers (and explicitly authorized operator diagnostics). Do not create public Ingress/Gateway routes. Private HTTP plus NetworkPolicy is the trust model: the bearer token is not transport encryption. Network restrictions must also prevent untrusted pods from reaching the Service. The ferret Helm chart does not deploy/manage wacli or this independent archive workload.

Only `GET /healthz` is unauthenticated and returns no archive information. All inventory, status and journal reads require the bearer secret. See the exact [archive HTTP contract](../api/whatsapp-archive.md).

## Connection configuration

First authenticate to `GET /v1/archive` on the private API and copy its `store_id`. Create a connection with plugin `whatsapp`, a source such as `chat/personal`, a normal schedule, and that explicit archive identity:

```json
{
  "url": "http://whatsapp-archive:8080",
  "token": "<64-character hexadecimal archive token>",
  "expected_store_id": "0123456789abcdef0123456789abcdef",
  "allow_chats": [],
  "block_chats": []
}
```

`url` is the direct API root reached by workers; redirects are not followed. `token` is required, encrypted by ferret and write-only. Changes to `expected_store_id`, `allow_chats` and `block_chats` preserve the stored token through the generic credential-safe configuration flow. Changing `url` requires entering the token again. Empty `allow_chats` includes every eligible direct/group chat. Both filters contain **exact chat IDs**, never names; `block_chats` wins.

`expected_store_id` is required, nonsecret and exactly 32 lowercase hexadecimal characters. Replace the example with the authenticated archive's actual ID, not a guessed value or an ID copied from another connection. The plugin compares it before starting any sync or writing any document, and sends the same expected ID throughout enumeration, replay and pre-commit checks. It is stored only in ordinary connection configuration, so failed/lost runs and cursor/exclusion resets do not discard the pin. There is no automatic first-use adoption.

Obtain IDs from authenticated `GET /v1/chats?store_id=<id>` after reading `GET /v1/archive`. Every configured ID must exist in the current inventory. Unknown, deleted or normalized IDs fail the run with an actionable filter error, rather than silently permitting a formerly blocked identity. wacli's LID-to-phone normalization removes the old chat identity, and its journal is not a permanent alias registry. Update affected filter IDs explicitly after inspecting the inventory. With no filter reference to the old identity, journal replay rebuilds both old and new threads and removes obsolete segments. A configuration change triggers full reconciliation; re-including a chat imports its retained history.

## Documents and privacy boundaries

- Direct/group messages include text, captions and available attachment type/name/MIME/size metadata. Segments are Markdown: each message starts with a bold sender line and its UTC time, followed by its text, with line breaks kept:

  ```text
  **Me** · 2026-02-21 11:50 UTC
  Shall we continue with the shelves this afternoon?

  **Alex Example** · 11:52 (edited)
  Attachment: image · shelf.jpg · image/jpeg · 1.2 MB
  Sure, see the photo
  ```

  The full date appears on a segment's first message and whenever the UTC day changes, otherwise only the time. Attachments take one line; unknown or zero sizes are omitted. Sent messages are by `Me`. A direct chat's other messages are by the sender's name, or else the chat's title (the raw chat ID for an unnamed chat); a group member without a name shows as their raw ID. No other JIDs appear in the text: the chat name is the title and the chat ID is in metadata. Message text cannot form Markdown headings, lists, quotes, code blocks, tables or HTML: a line starting with such syntax is escaped, other leading punctuation stays as written. Inline emphasis does render, so WhatsApp's `*bold*` shows as italics.
- Status broadcasts, newsletters, call history and Signal are not indexed. There is no binary download, OCR, transcription or sending. Media retrieval keys and paths are not returned by the API or indexed.
- Readable retained ephemeral/view-once messages are eligible. There is **no automatic ephemeral exclusion or expiry**. Explicit recorded deletions are honored. Decide whether the retained archive is appropriate for your privacy policy before connecting it.
- Segments are chronological, split between messages at pauses of at least 24 hours or approximately 8,000 Unicode characters. That is about one segment per day of conversation, without splitting a conversation at midnight. A single oversized message stays intact in one segment; ferret's normal chunker handles it. There are no segmentation configuration knobs.
- Every document uses exactly the connection's source root. Its thread is the stable raw chat ID. Segment external IDs are `<sha256-of-chat-id>/<sha256-of-first-message-id>`, avoiding collisions when different chats reuse message IDs. The title is the chat name (or raw ID), with dates stored separately. Documents use `markdown` content and ferret's normal viewer, without fabricated WhatsApp links.
- **Authors** are distinct actual speakers in that segment, including `Me`. An outgoing-only direct segment is authored by Me, not its counterparty. A group name or silent participant is not an author.
- `created_at` is the first message's exact archive timestamp; `modified_at` is unknown, not the run's time. Equal-second message order uses the archive rowid during rendering. The ordinary viewer orders equal-time documents by its document ID, so size-bound segments with equal-second starts have no stronger viewer tie-order guarantee. Metadata retains the first message ID/rowid and last message date.
- Edits, metadata changes, backfill and identity moves rebuild affected chats. Unchanged chats are skipped. Regrouping can change segment IDs and trigger reindexing. An exact segment exclusion does not exclude regrouped text under a different ID. Whole-thread exclusions cover every segment of that same stable chat ID; exact chat blocklist filters also remain available.
- Historical payload memory is limited to one API page and the open segment (plus chat inventory/message-ID deduplication metadata). Completed segments are put immediately, but sweeping still waits for complete traversal and replay.

## Reconciliation, status and replacement

The first run imports all retained local history. A successful run saves only its journal checkpoint, indexing settings and chat inventory metadata. It does not persist a segmentation manifest. Incremental runs use wacli's native insert/edit/delete/identity-move journal and compare chat metadata separately. Checkpoints are local sequence positions, not message dates, so late history is discovered.

Before full enumeration, the plugin captures a journal position. After traversal it captures a fixed end boundary, drains changes and rebuilds affected/new/removed chats. All pages must succeed before any sweep. Retention and store identity are checked again before destructive commits. Requests/drains are bounded; a continuously writing collector cannot keep the run draining forever. Writes after the fixed boundary are processed next run.

The journal retains at most 30 days or 100,000 references. Expired checkpoints automatically restart full local reconciliation and report a **lost replay range** in Activity. A failed/lost run receives empty disposable state next time and safely reconciles again. A fully completed local reconciliation is authoritative: messages absent from this archive and obsolete segments may be removed even without the original WhatsApp deletion event. A partial/failed traversal cannot authorize sweeping or checkpoint advancement. None of this claims the archive contains complete WhatsApp history.

### Layout and rendering cutovers

**From plain-text rendering and one-hour segments (cursor version 2)** the upgrade to Markdown and 24-hour segments is automatic. The first run after upgrading the plugin workers re-renders and regroups every chat in a full authoritative reconciliation and sweeps segment IDs that no longer exist. Activity logs this. It re-embeds the whole WhatsApp source once. Exclusions of single segments are lost wherever regrouping changes a segment's ID, so reapply them afterwards. Whole-thread exclusions and chat filters remain, because the chat ID is unchanged.

**From the old index layout (cursor version 1)** a **manual clean rebuild** is required, not an automatic data/exclusion migration. Such a cursor fails the run with these instructions. Before enabling new workers for an existing WhatsApp connection:

1. Disable the connection and wait for its active run to finish. Record its name, source root, schedule, `url`, `expected_store_id`, `allow_chats` and `block_chats`. Obtain the archive token from the original Secret; stored credentials are write-only and cannot be exported.
2. Upgrade the server and **all** plugin workers. Do not let old workers claim the recreated connection.
3. Delete the old connection through Connections, confirming its source root. This intentionally clears its indexed documents, remembered document/thread exclusions and run history. It does **not** change or delete the native wacli archive.
4. Recreate the WhatsApp connection with the same root and recorded settings, re-entering the original archive token. The new connection performs a full import with the current layout and rendering.

Old viewer UUID links and exclusions are not preserved. Reapply any desired exclusions after rebuilding; configured allow/blocklists and the explicit archive pin must be copied into the new connection.

### Archive identity and collector status

An archive whose authenticated `store_id` differs from `expected_store_id` fails before ingestion or sweep, even on an empty state or after a failed first run. For intentional replacement, inspect the new authenticated archive and explicitly edit `expected_store_id` through the ordinary connection configuration flow. Changing only this pin preserves the stored token. This authorizes a full authoritative reconciliation against the new archive and may remove content previously indexed under this source. Consider the retained history, source contents and exclusions before changing the pin. Archive identity is plugin configuration, not separate server-owned state.

The archive remains ingestible when the collector is unavailable or logged out. API status is `logged_out` when wacli's fixed `SESSION_REVOKED` marker exists, otherwise `unknown`; absence does not imply a live connection. `last_sync_at` is the archive's actual last successful collector sync, nullable when none is known. Activity reports both values. Inspect that timestamp for stale collection; a successful ferret run does not mean fresh WhatsApp collection.

## Compatibility evidence and development checks

Native SQLite mutations and journal semantics come from upstream [changes.go](https://github.com/openclaw/wacli/blob/v0.22.0/internal/store/changes.go), [retrieval.go](https://github.com/openclaw/wacli/blob/v0.22.0/internal/store/retrieval.go), [migrations.go](https://github.com/openclaw/wacli/blob/v0.22.0/internal/store/migrations.go) and [logout marker handling](https://github.com/openclaw/wacli/blob/v0.22.0/internal/app/session_state.go). The committed SQL fixture is an offline native v0.22.0 archive dump, including its real triggers, not a second hand-written journal implementation.

Run API boundaries with `cargo test -p ferret-whatsapp-archive --test archive`; run registered-worker ingestion against ParadeDB with `cargo test -p ferret-server --test integration whatsapp_plugin::`. The latter exercises the actual HTTP archive router, SQLite WAL writer, worker registration/run tokens and ferret server. Ordinary workspace fmt/clippy/tests and `.sqlx` freshness checks still apply.
