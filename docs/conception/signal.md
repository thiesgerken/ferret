# Signal archive options

Status: research, 2026-10-24. Not a design decision and not an operator contract. The question is how ferret gets a one-time history import plus continuous, read-only ingestion of a Signal account whose primary device is an Android phone, running always-on in k3s, without ever sending. Versions checked: signal-cli v0.14.9, libsignal v0.106.0, Signal-Desktop v8.32.0, Signal-Android v8.32.3, Signal-Server v20261022.0.0, presage main `6f63079`, mautrix-signal main `7e65e8e`.

## Result

There is no wacli analog for Signal. No maintained tool is collector, durable message archive and change journal in one. Every third-party collector either keeps no history (signal-cli) or keeps it in a store not meant as an archive (presage, gurk, mautrix-signal's staging table). ferret has to own the archive.

Recommendation: **signal-cli as a linked device feeding a small archive service we write, plus a one-time import of Signal Desktop's built-in JSON export**. Details and alternatives below.

## Existing tools

| Tool | Collects | Persists messages | History import | License |
|---|---|---|---|---|
| signal-cli 0.14.9 | yes, JSON-RPC/D-Bus | no | no | GPL-3.0 |
| signal-cli-rest-api 0.101 | wraps signal-cli | toy plugin only | no | MIT |
| presage + presage-store-sqlite | yes (Rust library) | yes, `thread_messages` | no, PR open | AGPL-3.0 |
| gurk-rs 0.10.2, Flare 0.22.7 | yes (presage clients) | yes, client-internal | no | AGPL-3.0 |
| mautrix-signal v0.2610.0 | yes (Go, `signalmeow`) | into Matrix | **yes**, link-and-sync | AGPL-3.0 |
| Signal Desktop 8.32 | yes | yes, SQLCipher | yes, link-and-sync | AGPL-3.0 |
| signal-export, signalbackup-tools | no | no (readers) | read Desktop DB / Android `.backup` | various |

- **signal-cli** has no messages table. Its account database holds sessions, keys, recipients, groups and storage records. `message_send_log` keeps outgoing content for one day for retry receipts. The message cache holds an envelope only between server ack and handler completion, longer only after an untrusted-identity failure. The maintainer: "signal-cli also doesn't store a message history". [C4][C11][C12]
- **signal-cli-rest-api** ships a persistence plugin that stores raw envelope JSON in SQLite, but only `dataMessage` (messages sent from the phone are dropped), fed by an unretried webhook. Its json-rpc mode runs the daemon in the default receive mode and drops events with no websocket listener. [R1][R2]
- **presage** does persist history: `thread_messages(ts, thread_id, sender_service_id, ..., content_body BLOB)` with `PRIMARY KEY (ts, thread_id)`, storing the raw `Content` protobuf, written with insert-or-replace. Two group members sending in the same millisecond would collide (inference from the schema). No change journal. Linking as a secondary device works. Link-and-sync history is not supported: the maintainer says so in issue #396, and the PR to fetch the transfer archive (#447) is open. [P1][P2][P3]
- **gurk-rs** keeps its own SQLite on top of presage and hard-deletes expired disappearing messages. No API, no history import. Flare uses presage's store. [P4]
- **mautrix-signal**'s `signalmeow` is the only third-party client that receives the link-time history transfer: it advertises `backup5` when linking, long-polls `/v1/devices/transfer_archive` for up to an hour, decrypts and reads libsignal backup frames (version 1). The archived messages land in a staging table keyed by `(account, sender, DateSent)` that is deleted after backfill into Matrix. Durable history is the Matrix homeserver. [M1][M2][M3]
- **signald** is deprecated and states it no longer works against production servers. [unverified beyond the repo description]

## History acquisition

**Link-and-sync ("Transfer Message History").** Signal's support article: "All your chats and the last 45 days of media can be synchronized from your mobile phone when you link a device." So it is the complete message history, with only media limited to 45 days. Choosing "Don't transfer" means reinstalling the linked client to get it later. [S1]

Mechanism: the linking client advertises `backup5` in the `sgnl://linkdevice` URI. Desktop does so only on a never-registered install. The phone generates a random backup key, sends it as `ProvisionMessage.ephemeralBackupKey`, exports with `BackupMode.LINK_SYNC` and no message cutoff, uploads to the attachment CDN and `PUT`s `/v1/devices/transfer_archive`. The linked client long-polls the `GET` of that endpoint (timeout 1 to 3600 s). The payload is the libsignal message-backup format. [D1][A1][A2][V1]

signal-cli advertises only `capabilities=nopni` ("Mode.Link(false) does not advertise any capabilities itself") and has no transfer-archive code. It cannot receive this transfer. Whether the phone then hides the transfer option is an inference from the capability check, not verified. [C10]

**The message-backup format** is public: `backup.proto` in libsignal, a `BackupInfo` header followed by frames (AccountData, Recipient, Chat, ChatItem, ...). Encryption is HKDF-derived AES-256-CBC plus HMAC-SHA256 over gzipped, length-prefixed frames; the key derives from the 64-character recovery key (account entropy pool) and the ACI, or from the ephemeral key for link-and-sync. libsignal ships `validator`, `decrypt_backup` and `binproto_to_json` tools, which accept a local backup's metadata file plus the recovery key. AGPL-3.0. [V1][V2][V3]

**Desktop's own JSON export.** Since v8.16.0 (2026-06-24, flag removed) Desktop has Settings > Chats > Export chat history: "Export a machine-readable JSON copy of all your chats. Disappearing messages will not be exported." It writes `signal-export-<ts>/main.jsonl`, the backup frames as one JSON object per line rendered by libsignal's exporter, plus `metadata.json` and optional media. The exporter's takeout mode drops every ChatItem with `expiresInMs` and clears view-once attachments. On Linux the OS-auth step uses polkit. [D2][D3][V4]

This makes the prior plan's `signalbackup-tools --dumpdesktopdb` unnecessary. That tool works, including `encryptedKey` via Keychain/Secret Service/KWallet, but reads Desktop's internal schema and warns that its SQLite must not lag Desktop's. The JSON export uses the versioned backup schema instead. [E1][E2]

**Android on-device backups** now use the same format as a folder `signal-backup-YYYY-MM-DD-HH-MM-SS`, restorable with the 64-character recovery key. Disappearing messages with timers of one day or less are excluded. This is an alternative history source that needs no Desktop, decrypted with libsignal's tools. Not verified end to end. [S3][A3]

**Secure Backups** (remote, paid above the free tier) contain "all of your text messages and the last 45 days of media" and exclude view-once and messages disappearing within 24 hours. Download needs the SVR-B forward-secrecy token and backup credentials. No open downloader found. [S4]

**Desktop database.** SQLCipher; the key is in `config.json` as `encryptedKey` (Electron safeStorage: macOS Keychain, Linux libsecret/KWallet) or plaintext `key`. On Linux the `basic_text` backend counts as no encryption and stores the key in plaintext; switching backends later throws `SafeStorageBackendChangeError`. The schema moved from migration 1440 (v7.70.0, 2025-09) to 1830 (v8.32.0, 2026-10), about three migrations a month; messages still carry a `json` column. [D4][D5]

## Continuous ingestion via signal-cli

**Linking.** `signal-cli link -n NAME` prints an `sgnl://linkdevice` URI to scan; never `register`. After linking it requests contacts, groups, blocked list and keys from the phone. At most five linked devices. [C2][C10][S1]

**Receive mode decides durability.** `daemon --receive-mode` is `on-start` (default), `on-connection` or `manual`. In `on-start` the daemon keeps receiving with zero clients; each envelope is acked to the server, handed to whichever handlers exist, then deleted from the cache. With no client attached it is gone. In `on-connection`/`manual` receiving runs only while a client is connected or subscribed, so messages stay queued on the server otherwise. The man page: "In order to not miss messages, automatic receiving of messages can be disabled with the `--receive-mode=manual` parameter." [C3][C5][C6]

Residual loss: JSON-RPC notifications are not acked by the client. If the consumer disconnects mid-delivery the in-flight envelope is dropped (envelopes are fetched one at a time, so at most one per disconnect). A signal-cli crash between server ack and handling is covered by the cache retry. The v0.14.9 HTTP SSE endpoint replays the last 1000 events by `Last-Event-ID`, in memory only, lost on daemon restart. [C4][C7][C8]

**Events** (`receive` notifications, `envelope` record) cover what an archive needs: [C9]

- Envelope: `sourceUuid` (ACI), `sourceNumber`, `sourceDevice`, `sourceName` (looked up at receive time, not historical), `timestamp`, `serverReceivedTimestamp`.
- `dataMessage`: `timestamp`, `message`, `quote` (`id` = sent timestamp, `authorUuid`), `mentions`, `attachments`, `groupInfo` (`groupId`, `revision`, `type` UPDATE/DELIVER), `expiresInSeconds`, `isExpirationUpdate`, `viewOnce`, `reaction` (`targetAuthorUuid`, `targetSentTimestamp`, `isRemove`), `remoteDelete` (`targetTimestamp`), `adminDelete`, polls, pins.
- `editMessage`: `targetSentTimestamp` plus a full `dataMessage`.
- `syncMessage.sentMessage`: messages sent from the phone, with `destinationUuid` or `groupInfo`, and an optional `editMessage`.
- Group v2 updates only signal that something changed; names and members come from `listGroups`. Contact names (system, profile, nickname) come from `listContacts`.
- Not exposed: "delete for me" sync and view-once-opened sync.

**Server and device limits.** Linked devices are unlinked after 45 days of inactivity, and the phone must come online every 30 days (support article, matching `ALLOWED_LINKED_IDLE_MILLIS = 45 days` in the server). Queued messages expire: the server sample config sets `messages.expiration: P30D` and support says messages queued for a Desktop inactive 30 days are dropped. The production value is not visible in source. [S1][S2][V5][V6]

**Upgrades.** signal-cli README: "The official Signal clients expire after three months and then the Signal-Server can make incompatible changes. So signal-cli releases older than three months may not work correctly." Releases are roughly monthly. The server's version blocking targets official user agents; third-party clients are rejected only when they lack required capabilities (currently SPQR). Breakage therefore comes from protocol changes, not a version string. [C1][V7]

**Runtime.** Java 25; the libsignal native library is bundled for x86_64 Linux/macOS/Windows. Official images `ghcr.io/asamk/signal-cli:<version>` and `:<version>-native` (GraalVM, still labelled experimental) are x86_64 only; amd64+arm64 builds exist from gitlab.com/packaging and bbernhard's native builds. No published memory numbers; signal-cli-rest-api only says the JVM mode "requires more memory" than native. [C1][C13][R1]

## Message identity

Signal addresses a message by **author ACI plus sent timestamp** (`AddressableMessage`). Edits (`targetSentTimestamp`, author implied), remote deletes (`targetSentTimestamp`, author implied), reactions and quotes (`targetAuthorAci`/`authorAci` plus timestamp) all point back with that pair. Desktop looks messages up by author (or "outgoing") plus `sent_at`, including edit revisions' timestamps; edits keep the original's identity. In the backup format a ChatItem has `authorId` (a Recipient frame carrying ACI or E164), `dateSent` and `revisions`. [D6][D7][V1]

So `(chat, author ACI, sent timestamp)` is a sound dedupe key between the JSON export and the live stream. Caveats: own messages need the account's ACI as author; a few old recipients may have only an E164.

## Disappearing and view-once messages

Messages carry `expireTimer`/`expireTimerVersion` and an expiry start; view-once is a flag with a `ViewOnceOpen` sync. In backups, ChatItems carry `expiresInMs`/`expireStartDate`, and a viewed view-once attachment is null. libsignal enforces per purpose: device transfer keeps unexpired short timers, remote backup drops timers under 24 h, takeout export drops every expiring message and all view-once media. [D6][V1][V3][V4]

Signal's own standard is therefore: archives do not keep disappearing content, and never view-once media. This differs from the WhatsApp plugin, which indexes retained ephemeral messages [F1]. Because the Desktop export already omits every expiring message, the live stream should too, or history and live data follow different rules. Proposed default: drop messages with a nonzero timer and view-once messages at the archive. Honoring expiry (index, then delete at expiry) is the more faithful alternative and costs a scheduled delete path. Policy decision for Thies.

## Headless Desktop

No headless mode: v8.32.0's main process knows `--start-in-tray` but no flag to run without a display. Container images (linuxserver, kasmweb) are interactive VNC/web desktops run with `--no-sandbox`, not collectors. No project continuously reads a live Desktop database in a container. Combined with the 90-day build expiry (30 if it cannot auto-update, shortenable remotely), the schema churn above and reading a SQLCipher database the app is writing, this is the worst always-on option. [D4][D8][E3]

## Options

**(a) signal-cli + own archive service. Recommended.**

- Pod with two containers: `signal-cli daemon --receive-mode manual` (pinned image, upgraded at least monthly) and a new `ferret-signal-archive` that subscribes over JSON-RPC, writes SQLite (chats, messages with revisions, contacts, a change journal) and serves a read-only HTTP API. Same pod, so consumer and daemon restart together and the one-envelope loss window stays narrow.
- History: link signal-cli first and confirm it receives. Then fresh-install Desktop on a workstation, link with Transfer Message History, run Export chat history without media, and import `main.jsonl` with an importer subcommand. Insert only absent `(chat, author, sent)` keys; the overlap between the two links is deduplicated. Unlink Desktop afterwards.
- Reuse: the whatsapp-archive contract (`archive`, `chats`, `messages`, `changes`, store-id pin, fixed-boundary journal drain, 409/410), its auth/error/keyset server skeleton, and the plugin's segmentation, Markdown rendering, escaping and reconciliation [F1][F2]. Share the code through a common chat module rather than faking WhatsApp JIDs in a Signal API.
- Effort: medium. The service is the new part (JSON-RPC consumer, schema plus journal, JSONL importer, API), a few times the size of `ferret-whatsapp-archive`. The plugin is small once segmentation is shared.
- Risks: monthly signal-cli upgrades; one in-flight message lost per consumer crash; queue expiry if the pod is down about 30 days; x86_64-only official images; no memory numbers. signal-cli runs as a separate GPL process, so ferret's MIT crates link nothing copyleft.

**(b) Headless Desktop + read-only DB API.** Rejected. Lowest upfront effort, highest ongoing cost, see above.

**(c) presage-based Rust collector.** Native to our stack and a single linked device, but no history import until presage #447 lands. Its store has no journal and a weak key, so we would still write our own archive. AGPL-3.0: it cannot live in ferret's MIT crates without a deliberate licensing decision. Revisit if link-and-sync lands, since it would then replace both signal-cli and the Desktop step.

**(d) signalmeow-based Go collector.** The only path to history plus live data through one third-party linked device today. Costs: Go plus CGO libsignal FFI, a library internal to a bridge without a stable API, AGPL, and we still build the archive. Full mautrix-signal plus a Matrix homeserver gives an archive with a change feed but is far too heavy for this. Fallback if the Desktop export step fails in practice.

## Open points

- The phone hiding Transfer Message History for signal-cli is inferred from capabilities, not observed.
- Whether the Desktop export carries edit revisions and group membership events in practice: check one real export before writing the importer.
- Android folder-backup decryption with libsignal's tools is untested.
- signal-cli memory footprint (JVM vs native) and the cluster's CPU architecture.
- Production queue TTL is not visible in Signal-Server source; the 30 days rest on the sample config and a support article.

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
- [C8: SSE replay buffer](https://github.com/AsamK/signal-cli/blob/v0.14.9/src/main/java/org/asamk/signal/http/SseEventBuffer.java)
- [C9: JSON output records](https://github.com/AsamK/signal-cli/tree/v0.14.9/src/main/java/org/asamk/signal/json)
- [C10: ProvisioningManagerImpl, link capabilities and sync request](https://github.com/AsamK/signal-cli/blob/v0.14.9/lib/src/main/java/org/asamk/signal/manager/internal/ProvisioningManagerImpl.java)
- [C11: MessageSendLogStore, one-day retention](https://github.com/AsamK/signal-cli/blob/v0.14.9/lib/src/main/java/org/asamk/signal/manager/storage/sendLog/MessageSendLogStore.java)
- [C12: signal-cli issue #1747, no message history](https://github.com/AsamK/signal-cli/issues/1747)
- [C13: signal-cli release workflow, container images](https://github.com/AsamK/signal-cli/blob/v0.14.9/.github/workflows/release.yml)
- [R1: signal-cli-rest-api README](https://github.com/bbernhard/signal-cli-rest-api/blob/74253a3a41c2cade40fd4a53cf3fb7a3fdf305dc/README.md)
- [R2: signal-cli-rest-api JSON-RPC client, dropped events](https://github.com/bbernhard/signal-cli-rest-api/blob/74253a3a41c2cade40fd4a53cf3fb7a3fdf305dc/src/client/jsonrpc2.go)
- [P1: presage-store-sqlite schema](https://github.com/whisperfish/presage/blob/6f630791b2c7bf3d5b00c75f5a6c94131bf79c8c/presage-store-sqlite/migrations/20250112201436_init.sql)
- [P2: presage issue #396, no history transfer](https://github.com/whisperfish/presage/issues/396)
- [P3: presage PR #447, link-and-sync archive (open)](https://github.com/whisperfish/presage/pull/447)
- [P4: gurk-rs v0.10.2](https://github.com/boxdot/gurk-rs/tree/v0.10.2)
- [M1: signalmeow transfer archive download and parsing](https://github.com/mautrix/signal/blob/7e65e8ecc9802dc952565d523c20661c2a4aae26/pkg/signalmeow/backup.go)
- [M2: signalmeow link capabilities](https://github.com/mautrix/signal/blob/7e65e8ecc9802dc952565d523c20661c2a4aae26/pkg/signalmeow/provisioning.go)
- [M3: mautrix-signal backfill, staging cleanup](https://github.com/mautrix/signal/blob/7e65e8ecc9802dc952565d523c20661c2a4aae26/pkg/connector/backfill.go)
- [E1: signalbackup-tools README](https://github.com/bepaald/signalbackup-tools/blob/c7a788985c1033b4fd4ea10676aba120bbd98b66/README.md)
- [E2: signal-export key handling](https://github.com/carderne/signal-export/blob/v3.9.3/sigexport/crypto.py)
- [E3: linuxserver Signal Desktop image](https://github.com/linuxserver/docker-signal/tree/20a2ffb7084c7cb50fef8797a6bf911d6e8932cb)
- [D1: Desktop provisioner, `backup5` capability](https://github.com/signalapp/Signal-Desktop/blob/v8.32.0/ts/textsecure/Provisioner.preload.ts)
- [D2: Desktop backups service, link-and-sync import and plaintext export](https://github.com/signalapp/Signal-Desktop/blob/v8.32.0/ts/services/backups/index.preload.ts)
- [D3: Desktop export strings](https://github.com/signalapp/Signal-Desktop/blob/v8.32.0/_locales/en/messages.json)
- [D4: Desktop main process, `getSQLKey` and startup flags](https://github.com/signalapp/Signal-Desktop/blob/v8.32.0/app/main.main.ts)
- [D5: Desktop SQL migrations](https://github.com/signalapp/Signal-Desktop/blob/v8.32.0/ts/sql/migrations/index.node.ts)
- [D6: SignalService.proto](https://github.com/signalapp/Signal-Desktop/blob/v8.32.0/protos/SignalService.proto)
- [D7: Desktop message lookup by author and sent time](https://github.com/signalapp/Signal-Desktop/blob/v8.32.0/ts/sql/Server.node.ts)
- [D8: Desktop build expiry](https://github.com/signalapp/Signal-Desktop/blob/v8.32.0/scripts/get-expire-time.mjs)
- [A1: Android link-and-sync export](https://github.com/signalapp/Signal-Android/blob/v8.32.3/app/src/main/java/org/thoughtcrime/securesms/linkdevice/LinkDeviceRepository.kt)
- [A2: Android backup repository, `exportForLinkAndSync`](https://github.com/signalapp/Signal-Android/blob/v8.32.3/app/src/main/java/org/thoughtcrime/securesms/backup/v2/BackupRepository.kt)
- [A3: Android local folder archives](https://github.com/signalapp/Signal-Android/blob/v8.32.3/app/src/main/java/org/thoughtcrime/securesms/backup/v2/local/ArchiveFileSystem.kt)
- [V1: libsignal backup.proto](https://github.com/signalapp/libsignal/blob/v0.106.0/rust/message-backup/src/proto/backup.proto)
- [V2: libsignal backup tool key arguments](https://github.com/signalapp/libsignal/blob/v0.106.0/rust/message-backup/src/bin/support/mod.rs)
- [V3: libsignal ChatItem validation per purpose](https://github.com/signalapp/libsignal/blob/v0.106.0/rust/message-backup/src/backup/chat.rs)
- [V4: libsignal takeout JSON exporter](https://github.com/signalapp/libsignal/blob/v0.106.0/rust/message-backup/src/json/exporter.rs)
- [V5: Signal-Server linked device idle limit](https://github.com/signalapp/Signal-Server/blob/v20261022.0.0/service/src/main/java/org/whispersystems/textsecuregcm/storage/Device.java)
- [V6: Signal-Server sample config, message expiration](https://github.com/signalapp/Signal-Server/blob/v20261022.0.0/service/config/sample.yml)
- [V7: Signal-Server remote deprecation filter](https://github.com/signalapp/Signal-Server/blob/v20261022.0.0/service/src/main/java/org/whispersystems/textsecuregcm/filters/RemoteDeprecationFilter.java)
- [S1: Support, Linked Devices (2026-09-22)](https://support.signal.org/hc/en-us/articles/360007320551-Linked-Devices)
- [S2: Support, Troubleshoot receiving messages](https://support.signal.org/hc/en-us/articles/360027913252-Troubleshoot-receiving-messages)
- [S3: Support, Android On-device Backups (2026-09-25)](https://support.signal.org/hc/en-us/articles/10066926526362-Android-On-device-Backups)
- [S4: Support, Signal Secure Backups (2026-10-14)](https://support.signal.org/hc/en-us/articles/9708267671322-Signal-Secure-Backups)
