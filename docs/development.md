# Development

Use the [quickstart](quickstart.md) to try the app; this page covers changing it. The [contribution rules](../AGENTS.md) apply to every PR. Read maintained topic docs for current behavior, the [glossary](glossary.md) for vocabulary and [ADRs](adr/) for rationale. Conception is background, not the endpoint/config contract.

## Prerequisites and repository map

Use the committed Nix flake/lockfile for builds and checks. It selects supported stable nixpkgs, consumes `rust-toolchain.toml`, and builds the pnpm version selected by `frontend/package.json`; Node 24 remains the frontend runtime. The sandbox database has the narrowly scoped package-input exception described below. `nix develop` provides optional native tooling (including `aarch64-darwin` on Apple Silicon). Docker with Compose v2.24.4 or newer remains the primary environment, not the dev shell.

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

Follow the [quickstart](quickstart.md) for either x64 pulls without Nix or native ARM Linux builds/loads on Apple Silicon. On ARM, keep `COMPOSE_FILE=docker-compose.yml:docker-compose.arm.yml` set. Then:

```sh
docker compose up -d
scripts/demo-seed.sh
```

The app is at <http://127.0.0.1:3380>, direct server at 8380, host database at 5439. Login is admin/ferret-development on a fresh database. `tei` serves Qwen3 for queries and the embed pump; `tei-bge` remains scaled to zero until explicitly enabled. This configuration has public development secrets and is not a deployment recipe.

Use `docker compose logs -f server plugins` and Activity for different layers of diagnostics. There is no Dockerfile or Compose build path. Rebuild and load the changed Nix image, then `docker compose up -d --force-recreate server` (or plugins/frontend). On x64, set `FERRET_SERVER_IMAGE=ferret:local`, `FERRET_PLUGINS_IMAGE=ferret-plugins:local` and `FERRET_FRONTEND_IMAGE=ferret-frontend:local` when testing local builds. The ARM overlay already selects these names without pulling replacements. Static frontend output is baked into its image. Do not start a second project on occupied fixed ports or seed an existing personal index.

Optional carapace demo:

```sh
export CARAPACE_KNOWLEDGE_DIR=/absolute/path/to/knowledge-repository
COMPOSE_FILE="${COMPOSE_FILE:-docker-compose.yml}:docker-compose.demo.yml" docker compose up -d
scripts/demo-seed.sh
```

The overlay mounts that local Git repository into workers and explicitly permits file transport for the demonstration. Only committed/mirrored content is indexed. Keep this exception out of untrusted deployments.

### Standalone embedding maps

The [Embedding Atlas scripts](../scripts/embedding-atlas/README.md) export one user's existing
embeddings from local Postgres or Kubernetes and serve a private snapshot on localhost. The
adjacent README covers prerequisites, commands, snapshot retention and production read load.
This is separate from Explore and changes neither Ferret's schema nor its deployment.

## Host server and workers

Enter the optional native shell with `nix develop` before the host commands below. Rust and pnpm versions come from the repository declarations; do not replace them with the older defaults of stable nixpkgs. The dev shell is native macOS on Apple Silicon. Packaged builds and integration checks remain Linux outputs.

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

Nix owns formatting, Clippy, workspace tests, SQLx metadata/schema consistency, frontend lint/tests/codegen/production build/type checking, and Helm validation. `ciBuilds.<host-system>` roots every mandatory check, all runtime packages/images and the native CI/release tool profiles in one `nix-fast-build` traversal. The frontend production check and frontend package share a derivation; deduplicating that build does not skip the check.

Bootstrap the runner from the locked nixpkgs input without evaluating the project's outputs:

```sh
nix build --inputs-from . nixpkgs#nix-fast-build --out-link result-build-tool
mkdir -p dist
```

On native x64 Linux:

```sh
result-build-tool/bin/nix-fast-build --no-nom \
  --flake .#ciBuilds.x86_64-linux --systems x86_64-linux \
  --out-link result-ci --result-file dist/ci-results.json
```

On Apple Silicon, the build set combines ARM Linux checks/packages/images through Native Linux Builder with native Darwin tools. Allow both ARM platforms, never x86:

```sh
result-build-tool/bin/nix-fast-build --no-nom \
  --flake .#ciBuilds.aarch64-darwin --systems "aarch64-darwin aarch64-linux" \
  --out-link result-ci --result-file dist/ci-results.json
```

