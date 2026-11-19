# Signal chat archive

The `signal` built-in plugin indexes the **Signal chat archive** that `ferret-signal-archive` keeps for one Signal account, not Signal itself. A linked `signal-cli` device is the collector; `ferret-signal-archive` stores what it receives in its own SQLite database and serves it over a private read-only HTTP API. Ordinary ferret workers run scheduled connections against that API. Nothing ever sends to Signal. Why ferret keeps this chat archive itself is [ADR 0009](../adr/0009-ferret-owns-the-signal-chat-archive.md).

## Chat archive and backups

Collector and chat archive run as one pod outside the ferret Helm chart, next to the cluster's other infrastructure: the official `signal-cli` image linked as an additional device of the account, and `ferret-signal-archive` with the database on a persistent volume. Expose the API only through an internal Service with a NetworkPolicy that admits the plugin workers; private HTTP plus NetworkPolicy is the trust model, the bearer token is not transport encryption. The exact HTTP contract is the [Signal archive API reference](../api/signal-archive.md).

**Backing up the chat archive is the operator's job.** After the one-time history import it is the only raw copy of everything the collector received. Signal keeps nothing to re-fetch, and ferret's documents are rendered segments, not the messages. Losing the database means losing that history.

## Connection configuration

Authenticate to `GET /v1/archive` on the private API and copy its `store_id`. Create a connection with plugin `signal`, a source such as `chat/signal`, a normal schedule, and that explicit chat archive identity:

```json
{
  "url": "http://signal-archive:8080",
  "token": "<64-character hexadecimal archive token>",
  "expected_store_id": "0123456789abcdef0123456789abcdef",
  "allow_chats": [],
  "block_chats": []
}
```

`url` is the API root reached directly by workers; redirects are not followed. `token` is required, encrypted by ferret and write-only. Changing `url` requires entering the token again; the other fields keep it.

`expected_store_id` is required, exactly 32 lowercase hexadecimal characters, and compared before any sync starts or any document is written. Every later request carries it too, so a chat archive replaced mid-run fails the run instead of sweeping the source. To switch to a new chat archive deliberately, edit the pin: that authorizes a full reconciliation against the new one and may remove documents only the old one had.

`allow_chats` and `block_chats` hold **exact chat IDs** from authenticated `GET /v1/chats?store_id=<id>`, never names. A chat ID is the counterpart's ACI for a direct chat, the group ID for a group and the account's own ACI for Note to Self. Empty `allow_chats` includes every chat; `block_chats` wins. An ID missing from the current inventory fails the run rather than silently letting a chat through. Changing the filters triggers a full reconciliation; re-including a chat imports its history.

## Documents

Chats become segments exactly as for [WhatsApp](whatsapp.md#documents-and-privacy-boundaries): Markdown with a bold sender line and UTC time per message, split at pauses of at least 24 hours or about 8,000 characters, one thread per chat (the raw chat ID), the chat name (or raw chat ID) as title, the distinct speakers of a segment as its authors, ferret's viewer as the origin.

- Indexed: text, captions, and attachment metadata (MIME type, file name, size, and whether it is a voice note). Binary media is never stored or indexed. Each attachment takes one line; captions follow the message text.
- Senders are `Me` for own messages, otherwise the name the account knows them by (the nickname it gave them, else the system contact name, else their profile name), else their phone number, else their ACI. A direct chat's title is that name or number, a group's its name. A rename re-renders exactly the chats it shows up in: the contact's direct chat and the chats they wrote in.
- **Disappearing and view-once messages are kept and indexed, and never expire.** This deliberately differs from Signal's own exports: what the account has read, ferret remembers. Decide whether that fits your privacy policy before connecting the chat archive, and block chats where it does not.
- Metadata holds the chat ID and kind (`direct`, `group`, `note_to_self`), the first message's ID (`<author ACI>/<sent timestamp in ms>`) and the last message's date.

## Reconciliation and status

The first run imports the whole chat archive. Later runs read the change journal from the saved checkpoint and rebuild every chat it names, plus chats whose inventory entry changed. The journal is never pruned, so there is no lost replay range and no expiry restart. Like WhatsApp, a run drains to a fixed boundary captured after traversal, and only a fully successful traversal sweeps segments that no longer exist.

A run succeeds while the collector is down, because the chat archive stays readable. Activity logs the collector status and when it last received anything from Signal; check those, not run success, to see whether Signal is still being collected.
