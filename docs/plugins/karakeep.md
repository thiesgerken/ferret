# Karakeep saved content

Use `karakeep` to search saved text and annotations, not just bookmark titles. Requires **Karakeep 0.34.0 or newer** with the current readable-content REST API. The plugin reads only the API-key user's own library. It does not discover other users or shared lists.

## Set up

In Karakeep, create an API key with exactly these read scopes:

- `bookmarks:read`
- `highlights:read`

No write, asset-download, tag-management or administrator scopes are needed. Tag names are included in bookmark responses, including AI-assigned tags. Give the connection its own source root, such as `saved/karakeep`, and choose a schedule, for example `*/15 * * * *` (every fifteen minutes):

```json
{
  "url": "http://karakeep:3000",
  "public_url": "https://saved.example.com",
  "token": "replace-with-karakeep-api-key"
}
```

`url` is the instance root reachable by the worker, **not** `/api/v1`. `public_url` is the optional browser-facing root and defaults to `url`. HTTP(S) instance paths are preserved; embedded credentials, query strings and fragments are rejected. Redirects are not followed, so point the worker directly at Karakeep rather than an interactive login proxy.

`token` is a top-level write-only field. Ferret encrypts it separately from visible connection configuration and provides it only to a claimed worker run. Changing, adding or removing the browser-only `public_url` preserves it. Changing the worker-facing `url` requires re-entry, even alongside a browser-link change. See [connections](../connections.md) for setup, **Run now** and exclusions.

## What is indexed

One bookmark becomes one plain-text document at the connection's source root, with the Karakeep bookmark ID as its stable external ID. Search hits open `{public_url}/dashboard/preview/{bookmarkId}`. Highlights and notes stay in that document, not separate hits.

The document includes:

- the bookmark title, or the extracted link title/upload filename when the custom title is unset or empty;
- the original URL, upload filename and tag names;
- all text from `/api/v1/bookmarks/{bookmarkId}/content?format=text`, following every continuation cursor beyond the default 12,000-character first response;
- bookmark notes under **Bookmark notes**;
- highlight text and highlight notes under **Highlights**, with **Quote** and **Note** labels and creation timestamps.

Links, text bookmarks, uploaded PDFs and images are included, whether archived or not, favourited or not. A bookmark with no extracted text still contributes its available title, URL, tags and annotations. Later full runs pick up completed crawling, OCR or extraction. **AI-generated summaries are never indexed or used as a fallback.**

Karakeep owns crawling, binary extraction, OCR and HTML-to-text conversion. Ferret consumes Karakeep's text only. It never fetches bookmarked URLs, downloads assets, converts HTML or tries an alternative extraction path after a failure. This is a search representation, not a rich-text archive.

Metadata contains bookmark type, archived/favourited flags, original URL, tag names and upload type/filename. It does not duplicate extracted bodies, annotations, asset inventories or AI summaries. Link authors are used when supplied. Creation uses the bookmark's creation time; modification uses its modification time (creation when unset), or the latest remaining highlight creation time if later. Karakeep does not supply highlight modification times here, so this timestamp is not a complete annotation-change cursor.

## Full reconciliation and failures

Every scheduled or manual run enumerates the complete bookmark inventory and rereads every bookmark's content and highlights. There is no incremental state or content cache. Tag renames, highlight edits/removals, note edits, extraction completion and bookmark deletions do not depend on timestamps changing. Identical documents use Ferret's normal unchanged detection and do not needlessly re-index. Existing exclusions remain excluded.

The plugin commits the missing-document sweep only after **all inventory pages, content pages, annotation responses and document writes succeed**. Missing required pagination fields, duplicate bookmark/highlight IDs, cursor loops, inconsistent content ranges or versions, and failed HTTP/JSON responses fail the run without committing the sweep. A successful empty content response is legitimate; a failed content request is not treated as empty text. A content change between pages (HTTP 409 or a changed content version) fails the run; the next full run starts from the beginning.

Earlier successful document writes in a failed run are **not rolled back**. They may already be searchable while missing-origin documents remain unswept. Karakeep's list/content/annotation requests are not one transaction or an inventory snapshot. Concurrent origin changes that the API does not expose as inconsistency can be reconciled by the next successful full run.

## Verify and troubleshoot

Inspect **Activity** for bookmark and put/unchanged/deleted counts. Search for a distinctive word near the end of a long saved article, a highlight note and an uploaded document's extracted text. Verify that annotations return the bookmark itself and the origin link opens in Karakeep. Repeat unchanged, edit a note/tag/highlight, then remove a bookmark and use **Run now** to observe reconciliation.

- **401/403:** check key validity and both read scopes. Do not grant write/admin scopes to fix ingestion.
- **404 or invalid API response:** check the root URL and Karakeep version. There is no legacy API fallback.
- **Redirect:** bypass the interactive login proxy with the worker-facing address.
- **Changed/incomplete content or inventory:** let edits/extraction settle and run again. Failed runs do not authorize deletion sweeps.
- **Sparse content:** inspect extraction/crawl status in Karakeep. Ferret does not run its own extraction or substitute AI summaries.
- **Wrong origin link:** set `public_url`; the next run updates links on the existing documents.

As with other HTTP plugins, the worker can reach operator-accessible services. Constrain its egress where connections are untrusted. Origin response bodies, decoder details, credentials and secret-bearing request URLs are not included in plugin errors or state.

Implementation: [plugin](../../crates/ferret-plugins/src/plugins/karakeep.rs), [behavioral tests](../../crates/ferret-server/tests/karakeep_plugin.rs). API contract: Karakeep v0.34.0 [REST routes](https://github.com/karakeep-app/karakeep/blob/v0.34.0/packages/api/routes/bookmarks.ts), [text pagination](https://github.com/karakeep-app/karakeep/blob/v0.34.0/packages/api/utils/readableContent.ts), [bookmark model](https://github.com/karakeep-app/karakeep/blob/v0.34.0/packages/trpc/models/bookmarks.ts) and [API-key scopes](https://github.com/karakeep-app/karakeep/blob/v0.34.0/packages/shared/types/apiKeys.ts).
