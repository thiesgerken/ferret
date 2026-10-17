# MCP for agents

## Availability prerequisite

**Stock main at the documentation baseline (0.14.0) does not expose `/mcp`.** The implementation is in [open PR #131](https://github.com/thiesgerken/ferret/pull/131), branch `128-mcp`, and was still unmerged when this guide was written. The README's earlier MCP description is not evidence that your deployed server contains it. Deploy a reviewed build containing that change before using the instructions below. Do not replace a working server with an unreviewed branch solely to follow this guide. Until then, use the [HTTP API](../ingestion.md) for agent integration.

The following contract describes the implementation inspected in that PR, not a guarantee for the released main build. Source: [mcp.rs in PR #131](https://github.com/thiesgerken/ferret/blob/128-mcp/crates/ferret-server/src/mcp.rs).

## Client setup on an MCP-enabled build

Create an API key in Settings with `search:read` and only the source roots the agent should read. An empty source restriction means all of that user's sources, not no access. Do not give an agent `ingest:write` merely to search. The endpoint uses API-key bearer authentication, not the UI session cookie or an OAuth sign-in flow.

```sh
claude mcp add --transport http ferret https://ferret.example.com/mcp \
  --header "Authorization: Bearer $FERRET_KEY"
```

Use your actual HTTPS origin and key. This is Streamable HTTP, not stdio or the old SSE endpoint. Ensure your reverse proxy actually routes `/mcp` to the MCP-enabled server; baseline proxy/chart routes may need the corresponding PR's changes. Protect the stored client configuration as a credential.

## Tools and workflow

| Tool | Arguments | Result |
| --- | --- | --- |
| `list_sources` | `{}` | sources visible to the key, counts and writer names |
| `search` | `query`; optional `sources`, `mode`, `model`, `limit`, `cursor` | hits with best matching chunk, origin URL, metadata, total and next cursor |
| `get_document` | `id` (a hit's document UUID) | full authorized text and metadata |

For example, MCP `tools/call` parameters are:

```json
{"name":"search","arguments":{"query":"cobalt torque wrench","sources":["manual"],"mode":"keyword","limit":5}}
```

Then call `get_document` with the returned `document_id`, or use the hit's `url` with the origin's own tools. `external_id` is not the UUID expected by `get_document`. `list_sources` takes no filters. `search` defaults to hybrid and ten hits, accepts limits 1 through 100, collapses threads, and returns `next_cursor` for subsequent pages. Pass the same search arguments with that cursor; it is not a snapshot of a changing index.

Authorization is checked on every request and tool. A key only sees its user's documents within its allowed source roots. Hybrid search can return keyword-only hits with `semantic_unavailable`; semantic-only search fails when embeddings cannot serve the query. The tools are read-only. There is no MCP ingest tool.

## Diagnose before changing credentials

A 404 on stock main is expected: the route is absent, not a missing scope. On an MCP-enabled build, distinguish proxy routing errors from invalid/revoked keys and insufficient `search:read` scope. A document outside the key's scope is not readable even if another client knows its id. No MCP runtime success is claimed for the main baseline; verify against the exact build you deploy.