The resulting `result-ci-tools/bin` contains Bash, coreutils, Git, Skopeo, Helm, Python and the other CI commands; the profile also supplies the CA bundle. `result-ci-release-tools` supplies the selected Rust toolchain, uv and Python 3.14. Reuse these realized profiles instead of repeatedly starting tool shells:

```sh
export PATH="$PWD/result-ci-tools/bin:$PATH"
export SSL_CERT_FILE="$PWD/result-ci-tools/etc/ssl/certs/ca-bundle.crt"
```

Do not use `--skip-cached`: image archives and executable tools must actually be realized locally, including cache downloads. The currently locked nix-fast-build 1.4.0 records output paths on successful `BUILD` records; its `EVAL` durations are zero placeholders, not measured evaluation time. Consume the named outlinks or `BUILD.outputs`, not a newer version's result schema or a fixed build-record count. Evaluation and builds overlap, so summing build durations does not produce wall time.

The database-backed checks create an isolated PostgreSQL 18 cluster with `pg_search` and `pgvector`, and a disposable Dovecot in the sandbox. They initialize writable temporary state, preload the extension, wait for readiness and clean up on success/failure. They override host database/mailbox defaults and require no existing developer data, external Docker services, origin account or publishing credential.

Crane caches compiled test executables independently of disposable service configuration; only that compile artifact stores Cargo targets, and the mandatory `rust-test` check executes the complete workspace. The Dovecot fixture uses `/` mailbox hierarchy separators, auto-subscribes the standard Drafts/Junk/Sent/Trash folders, and marks the localized `Papierkorb` mailbox as `\Trash`, matching the behaviors exercised by the existing IMAP tests.

PostgreSQL 18, `pgvector`, Dovecot and all application/image/developer tooling come from the stable nixpkgs lock. The narrowly scoped exception is `pg_search`: stable 0.23.0 lacks the required access method, while the checked newer nixpkgs 0.25.10 package still requires an explicit index `key_field` that the existing migrations do not supply. `nix/pg-search-binaries.json` pins official native PostgreSQL 18 release packages by SHA-256. Nix installs their extension library, SQL and license notices, and `autoPatchelfHook` resolves the declared stable GCC/OpenBLAS dependencies without ignoring missing libraries. This avoids reconstructing upstream's pgrx/dictionary release toolchain. No newer nixpkgs input or exact-Compose-version override is maintained; Compose's ordinary upstream image remains independent.

The selected `pg_search` package declares `requires = 'vector'`. The isolated bootstrap therefore creates the real `vector` extension in `template1` before creating the check database; SQLx-created test databases inherit this prerequisite too. The unchanged migrations still create `pg_search` and run in full. This is a package dependency requirement, not a compatibility alias or a claim that the Nix test cluster is identical to the Compose image.

Ferret requires the `paradedb` index access method with automatic key-field inference; `pdb.simple` with German/English stemmers and aliases; `pdb.ngram`; `pdb.literal`; query-side `pdb.alias`; `&&&`; `pdb.score`; and the `paradedb.planner_warnings=off` setting. Package version alone is not a compatibility proof: all existing migrations, SQLx metadata checks and database integration tests must pass unchanged.

After loading the three native Linux image archives as in the quickstart:

```sh
result-ci-tools/bin/python scripts/smoke-images.py
```

This runs an isolated Compose project with its own disposable database volume and dynamic ports, then removes only that project. It exercises actual server migrations/database health, authentication, Git worker ingestion and keyword search, Git HTTPS certificate verification, frontend routes/cache headers, and non-root/read-only execution. Its Git TLS probe needs public GitHub access. It does not download model weights or claim to verify model inference.

For focused host iteration, `.cargo/config.toml` supplies non-forced development defaults for DATABASE_URL, FERRET_TEST_IMAP, offline SQL metadata and six test threads. Start `paradedb` and scale `dovecot` to one before `cargo test --workspace --locked`. These manual commands are conveniences, not a replacement CI pipeline. IMAP tests use per-test mailboxes, not personal accounts; embedding tests use protocol fixtures.

