# Development

Use the [quickstart](quickstart.md) to try the app. The [contribution rules](../AGENTS.md), [glossary](glossary.md) and [ADRs](adr/) cover conventions and design decisions.

## Tooling

The committed Nix flake and lockfile own builds and checks. Rust comes from `rust-toolchain.toml`, pnpm from `frontend/package.json`, and Node from the locked nixpkgs. Enter the native host toolchain with:

```sh
nix develop
```

The shell includes Rust, Node, pnpm, sqlx, Git, Helm and Skopeo. Compose v2.24.4 or newer provides the local service environment.

## Apple Silicon: build native ARM Linux images

Install [Determinate Nix](https://docs.determinate.systems/) and authorize its Native Linux Builder with a free FlakeHub account:

```sh
determinate-nixd auth login
determinate-nixd status
nix build .#packages.aarch64-linux.{server,plugins,frontend}-image --out-link result-image
for image in result-image*; do docker load -i "$image"; done
export COMPOSE_FILE=docker-compose.yml:docker-compose.local.yml:docker-compose.arm.yml
```

The local overlay selects `:local` images with `pull_policy: never`. The ARM overlay pins all services to `linux/arm64` and selects native TEI. No x86 emulation is needed.

## Compose development

Follow the [quickstart](quickstart.md) for initial startup and demo data. To use local images on x64:

```sh
nix build .#server-image .#plugins-image .#frontend-image --out-link result-image
for image in result-image*; do docker load -i "$image"; done
export COMPOSE_FILE=docker-compose.yml:docker-compose.local.yml
```

On Apple Silicon, keep the three-file `COMPOSE_FILE` above. Rebuild an individual component and restart it with:

```sh
nix build .#packages.aarch64-linux.server-image && docker load -i result && docker compose up -d server
# On x64 Linux: nix build .#server-image && docker load -i result && docker compose up -d server
```

Use `plugins-image`/`plugins` or `frontend-image`/`frontend` for the other components. The UI is at <http://127.0.0.1:3380>, the server at 8380 and the database at 5439. Inspect `docker compose logs -f server plugins` and Activity for diagnostics.

Optional carapace demo:

```sh
export CARAPACE_KNOWLEDGE_DIR=/absolute/path/to/knowledge-repository
COMPOSE_FILE="${COMPOSE_FILE:-docker-compose.yml}:docker-compose.demo.yml" docker compose up -d
scripts/demo-seed.sh
```

The demo explicitly permits local Git file transport. Only committed content is indexed.

### Standalone embedding maps

The [Embedding Atlas scripts](../scripts/embedding-atlas/README.md) export existing embeddings and serve a private localhost snapshot. They are separate from Explore.

## Host iteration

Inside `nix develop`, start the database and run the server:

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

Create host-ferret.yaml from the [configuration example](configuration.md#complete-server-yaml), with embedding endpoints reachable from the host. Compose's `http://tei:80/v1` names resolve only inside its network. The host server listens on public port 8080 and internal port 8081.

In another dev shell:

```sh
FERRET_INTERNAL_URL=http://127.0.0.1:8081 \
FERRET_SERVER_URL=http://127.0.0.1:8080 \
FERRET_WORKER_SECRET='development worker secret, not for production' \
cargo run -p ferret-plugins
```

For frontend iteration:

```sh
pnpm --dir frontend install --frozen-lockfile
pnpm --dir frontend dev
```

Next serves port 3000 and proxies API requests to the Compose server at 8380. Set `FERRET_SERVER_URL=http://127.0.0.1:8080` for a host server. Production is a static export served by nginx. GraphQL client artifacts regenerate before dev, build and test; do not edit `frontend/src/gql/`.

## Tests and checks

`ciBuilds.<host-system>` contains every mandatory check, runtime package/image and CI/release tool profile. Checks cover Rust fmt/Clippy/tests, SQLx metadata and GraphQL schema freshness, frontend lint/tests/build/type checking and Helm validation. Database tests use isolated PostgreSQL 18 with `pg_search`/`pgvector` and Dovecot inside the Nix sandbox, without Docker or existing developer data.

Bootstrap the locked runner:

```sh
nix build --inputs-from . nixpkgs#nix-fast-build --out-link result-build-tool
```

On x64 Linux:

```sh
result-build-tool/bin/nix-fast-build --no-nom --flake .#ciBuilds.x86_64-linux --systems x86_64-linux --out-link result-ci
```

On Apple Silicon:

```sh
result-build-tool/bin/nix-fast-build --no-nom --flake .#ciBuilds.aarch64-darwin --systems "aarch64-darwin aarch64-linux" --out-link result-ci
```

Load the images and exercise the runtime restrictions:

```sh
for image in result-ci-{server,plugins,whatsapp-archive,frontend}-image; do docker load -i "$image"; done
result-ci-tools/bin/python scripts/smoke-images.py
```

The smoke uses disposable volumes and dynamic ports. It checks server health/login, Git ingestion and keyword search, frontend/static/404 responses, archive health and an authenticated read, and non-root/read-only execution. It requires no model download or WhatsApp account.

For focused host tests, `.cargo/config.toml` supplies development database/mailbox defaults. Start `paradedb` and `docker compose up -d --scale dovecot=1 dovecot`, then use `cargo test --workspace --locked`. Linux files-plugin tests additionally require `FERRET_TEST_FILES_ROOT` to name an empty disposable mount. Nix checks supply their own isolated tmpfs. Never use personal files as test fixtures.

## Migrations and generated contracts

Create a migration instead of editing an applied one; follow AGENTS.md's timestamp ordering rule:

```sh
cargo sqlx migrate add --source crates/ferret-server/migrations describe_change
DATABASE_URL=postgres://ferret:ferret@localhost:5439/ferret cargo sqlx migrate run --source crates/ferret-server/migrations
DATABASE_URL=postgres://ferret:ferret@localhost:5439/ferret cargo sqlx prepare --workspace
```

Commit `.sqlx/` with query/migration changes. Builds use offline metadata; regeneration needs a migrated database. For GraphQL changes:

```sh
cargo run --locked -q -p ferret-server -- schema > frontend/schema.graphql
pnpm --dir frontend codegen
```

Update the relevant maintained [topic/API/plugin guide](README.md) with behavior changes.

## Native Linux Builder constraints

See the [builder prerequisites and known issues](https://docs.determinate.systems/troubleshooting/native-linux-builder/). Its shared store must be case-sensitive for case-colliding outputs. `cp --no-preserve=mode` can fail on the shared filesystem; concurrent guest-store initialization failures may require a retry with `--max-jobs 1`.

## Dependency updates and CI

[Renovate](../renovate.json) updates dependencies and `flake.lock`. After changing pnpm, its lockfile or the `pg_search` version, refresh binary pins and the pnpm dependency hash inside the dev shell:

```sh
python3 scripts/update-nix-deps.py
```

The command selects native Linux outputs (ARM Linux on Apple Silicon); `--system` can select another supported Linux target. Commit its pin/hash changes with the dependency update. Hash refresh is manual on Renovate PRs; these updates are not auto-merged. `pg_search` uses SHA-256-pinned official PostgreSQL 18 binaries; the other packages come from stable nixpkgs.

[CI](../.github/workflows/ci.yml) calls [Nix verification](../.github/workflows/nix-verify.yml). [PR previews](../.github/workflows/publish-pr.yml) publish verified x64 images and charts; [releases](../.github/workflows/release.yml) verify the version-bumped tag before publishing. Semantic-release is a known tooling exception: `uv tool run` pins its PyPI version, but not its transitive dependencies.
