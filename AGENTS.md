# AGENTS.md

## Project overview

ferret is a personal search index: keyword (BM25) and semantic search over notes, documents, mail,
chats and agent memory, pointing back to where each piece lives. Rust (axum, sqlx) on Postgres with
ParadeDB `pg_search` and pgvector, embeddings from an OpenAI-compatible endpoint per model (TEI in
development), Next.js frontend.

Read before working on a feature:

- `docs/conception/`: the design (`README.md` incl. "v1 scope", `architecture.md`, `auth.md`,
  `embeddings.md`, `plugins.md`)
- `docs/glossary.md`: the domain vocabulary. Use its terms in code, tests, commits and PRs, and
  avoid the ones it lists under _Avoid_.
- `docs/adr/`: decisions and why they were made
- `docs/*.md`, `docs/api/` and `docs/plugins/`: the user and operator docs, the maintained
  documentation of current behavior (configuration, operations, indexing, search, embeddings,
  the APIs, each plugin)

Every PR must assess whether the maintained documentation (not `docs/conception/`) needs updating and update it when applicable.

## Commands

- Enter pinned host tooling: `nix develop`.
- Build images and run the development stack: follow `docs/quickstart.md`; local changes use
  `docker-compose.local.yml`, plus `docker-compose.arm.yml` on Apple Silicon.
- Run all checks/packages/images: bootstrap `nix-fast-build` from the locked nixpkgs and build
  `ciBuilds.x86_64-linux` (Linux x64) or `ciBuilds.aarch64-darwin` (Apple Silicon).
  Exact commands and image smoke instructions are in `docs/development.md`.
- Host iteration is optional: `cargo run -p ferret-server`, `cargo run -p ferret-plugins` or
  `pnpm --dir frontend dev` inside the dev shell. Host services/secrets are documented there.
- Regenerate GraphQL: `cargo run -p ferret-server -- schema > frontend/schema.graphql`.
- Refresh SQLx metadata after query/migration changes using the dev shell's sqlx and a migrated
  database: `cargo sqlx prepare --workspace`. Commit `.sqlx/`.
- Refresh dependency pins/hashes: `python3 scripts/update-nix-deps.py` inside the dev shell.

## Project structure

```text
crates/
  ferret-server/     binary `ferret`: APIs, scheduling, embed pump
    migrations/      sqlx migrations, embedded and applied at startup
    tests/           black-box tests against the server's APIs
  ferret-plugins/    binary `ferret-plugins`: plugin worker and plugin implementations
  ferret-api/        shared request and response types, plus a small HTTP client
frontend/            Next.js UI
docs/                conception, glossary, ADRs
docker-compose.yml   development only, the deployment is the Helm chart
```

## Testing

- Nix runs the complete workspace suite against isolated PostgreSQL 18 with `pg_search` and
  `pgvector`, plus disposable Dovecot. No Compose services or personal data are needed.
- Server tests are black-box HTTP tests against the real database; `#[sqlx::test(migrations = false)]`
  gives each test a database migrated by server startup.
- Queries use checked sqlx macros (ADR 0005), except QueryBuilder-based search. Builds use `.sqlx/`
  offline metadata.
- Plugin parsers use fixtures; frontend tests use `node --test`.
- Focused host tests are a convenience: start Compose ParadeDB/Dovecot and use
  `cargo test --workspace --locked`. `.cargo/config.toml` supplies non-forced service defaults.
  Linux mounted-files tests also require an empty disposable mount at `FERRET_TEST_FILES_ROOT`.

## CI

`ci.yml` calls `nix-verify.yml`, which realizes `ciBuilds.x86_64-linux` and smokes the hardened
runtime images. `publish-pr.yml` publishes verified PR images/charts with `pr<N>` and SHA tags.
`release.yml` verifies version-bumped release tags before publishing. Publishing tools run from
trusted refs; untrusted PRs do not receive publishing credentials or write the trusted cache.

## Conventions

- The Rust toolchain is pinned in `rust-toolchain.toml`; Nix consumes it for tooling, checks and
  runtime binaries. Nix is the single build/check definition.
- Migrations are never edited after they are merged: the server refuses to start when an applied
  migration's checksum changed. Add a new one instead.
- Migration versions are creation timestamps (`cargo sqlx migrate add --source
  crates/ferret-server/migrations <name>`, current UTC), never invented dates: parallel branches
  that pick round numbers collide, and sqlx runs only one migration per version. Some migrations
  are dated ahead of the clock. While the newest version on integration/v1 or in an open PR still
  lies in the future, take the day after it, with the current UTC time of day, so a migration
  still sorts after the ones it builds on.
- Invalid state fails loudly: required config and fields stay required, no silent defaults.
- Keep hints, examples, fixtures and demo data independent of the maintainer's deployment: use reserved example domains, not personal hostnames, local checkout paths or provider-specific setup instructions.

### Commits

Commit messages and PR titles use [gitmoji](https://gitmoji.dev/) in the form
`<emoji> <type>(<scope>): <message>`, e.g. `✨ feat(server): add health endpoint`. The release
pipeline derives the version bump from the emoji (semantic-release's emoji parser):

| emoji | type | release |
|---|---|---|
| 💥 | breaking change | major |
| ✨ | `feat` | minor |
| 🐛 | `fix` | patch |
| 🚑 | `fix` (hotfix) | patch |
| ♻️ | `refactor` | patch |
| ⚡ | `perf` | patch |
| 🔒 | `fix` (security) | patch |
| 🩹 | `fix` (minor) | patch |
| ⬆️ | `chore` (upgrade dependencies) | patch |
| 🔧 | `chore` (config) | patch |
| 💄 | `style` (UI) | patch |
| 🗑️ | `chore` (deprecate, remove) | patch |
| 👷 | `ci` | patch |
| ✅ 📝 and others | `test`, `docs`, ... | none |

Keep commits isolated: one fix or feature per commit.
