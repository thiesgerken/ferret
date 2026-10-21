# Development

Use the [quickstart](quickstart.md) to try the app; this page covers changing it. The [contribution rules](../AGENTS.md) apply to every PR. Read maintained topic docs for current behavior, the [glossary](glossary.md) for vocabulary and [ADRs](adr/) for rationale. Conception is background, not the endpoint/config contract.

## Prerequisites and repository map

Install the Rust toolchain pinned in [rust-toolchain.toml](../rust-toolchain.toml), a C/C++ build toolchain for native dependencies, Git, Docker Compose v2, curl and jq. Frontend development uses Node 24 and the pnpm version pinned in frontend/package.json (12.8.1 at this baseline). Use that package manager with the checked lockfile.

| Path | Responsibility |
| --- | --- |
| `crates/ferret-server/src` | axum routers, GraphQL, auth, ingestion, search, scheduling and embed pump |
| `crates/ferret-server/migrations` | embedded sqlx migrations, applied before startup listens |
| `crates/ferret-server/tests` | real-Postgres HTTP integration tests plus fixtures/helpers |
| `crates/ferret-api` | shared public/internal wire types and clients |
| `crates/ferret-plugins` | worker orchestration, schemas and origin implementations |
| `frontend/src` | static Next.js UI, GraphQL operations and unit tests |
| `frontend/schema.graphql` | checked generated server schema |
| `.sqlx` | checked offline query metadata |
| `docker`, `scripts/demo-seed.sh` | local config, origin fixtures and demo ingestion |
| `charts/ferret` | deployment templates and canonical Helm values |

## Full Compose development

```sh
# Apple Silicon only:
export TEI_IMAGE=ghcr.io/huggingface/text-embeddings-inference:cpu-arm64-1.9.4
docker compose up -d --build
scripts/demo-seed.sh
```

The app is at <http://127.0.0.1:3380>, direct server at 8380, host database at 5439. Login is admin/ferret-development on a fresh database. First startup builds images and downloads weights. `tei` serves Qwen3 for queries and the embed pump; `tei-bge` (bge-m3) is scaled to 0 until `docker compose up -d --scale tei-bge=1 tei-bge`. This is development/demo configuration with public secrets, not a deployment recipe.

Use `docker compose logs -f server plugins` and Activity for different layers of diagnostics. Rebuild the service you changed with `docker compose up -d --build server` (or plugins/frontend); frontend static output is baked into its image, not hot-reloaded from the checkout. Do not start a second project on occupied fixed ports or seed an existing personal index. Use isolated project names, host-port overrides and separate volumes when working alongside one.

Optional carapace demo:

```sh
export CARAPACE_KNOWLEDGE_DIR=/absolute/path/to/knowledge-repository
docker compose -f docker-compose.yml -f docker-compose.demo.yml up -d --build
scripts/demo-seed.sh
```

The overlay mounts that local Git repository into workers and explicitly permits file transport for the demonstration. Only committed/mirrored content is indexed. Keep this exception out of untrusted deployments.

### Standalone embedding maps

The [Embedding Atlas scripts](../scripts/embedding-atlas/README.md) export one user's existing
embeddings from local Postgres or Kubernetes and serve a private snapshot on localhost. The
adjacent README covers prerequisites, commands, snapshot retention and production read load.
This is separate from Explore and changes neither Ferret's schema nor its deployment.

## Host server and workers

Start only the test/development database if port 8080/8081 will be used by the host server:

```sh
docker compose up -d paradedb
export FERRET_DATABASE_URL=postgres://ferret:ferret@localhost:5439/ferret
export FERRET_CONFIG_FILE=/absolute/path/to/host-ferret.yaml
export FERRET_SESSION_SECRET='development session secret, not for production'
export FERRET_WORKER_SECRET='development worker secret, not for production'
export FERRET_CONFIG_KEY=ZGV2ZWxvcG1lbnQgY29uZmlnIGtleSwgbm90IHByb2Q=
export FERRET_BOOTSTRAP_PASSWORD=ferret-development
cargo run -p ferret-server
```

