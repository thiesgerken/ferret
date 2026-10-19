# WhatsApp archive

The `whatsapp` built-in plugin indexes a **retained local wacli archive**, not WhatsApp itself. A separate read-only HTTP container exposes one archive/account; ordinary ferret workers run scheduled connections against it. Pairing, collection and backfill remain the operator's responsibility. Neither component sends messages or starts a collector.

## Prerequisites and archive API

Use released **wacli v0.22.0 (schema 32)**, or upstream commit [`c45422b4f29eee225d06d326163845a88fe0d6ae`](https://github.com/openclaw/wacli/tree/c45422b4f29eee225d06d326163845a88fe0d6ae) (schema 33). The collector must open its archive writable at least once to apply native migrations. Versions 30–32 must carry their upstream retrieval/journal migration names; schema 33 additionally requires `ordered chat auto-unarchive`. Unknown future versions and prerelease swapped migration IDs are rejected. The API never migrates the archive. This is a deployment prerequisite, not a request to let the API write it.

Build with `docker build --target whatsapp-archive -t ferret-whatsapp-archive .`. CI publishes `ghcr.io/<owner>/ferret-whatsapp-archive:pr<N>` and `:pr<N>-<sha>` alongside the ordinary images; releases publish version tags and `latest`.

Create a mounted bearer Secret containing exactly 64 hexadecimal characters generated from 32 random bytes:

```sh
openssl rand -hex 32 > archive-token
chmod 600 archive-token
```

Run the container with the archive **directory**, including `wacli.db-wal` and `wacli.db-shm`, mounted read-only. Do not mount only `wacli.db`, copy it while the collector writes, or use SQLite `immutable=1`. A collector using WAL must have created its sidecars before read-only readers start. Keep the collector running while readers use its WAL sidecars. Read-only WAL opening requires the sidecars to be readable; the API will not create them on the collector's behalf.

Native wacli puts `session.db` in the same store directory. The whole-directory example below therefore gives this **trusted container** filesystem visibility of session credentials, although the API never opens `session.db` or returns its contents. Trust the API image accordingly, or supply a deliberate filesystem projection exposing only the archive, sidecars and optional logout marker. There is no automatic credential-isolating projection or session rewriting.

```sh
docker run --rm --name whatsapp-archive \
  --user "$(id -u):$(id -g)" --read-only \
  --mount type=bind,src="$(pwd)/archive",dst=/archive,readonly \
  --mount type=bind,src="$(pwd)/archive-token",dst=/run/secrets/archive-token,readonly \
  -e WHATSAPP_ARCHIVE_DB=/archive/wacli.db \
  -e WHATSAPP_ARCHIVE_TOKEN_FILE=/run/secrets/archive-token \
  -p 127.0.0.1:8080:8080 ferret-whatsapp-archive
```

Required environment variables are `WHATSAPP_ARCHIVE_DB` (existing database file) and `WHATSAPP_ARCHIVE_TOKEN_FILE` (mounted Secret file). The process refuses missing/invalid configuration and listens on port 8080. An optional final newline in the Secret file is stripped. The image defaults to `nobody`; override its numeric user to match the archive owner when native wacli's private file permissions require that. Do not loosen archive/session permissions globally to make a container work.

In Kubernetes deploy this as an independent workload next to the collector's storage. Use a read-only volume mount and root filesystem, a Secret volume, a UID able to read the archive, an internal Service on 8080 and a NetworkPolicy permitting ingress only from the ferret plugin workers (and explicitly authorized operator diagnostics). Do not create public Ingress/Gateway routes. Private HTTP plus NetworkPolicy is the trust model: the bearer token is not transport encryption. Network restrictions must also prevent untrusted pods from reaching the Service. The ferret Helm chart does not deploy/manage wacli or this independent archive workload.

Only `GET /healthz` is unauthenticated and returns no archive information. All inventory, status and journal reads require the bearer secret. See the exact [archive HTTP contract](../api/whatsapp-archive.md).

## Connection configuration

Create a connection with plugin `whatsapp`, a source such as `chat/personal`, and a normal schedule:

```json
{
  "url": "http://whatsapp-archive:8080",
  "token": "<64-character hexadecimal archive token>",
  "allow_chats": [],
  "block_chats": []
}
```

`url` is the direct API root reached by workers; redirects are not followed. `token` is required, encrypted by ferret and write-only. As for other connections, changing configuration requires entering the credential again. Empty `allow_chats` includes every eligible direct/group chat. Both filters contain **exact chat IDs**, never names; `block_chats` wins.

Obtain IDs from authenticated `GET /v1/chats?store_id=<id>` after reading `GET /v1/archive`. Every configured ID must exist in the current inventory. Unknown, deleted or normalized IDs fail the run with an actionable filter error, rather than silently permitting a formerly blocked identity. wacli's LID-to-phone normalization removes the old chat identity, and its journal is not a permanent alias registry. Update affected filter IDs explicitly after inspecting the inventory. With no filter reference to the old identity, journal replay rebuilds both old and new namespaces and removes obsolete segments. A configuration change triggers full reconciliation; re-including a chat imports its retained history.

## Documents and privacy boundaries

- Direct/group messages include text, captions and available attachment type/name/MIME/size metadata. The renderer includes chat ID/name, sender, direction (`Me` for sent messages), UTC message date and an edit indicator.
- Status broadcasts, newsletters, call history and Signal are not indexed. There is no binary download, OCR, transcription or sending. Media retrieval keys and paths are not returned by the API or indexed.
- Readable retained ephemeral/view-once messages are eligible. There is **no automatic ephemeral exclusion or expiry**. Explicit recorded deletions are honored. Decide whether the retained archive is appropriate for your privacy policy before connecting it.
- Segments are chronological, split between messages at pauses of at least one hour or approximately 8,000 Unicode characters. A single oversized message stays intact in one segment; ferret's normal chunker handles it. There are no segmentation configuration knobs.
- Each chat uses source `<root>/chats/<sha256-of-chat-id>` and a thread identified by the chat ID. Segment external IDs hash the first message ID. Documents use `plain` content and ferret's normal viewer, without fabricated WhatsApp links.
- `created_at` is the first message's exact archive timestamp; `modified_at` is unknown, not the run's time. Equal-second message order uses the archive rowid during rendering. The ordinary viewer orders equal-time documents by its document ID, so size-bound segments with equal-second starts have no stronger viewer tie-order guarantee. Metadata retains the first message ID/rowid and last message date.
- Edits, metadata changes, backfill and identity moves rebuild affected chats. Unchanged chats are skipped. Regrouping can change segment IDs and trigger reindexing. An exact segment exclusion does not exclude regrouped text under a different ID. Use the chat blocklist for reliable whole-chat filtering.

## Reconciliation, status and replacement

The first run imports all retained local history. A successful run saves only its journal checkpoint, indexing settings and chat inventory metadata. It does not persist a segmentation manifest. Incremental runs use wacli's native insert/edit/delete/identity-move journal and compare chat metadata separately. Checkpoints are local sequence positions, not message dates, so late history is discovered.

Before full enumeration, the plugin captures a journal position. After traversal it captures a fixed end boundary, drains changes and rebuilds affected/new/removed chats. All pages must succeed before any sweep. Retention and store identity are checked again before destructive commits. Requests/drains are bounded; a continuously writing collector cannot keep the run draining forever. Writes after the fixed boundary are processed next run.

The journal retains at most 30 days or 100,000 references. Expired checkpoints automatically restart full local reconciliation and report a **lost replay range** in Activity. A failed/lost run receives empty disposable state next time and safely reconciles again. A fully completed local reconciliation is authoritative: messages absent from this archive and obsolete segments may be removed even without the original WhatsApp deletion event. A partial/failed traversal cannot authorize sweeping or checkpoint advancement. None of this claims the archive contains complete WhatsApp history.

On the first authenticated use, ferret atomically binds the connection to the archive's `store_id`, **before any ingestion write**. This binding survives failure of the first ingest, checkpoint expiry, configuration changes and exclusion resets. A replacement archive fails before ingestion or sweep and is never silently adopted. For intentional replacement, explicitly delete/recreate the connection after considering its existing indexed content and exclusions. There is no expected-ID config knob or API-key-authorized rebinding operation.

The archive remains ingestible when the collector is unavailable or logged out. API status is `logged_out` when wacli's fixed `SESSION_REVOKED` marker exists, otherwise `unknown`; absence does not imply a live connection. `last_sync_at` is the archive's actual last successful collector sync, nullable when none is known. Activity reports both values. Inspect that timestamp for stale collection; a successful ferret run does not mean fresh WhatsApp collection.

## Compatibility evidence and development checks

Native SQLite mutations and journal semantics come from upstream [changes.go](https://github.com/openclaw/wacli/blob/v0.22.0/internal/store/changes.go), [retrieval.go](https://github.com/openclaw/wacli/blob/v0.22.0/internal/store/retrieval.go), [migrations.go](https://github.com/openclaw/wacli/blob/v0.22.0/internal/store/migrations.go) and [logout marker handling](https://github.com/openclaw/wacli/blob/v0.22.0/internal/app/session_state.go). The committed SQL fixture is an offline native v0.22.0 archive dump, including its real triggers, not a second hand-written journal implementation.

Run API boundaries with `cargo test -p ferret-whatsapp-archive --test archive`; run registered-worker ingestion against ParadeDB with `cargo test -p ferret-server --test whatsapp_plugin`. The latter exercises the actual HTTP archive router, SQLite WAL writer, worker registration/run tokens and ferret server. Ordinary workspace fmt/clippy/tests and `.sqlx` freshness checks still apply.
