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

Use your provider's IMAP hostname; `imap.example.com` is a placeholder.

`server`, `username`, `password` are required. Port defaults to 993, TLS defaults to true. This is implicit TLS, not a STARTTLS configuration. Disabling TLS sends the password in cleartext and is suitable only for a test server next to the worker. `exclude_folders` defaults to Trash/Spam/Junk/Drafts; providing a list replaces that default. Server folders marked trash, junk or drafts are always excluded. `url_template` is optional: omit it to use Ferret's viewer. A `mid:` origin requires an operating-system mail handler that understands it; Ferret cannot install that handler for you.

Changing `url_template` or `exclude_folders`, including adding/removing either field, preserves the stored password. Changing `server` (including its port), `tls` or `username` requires entering the password again, even when changing a safe setting in the same edit. Link changes refetch existing mails; folder exclusions sweep messages no longer selected and removing an exclusion can index them again.

## Identity, threads and content

One message becomes one email document at the connection source. Message-ID is its stable identity, so copies across folders deduplicate and moves are invisible. The parser provides an identity for messages without a usable Message-ID. Subject, sender, date and recipient metadata come from the parsed mail. References/In-Reply-To connect messages into threads; In-Reply-To (else the last References entry) names the mail a message answers, so the viewer shows the thread as a tree. Quoted earlier messages and common reply boilerplate are stripped so the same passage does not dominate every result. Attachments are not separately ingested as documents.

HTML bodies take precedence over plain-text alternatives. `style`, `script` and `template` elements and their contents are removed before conversion to text, regardless of their attributes. Forwards (`Fwd:`, `Fw:`, `WG:`) keep forwarded message content, but not those non-content elements.

Each run lists all eligible messages, marks known identities seen, fetches new UIDs and commits a sweep. Folder cursors include UIDVALIDITY; a changed UIDVALIDITY refetches that folder. Settings that change message rendering trigger refetches, and so does an upgrade that changes the rendering rules (e.g. recording reply parents): plan for one full mailbox download then. Moving the last eligible copy to an excluded folder removes the document on a successful sweep. Mail content is treated as immutable under an already-indexed identity; this is not an arbitrary in-place mailbox-edit detector.

After upgrading to the HTML non-content cleanup, the next sync refetches every message in eligible folders once to replace previously indexed CSS or other non-content text. This uses the existing rendering-rule cursor invalidation; no manual reset or database migration is needed. Allow for a full mailbox download and reprocessing on that run. Later successful syncs return to incremental fetching.

## Verify and diagnose

Inspect Activity for folder/message/new counts and run errors. Search a distinctive subject with source `mail/personal`; open a hit's title and use the viewer's thread tree to read individual messages. Test the origin link on the device where you actually read mail.

Check server address, certificate trust and app-password policy for connection failures. The worker must reach the mail server, not your browser. Do not turn off TLS to work around an untrusted public certificate. A provider that only supports OAuth needs a different ingestion route.

See [connections](../connections.md). Source: [schema and sync](../../crates/ferret-plugins/src/plugins/imap/mod.rs), [mail mapping](../../crates/ferret-plugins/src/plugins/imap/mail.rs), [mailbox transport](../../crates/ferret-plugins/src/plugins/imap/mailbox.rs).
