# IMAP mailboxes

Use `imap` for one mailbox account. The worker lists mail across eligible folders and fetches new messages. Use an app password where the provider supports one, restricted to the account you intend to index. This plugin uses username/password login, not OAuth. The account's readable mail becomes a separate searchable copy in Ferret.

## Set up

Create plugin `imap`, source `mail/personal`, a name/icon and schedule `*/15 * * * *`:

```json
{
  "server": "imap.example.com:993",
  "tls": true,
  "username": "alex@example.com",
  "password": "replace-with-app-password",
  "exclude_folders": ["Trash", "Spam", "Junk", "Drafts", "Private"],
  "url_template": "mid:{message_id}"
}
```

`server`, `username`, `password` are required. Port defaults to 993, TLS defaults to true. This is implicit TLS, not a STARTTLS configuration. Disabling TLS sends the password in cleartext and is suitable only for a test server next to the worker. `exclude_folders` defaults to Trash/Spam/Junk/Drafts; providing a list replaces that default. Server folders marked trash, junk or drafts are always excluded. `url_template` is optional: omit it to use Ferret's viewer. A `mid:` origin requires an operating-system mail handler that understands it; Ferret cannot install that handler for you.

## Identity, threads and content

One message becomes one email document at the connection source. Message-ID is its stable identity, so copies across folders deduplicate and moves are invisible. The parser provides an identity for messages without a usable Message-ID. Subject, sender, date and recipient metadata come from the parsed mail. References/In-Reply-To connect messages into threads. Quoted earlier messages and common reply boilerplate are stripped so the same passage does not dominate every result. Attachments are not separately ingested as documents.

Each run lists all eligible messages, marks known identities seen, fetches new UIDs and commits a sweep. Folder cursors include UIDVALIDITY; a changed UIDVALIDITY refetches that folder. Settings that change message rendering trigger refetches. Moving the last eligible copy to an excluded folder removes the document on a successful sweep. Mail content is treated as immutable under an already-indexed identity; this is not an arbitrary in-place mailbox-edit detector.

## Verify and diagnose

Inspect Activity for folder/message/new counts and run errors. Search a distinctive subject with source `mail/personal`; open a hit's title and use the viewer's previous/next thread links to read individual messages. Test the origin link on the device where you actually read mail.

Check server address, certificate trust and app-password policy for connection failures. The worker must reach the mail server, not your browser. Do not turn off TLS to work around an untrusted public certificate. A provider that only supports OAuth needs a different ingestion route.

See [connections](../connections.md). Source: [schema and sync](../../crates/ferret-plugins/src/plugins/imap/mod.rs), [mail mapping](../../crates/ferret-plugins/src/plugins/imap/mail.rs), [mailbox transport](../../crates/ferret-plugins/src/plugins/imap/mailbox.rs).
