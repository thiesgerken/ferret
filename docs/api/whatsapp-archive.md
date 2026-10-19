# WhatsApp archive HTTP API

This independent, ferret-unaware container reads one wacli SQLite archive/account. It does not expose ferret documents, mutate/migrate SQLite, connect to WhatsApp, pair devices, send messages or retrieve binary media. Deployment, schema prerequisites and privacy boundaries are in the [WhatsApp plugin guide](../plugins/whatsapp.md).

## Authentication and errors

Listen port: 8080. Every `/v1/*` request requires `Authorization: Bearer <token>` matching the mounted 64-hex-character Secret. The credential is compared as fixed-length SHA-256 digests in constant time. `GET /healthz` returns only HTTP 200; it is a process probe, not archive freshness/readiness evidence.

Errors have an allowlisted JSON code, never raw SQLite, message or credential details:

| HTTP | `code` | Meaning |
|---|---|---|
| 401 | `unauthorized` | Missing/invalid bearer secret |
| 400 | `invalid_query` | Missing, unknown or malformed query fields |
| 400 | `invalid_checkpoint` | Reversed or future sequence range |
| 409 | `store_replaced` | Requested archive identity differs from current SQLite store |
| 410 | `checkpoint_expired` | Replay position is below the effective retention floor |
| 503 | `unsupported_schema` | Unsupported migration/version/identity contract |
| 503 | `archive_unavailable` | SQLite cannot be read safely |
| 503 | `collector_status_unavailable` | The fixed logout marker cannot be inspected safely |

Unknown paths and unsupported methods use ordinary HTTP 404/405 without archive data. Query decoding errors are deliberately not echoed. No endpoint accepts SQL, filesystem paths or arbitrary table/column selectors.

## `GET /v1/archive`

No query fields. Returns:

```json
{
  "store_id": "0123456789abcdef0123456789abcdef",
  "schema_version": 32,
  "revision": 1234,
  "last_sync_at": 1760000000,
  "collector_status": "unknown",
  "floor": 100,
  "high": 1200
}
```

`last_sync_at` is Unix seconds or null. `collector_status` is `logged_out` if the fixed sibling `SESSION_REVOKED` exists, otherwise `unknown`. Neither `unknown` nor the timestamp proves the collector is currently connected. Only marker existence is read; its contents and paths are never returned. `revision` is native archive revision metadata, not an enumeration cursor or an assertion of complete remote history.

`floor` is the maximum of native `change_state.floor` and sequences whose `recorded_at` is at least 30 days old, even before the next writable cleanup. `high` comes from SQLite's `sqlite_sequence` for `message_changes`, **not** the highest retained row; purge may have removed that row.

## `GET /v1/chats?store_id=<id>[&after=<jid>]`

A stable ascending JID keyset over all archive chats, up to 500 per page:

```json
{
  "chats": [{"jid":"123@s.whatsapp.net","kind":"dm","name":"Example"}],
  "next": null
}
```

The only exposed chat fields are `jid`, `kind` and nullable `name`. Kinds are raw native values, including ineligible kinds; consumers choose their scope. `next` is the last returned JID when more rows exist, otherwise null. Repeat with `after=next`. `store_id` is required and compared inside the page's read transaction. Concurrent inserts behind the keyset must be discovered by journal replay, not by restarting enumeration on each native revision.

## `GET /v1/messages?store_id=<id>&chat=<jid>[&after_ts=<seconds>&after_rowid=<rowid>]`

Ascending `(ts,rowid)` keyset, up to 500 retained, readable messages from one chat per page. Both continuation fields must be supplied together. Native revoked, deleted-for-me, tombstoned and payload-purged rows are omitted.

Response: `{"messages":[...],"next":{"ts":1760000000,"rowid":123}}`, with `next:null` when exhausted. Each message contains exactly:

- `rowid` (integer), `msg_id` (string), `ts` (Unix seconds), `from_me` and `edited` (booleans).
- Nullable strings: `sender_jid`, `sender_name`, `text`, `display_text`, `media_type`, `media_caption`, `filename`, `mime_type`.
- `file_length` (nullable integer bytes).

No raw full-row serialization, media keys/hashes, direct/local paths, session credentials, call/status content or binary media are returned. Names are the chat/message names stored in the archive, not live contact lookups. Each page has a short read-only transaction. Native revision changes do not invalidate continuation: replay covers rows inserted or changed behind the keyset.

The process keeps one additional idle read-only SQLite handle to preserve existing WAL sidecars across a clean collector exit. It holds no transaction or snapshot and is never used for request reads. On a read-only mount, startup after SQLite already removed sidecars requires the collector to reopen the archive writable first; no immutable/write/copy fallback is used. See the [cold-start prerequisite](../plugins/whatsapp.md#prerequisites-and-archive-api).

## `GET /v1/changes?store_id=<id>&after=<sequence>&through=<sequence>`

Reads native references with `after < sequence <= through`, ascending, up to 500 per page:

```json
{
  "changes": [{
    "sequence": 1201,
    "recorded_at": 1760000000,
    "operation": "identity_move",
    "chat_jid": "123@lid",
    "msg_id": "example-message-id",
    "new_chat_jid": "456@s.whatsapp.net",
    "new_msg_id": null,
    "reason": "identity_normalization",
    "message_ts": 1750000000
  }],
  "next": 1201,
  "through": 1201,
  "has_more": false
}
```

Operations are native `upsert`, `delete` and `identity_move`. New identity fields are nullable; upstream's LID normalization can provide only `new_chat_jid` while retaining the original message ID. Reasons are the native structured reason, never historical message text. Fetch current message state/rebuild affected chats instead of treating journal references as historical payloads.

Capture `through` from `/v1/archive.high` and hold it fixed throughout one drain. Repeat `after=next` while `has_more` is true. On the last page `next=through`, including when retained gaps yield no rows. Store identity, position bounds and effective retention floor are validated on **every** page. `after=through` is a useful empty-range retention/identity check. Store replacement (409) and expired replay (410) are distinct failures.

For full reconciliation: capture a position **before** enumeration, finish all pages, capture/drain a fixed end boundary **after** enumeration, rebuild affected identities and validate retention before any destructive sweep. Never authorize deletion after an incomplete page sequence. Restart full reconciliation after 410; never adopt a different store after 409. There are no snapshot sessions, CLI revision-invalidating cursors, long-lived transactions, durable API state, broker or webhooks.

## Ferret's separate archive-binding operation

This is on the **ferret server**, not the archive API:

`PUT /api/run/archive-binding` with `{"store_id":"<32 lowercase hex characters>"}` returns 204 when the connection is unbound or already bound to this ID, 409 for another ID, 403 for API keys/non-WhatsApp runs, and 401 for invalid/ended run tokens.

Only the caller's currently live WhatsApp run token authorizes it. The server locks/rechecks the running row and connection in the same transaction as compare-or-initialize; the body cannot select another connection. Binding is persistent connection identity, not disposable working state. Recreate the connection for intentional replacement. Ordinary ingest API keys cannot initialize or mutate a binding.
