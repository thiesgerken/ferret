# Public REST reference

Base URL: your reverse-proxied Ferret origin, for example `https://ferret.example.com`. All endpoints below are on the public server (8080 internally). [MCP](mcp.md) exposes its own agent-tool protocol on the same server; it is not another `/api` route.

## Authentication and errors

Every `/api` route requires an API-key bearer credential, or a live plugin run token where its scope permits. Keys use exact scope families from [auth](../auth.md). All document operations are constrained to the credential's user and source roots. Ordinary keys cannot write into connection-owned roots, including an ancestor sweep.

```sh
export FERRET_URL=http://127.0.0.1:3380
export FERRET_KEY='fk_replace_with_your_key'
```

| Status | Typical cause |
| --- | --- |
| 400 | invalid search/source/id syntax or unsafe origin URL |
| 401 | missing, invalid, revoked or expired credential; ended run token |
| 403 | missing scope, prohibited source, connection-owned write |
| 404 | missing/unreadable document, unknown sync |
| 409 | sync already committed |
| 415 / 422 | JSON content type / body deserialization rejection |
| 500 | database/internal error (`{"error":"internal error"}`) |
| 503 | semantic-only query cannot obtain its query embedding |

Application/extractor errors are `{"error":"message"}`; status and message matter, not an invented numeric error code. Framework body-size limits/proxy errors and route-not-found responses need not share that envelope. No endpoint returns 201 merely because a put created an item: successful JSON endpoints here return 200.

## Search: `GET /api/search`

Requires `search:read`.

| Parameter | Contract |
| --- | --- |
| `q` | required positive words, quoted phrases, `-excluded` tokens |
| repeated `source` | optional source roots and descendants; omit for all authorized roots |
| repeated `author` | any selected author |
| repeated `content_type` | `markdown`, `plain`, `email` |
| `from`, `to` | creation timestamp at/after `from`, strictly before `to` |
| `k` | 1–100, default 10 |
| `offset` | unsigned number of ranked hits to skip, default 0 |
| `model` | configured model name, otherwise user/configured default |
| `collapse` | boolean, default true; collapse same-source threads |
| `mode` | `hybrid` (default), `keyword`, `semantic` |

Unknown search parameters are rejected. Values are URL-encoded; arrays are repeated keys, not JSON/comma-delimited values.

```sh
curl --fail-with-body -sS -G "$FERRET_URL/api/search" \
  -H "Authorization: Bearer $FERRET_KEY" \
  --data-urlencode 'q=Brocken' --data-urlencode 'source=demo/chat' \
  --data-urlencode 'mode=keyword' --data-urlencode 'collapse=false' \
  --data-urlencode 'k=10' --data-urlencode 'offset=0'
```

Response fields:

- `hits`: best-first documents with `document_id`, source/external id, content type, nullable title/url/thread, authors, independently nullable origin `created_at`/`modified_at`, nullable writer, keyword_score/similarity, collapsed count and best `chunk`.
- `chunk`: `start_offset`, `end_offset` in Unicode scalar values and `parts`, each `{text, highlight}`. Highlighted parts are data, not HTML. Join all parts for the matched passage.
- `writer`: nullable `{kind: "connection"|"api_key", id, name, icon}`.
- `total`, `total_is_lower_bound`, `documents_in_scope`, `has_more`, `facets`, `took_ms`, `semantic_unavailable` and selected `model`.
- `facets.sources`: `{value,count,writer}`; `facets.authors`: `{value,count}`; `facets.are_lower_bounds` indicates candidate-bounded counts.

`documents_in_scope` is informational source-scope metadata, cached for 60 seconds per user and
effective source roots after narrowing to the key's allowed sources. Query words, exclusions,
time, authors and content types do not narrow this count. Root permutations/duplicates share a
snapshot. Hits, matching `total` and facets remain fresh, so this count can temporarily lag
ingestion/deletion or be lower than the current matching total.

For the seeded message, a hit's external id is `harz-trip/4` and its parts join to `And a Brocken hike on Saturday if the weather holds?`. Ids, timings, scores and broader hybrid rankings vary. See [search](../search.md) for ranking/candidate/count semantics. Follow pages with the same arguments and an increased offset; `has_more=false` ends this ranking, not a full document export.

## Read a document: `GET /api/documents/{id}`

Requires `search:read` **or** `ingest:read` (also granted by ingest write). `{id}` is a Ferret UUID, not the origin's external id. Unknown or out-of-scope document ids return 404.

```sh
curl --fail-with-body -sS "$FERRET_URL/api/documents/$DOCUMENT_ID" \
  -H "Authorization: Bearer $FERRET_KEY"
```

Returns the full stored document: `id`, `source`, `external_id`, `content_type`, nullable `title`, `text`, nullable `url` and `thread`, `authors`, independently nullable origin `created_at` and `modified_at`, `meta` and nullable `writer`. Unknown dates are JSON `null`, never observation time or epoch substitutes.

