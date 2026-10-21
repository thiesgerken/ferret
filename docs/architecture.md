# Architecture

Ferret is a search copy, not a replacement for its origins. Plugins and HTTP ingesters copy text into the index; search returns a matching passage and an origin URL. The origin still owns editing, binary assets and its own authorization.

```mermaid
flowchart LR
    Browser[Browser] --> Proxy[Reverse proxy]
    Agent[HTTP client] --> Proxy
    Proxy --> UI[Static frontend]
    Proxy --> Public[Server public :8080]
    Worker[Plugin worker] -->|PSK: register / claim / heartbeat / finish| Internal[Server internal :8081]
    Worker -->|run token: document writes| Public
    Origin[Origin systems] -->|worker reads| Worker
    Public --> DB[(Postgres: ParadeDB + pgvector)]
    Internal --> DB
    Pump[Server embed pump] --> DB
    Pump --> Endpoints[Embedding endpoints]
    Public -->|query embedding| Endpoints
```

## Components and ownership

- **Frontend:** a statically exported Next.js app, served by nginx. It talks to `/graphql` using the browser session and `/auth` for login/password/logout. It is not a separate authorization service.
- **Server:** Rust/axum process with public and internal listeners. It applies migrations, bootstraps users, authorizes requests, stores/chunks documents, executes search, manages connection schedules and records Activity. GraphQL and REST reuse the same domain code.
- **Plugin workers:** independent Rust processes that advertise plugin schemas and pull due work. No inbound webhook or worker service is required. A worker reaches the origin, receives credentials only for its claimed run, and ingests using the public API with a source-limited run token.
- **WhatsApp archive API (optional):** an independent Rust/axum container reads one wacli archive through short read-only SQLite transactions and exposes raw allowlisted archive data to the normal WhatsApp plugin. It has no ferret database access or collector lifecycle responsibility. Only its transport DTOs, without SQLite/server dependencies, are linked into standalone workers. See [setup and boundaries](plugins/whatsapp.md).
- **Postgres:** the persistent store for users, sessions, keys, encrypted connection credentials, runs/cursors and bulk plugin inventories, documents, chunks, BM25 indexes, model vectors/cache, failures and judgments. ParadeDB and pgvector are requirements, not optional remote search services.
- **Embedding endpoints:** an OpenAI-compatible endpoint per model (a TEI, a proxy such as LiteLLM, or a cloud API), one for chunks and one for queries if they differ. The server runs the pump and checks every answer; routing, failover and serving are the endpoint's job.
- **Proxy:** routes static pages to the frontend and `/graphql`, `/auth`, `/api`, `/mcp` to the public server. Internal port 8081 stays private. [MCP](api/mcp.md) exposes read-only search/document/source tools using scoped API keys.

## Data flows

### Ingestion

An HTTP producer authenticates with an API key; a plugin uses its live run token. A put checks user/source ownership and exclusions, computes document change identity, updates text and chunk rows transactionally, and records writer activity. Unchanged puts mark the document seen without rewriting chunks. Full-listing producers use sync/seen/commit to sweep absent documents. See [ingestion](ingestion.md), [indexing](indexing.md) and [plugin contract](plugins/README.md).

Syncs can optionally target one exact source/thread, leaving sibling threads and descendants alone. A live connection can also atomically flatten a source namespace while preserving document UUIDs, payloads, derived rows and exclusions. This generic identity operation precedes reconciliation and does not authorize sweeping incomplete imports. Plugins own their layout transforms; the core has no origin-specific source rules. See the [REST contracts](api/rest.md#reconciliation).

The pump observes pending chunks, claims bounded work with database advisory locks and sends title/chunk inputs to the model's embedding endpoint. Ingestion does not wait for this network call. The same database coordinates multiple server pumps and run claims. Missing notifications are recovered by polling/reconnecting, not by requiring a separate message broker.

### Search

The caller's user and source restrictions narrow the corpus. Query parsing separates positive words, phrases and exclusions. Keyword search reads the BM25 chunk index; semantic search embeds the positive query and finds model-specific vector neighbors. Hybrid fuses ranks before selecting the best chunk per document and optionally the best document per thread. [Search](search.md) explains bounded candidates and partial results.

### Connections

Creation makes a connection due. A registered worker claims it, receives the previous successful state and a token, heartbeats while running, then reports outcome and new state. Expired leases produce lost runs. Origin writes are incremental, not rolled back if the run later fails. [Connections](connections.md) is the user guide; [plugins](plugins/README.md) is the wire/implementation contract.

The files plugin stages metadata fingerprints in bounded HTTP batches in a server-owned inventory.
Its cursor references only a successful generation and settings fingerprint. Failed/lost runs and
remembered-exclusion resets invalidate reuse; the next worker fully reconciles even if older
inventory rows still exist. Read-only filesystem mounts belong only to workers, consistently across
replicas. See [mounted files](plugins/files.md) for availability checks and operational limits.

## Boundaries and failure domains

Public credentials are deliberately separate: session cookies for UI GraphQL, scoped API keys/run tokens for REST, shared worker PSK for the private worker surface. Database user isolation is enforced by application queries; the server and database operator remain trusted. A compromised worker/PSK is broader than a compromised single API key. See [auth](auth.md).

Database failure affects health and request handling. An embedding endpoint's failure does not stop document ingestion, keyword search or other models: hybrid degrades explicitly and semantic-only search fails. Origin failure fails a run without committing an incomplete enumeration. The static UI may still load while server APIs fail. These are distinct health signals.

Source map: [startup](../crates/ferret-server/src/main.rs), [routers](../crates/ferret-server/src/lib.rs), [GraphQL](../crates/ferret-server/src/graphql.rs), [workers](../crates/ferret-plugins/src/worker.rs), [migrations](../crates/ferret-server/migrations/), [proxy](../docker/traefik-dynamic.yml). For rationale rather than current procedure, see [ADRs](adr/) and [conception](conception/README.md).
