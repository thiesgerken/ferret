# API guide

Choose the surface by caller, not by which credential is easiest to copy.

| Surface | Use it for | Credential |
| --- | --- | --- |
| [REST `/api`](rest.md) | external ingesters, scripts, agents | scoped API key; plugin runs use live run tokens |
| [GraphQL `/graphql`](graphql.md) | the browser UI, personal settings and connection/admin management | signed-in user's session cookie |
| [MCP `/mcp`](mcp.md) | agent discovery tools using Streamable HTTP | API key with `search:read` |
| [Internal `/internal`](../plugins/README.md) | worker registration, claims, heartbeat and finish on port 8081 | shared worker PSK, private network only |
| `/auth` | password login/logout/change for browser sessions | password or current session, depending on operation |
| `/healthz` | database reachability probe on public server port | none; proxy need not publish it |

The REST reference owns machine endpoint contracts. [Ingestion](../ingestion.md) owns safe producer workflows; [auth](../auth.md) owns permissions and trust boundaries. Shared wire types live in [ferret-api](../../crates/ferret-api/src/lib.rs). The [GraphQL schema](../../frontend/schema.graphql) is canonical for the UI API.

The independent [WhatsApp archive HTTP API](whatsapp-archive.md) is a separate read-only service with its own mounted bearer Secret, not another ferret public endpoint. Its guide also specifies ferret's live-run-only archive-binding operation.

## Conventions

REST uses JSON, snake_case fields, RFC 3339 timestamps and UUID document/sync ids. GraphQL uses camelCase and uppercase enum values. Both scope document access to the authenticated user. A source root includes descendants by whole path segments, not arbitrary prefix matches.

Send `Authorization: Bearer <key>` to `/api` and `Content-Type: application/json` for JSON bodies. Browser sessions do not authorize REST, and ordinary API keys do not authorize GraphQL. Never publish the worker secret or run token as an agent key.

Most REST errors handled by the application are JSON `{"error":"message"}` with the relevant HTTP status; unknown routes/proxy errors may not be JSON. GraphQL can return an HTTP-success response containing `errors`, so check that array before reading `data`. Authentication transport errors can be non-200.

Pagination is surface-specific, not a universal convention: REST search uses `k`/`offset`; thread GET and users GET are unpaginated. GraphQL search/explore use their schema inputs, Activity uses a `before` cursor, and connections/runs have limits where declared. Do not invent `page` parameters on every endpoint. [MCP search](mcp.md) returns `next_cursor`; pass it as `cursor` with the same search arguments for the next page.

There is no public REST endpoint to create keys, manage connections, list arbitrary documents or export the database. Use the authenticated UI/GraphQL for management. For a full archive backup, use a database backup procedure rather than scraping search results.
