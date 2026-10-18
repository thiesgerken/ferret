# GraphQL UI API

`POST /graphql` is the UI API. Use it for signed-in-user queries, connection/key/settings mutations and admin management. External long-lived ingesters should use [REST](rest.md), not a captured personal session.

The canonical generated [frontend/schema.graphql](../../frontend/schema.graphql) describes every field/input/enum; [graphql.rs](../../crates/ferret-server/src/graphql.rs) implements resolvers and authorization. This guide intentionally does not duplicate the whole schema. GraphQL uses camelCase and uppercase enum values (`KEYWORD`, `SEARCH_READ`), unlike REST's lowercase snake_case.

## Authenticate in a shell

Login requires username/password and returns 204 plus a Secure HttpOnly session cookie. For a local plain-HTTP smoke, explicitly forward the cookie as the seed script does. Keep temporary header/key files private and remove them afterward:

```sh
export FERRET_URL=http://127.0.0.1:3380
export FERRET_PASSWORD=ferret-development
umask 077
headers=$(mktemp)
curl --fail-with-body -sS -D "$headers" -o /dev/null \
  "$FERRET_URL/auth/login" -H 'Content-Type: application/json' \
  -d "$(jq -n --arg password "$FERRET_PASSWORD" '{username:"admin",password:$password}')"
cookie=$(grep -i '^set-cookie:' "$headers" | sed 's/^[^:]*: *//; s/;.*//')
rm "$headers"

curl --fail-with-body -sS "$FERRET_URL/graphql" \
  -H "Cookie: $cookie" -H 'Content-Type: application/json' \
  -d '{"query":"{ me { username isAdmin } connections { id name source } }"}'
```

The shell snippet uses grep/sed as user tooling, not an API requirement. On HTTPS a normal cookie jar can also preserve the session. API-key bearer headers do not authenticate this endpoint. Check `errors` even on HTTP 200; successful transport is not successful execution. Do not persist this cookie as an integration credential.

## Search and inspect runs

```sh
curl --fail-with-body -sS "$FERRET_URL/graphql" \
  -H "Cookie: $cookie" -H 'Content-Type: application/json' \
  -d '{"query":"query($input:SearchInput!){search(input:$input){total hasMore semanticUnavailable hits{documentId title source url chunk{parts{text highlight}}}}}","variables":{"input":{"q":"Brocken","sources":["demo/chat"],"mode":"KEYWORD","k":10}}}'

curl --fail-with-body -sS "$FERRET_URL/graphql" \
  -H "Cookie: $cookie" -H 'Content-Type: application/json' \
  -d '{"query":"{connections{id source runs(limit:5){id status log error stats{put unchanged deleted}}}}"}'
```

Search and REST share retrieval logic. GraphQL adds UI concerns such as judgments and relative ranges (`DAY`, `WEEK`, `MONTH`, `YEAR`) in place of an explicit `from`; inspect the canonical enum before constructing other inputs. `document(id:)` returns full text and thread neighbors for the viewer. `explore(input:)` browses documents without query terms.

Activity query:

```graphql
query Activity($before: String) {
  activity(limit: 20, before: $before) {
    id
    cursor
    kind
    startedAt
  }
}
```

Pass the last row's cursor for the next older page. Limits are 1–200, default 50. `writer` filters by key/connection id; `allUsers:true` requires admin. Do not confuse this cursor with REST search's numeric offset.

`writerStatistics` always returns the signed-in user's current whole index, including for admins,
independently of `activity(writer: ..., allUsers: ...)`. `writer: null` is the explicit Unknown
writer group. Counts are exact; storage fields are estimated bytes:

```graphql
{
  writerStatistics {
    documents
    chunks
    storage { documents chunks embeddings indexes total }
    rows {
      writer { kind id name icon }
      sources
      documents
      chunks
      storage { documents chunks embeddings indexes total }
      models { model embedded rowBytes indexBytes }
    }
  }
}
```

Row sums reconcile with the returned user totals. Each percentage divides a row's count/storage
by the corresponding user total, using zero when the denominator is zero. `models` includes
unconfigured models with retained embeddings; `indexBytes` is the allocated share of that model's
HNSW index. The shared embedding cache is not included. See [writer statistics](../indexing.md#writer-statistics-and-estimated-storage)
for allocation, refresh cost and physical-storage exclusions.


## Create a scoped key

```sh
curl --fail-with-body -sS "$FERRET_URL/graphql" \
  -H "Cookie: $cookie" -H 'Content-Type: application/json' \
  -d '{"query":"mutation($input:NewApiKey!){createApiKey(input:$input){key apiKey{id name prefix}}}","variables":{"input":{"name":"Notes ingester","icon":"key-round","scopes":["INGEST_WRITE"],"sources":["manual/notes"]}}}'
```

`key` is returned only at creation. Store it securely, not in a transcript. Omit sources for all user sources; `[]` is invalid. `revokeApiKey(id:)` revokes without deleting indexed documents. UI edits can rename/change icons, not silently expand an existing key's scopes. See [auth](../auth.md).

## Create a connection

The `config` field is **JSON text inside the GraphQL input**, not a nested GraphQL object. jq avoids hand-escaping it:

```sh
input=$(jq -n '{input:{name:"Ferret documentation",icon:"git-branch",plugin:"git",source:"docs/example",schedule:"0 * * * *",config:({remote:"https://github.com/thiesgerken/ferret.git",include:["README.md"],url_template:"https://github.com/thiesgerken/ferret/blob/{commit}/{path}"}|tojson)}}')
jq -n --arg query 'mutation($input:NewConnection!){createConnection(input:$input){id source}}' \
  --argjson variables "$input" '{query:$query,variables:$variables}' |
  curl --fail-with-body -sS "$FERRET_URL/graphql" \
    -H "Cookie: $cookie" -H 'Content-Type: application/json' --data-binary @-
```

Use a fresh root and wait for plugin registration; creation schedules the first run immediately. Subsequent `runConnectionNow`, updates and deletion follow the [connection lifecycle](../connections.md), including root confirmation and credential re-entry rules. Query `pluginTypes { name configSchema }` for the registered JSON Schemas.

## Session lifecycle

`POST /auth/logout` revokes the presented session and clears its cookie; it succeeds even without a valid session. `POST /auth/password` takes `current_password` and `new_password` using the session cookie. It returns 204 and refreshes that cookie while invalidating the user's other sessions. These are REST-shaped auth endpoints outside `/api`, not GraphQL operations. Admin password reset/user creation use the schema mutations and admin session authorization.