The Gitea regression suite (`cargo test -p ferret-server --test gitea_plugin`) uses the real Ferret server/ParadeDB, temporary Git repositories served over authenticated HTTP, and deterministic Gitea API responses. It does not need a Gitea account or container. It covers discovery pagination, repository/file patterns, threaded discussions/reviews, edits/deletions, rename/exclusion behavior and the connection-wide sweep/cursor failure gate.

## Migrations and sqlx metadata

The native dev shell provides the matching sqlx CLI. Create a new migration rather than editing an applied one:

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

## Native Linux Builder constraints

Determinate Nix and a FlakeHub login are required; no FlakeHub project cache is configured. See the [builder prerequisites and known issues](https://docs.determinate.systems/troubleshooting/native-linux-builder/). The VM shares the host Nix store. Case-colliding outputs require a case-sensitive store, and `cp --no-preserve=mode` can fail on the shared filesystem. Do not reinstall Nix, reset its store, change global configuration or remove DHCP leases as an automatic repair. Report the exact failing derivation and obtain operator authorization for host changes. If concurrent guest-store initialization fails, retry the affected build with `--max-jobs 1`; do not switch to x86 or weaken the sandbox.

## Dependency updates and publication

Renovate updates Nix inputs/`flake.lock` as well as the existing Rust/frontend dependencies. pnpm uses official native upstream release archives, pinned by SHA-256 in `nix/pnpm-binaries.json` and asserted against `packageManager`; it is not rebuilt from pnpm's Rust workspace or replaced by stable nixpkgs' older pnpm. After changing that declaration or `frontend/pnpm-lock.yaml` (including on a Renovate PR), refresh the binary pins and fixed-output cache with the pinned tooling:

```sh
# On Apple Silicon:
nix build .#ci-tools --out-link result-ci-tools
export SSL_CERT_FILE="$PWD/result-ci-tools/etc/ssl/certs/ca-bundle.crt"
result-ci-tools/bin/python scripts/update-nix-deps.py --system aarch64-linux
# On native x64 Linux, use --system x86_64-linux instead.
```

The refresh command reads official GitHub release-asset digests when the pnpm version changes (public GitHub access is required), and Nix verifies the archive hashes and executable version. Commit the resulting binary pins and dependency hash with the dependency update, then run the full checks/builds/smoke above. A stale hash fails required checks; hosted Renovate cannot automatically repair this aggregate fetchPnpmDeps hash. Updates requiring this documented refresh are not auto-merged. Do not bypass hash checks or change the pnpm dependency-build allow/deny policy to make an update pass.

Renovate proposes `pg_search` release-version updates in `nix/pg-search-binaries.json`, but these are not auto-merged. With the tool profile above realized, refresh the official native-asset hashes with `result-ci-tools/bin/python scripts/update-nix-deps.py --pg-search`, then run the complete Linux checks and image smoke. The exception is this extension package only; do not move unrelated packages off stable or modify existing migrations to accommodate an incompatible extension. PostgreSQL's major remains explicitly 18.

GitHub Actions orchestrates verification, the standard nixpkgs binary cache and GitHub Actions artifact caching, smoke execution, release/version operations and Skopeo publication. Verification has no publishing credentials and untrusted PRs cannot write the trusted cache. Only x64 images are published. Docker-compatible archives from dockerTools are copied with Skopeo's OCI manifest conversion; they are not OCI-layout archives.

Verification stages stable image filenames as symlinks to realized archives; Docker loading and artifact upload read those paths without making another local copy of the image bytes. GitHub Actions artifacts still transfer the verified archives/chart data to separate publishing jobs. A credentialed publisher realizes its own trusted main/tag-derived tool profile once and reuses its PATH/CA bundle; it never executes PR-provided tool paths or result manifests. Release versioning, post-bump verification and publishing are distinct jobs/ref contexts, not one global evaluation.

Image-copy commands explicitly accept unsigned, CI-verified archives instead of depending on a host `containers/policy.json`. Skopeo's `--insecure-policy` selects this image-signature policy; it does not disable registry TLS verification. Publication still requires the trusted run/repository/head checks described below.

PR publishing is a separate trusted `workflow_run` workflow. It validates the successful CI run and exact current PR head against GitHub, checks out `main` for tooling/scripts, and treats verified images/chart as data. Fork PRs do not publish. The first migration PR cannot activate this new publisher until it exists on the default branch; subsequent successful PR runs publish the existing mutable and SHA-specific image/chart tags. Release verification runs again against the version-bumped release tag before publication.
