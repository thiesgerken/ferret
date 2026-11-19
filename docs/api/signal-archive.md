# Signal archive HTTP API

`ferret-signal-archive` is the chat archive of one Signal account. It receives messages from a linked `signal-cli` daemon, stores them in its own SQLite database and serves them over this read-only API. It is ferret-unaware: it does not expose ferret documents, send to Signal, link devices or store binary media. The shared wire types live in [ferret-signal-archive](../../crates/ferret-signal-archive/src/lib.rs).

All timestamps are Unix **milliseconds**: a message's sent timestamp is part of its Signal identity.

## Authentication and errors

Listen port: 8080. Every `/v1/*` request requires `Authorization: Bearer <token>` matching the mounted Secret. `GET /healthz` returns only HTTP 200; it is a process probe, not collector or archive freshness.

Errors have an allowlisted JSON code `{"code":"..."}`, never raw SQLite, message or credential details:

| HTTP | `code` | Meaning |
|---|---|---|
| 401 | `unauthorized` | Missing or invalid bearer token |
| 400 | `invalid_query` | Missing, unknown or malformed query fields |
| 400 | `invalid_checkpoint` | Reversed or future sequence range |
| 409 | `store_replaced` | Requested `store_id` differs from the current chat archive |
| 503 | `archive_unavailable` | SQLite cannot be read |

Unknown paths and unsupported methods use ordinary HTTP 404/405 without archive data.

## `GET /v1/archive`

No query fields. Returns:

```json
{
  "store_id": "0123456789abcdef0123456789abcdef",
  "status": "unknown",
  "last_received_at": 1760000000000,
  "high": 1200
}
```

`store_id` is 32 lowercase hex characters, random at database creation; a new database is a different chat archive. `status` is `unlinked` once signal-cli reports the device as removed, otherwise `unknown`; `unknown` does not prove the collector is connected. `last_received_at` is the last received envelope's arrival, or null. `high` is the change journal's highest sequence, 0 when empty.

## `GET /v1/chats?store_id=<id>[&after=<chat>]`

Ascending chat ID keyset over all chats, up to 500 per page:

```json
{
  "chats": [{"id":"a1b2c3d4-...","kind":"direct","name":null}],
  "next": null
}
```

`id` is the counterpart's ACI for `direct`, the group ID for `group`, and the account's own ACI for `note_to_self`. `name` is nullable. `next` is the last returned ID when more rows exist, otherwise null; repeat with `after=next`.

## `GET /v1/messages?store_id=<id>&chat=<chat>[&after_sent_at=<ms>&after_author=<aci>]`

Ascending `(sent_at, author)` keyset over one chat's messages, up to 500 per page. Both continuation fields must be supplied together.

```json
{
  "messages": [{
    "sent_at": 1760000000000,
    "author": "a1b2c3d4-...",
    "author_number": "+15555550100",
    "from_me": false,
    "text": "See attached",
    "attachments": [{"mime_type":"application/pdf","filename":"plan.pdf","size":48213,"caption":null}],
    "edited": false
  }],
  "next": {"sent_at": 1760000000000, "author": "a1b2c3d4-..."}
}
```

`(chat, author, sent_at)` identifies a message; `author` is the sender's ACI, the account's own for `from_me`. `author_number` is the sender's phone number if signal-cli knows it. `text` is null for attachment-only messages. Attachments carry metadata only; every attachment field is nullable. Disappearing and view-once messages are returned like any other: the chat archive never expires them. `next` is null when the chat is exhausted.

## `GET /v1/changes?store_id=<id>&after=<sequence>&through=<sequence>`

Journal entries with `after < sequence <= through`, ascending, up to 500 per page:

```json
{
  "changes": [{
    "sequence": 1201,
    "recorded_at": 1760000000000,
    "operation": "insert",
    "chat": "a1b2c3d4-...",
    "sent_at": 1759990000000,
    "author": "a1b2c3d4-..."
  }],
  "next": 1201,
  "through": 1201,
  "has_more": false
}
```

Each entry references one message by its identity. Fetch the current state of the affected chat instead of treating entries as historical payloads.

Capture `through` from `/v1/archive.high` and hold it fixed for one drain. Repeat with `after=next` while `has_more` is true; the last page returns `next=through`. The journal is never pruned, so every position from 0 stays replayable. For a full reconciliation, capture `high` before enumerating chats and messages, then drain the journal from it afterwards.
