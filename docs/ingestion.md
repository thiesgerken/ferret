# Build an HTTP ingester

Use HTTP ingestion for data without a built-in plugin. Your program reads an origin, turns each item into a document, and reconciles deletions. Ferret never writes back to the origin. This page explains producer workflows; the [REST reference](api/rest.md) owns endpoint/field/auth/error contracts. Canonical wire types are in [ferret-api](../crates/ferret-api/src/lib.rs).

## Choose identity and ownership first

Documents are identified by `(user, source, external_id)`. Use a stable origin id, not a title or current position. Source paths are hierarchical namespaces: `notes/work` is below `notes`, but `notes-other` is not. One ingester should own each root it sweeps. Do not overlap concurrent reconciliations or share a sweep root between independent producers: `seen_at` records observations globally, not membership in one particular sync.

In **Settings → API keys**, create a key with `ingest:write` and only the roots it needs. Add `search:read` only if your script must verify reads. Save the returned secret at creation; it cannot be retrieved later. The key belongs to your signed-in user. Connection-owned roots reject API-key writes.

## Put a complete document

Set these variables to your own instance and key. The example deliberately uses a root outside any connection.

```sh
export FERRET_URL=http://127.0.0.1:3380
export FERRET_KEY='fk_replace_with_your_key'

cat > /tmp/ferret-note.json <<'JSON'
{
  "source": "manual/notes",
  "external_id": "note-42",
  "content_type": "markdown",
  "title": "Workshop checklist",
  "text": "# Workshop checklist\n\nBring the cobalt torque wrench.",
  "url": "https://notes.example.com/notes/42",
  "thread": null,
  "parent": null,
  "authors": ["Alex"],
  "created_at": "2026-01-02T10:00:00Z",
  "modified_at": "2026-01-02T10:00:00Z",
  "meta": {"notebook": "Workshop"}
}
JSON

curl --fail-with-body -sS -X PUT "$FERRET_URL/api/documents" \
  -H "Authorization: Bearer $FERRET_KEY" -H 'Content-Type: application/json' \
  --data-binary @/tmp/ferret-note.json
```

The first response is `{"outcome":"created","id":"<uuid>"}`. Repeating an equivalent normalized request returns `unchanged` with that id. Updating any serialized document field after normalization, including timestamps or metadata, returns `updated`. Do not set `modified_at` to the current time on each enumeration: that defeats no-op detection. `excluded` has no id and means a user's exclusion prevented storage.

A put replaces the complete document, not selected fields. Omitted optional values become absent; `authors`, `created_at`, `modified_at`, `source`, `external_id`, `content_type` and `text` are required keys. Each origin timestamp is independently an RFC 3339 string or `null` when unknown. Never substitute ingestion time, the other origin date or the Unix epoch for an unknown date. `meta` defaults to an empty object. Content types are `markdown`, `plain`, `email`. `url` is an origin deep link; without it Ferret's viewer is the origin. `thread` groups documents only within one exact source. `parent` is the `external_id` of the document in the same thread this one answers (a mail's reply target); it needs `thread`, can't name the document itself, and may name a document that isn't indexed (yet). Unknown fields are rejected.

Ferret strips U+0000 (NUL) from document text, title, every author and recursively nested metadata string values before exclusion checks, hashing and chunking. Read-back contains the normalized values, and chunk offsets point into normalized text. Adding or removing only these NUL characters does not trigger an update. NUL in `source`, `external_id`, `thread`, `parent`, `url` or metadata object keys at any depth instead returns 400 without changing stored data. Fix those inputs at the producer; Ferret will not rewrite identities, URLs or keys.

Chunking happens with the put; embeddings are asynchronous. Updates reuse unchanged chunk text where possible, but changing the title invalidates its embedding input. The writer is the connection or API key that last put the document, including an unchanged put. Revoking a key does not delete its documents.

## Reconcile a complete listing safely

A sync is for a producer that can enumerate **everything still present** below its root. It is not required for individual puts. Start before enumerating; put changed/new documents and mark unchanged existing ones seen, then commit only after every page and write succeeds.

The example retains `note-42` and sweeps any other old documents under `manual/notes`. Run it only for a root you own:

```sh
sync_id=$(curl --fail-with-body -sS -X POST "$FERRET_URL/api/syncs" \
  -H "Authorization: Bearer $FERRET_KEY" -H 'Content-Type: application/json' \
  -d '{"source":"manual/notes"}' | jq -er .sync_id)

curl --fail-with-body -sS -X POST "$FERRET_URL/api/syncs/$sync_id/seen" \
  -H "Authorization: Bearer $FERRET_KEY" -H 'Content-Type: application/json' \
  -d '{"source":"manual/notes","external_ids":["note-42"]}' &&
curl --fail-with-body -sS -X POST "$FERRET_URL/api/syncs/$sync_id/commit" \
  -H "Authorization: Bearer $FERRET_KEY"
```

`seen` counts only existing ids; it does not create missing documents. A put after sync start also counts as seen, even when unchanged. The commit deletes documents in the root and descendants whose `seen_at` predates sync start, and returns `{"deleted":N}`. A second commit is a conflict. The sync belongs to the user and authorized source, not exclusively to the creating key.

To rebuild only one thread, pass `"thread":"<origin-thread-id>"` at start. The sync then covers that exact source and thread: seen ids from other threads are ignored, and commit leaves sibling threads and descendant sources alone.

If a listing is truncated, authentication fails, or a write fails, **do not commit**. Leave the sync uncommitted and start a new one on the next complete attempt. Earlier puts remain stored; leaving a sync open does not roll them back and does not sweep anything. Never interpret a failed origin listing as an empty origin. An intentionally empty successful listing followed by commit deletes the entire root.

## Explicit deletion and read-back

For event-driven origins, delete the exact document when you receive a deletion event. Query values must be URL-encoded:

```sh
curl --fail-with-body -sS -G "$FERRET_URL/api/search" \
  -H "Authorization: Bearer $FERRET_KEY" \
  --data-urlencode 'q=cobalt' --data-urlencode 'source=manual/notes' \
  --data-urlencode 'mode=keyword'

curl --fail-with-body -sS -X DELETE \
  "$FERRET_URL/api/documents?source=manual%2Fnotes&external_id=note-42" \
  -H "Authorization: Bearer $FERRET_KEY"
```

Search requires `search:read`, deletion requires `ingest:write`. Successful deletion has no JSON body; deleting an unknown identity returns 404. Read a hit in full with `GET /api/documents/<document_id>`. There is no general paginated document-export endpoint: the collection GET reads a specific thread by `source` and `thread`.

Keep key material out of repository files, process logs and recorded command output. Treat all copied text as private user data. See [connections](connections.md) for plugin-owned roots and [MCP availability](api/mcp.md) for agent access.

Implementation: [documents](../crates/ferret-server/src/documents.rs), [syncs](../crates/ferret-server/src/syncs.rs), [source paths](../crates/ferret-server/src/sources.rs).
