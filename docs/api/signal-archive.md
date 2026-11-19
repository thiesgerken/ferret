# Signal archive HTTP API

`ferret-signal-archive` is the chat archive of one Signal account. It receives messages from a linked `signal-cli` daemon, stores them in its own SQLite database and serves them over this read-only API. It is ferret-unaware: it does not expose ferret documents, send to Signal, link devices or store binary media. The shared wire types live in [ferret-signal-archive](../../crates/ferret-signal-archive/src/lib.rs).

All timestamps are Unix **milliseconds**: a message's sent timestamp is part of its Signal identity.

## Running it

Image: `ghcr.io/thiesgerken/ferret-signal-archive:<version>`, non-root, read-only root filesystem. It needs:

| Environment | Value |
|---|---|
| `SIGNAL_ARCHIVE_DB` | Path of the SQLite database on a persistent, writable volume. Created and migrated at startup. |
| `SIGNAL_ARCHIVE_TOKEN_FILE` | Mounted Secret holding the bearer token: 64 hexadecimal characters (32 random bytes), optionally newline-terminated. |
| `SIGNAL_CLI_ADDRESS` | `host:port` of the collector, `signal-cli daemon --tcp=<host:port> --receive-mode manual`, without `-a`. |

`--receive-mode manual` is required: only then does signal-cli leave messages queued on the Signal server while this service is down, instead of acknowledging and dropping them. The daemon runs in multi-account mode (no `-a`), because only there does `listAccounts` report the account's own ACI. On every connect the service requires exactly one linked account, records its ACI on first use and passes it as `account` on every call. It then pulls envelopes one at a time (`receive` with `maxMessages: 1`): signal-cli acknowledges an envelope to the Signal server before handing it over, so a crash loses at most the one envelope in flight. It reconnects every 5 seconds while the daemon is unreachable or has no linked account yet, and exits when the database cannot be written, the daemon serves several accounts, or the account differs from the recorded one: a different account is a different chat archive. The database is the only raw copy of everything received, so back it up.

What is stored: direct, group and Note to Self messages, including sync transcripts of messages sent from the phone or other linked devices, with text and attachment metadata. Disappearing and view-once messages are stored and never expire. Story replies are ordinary messages in their chat. Reactions, polls, pins, stories, calls, receipts, typing, group changes and timer changes are skipped, as are messages with neither text nor attachments (e.g. stickers).

## Authentication and errors

Listen port: 8080. Every `/v1/*` request requires `Authorization: Bearer <token>` matching the mounted Secret, compared as fixed-length SHA-256 digests in constant time. `GET /healthz` returns only HTTP 200; it is a process probe, not collector or archive freshness.

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
    "attachments": [{"mime_type":"application/pdf","filename":"plan.pdf","size":48213,"caption":null,"voice_note":false}],
    "edited": false
  }],
  "next": {"sent_at": 1760000000000, "author": "a1b2c3d4-..."}
}
```

`(chat, author, sent_at)` identifies a message; `author` is the sender's ACI, the account's own for `from_me`. `author_number` is the sender's phone number if signal-cli knows it. `text` is null for attachment-only messages. Attachments carry metadata only. Their type is the MIME type (stickers are not attachments in Signal); `voice_note` marks recorded voice messages, the one type MIME does not distinguish. `mime_type`, `filename`, `size` and `caption` are nullable. Disappearing and view-once messages are returned like any other: the chat archive never expires them. `next` is null when the chat is exhausted.

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
