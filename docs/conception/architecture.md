# Architecture

## TLDR

- **ferret-server** (Rust) owns Postgres and is the only component that talks to it. It serves the
  UI's GraphQL API, a REST API for machines, and an internal API for plugin workers. It also
  schedules plugin runs and pumps chunks to each model's embeddings endpoint.
- **ferret-plugins** (Rust) workers pull plugin runs from the server via long polling and ingest
  through the same REST API any external ingester uses.
- **Embeddings endpoints** compute embeddings: per model an OpenAI-compatible endpoint (a TEI, a
  proxy like LiteLLM, or a cloud API) that the deployment provides.
- **frontend** (Next.js) is a copy of carapace's stack and look: Google-like search page, plus
  Explore, Connections, Settings and Admin.

Every component is stateless except Postgres, so server and workers scale by replicas. All
coordination goes through Postgres (`SKIP LOCKED` and advisory locks, `LISTEN/NOTIFY`). No message broker.

## Components

```text
                         ┌──────────────── Ingress ─────────────────┐
                         │ /           /graphql, /mcp, /api, /auth  │
                         ▼                   ▼                      │
                    ┌──────────┐      ┌──────────────────────┐      │   external ingesters,
 browser ─────────► │ frontend │      │ ferret-server        │ ◄────┘   agents (MCP, skills)
                    │ (Next.js)│      │  :8080 public        │
                    └──────────┘      │    GraphQL (UI)      │ ──────► embeddings endpoints
                                      │    REST (machines)   │          (per model, OpenAI API)
                                      │    MCP (agents)      │
                                      │  :8081 internal      │
                                      │    worker API        │ ──────► Postgres
                                      │  embed pump          │          (ParadeDB + pgvector)
                                      └──────────────────────┘
                                          ▲            ▲
                         long-poll claim, │            │ ingest (REST, run token)
                         heartbeat, finish│            │
                                      ┌──────────────────────┐
                                      │ ferret-plugins       │ ──────► origins (git, Paperless,
                                      │  (n replicas)        │          carapace, ...)
                                      └──────────────────────┘
```

## Repository layout

```text
crates/
  ferret-server/     binary `ferret`: APIs, scheduling, embed pump, migrations
  ferret-plugins/    binary `ferret-plugins`: worker loop and all plugin implementations
  ferret-api/        shared request and response types of the REST and internal APIs, plus a
                     small HTTP client for them
frontend/            Next.js, copied from carapace
charts/ferret/       Helm chart
docker-compose.yml   development only
```

The workspace split keeps compile times and the server image small: plugin dependencies (IMAP,
git, parsers) never end up in the server.

## APIs

Four APIs plus the login endpoints. The path decides the credential, each accepts exactly one
(details in [auth.md](auth.md)):

| API | Port | Consumers | Auth |
|---|---|---|---|
| GraphQL `/graphql` | public | the frontend | session cookie |
| `/auth/login`, `/auth/logout` | public | the frontend | password, exchanged for the session cookie |
| `/auth/password` | public | the frontend | session cookie plus the current password |
| REST `/api/...` | public | plugins, external ingesters, agents | API key or plugin run token (bearer) |
| MCP `/mcp` | public | agents (MCP clients) | API key with `search:read` (bearer) |
| REST `/internal/...` | internal | plugin workers | worker PSK |

