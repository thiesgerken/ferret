# Signal chat archive

Status: approved design, superseding the research note of 2026-10-24. This is rationale and scope, not the operator contract; the maintained plugin guide and archive API reference follow with the implementation. Why ferret owns this chat archive at all is [ADR 0009](../adr/0009-ferret-owns-the-signal-chat-archive.md). Versions checked: signal-cli v0.14.9, libsignal v0.106.0, Signal-Desktop v8.32.0, Signal-Android v8.32.3, Signal-Server v20261022.0.0.

## Purpose and boundaries

One Signal account, whose primary device is an Android phone, becomes searchable like WhatsApp: a one-time history import plus continuous ingestion, always on in Kubernetes, read-only towards Signal. Nothing ever sends. Same document model as WhatsApp: chat segments in Markdown, one thread per chat, authors per segment, ferret's viewer as the origin. [F1]

There is no wacli for Signal. No maintained tool is collector, durable chat archive and change journal in one: signal-cli keeps no message history, presage keeps messages in a client store without a journal and cannot import history, mautrix-signal stages history only until it is in Matrix, Signal Desktop has no headless mode. So ferret owns the chat archive. [C4][C12][P1][P2][M3][D4]

## Components

One pod, two containers, one Signal account:

- **Collector:** the official `signal-cli` image, linked as an additional device (`signal-cli link`, never `register`, which would unregister the phone), running `daemon --tcp --receive-mode manual` in multi-account mode (no `-a`, so that `listAccounts` reports the account's own ACI), its account data on a PVC. Pinned and bumped at least monthly by Renovate, independent of ferret releases: signal-cli releases older than three months may stop working. [C1][C2]
- **`ferret-signal-archive`:** a new crate and image in this repo, built with Nix like `ferret-whatsapp-archive`. One process pulls from the daemon over JSON-RPC, is the only writer of its SQLite chat archive, refreshes contact and group names via `listContacts`/`listGroups`, and serves the read-only HTTP API the plugin polls.

Writer and API share a process because SQLite has one writer anyway; WhatsApp splits them only because wacli is the writer. Neither container is part of the Helm chart. Deployment, the PVCs, the NetworkPolicy and backups belong to the cluster's infrastructure, next to wacli. The chat archive is the only raw copy of everything after the history import, so backing it up is the operator's job, as for wacli.

Linking is a one-time manual step (`kubectl exec` into the collector, scan the `sgnl://linkdevice` URI). The Android phone must come online at least every 30 days, and a linked device idle for 45 days is unlinked. [S1][V5]

## Receiving without loss

`--receive-mode manual` is required. In the default `on-start` the daemon acks every envelope to the server and hands it to whatever handlers are connected, so with the consumer down it is gone. In manual mode envelopes stay queued on the Signal server until the consumer asks for them. The server drops queued messages after about 30 days (sample config and support article; the production value is not in the source). [C3][C5][C6][V6][S2]

JSON-RPC has no client ack: signal-cli acks an envelope to the server before handing it over. A consumer crash mid-delivery loses the one envelope in flight. Accepted: envelopes are pulled one at a time with `receive` and `maxMessages: 1` rather than pushed by `subscribeReceive`, which would stream and ack a whole backlog, and both containers share a pod lifecycle, so the window stays one message per crash. [C4][C7][C14]

## History import

Once, after the collector is linked and receiving, so the two overlap instead of leaving a gap:

1. Fresh-install Signal Desktop on a workstation, link it with **Transfer Message History**. That transfers all messages and the last 45 days of media. signal-cli cannot receive this transfer itself: it does not advertise the `backup5` capability. [S1][D1][C10]
2. Settings > Chats > Export chat history, without media. Desktop writes `main.jsonl`: libsignal backup frames as JSON lines, in the versioned backup schema rather than Desktop's internal database schema, which gains about three migrations a month. [D2][D3][D5][V1][V4]
3. Copy it into the pod and run `ferret-signal-archive import <main.jsonl>` via `kubectl exec`, against the live database (WAL plus a busy timeout; the daemon keeps writing). It inserts only messages whose key is absent and journals them as ordinary inserts, so the next plugin run ingests them incrementally. There is no import endpoint; the API stays read-only.
4. Unlink Desktop.

Accepted gap: the export drops every message with a disappearing timer and all view-once media. Disappearing messages still unexpired at import time are therefore missing from history, at most one timer window per disappearing chat. Reading Desktop's SQLCipher database or decrypting an Android folder backup would close it, at the price of an internal-schema reader or AGPL tooling in the import step. [V4][S3]

## Message identity

Signal addresses a message by its author's ACI plus its sent timestamp. Edits, remote deletes, reactions and quotes all point back with that pair, and an edit keeps the original's identity. A later edit or delete may name any revision's timestamp instead of the original's: Signal-Desktop addresses an edit to the latest revision and matches deletes against all of them, so the chat archive maps every revision timestamp back to the original [D9][D10]. A deleted message stays as a tombstone, so neither a redelivered envelope nor the history import can bring it back. The chat archive keys messages by `(chat, author ACI, sent timestamp)`, which also deduplicates the history import against the live stream. Own messages carry the account's ACI as author. [D6][D7][V1]

Chat IDs are the counterpart's ACI for a direct chat, the group ID for a group, and the account's own ACI for Note to Self.

## Content

Same scope as WhatsApp [F1]:

- Indexed: text, captions, and attachment type/name/MIME/size metadata. Messages sent from the phone or another linked device arrive as sync transcripts and are stored like any other. [C9]
- Edits: the latest text with an "(edited)" marker. Remote deletes (including group admin deletes) remove the message.
- Skipped: reactions, quotes, polls, pins, stories, calls, group change events, timer changes, binary media.
- **Disappearing and view-once messages are stored and indexed, and never expired.** This deliberately differs from Signal's own exports, which drop them: what the user has read, ferret may remember. Media is never stored, view-once or not.
- Sender names: the contact name (system or nickname), else the Signal profile name, else the phone number, else the raw ACI; `Me` for own messages. Resolved when rendering, so they are current names, and a name change re-renders the affected chats once.

## Chat archive API

The shape of the [WhatsApp archive API](../api/whatsapp-archive.md), so the plugin's reconciliation carries over [F2]:

- `GET /v1/archive`: `store_id` (32 lowercase hex characters, random at database creation), collector `status` and `last_received_at`.
- `GET /v1/chats`, keyset-paged `GET /v1/messages` per chat, `GET /v1/changes` with a fixed-boundary drain, all pinned to the `store_id`.
- Bearer token from a mounted Secret, `GET /healthz` unauthenticated, private HTTP plus NetworkPolicy as the trust model.

The change journal is **never pruned**. At personal volume it stays small, and an unpruned journal removes WhatsApp's retention handling entirely: no 410, no lost replay range, no expiry restart.

Collector `status` is `unlinked` once signal-cli reports the device as removed, otherwise `unknown`; `last_received_at` is the last envelope's arrival. Activity reports both, and a run succeeds while the collector is down, because the chat archive stays readable.

## Plugin

A separate `signal` plugin, a thin adapter over this API. Segmentation (about one segment per day of conversation, splits at 24 h pauses or about 8,000 characters), Markdown rendering and escaping, authors and reconciliation move out of `whatsapp.rs` into a shared chat module that both plugins use. A single generic chat plugin was rejected: wacli's quirks (identity moves, schema pinning, logout marker) would leak into a contract meant to be neutral.

Connection configuration copies WhatsApp's: `url`, write-only `token`, required `expected_store_id`, and `allow_chats`/`block_chats` with exact chat IDs (empty allows everything, block wins, unknown IDs fail the run).

## Open points

- Whether the Desktop export carries edit revisions and group events in practice: check one real export before writing the importer.
- How signal-cli reports an unlinked device over JSON-RPC, for `status`.
- signal-cli's memory footprint, JVM versus native image (cluster nodes are amd64, which both official images support). [C13]

## Rejected alternatives

- **Headless Signal Desktop** as collector and chat archive: no headless mode, builds expire after 90 days, schema churn, and reading a SQLCipher database the app is writing. [D4][D5][D8][E3]
- **presage** (Rust): no history import until PR #447 lands, no journal, a store key that collides on equal timestamps, AGPL. Revisit if link-and-sync lands. [P1][P2][P3]
- **signalmeow** (Go, from mautrix-signal): the only third-party client receiving link-and-sync history, but a bridge-internal library without a stable API, CGO libsignal, AGPL, and the chat archive would still be ours to write. [M1][M2]
- **No chat archive**, ferret's documents as the only copy: nothing raw remains to re-segment, re-render or reconcile against.

## Sources

- [F1: WhatsApp plugin guide](../plugins/whatsapp.md)
- [F2: WhatsApp archive HTTP API](../api/whatsapp-archive.md)
- [C1: signal-cli README, upgrade policy and requirements](https://github.com/AsamK/signal-cli/blob/v0.14.9/README.md)
- [C2: signal-cli man page, `link` and receive options](https://github.com/AsamK/signal-cli/blob/v0.14.9/man/signal-cli.1.adoc)
- [C3: signal-cli JSON-RPC man page, receive modes](https://github.com/AsamK/signal-cli/blob/v0.14.9/man/signal-cli-jsonrpc.5.adoc)
- [C4: ReceiveHelper, cache-then-ack and retry](https://github.com/AsamK/signal-cli/blob/v0.14.9/lib/src/main/java/org/asamk/signal/manager/helper/ReceiveHelper.java)
- [C5: ManagerImpl, receive handlers](https://github.com/AsamK/signal-cli/blob/v0.14.9/lib/src/main/java/org/asamk/signal/manager/internal/ManagerImpl.java)
- [C6: DaemonCommand, receive-mode wiring](https://github.com/AsamK/signal-cli/blob/v0.14.9/src/main/java/org/asamk/signal/commands/DaemonCommand.java)
- [C7: JSON-RPC dispatcher, unacked notifications](https://github.com/AsamK/signal-cli/blob/v0.14.9/src/main/java/org/asamk/signal/jsonrpc/SignalJsonRpcDispatcherHandler.java)
- [C9: JSON output records](https://github.com/AsamK/signal-cli/tree/v0.14.9/src/main/java/org/asamk/signal/json)
- [C10: ProvisioningManagerImpl, link capabilities and sync request](https://github.com/AsamK/signal-cli/blob/v0.14.9/lib/src/main/java/org/asamk/signal/manager/internal/ProvisioningManagerImpl.java)
- [C12: signal-cli issue #1747, no message history](https://github.com/AsamK/signal-cli/issues/1747)
- [C13: signal-cli release workflow, container images](https://github.com/AsamK/signal-cli/blob/v0.14.9/.github/workflows/release.yml)
- [C14: ReceiveCommand, JSON-RPC `receive` with `maxMessages` and `timeout`](https://github.com/AsamK/signal-cli/blob/v0.14.9/src/main/java/org/asamk/signal/commands/ReceiveCommand.java)
- [P1: presage-store-sqlite schema](https://github.com/whisperfish/presage/blob/6f630791b2c7bf3d5b00c75f5a6c94131bf79c8c/presage-store-sqlite/migrations/20250112201436_init.sql)
- [P2: presage issue #396, no history transfer](https://github.com/whisperfish/presage/issues/396)
- [P3: presage PR #447, link-and-sync archive (open)](https://github.com/whisperfish/presage/pull/447)
- [M1: signalmeow transfer archive download and parsing](https://github.com/mautrix/signal/blob/7e65e8ecc9802dc952565d523c20661c2a4aae26/pkg/signalmeow/backup.go)
- [M2: signalmeow link capabilities](https://github.com/mautrix/signal/blob/7e65e8ecc9802dc952565d523c20661c2a4aae26/pkg/signalmeow/provisioning.go)
- [M3: mautrix-signal backfill, staging cleanup](https://github.com/mautrix/signal/blob/7e65e8ecc9802dc952565d523c20661c2a4aae26/pkg/connector/backfill.go)
- [E3: linuxserver Signal Desktop image](https://github.com/linuxserver/docker-signal/tree/20a2ffb7084c7cb50fef8797a6bf911d6e8932cb)
- [D1: Desktop provisioner, `backup5` capability](https://github.com/signalapp/Signal-Desktop/blob/v8.32.0/ts/textsecure/Provisioner.preload.ts)
- [D2: Desktop backups service, link-and-sync import and plaintext export](https://github.com/signalapp/Signal-Desktop/blob/v8.32.0/ts/services/backups/index.preload.ts)
- [D3: Desktop export strings](https://github.com/signalapp/Signal-Desktop/blob/v8.32.0/_locales/en/messages.json)
- [D4: Desktop main process, `getSQLKey` and startup flags](https://github.com/signalapp/Signal-Desktop/blob/v8.32.0/app/main.main.ts)
- [D5: Desktop SQL migrations](https://github.com/signalapp/Signal-Desktop/blob/v8.32.0/ts/sql/migrations/index.node.ts)
- [D6: SignalService.proto](https://github.com/signalapp/Signal-Desktop/blob/v8.32.0/protos/SignalService.proto)
- [D7: Desktop message lookup by author and sent time](https://github.com/signalapp/Signal-Desktop/blob/v8.32.0/ts/sql/Server.node.ts)
- [D8: Desktop build expiry](https://github.com/signalapp/Signal-Desktop/blob/v8.32.0/scripts/get-expire-time.mjs)
- [D9: Desktop edit target, latest revision](https://github.com/signalapp/Signal-Desktop/blob/v8.32.0/ts/util/sendEditedMessage.preload.ts)
- [D10: Desktop delete matching, every revision](https://github.com/signalapp/Signal-Desktop/blob/v8.32.0/ts/messageModifiers/Deletes.preload.ts)
- [V1: libsignal backup.proto](https://github.com/signalapp/libsignal/blob/v0.106.0/rust/message-backup/src/proto/backup.proto)
- [V4: libsignal takeout JSON exporter](https://github.com/signalapp/libsignal/blob/v0.106.0/rust/message-backup/src/json/exporter.rs)
- [V5: Signal-Server linked device idle limit](https://github.com/signalapp/Signal-Server/blob/v20261022.0.0/service/src/main/java/org/whispersystems/textsecuregcm/storage/Device.java)
- [V6: Signal-Server sample config, message expiration](https://github.com/signalapp/Signal-Server/blob/v20261022.0.0/service/config/sample.yml)
- [S1: Support, Linked Devices (2026-09-22)](https://support.signal.org/hc/en-us/articles/360007320551-Linked-Devices)
- [S2: Support, Troubleshoot receiving messages](https://support.signal.org/hc/en-us/articles/360027913252-Troubleshoot-receiving-messages)
- [S3: Support, Android On-device Backups (2026-09-25)](https://support.signal.org/hc/en-us/articles/10066926526362-Android-On-device-Backups)
