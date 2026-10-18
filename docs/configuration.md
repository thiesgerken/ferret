# Configuration

Ferret separates process configuration from database-backed user/admin settings. Environment variables supply secrets and addresses; one YAML file declares embedding models and their endpoints. Connections and their schedules/configuration are managed in the UI/GraphQL, not added to server YAML.

For Kubernetes, [chart values.yaml](../charts/ferret/values.yaml) is the canonical Helm values reference. Helm uses camelCase and renders the snake_case server YAML. Do not paste this server file directly into values.yaml.

## Server environment

| Variable | Requirement and meaning |
| --- | --- |
| `FERRET_DATABASE_URL` | required Postgres connection URL; database must provide ParadeDB/pgvector extensions |
| `PGPASSWORD` | optional libpq/sqlx password input when URL omits password; used by chart to avoid URL escaping |
| `FERRET_CONFIG_FILE` | required path to readable YAML below |
| `FERRET_SESSION_SECRET` | required, at least 32 bytes; shared by replicas, signs sessions and run tokens |
| `FERRET_WORKER_SECRET` | required, at least 32 bytes; identical on server/workers, authorizes private worker API |
| `FERRET_CONFIG_KEY` | required base64-encoded 32 random bytes; encrypts plugin credentials |
| `FERRET_BOOTSTRAP_PASSWORD` | required only when no enabled admin exists; at least 16 characters; creates/restores admin, not an ordinary password reset |
| `RUST_LOG` | optional tracing filter, defaults to info; logs are JSON |

Generate independent secrets, not one reused string:

```sh
openssl rand -hex 32
openssl rand -hex 32
openssl rand -base64 32
```

Use the first two for session/worker secrets and the third for the config key. Bootstrap/database passwords are separate. Protect their storage and backups. Required values have no silent application defaults. Database credentials containing URL-reserved characters must be encoded, or use a password-free URL with `PGPASSWORD` as the chart does.

The public listener is fixed at `0.0.0.0:8080`, private worker listener at `0.0.0.0:8081`. There is no implemented environment variable for choosing listener ports or frontend origin. Map host ports in Compose or route Services/Gateway separately.

## Complete server YAML

```yaml
embedding:
  default: qwen3-embedding-0.6b
  models:
    - id: qwen3-embedding-0.6b
      dimension: 1024
      query_prefix: "Instruct: Given a search query, retrieve relevant passages from personal notes, documents and chats\nQuery: "
      document_prefix: ""
      min_similarity: 0.42
      embedding_api:
        url: http://litellm:4000/v1
        model: qwen3-embed
        token_env: FERRET_LITELLM_TOKEN
        concurrency: 4
      query_api:
        url: http://tei:80/v1
        model: Qwen/Qwen3-Embedding-0.6B
```

These example URLs must be reachable **from the server**. `tei` is Compose/internal DNS, not a host-machine address. The endpoint decides which weights compute the vectors; this file tells Ferret where to send text and what to expect back. See [embeddings](embeddings.md#setups) for TEI, proxy and cloud setups.

| Field | Requirement/default and validation |
| --- | --- |
| `embedding` | required object; the only top-level section |
| `embedding.default` | required, must name a configured model |
| `embedding.models` | required list containing the default model |
| `models[].id` | required unique lowercase id stored with every embedding, 1–40 characters, `[a-z0-9][a-z0-9._-]*`, ends alphanumeric |
| `models[].dimension` | required positive integer, checked against returned/cached vectors |
| `models[].query_prefix` | defaults to empty string; prepended to positive query text |
| `models[].document_prefix` | defaults to empty string; prepended to title/chunk text |
| `models[].min_similarity` | required finite-range cutoff, at least 0 and below 1 |
| `models[].embedding_api` | the endpoint that embeds chunks; `query_api` when absent |
| `models[].query_api` | the endpoint that embeds search queries; `embedding_api` when absent. At least one of the two is required |
| `*_api.url` | required, up to the version (`http://tei:80/v1`); requests go to `{url}/embeddings` |
| `*_api.model` | required, what the endpoint calls the model |
| `*_api.token_env` | optional name of an environment variable holding a bearer token; startup fails when it's unset or empty |
| `*_api.concurrency` | optional, default 1: embedding requests at once per server replica. Only on the endpoint that embeds chunks |

Unknown fields are rejected at every defined config struct. Ids differing only by dot/dash/underscore collide in SQL index slugs and are rejected, and so are duplicate ids. Endpoints aren't contacted at startup: one that is down makes searches with its model answer by keyword until it's back.

## Worker and frontend environment

| Process | Variable | Meaning |
| --- | --- | --- |
| worker | `FERRET_INTERNAL_URL` | required internal server root, e.g. `http://server:8081` |
| worker | `FERRET_SERVER_URL` | required public API root as reached by worker, e.g. `http://server:8080` |
| worker | `FERRET_WORKER_SECRET` | required shared PSK; server enforces length/auth |
| worker | `FERRET_GIT_PROTOCOLS` | optional colon-separated Git transport allowlist, defaults `https`; empty/non-Unicode rejected |
| worker | `HOSTNAME` | informational worker name, otherwise `ferret-plugins` |
| worker | `RUST_LOG` | optional JSON tracing filter; async-imap command tracing is forcibly off to avoid password disclosure |
| frontend dev | `FERRET_SERVER_URL` | optional Next dev proxy target, trimmed, defaults `http://127.0.0.1:8380` |
| Compose | `TEI_IMAGE` | selects the image of the local TEIs; native ARM image required on Apple Silicon |
| demo seed | `FERRET_URL`, `FERRET_PASSWORD` | optional target/password, default local3380/development bootstrap password |
| demo overlay | `CARAPACE_KNOWLEDGE_DIR` | local repository path to mount for carapace demonstration |
| tests | `DATABASE_URL`, `FERRET_TEST_IMAP` | override host test-service defaults in .cargo/config.toml |
| tests | `FERRET_TEST_EMBEDDING_TOKEN` | the token a test endpoint's `token_env` names, from .cargo/config.toml |
| tests | `RUST_TEST_THREADS` | repository defaults to 6 to bound concurrent test database/pool pressure |
| build | `SQLX_OFFLINE` | repository defaults true for checked metadata, not a runtime server configuration |

The static production frontend does not read a runtime API-origin environment variable. Its same-origin routes are provided by the reverse proxy. The frontend dev variable is distinct from the worker variable despite the shared name.

## Precedence and change boundaries

There is no generic YAML/environment overlay or hot reload. Server required environment values and YAML are read at startup. Exported environment values override Cargo's non-forced development defaults, but `DATABASE_URL` is for tooling/tests; it does not replace runtime `FERRET_DATABASE_URL`. Docker Compose supplies its own explicit container environment.

Connection changes, users, keys, exclusions and personal model defaults live in Postgres. YAML controls the models and their endpoints. A checksum of rendered chart config rolls server pods on model or endpoint changes; secret changes, including endpoint tokens, need deliberate rollout/coordination. See [embeddings](embeddings.md) before changing model identity and [operations](operations.md) before changing secrets.

Implementation: [server startup](../crates/ferret-server/src/main.rs), [YAML validation](../crates/ferret-server/src/config.rs), [worker startup](../crates/ferret-plugins/src/main.rs), [frontend dev proxy](../frontend/next.config.ts), [Cargo defaults](../.cargo/config.toml), [Helm translation](../charts/ferret/templates/_helpers.tpl).