Create host-ferret.yaml from the complete [configuration example](configuration.md#complete-server-yaml), replacing endpoint URLs with addresses reachable from the host. Compose's `http://tei:80/v1` names are not host DNS. Run an independent TEI with a host-published port, or configure an existing reachable endpoint; do not copy container-only addresses and then diagnose their expected DNS failure as a model bug. Endpoints may be down at startup, but semantic queries need a reachable one.

In another terminal, with the same worker secret:

```sh
FERRET_INTERNAL_URL=http://127.0.0.1:8081 \
FERRET_SERVER_URL=http://127.0.0.1:8080 \
FERRET_WORKER_SECRET='development worker secret, not for production' \
cargo run -p ferret-plugins
```

Host listeners are fixed at public8080/internal8081. If another local app owns8080, either stop your own conflicting instance or keep Ferret containerized with mapped host ports. There is no implemented listener override variable.

## Frontend iteration

```sh
pnpm --dir frontend install --frozen-lockfile
pnpm --dir frontend dev
```

Next dev serves port3000 by default. It proxies GraphQL/MCP/API/auth to `http://127.0.0.1:8380`, fitting the Compose server. For the host server:

```sh
FERRET_SERVER_URL=http://127.0.0.1:8080 pnpm --dir frontend dev
```

Production builds are static exports served by nginx; reverse-proxy routing replaces dev rewrites. `predev`, `prebuild` and `pretest` regenerate typed GraphQL client artifacts from the checked schema and operations. Do not hand-edit `frontend/src/gql/`.

## Tests and checks

Server tests start in-process servers against a real ParadeDB, not a mocked SQL interface. sqlx tests create an isolated database per test, so the configured test role needs database-creation permission. Use disposable test infrastructure, never an irreplaceable production database server.

```sh
docker compose up -d paradedb
docker compose up -d --scale dovecot=1 dovecot
cargo test --workspace --locked

pnpm --dir frontend test
pnpm --dir frontend lint
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
helm lint charts/ferret --set ingress.hostname=ferret.example.com
```

[.cargo/config.toml](../.cargo/config.toml) supplies DATABASE_URL at localhost5439, FERRET_TEST_IMAP at localhost3143, offline SQL metadata and six test threads. Exported variables override those non-forced defaults. IMAP tests each use their own test mailbox user on the development Dovecot; they do not use personal mail accounts. Many origin tests use protocol fixtures; embedding tests use a fake OpenAI-compatible endpoint (`tests/common/fake_endpoint.rs`). See actual test setup before assuming an external account/GPU is required.

The Gitea regression suite (`cargo test -p ferret-server --test gitea_plugin`) uses the real Ferret server/ParadeDB, temporary Git repositories served over authenticated HTTP, and deterministic Gitea API responses. It does not need a Gitea account or container. It covers discovery pagination, repository/file patterns, threaded discussions/reviews, edits/deletions, rename/exclusion behavior and the connection-wide sweep/cursor failure gate.

The Linux mounted-files regression (`cargo test -p ferret-server --test files_plugin`) uses an empty
isolated mount at `FERRET_TEST_FILES_ROOT`. CI creates a disposable bind mount before the workspace
tests and unmounts it afterwards. The test refuses a nonempty fixture, creates and removes only its
own content, and exercises the real server/database, inventory reuse, links, filters, failure/no-sweep
recovery, a directory above the request hard limit and an available empty share. On non-Linux hosts
run this test in a Linux container with an empty volume mounted directly at that configured root.
Never point it at personal or production files.

## Migrations and sqlx metadata

Install a matching sqlx CLI (`cargo install sqlx-cli --version 0.9.0`). Create a new migration rather than editing an applied one:

```sh
cargo sqlx migrate add --source crates/ferret-server/migrations describe_change
DATABASE_URL=postgres://ferret:ferret@localhost:5439/ferret \
  cargo sqlx migrate run --source crates/ferret-server/migrations
DATABASE_URL=postgres://ferret:ferret@localhost:5439/ferret \
  cargo sqlx prepare --workspace
```

Follow the migration-version ordering rule in AGENTS.md when existing migrations are future-dated. Commit `.sqlx/` with query/migration changes: builds use checked metadata and do not require a live database; tests/metadata regeneration do. CI also checks metadata against the migrated schema. The runtime server uses FERRET_DATABASE_URL, not the tooling DATABASE_URL.

## GraphQL schema changes

```sh
cargo run --locked -q -p ferret-server -- schema > frontend/schema.graphql
pnpm --dir frontend codegen
```

The schema command prints SDL without starting the server or needing runtime secrets/database. Commit the canonical schema with resolver/input changes. Client codegen reads [codegen.ts](../frontend/codegen.ts) and frontend operations; generated client files are not the canonical API contract.

For behavior changes, update the appropriate maintained [topic/API/plugin guide](README.md), not just conception. [CI](../.github/workflows/ci.yml) defines the actual checks and PR preview image/chart publication; do not claim a check passed unless you ran it.

## Dependency updates

[renovate.json](../renovate.json) configures automated dependency updates. Digest pinning is disabled for the `helm-values` manager in `charts/ferret/values.yaml` so the application images' empty tags keep selecting `Chart.AppVersion`. Version updates remain enabled for external chart images, and digest pinning remains enabled outside this file.
