# Paperless-ngx

Use `paperless` to search OCR text and metadata from Paperless-ngx 3.0 or later (API version 10). Ferret stores the text, not the PDF or its attachments. Give a dedicated Paperless user visibility only into the documents it should index, then obtain its **My Profile → API Auth Token**. It needs document read access; do not use an administrator token just for convenience.

## Set up

Create a connection with plugin `paperless`, source `documents/paperless`, a name/icon and schedule `0 * * * *`:

```json
{
  "url": "http://paperless:8000",
  "public_url": "https://paperless.example.com",
  "token": "replace-with-paperless-token"
}
```

`url` is the instance as reached **from the worker**, not from your browser. It must reach the API directly, not an interactive login proxy. `public_url` is optional and defaults to `url`; use it when browser-facing and internal addresses differ. `token` is required and write-only.

Changing, adding or removing the browser-only `public_url` preserves the stored token. Changing the worker-facing `url` requires re-entering it, even alongside a browser-link change.

Open Activity after creation, inspect the document count and run outcome, then search the title or a distinctive OCR phrase in keyword mode. Open the hit's origin and confirm it reaches `/documents/<id>/details` on your public instance.

## Mapping and reconciliation

Each Paperless id becomes one plain-text document at the connection source, with its numeric id as `external_id`. Title and OCR content come from Paperless. The correspondent name becomes an author. Metadata includes visible correspondent, document type, tag names and archive serial number. There is no thread. Paperless's date-only creation time becomes noon UTC; the original modification timestamp is retained.

Every run lists all ids in stable id order, marks them seen, fetches changed documents and commits a sweep. Thus deleting a document or removing the token user's visibility deletes its indexed copy after a successful run. A ten-minute overlap around the modified cursor catches transaction timing races; unchanged puts are harmless. Changing instance addresses refetches all documents so stored deep links change too.

## Limits and troubleshooting

Redirects are rejected rather than followed to an authentication page. Check the worker-visible address if the log mentions a login proxy. Listings that change inconsistently while paging fail the run instead of risking a sweep. Missing visibility into a referenced tag/correspondent/type is logged and that metadata is omitted. Paperless credentials do not carry over into origin links: the human still signs in to Paperless separately.

See [connections](../connections.md). Source: [schema, API and mapping](../../crates/ferret-plugins/src/plugins/paperless.rs).
