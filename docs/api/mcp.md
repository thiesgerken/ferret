# MCP for agents

## Availability and credentials

Ferret exposes MCP at `/mcp` on current main (0.15.0). Use a deployment containing that implementation, with the reverse proxy forwarding `/mcp` to the server. Older builds without that route require an upgrade, not a different API-key scope.

Create an API key in Settings with `search:read` and only the source roots the agent should read. Omit the source restriction for all of that user's sources; an explicit empty array is rejected. Do not give an agent `ingest:write` merely to search. The endpoint uses API-key bearer authentication, not the UI session cookie or an OAuth sign-in flow.

## Client setup

```sh
claude mcp add --transport http ferret https://ferret.example.com/mcp \
  --header "Authorization: Bearer $FERRET_KEY"
```

Use your actual HTTPS origin and key. This is Streamable HTTP, not stdio or the old SSE endpoint. The included proxy and Helm HTTPRoute route `/mcp` to the public server. Protect the stored client configuration as a credential.

## Tools and workflow

| Tool | Arguments | Result |
| --- | --- | --- |
| `list_sources` | `{}` | sources visible to the key, counts and writer names |
| `search` | `query`; optional `sources`, `authors`, `threads`, `mode`, `model`, `limit`, `cursor` | hits with best matching chunk, origin URL, metadata, total and next cursor |
| `get_document` | `id` (a hit's document UUID) | full authorized text and metadata |

For example, MCP `tools/call` parameters are:

```json
{"name":"search","arguments":{"query":"cobalt torque wrench","sources":["manual"],"mode":"keyword","limit":5}}
```

Then call `get_document` with the returned `document_id`, or use the hit's `url` with the origin's own tools. `external_id` is not the UUID expected by `get_document`. `list_sources` takes no filters. `search` defaults to hybrid and ten hits, accepts limits 1 through 100, collapses threads, and returns `next_cursor` for subsequent pages. Pass the same search arguments with that cursor; it is not a snapshot of a changing index.

`threads` is an array of exact source/thread objects, for example `{"query":"car","authors":["Me"],"threads":[{"source":"chats/personal","thread":"alice"}]}`. Values inside a filter are alternatives; author/thread/source/query filters combine. A thread id alone is not globally unique. Authors describe the actual speakers of a document, not its chat's name; selecting a thread without an author also includes outgoing-only segments. Retain these filters when requesting the next cursor.

Search hits and full-document responses carry independently nullable `created_at` and `modified_at` origin dates. Null means unknown, not the ingestion date. Unfiltered search includes undated documents; explicit creation-date bounds exclude unknown creation dates.

Authorization is checked on every request and tool. A key only sees its user's documents within its allowed source roots. Hybrid search can return keyword-only hits with `semantic_unavailable`; semantic-only search fails when embeddings cannot serve the query. The tools are read-only. There is no MCP ingest tool.

## Diagnose before changing credentials

Distinguish proxy routing/older-build 404 errors from invalid/revoked keys (401) and insufficient `search:read` scope (403). A document outside the key's scope produces a tool error even if another client knows its id. MCP returns protocol/tool errors inside the JSON-RPC response; successful HTTP transport does not guarantee a successful tool result.

## Raw HTTP example

For a protocol smoke, send both accepted response types and initialize before tool calls. This server is stateless and returns JSON rather than requiring a session id:

```sh
curl --fail-with-body -sS "$FERRET_URL/mcp" \
  -H "Authorization: Bearer $FERRET_KEY" \
  -H 'Content-Type: application/json' -H 'Accept: application/json, text/event-stream' \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"ferret-smoke","version":"1"}}}'

curl --fail-with-body -sS "$FERRET_URL/mcp" \
  -H "Authorization: Bearer $FERRET_KEY" \
  -H 'Content-Type: application/json' -H 'Accept: application/json, text/event-stream' \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"search","arguments":{"query":"Brocken","sources":["demo/chat"],"mode":"keyword","limit":5}}}'
```

Tool results include structured content. Read `result.isError` and protocol `error`, not just the HTTP status. Search first to obtain a real UUID, then call `get_document` with it. Source and permission enforcement is shared with REST search/document reads; MCP additionally requires `search:read` at its entry point.

Implementation: [MCP server](../../crates/ferret-server/src/mcp.rs), [transport/auth tests](../../crates/ferret-server/tests/mcp.rs), [public routing](../../crates/ferret-server/src/lib.rs).