## Read a thread: `GET /api/documents?source=&thread=`

Same read scopes. Both query parameters are required; source is exact, not recursive. Returns `{"documents":[...]}` with full documents ordered by creation time then id, with unknown creation dates last (id breaks ties among them). Viewer previous/next navigation follows the same ordering. Empty/unauthorized scope yields an empty list. This endpoint has no page parameter and is not a general collection listing.

```sh
curl --fail-with-body -sS -G "$FERRET_URL/api/documents" \
  -H "Authorization: Bearer $FERRET_KEY" \
  --data-urlencode 'source=demo/chat' --data-urlencode 'thread=harz-trip'
```

## Upsert: `PUT /api/documents`

Requires `ingest:write`. Send a complete JSON object:

| Field | Type and requirement |
| --- | --- |
| `source` | required valid path, such as `manual/notes` |
| `external_id` | required nonempty stable string |
| `content_type` | required lowercase `markdown`, `plain` or `email` |
| `title` | optional/null string |
| `text` | required string; may be empty but then yields no chunks |
| `url` | optional/null origin string; javascript/data/vbscript schemes rejected |
| `thread` | optional/null string, local to exact source |
| `authors` | required array of strings |
| `created_at`, `modified_at` | required keys; each independently an RFC 3339 string or `null` for an unknown origin date |
| `meta` | object, defaults to `{}` |

Unknown fields are rejected. A put replaces stored fields at the same `(user,source,external_id)`, not a patch. See the complete runnable [ingestion example](../ingestion.md).

Before exclusion checks, hashing or chunking, Ferret removes U+0000 (NUL) from `text`, `title`, each `authors` string and metadata string values at every nesting depth, including arrays and objects. Other characters and non-string metadata values are preserved. Read-back and chunk offsets refer to this normalized text. Dirty and equivalent clean payloads have the same unchanged/hash behavior.

NUL in `source`, `external_id`, `thread`, `url` or any metadata object key returns 400 with a field-specific error. These fields are never silently rewritten: doing so could change an identity, URL or metadata key. A rejected put leaves any existing document unchanged.

Successful response is exactly one of these shapes:

```json
{"outcome":"created","id":"01900000-0000-7000-8000-000000000001"}
```

`updated` and `unchanged` use the same id-bearing shape. `{"outcome":"excluded"}` has no id. The UUID above only illustrates shape; obtain actual ids from your response. Chunking and activity commit with the write; embeddings happen later. Stable identical payloads refresh seen time/writer without rechunking.

## Delete: `DELETE /api/documents?source=&external_id=`

Requires `ingest:write`. Parameters identify one exact document. Success: 204 with no body. Unknown identity: 404. URL-encode path/id values, including spaces/slashes. Deletion removes the searchable copy, never the origin.

```sh
curl --fail-with-body -sS -X DELETE \
  "$FERRET_URL/api/documents?source=manual%2Fnotes&external_id=note-42" \
  -H "Authorization: Bearer $FERRET_KEY"
```

## Reconciliation

All three operations require `ingest:write`. JSON request types reject unknown fields.

| Endpoint | Request | 200 response |
| --- | --- | --- |
| `POST /api/syncs` | `{"source":"manual/notes"}` | `{"sync_id":"<uuid>"}` |
| `POST /api/syncs/{id}/seen` | `{"source":"manual/notes","external_ids":["note-42"]}` | `{"seen":1}` |
| `POST /api/syncs/{id}/commit` | no body | `{"deleted":0}` |

Start validates a complete authorized sweep root. Seen's exact source must be the root or a descendant; unknown external ids are ignored and not counted. Commit sweeps all root/descendant documents not put/seen since start and closes the sync. A repeated seen/commit after closure returns 409. Missing syncs return 404. Sync access is user/root-bound rather than exclusive to the creating key. Puts do not take a sync-id parameter. The [ingester workflow](../ingestion.md) explains why enumeration failure must never reach commit.

## Users: `GET /api/users`

Requires `admin:read` (or `admin:write`) and an owner who is currently an admin. Returns a JSON array of `{id, username, display_name, is_admin, enabled}` for every user. No pagination and no password hashes. This is the only current public REST admin endpoint; connection/user/key mutations are GraphQL.

```sh
curl --fail-with-body -sS "$FERRET_URL/api/users" \
  -H "Authorization: Bearer $FERRET_ADMIN_KEY"
```

Implementation authority: [routes](../../crates/ferret-server/src/lib.rs), [shared wire types](../../crates/ferret-api/src/lib.rs), [documents](../../crates/ferret-server/src/documents.rs), [search](../../crates/ferret-server/src/search.rs), [syncs](../../crates/ferret-server/src/syncs.rs), [errors](../../crates/ferret-server/src/error.rs), [scope checks](../../crates/ferret-server/src/api_auth.rs).