- **GraphQL for the UI.** The UI reads nested, filtered data (connections with their runs and logs,
  facet counts in Explore, search hits with their documents), which is what GraphQL is good at.
  The server uses juniper with its own small axum handler instead of `juniper_axum`, whose request
  extractor also takes `GET`, `application/graphql` and forms, which the CSRF rules forbid
  ([auth.md](auth.md#browser-sessions)). The patterns come from moneta. The frontend uses urql
  with GraphQL Code Generator for typed documents. `ferret schema` prints the SDL into
  `frontend/schema.graphql`, codegen runs against that file, and CI fails when it's out of date.
- **REST for machines.** Ingest is a natural `PUT` of a document by id, agents call search with a
  plain `GET`, and scopes map one-to-one onto routes. A third-party ingester needs nothing but curl.
- **MCP for agents.** An agent with an MCP client needs no skill or wrapper script: it connects to
  `/mcp` and gets three tools whose descriptions tell it what ferret is and when to use it.
  `search` takes a query, sources, mode, model, limit and a cursor, and answers with hits (title,
  writer, source, best-matching chunk, scores, origin `url`, document id), the total and the next
  cursor. `get_document` returns one document's whole text and metadata. `list_sources` lists
  the sources the key may read, with document counts and writers. The server uses the official
  Rust SDK (`rmcp`) over Streamable HTTP, stateless and with JSON responses: every request stands
  alone, so any replica answers it and no session needs to be shared. Host validation is off: it
  guards local servers without credentials against DNS rebinding, and every request here carries
  a key.
- **The internal API is HTTP like the others.** It listens on its own port, which the Ingress
  doesn't expose and a NetworkPolicy restricts to the worker pods. Running workers elsewhere (over
  the internet) works, it is just not the default.

Search and document reads exist in GraphQL, REST and MCP. All call the same service functions, the
API layers stay thin.

## Plugin orchestration

A plugin configured by a user is a **connection** (Connections in the UI). Workers pull its runs.

### Data model

```sql
-- Upserted by workers at startup. The UI renders config forms from config_schema.
create table plugin_types (
    name           text primary key,           -- 'paperless', 'imap', 'git', ...
    config_schema  jsonb not null,             -- JSON Schema (schemars), secret fields marked writeOnly
    registered_at  timestamptz not null
);

create table workers (
    id            uuid primary key,
    name          text not null,               -- pod name, informational
    plugins       text[] not null,             -- capabilities: this worker only gets these
    last_seen_at  timestamptz not null
);

create table connections (
    id           uuid primary key,
    user_id      uuid not null references users on delete cascade,
    name         text not null,                -- shown instead of the source path
    icon         text not null,                -- lucide icon name, e.g. 'git-branch'
    plugin       text not null references plugin_types,
    source       text not null,                -- source root: documents go to it or below it
    config       jsonb not null,               -- without the secret fields
    credentials  bytea,                        -- the secret fields, sealed (see below), null without any
    schedule     text not null,                -- cron expression
    enabled      boolean not null default true,
    next_run_at  timestamptz not null,
    state        jsonb not null default '{}',  -- incremental cursor: IMAP UID, git commit, ...
    unique (user_id, source)
);

create table plugin_runs (
    id            uuid primary key,
    connection_id uuid not null references connections on delete cascade,
    worker_id     uuid not null references workers,
    status        text not null,               -- 'running', 'succeeded', 'failed', 'lost'
    started_at    timestamptz not null,
    heartbeat_at  timestamptz not null,
    finished_at   timestamptz,
    stats         jsonb,                       -- documents put, deleted, unchanged
    put           integer not null default 0,  -- the same, counted by the server while running
    unchanged     integer not null default 0,
    deleted       integer not null default 0,
    log           text not null default '',    -- capped
    error         text
);

-- An API key's writes, grouped into bursts: a write within 10 minutes of the burst's last one
-- belongs to it. With the plugin runs, the Activity page's feed (`activity` in GraphQL).
create table api_ingests (
    id          uuid primary key,
    user_id     uuid not null references users on delete cascade,
    api_key_id  uuid not null references api_keys on delete cascade,
    started_at  timestamptz not null,
    last_at     timestamptz not null,
    put         integer not null default 0,
    unchanged   integer not null default 0,
    deleted     integer not null default 0
);
```

### Disabling and deleting

- **Disable** stops runs and keeps the documents.
- **Delete** removes the connection and every document below its source root, after the user types
  the source root to confirm. The confirmation warns that documents the origin no longer has (e.g.
  chat exports) exist only in ferret.

### Exclusions

"Exclude from index" in the viewer deletes a document and adds it to its connection's exclusion
list: by `external_id`, or by `thread` for a whole chat or mail thread. A `PUT` for an excluded
document is dropped by the server and answered with `excluded`, so plugins need no knowledge of the
list. The list is visible and editable on the connection's page.

```sql
create table exclusions (
    id             uuid primary key,
    connection_id  uuid not null references connections on delete cascade,
    source         text not null,
    external_id    text,                -- exactly one of external_id and thread is set
    thread         text,
    created_at     timestamptz not null
);
```

- A thread exclusion covers every document of that `(source, thread)`, also ones that arrive later.
- Removing an exclusion resets the connection's `state`, so its next run starts over from scratch:
  an incremental plugin (Paperless) wouldn't otherwise fetch an unchanged document again.
- A document no connection owns (an API key wrote it) is deleted, but nothing is recorded: there's
  no connection to drop its next `PUT`, and API keys get no exclusion list of their own. The UI
  says so.

### Source ownership

Every source below a connection's root belongs to that connection exclusively: API keys can't
write there, and connection roots can't overlap. Otherwise a sync sweep would delete documents
someone else wrote. API keys write only to sources outside every connection's root, and a sync of
an API key can't contain a connection's root either. Reading and searching stay governed by the
key's own sources. A new connection's root must not hold documents yet: its first sync would sweep
what API keys wrote there.

### Run lifecycle

```text
POST /internal/workers/register   {name, plugins: [{name, config_schema}]}    -> {worker_id}
POST /internal/runs/claim?wait=30 {worker_id}                                -> 204 | {run}
POST /internal/runs/{id}/heartbeat {log_lines}                               -> 204, 409 once not live
POST /internal/runs/{id}/finish    {result: succeeded, state, stats, log_lines}
                                   | {result: failed, error, log_lines}      -> 204, 409 once not live
```

1. **Register**: at startup the worker upserts its plugin types and announces its capabilities.
   Every worker registers every plugin it knows. Dedicated workers per plugin (`--plugins imap`)
   would come for free from this, but don't exist yet.
2. **Claim (long poll)**: the server looks for an enabled connection of one of the worker's plugins
   with `next_run_at <= now()` and no live run, using `FOR UPDATE SKIP LOCKED`. On a hit it inserts
   a `plugin_runs` row, advances `next_run_at` to the next cron slot, and returns the run: config
   with decrypted secrets, `state`, and a run token. Otherwise it holds the request until the
   earliest `next_run_at` among the worker's plugins, a running run's lease expiry, a `NOTIFY` on
   `ferret_runs` (run now, a new or changed connection), or the wait timeout (at most 60 s).
   Connections of disabled users are never claimed.
3. **Ingest**: the worker runs the plugin, which calls the public REST API with the run token, like
   any external ingester. Full-state plugins wrap the run in a sync and commit at the end.
4. **Heartbeat** every 30 s, carrying new log lines.
5. **Finish**: status, the new `state`, stats. The server stores `state` only on success, so a
   failed run retries from the old cursor.

There is no scheduler process. Due runs are found at claim time, which works with any number of
server replicas.

**Lost runs**: a run whose heartbeat is older than the lease (2 min) counts as lost. The next claim
marks it `lost` and may start the connection again. An uncommitted sync deletes nothing, so a crashed
run is harmless.

### Plugin credentials

Secret config fields (marked `writeOnly` in the plugin's schema) are encrypted with
XChaCha20-Poly1305 under `FERRET_CONFIG_KEY`, a 32-byte key in base64 mounted into the server from a
Secret (generated once by Helm via `lookup`, or `existingSecret`). The server refuses to start
without it.

- They're stored apart from the rest of the config (`connections.credentials`: nonce, then
  ciphertext), so the GraphQL API can't return them. The ciphertext is bound to its user and
  connection (as associated data): copied to another row, it doesn't open.
- Stored secrets survive changes only to top-level non-secret config fields explicitly marked
  `x-ferret-preserve-credentials: true`, plus name, icon, schedule and enabled changes. Other
  config changes need every stored secret entered again, so a session can't point a connection
  at its own server and have the next run send the secret there. Unknown fields stay protected;
  safe changes do not override protected changes in the same edit. `null` removes a secret.
- Validation errors under a secret field, or ones that would quote a secret, only name the path.
- Only claimed runs get decrypted secrets, over the internal API.
- The key must be backed up separately from the database. A database backup without it restores
  everything except plugin credentials, which then have to be re-entered: runs fail with that
  message, and an update has to enter them.

## Embedding orchestration

- **Embeddings endpoints** are outside ferret: each model names the OpenAI-compatible endpoint
  that embeds its chunks and the one that embeds queries, often the same
  ([embeddings.md](embeddings.md#endpoints)). Routing, failover and serving are the endpoint's job.
- **The embed pump** is a tokio task in ferret-server. `LISTEN chunks_pending` wakes it (polling as
  fallback). It runs `concurrency` loops per model (default 1): each claims up to 32 chunks the
  model hasn't embedded yet, newest first, under transaction-scoped advisory locks, sends them to
  the model's endpoint, checks the answer, and writes `chunk_embeddings` and `embedding_cache` in
  one transaction. Chunks the endpoint rejects (400, 413, 422) go to `embedding_failures` and are
  skipped from then on, so one bad chunk can't block the rest. An endpoint that is down only holds
  up its own model: the loops back off and try again.
- **Several loops** (per model and server replica) share the queue through those advisory locks:
  they skip each other's chunks, and an ingest never waits for an endpoint call. No worker
  protocol: routing texts through ferret-plugins would only add a hop.
- **Newest documents first**: during the initial backfill (hours), recent documents become
  semantically searchable first. Keyword search covers everything from the moment of ingest.
- Settings shows how much of the user's chunks each model has embedded. The backlog per source and
  model shows in Admin.

## Frontend

Next.js with carapace's stack and look and feel (React, Tailwind, shadcn-style components). In
production the HTTPRoute sends `/graphql`, `/mcp`, `/api` and `/auth` to the server and everything else to
the frontend. In development Next rewrites those paths to the local server, as in carapace.

| Section | Contents |
|---|---|
| **Search** (`/`) | Login if signed out. Otherwise only a logo and a centered search field. Results: field moves to the top and searches as you type (after a 250 ms pause; the first pause after a submitted query adds a history entry, later ones replace it), a stats line (matches, documents in the selected sources, server time), filter chips (source, time range, author), model switch, snippets with highlighting, each hit's similarity and keyword match, hits collapsed per thread, a hit opens the viewer at its position, a ✓ marks it as a right hit for the eval set (several per search) |
| **Explore** (`/explore`) | v1: a filterable, sortable table of documents with facet counts (per source, content type, the 20 most frequent authors plus the selected ones, month in UTC), opening the viewer. v2: the map (UMAP via Embedding Atlas) |
| **Connections** (`/connections`) | One card per connection: plugin, schedule, last run, next run. Detail: config form (from the plugin's JSON Schema), schedule, run now, run history with logs, excluded documents and threads with "Include again", disable, delete. Document count and embedding backlog per card are post-v1 (#21) |
| **Settings** (`/settings`) | Account (change your own password, #54), API keys, Models (coverage on my data and my default model in v1; recall@10 and MRR on my judgments after v1, #21), Language |
| **Admin** (`/admin`) | Users, workers and their plugins, models with their backfill and endpoints (reachable, last error). A worker is online while it asks for runs (an idle one every 30 s) or holds a running run, offline after two minutes without either, and listed for a day after it was last seen. A restarted worker registers anew, so it shows as a new row while its old one stays offline |

Everything except Search is reached through an app menu in the top right, like Google's.

**Languages.** The UI speaks German and English (next-intl, catalogues in `frontend/messages/`, as
in carapace). It follows the browser's languages until the user picks one in Settings. The choice is
stored per browser, like the theme: the static export has no locale routes, and the sign-in page
needs the language before there's an account to ask. Dates and numbers format with the active
locale. Server error messages and the plugins' config descriptions (from their JSON Schemas) stay
English and are shown verbatim.

**The viewer** is shared by Search and Explore. It renders a document by content type (Markdown,
plain text, email with headers), scrolled to the hit. Documents with a `thread` get previous and
next navigation within it, which turns chat segments into a scrollable conversation and mails into
a thread. It always shows the link back to the origin when there is one.

Routes that carry an id use query parameters (`/view?id=…&start=…&end=…`, the hit's character
offsets), not path segments: the static export has no page per document, so only such a route
survives a reload or a shared link.

## Deployment

The Helm chart, modelled on carapace's:

- `ferret-server` Deployment (replicas configurable) and one Service with the public (8080) and the
  internal (8081) port.
- `ferret-plugins` Deployment, without a service account token: runs read files from repositories
  they don't control.
- `frontend` Deployment.
- No TEI by default: each model names its endpoints in the values, with tokens from existing
  Secrets. `bundledTei` deploys one CPU TEI for exactly one model, for a quickstart.
- Postgres Deployment with the ParadeDB image and a PVC, like carapace's own Postgres (no operator
  for now).
- Ingress as a Gateway API `HTTPRoute`, like carapace's, and a NetworkPolicy for the internal port.
- Secrets, each generated once via `lookup` or supplied via `existingSecret`: Postgres password,
  `FERRET_BOOTSTRAP_PASSWORD`, `FERRET_SESSION_SECRET`, `FERRET_WORKER_SECRET`,
  `FERRET_CONFIG_KEY`.

Migrations (sqlx, embedded in the binary) run at server startup under sqlx's advisory lock, so
replicas starting together are safe.

### Server environment

| Variable | Required | Meaning |
|---|---|---|
| `FERRET_CONFIG_FILE` | yes | path to the configuration file, so far the `embedding` section ([embeddings.md](embeddings.md#models)) |
| `FERRET_DATABASE_URL` | yes | Postgres URL. Without a password in it, `PGPASSWORD` supplies one, so the password needs no URL-encoding |
| `FERRET_SESSION_SECRET` | yes | signs session JWTs, at least 32 bytes. Rotating it signs everyone out |
| `FERRET_BOOTSTRAP_PASSWORD` | while no enabled admin exists | password for the bootstrap `admin`, at least 16 characters ([auth.md](auth.md#bootstrap)) |
| `FERRET_WORKER_SECRET` | yes | the plugin workers' pre-shared key for the internal API, at least 32 bytes ([auth.md](auth.md#worker-psk)) |
| `FERRET_CONFIG_KEY` | yes | encrypts plugin credentials, 32 bytes in base64 ([Plugin credentials](#plugin-credentials)) |
| `RUST_LOG` | no | log filter (`tracing` EnvFilter), default `info`. Logs are always JSON |

### Plugin worker environment

| Variable | Required | Meaning |
|---|---|---|
| `FERRET_INTERNAL_URL` | yes | the server's internal API, e.g. `http://ferret-server:8081` |
| `FERRET_SERVER_URL` | yes | the server's public API, where runs ingest, e.g. `http://ferret-server:8080` |
| `FERRET_WORKER_SECRET` | yes | the same pre-shared key as the server's |
| `FERRET_GIT_PROTOCOLS` | no | the transports git may use, colon-separated, default `https` ([git plugin](plugins.md#git)) |
| `HOSTNAME` | no | the worker's name in the server's records, the pod name in Kubernetes |
| `RUST_LOG` | no | as for the server |

The public port is 8080, the internal one 8081, both fixed. `GET /healthz` is the readiness
probe: 200, or 503 while the database is unreachable. It lives outside `/api` because every `/api`
route takes a bearer credential, and isn't routed by the Ingress. Liveness checks the TCP port only,
so a database outage doesn't restart every server pod.

docker-compose runs the same components for development. A traefik container stands in for the
Ingress: it serves UI and server on one origin (`http://127.0.0.1:3380`) with the same routes. Next's
dev server (`pnpm dev`) proxies the server paths to the compose server itself.
