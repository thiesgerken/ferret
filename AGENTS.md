# AGENTS.md

## Project overview

ferret is a personal search index: keyword (BM25) and semantic search over notes, documents, mail,
chats and agent memory, pointing back to where each piece lives. Rust (axum, sqlx) on Postgres with
ParadeDB `pg_search` and pgvector, embeddings from TEI, Next.js frontend.

Read before working on a feature:

- `docs/conception/`: the design (`README.md` incl. "v1 scope", `architecture.md`, `auth.md`,
  `embeddings.md`, `plugins.md`)
- `docs/glossary.md`: the domain vocabulary. Use its terms in code, tests, commits and PRs, and
  avoid the ones it lists under _Avoid_.
- `docs/adr/`: decisions and why they were made

Every PR must assess whether the maintained documentation (not `docs/conception/`) needs updating and update it when applicable.

## Commands

- Start everything for development: `docker compose up`
- Start only the database (for tests and `cargo run`): `docker compose up -d paradedb`, plus
  `docker compose up -d --scale dovecot=1 dovecot` for the IMAP plugin's tests
- Build: `cargo build --workspace`
- Run the server on the host: `FERRET_DATABASE_URL=postgres://ferret:ferret@localhost:5439/ferret FERRET_CONFIG_FILE=<config.yaml> FERRET_SESSION_SECRET=<32+ bytes> FERRET_WORKER_SECRET=<32+ bytes> FERRET_CONFIG_KEY=<base64 of 32 bytes> FERRET_BOOTSTRAP_PASSWORD=<16+ chars> cargo run -p ferret-server`.
  The config file's TEI URLs must be reachable from the host (compose's `docker/ferret.yaml` uses
  `http://tei:80` and `http://tei-bulk:80`, which only work inside compose).
- Regenerate the GraphQL schema for the frontend: `cargo run -p ferret-server -- schema > frontend/schema.graphql`
- After changing a query or a migration, refresh the sqlx query metadata (needs
  `cargo install sqlx-cli` and a migrated database):
  `DATABASE_URL=postgres://ferret:ferret@localhost:5439/ferret cargo sqlx migrate run --source crates/ferret-server/migrations`,
  then the same `DATABASE_URL` with `cargo sqlx prepare --workspace`, and commit `.sqlx/`
- Test: `cargo test --workspace` (needs the compose ParadeDB, see Testing)
- Lint: `cargo fmt --all --check` and `cargo clippy --workspace --all-targets -- -D warnings`

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

- Server tests are black-box: they start the server in-process (`tests/common`) and talk to it
  over HTTP (the `ferret-api` client for the machine API), against a real ParadeDB. No mocked
  database.
- `#[sqlx::test(migrations = false)]` gives each test its own database, the server's startup
  migrates it. `DATABASE_URL` comes from `.cargo/config.toml` (the compose ParadeDB), an exported
  one takes precedence.
- Queries use sqlx's checked macros (`query!`, `query_as!`, ADR 0005), except search, which is
  built with `QueryBuilder`. Builds read their metadata from `.sqlx/` (`SQLX_OFFLINE`), so only
  tests need a database.
- Plugin parsers are tested against fixtures, the frontend with `node --test`. The IMAP plugin's
  tests also need the compose Dovecot, which is scaled to 0 by default
  (`docker compose up -d --scale dovecot=1 dovecot`, `FERRET_TEST_IMAP` in `.cargo/config.toml`),
  each test logging in as a user of its own.

## CI

GitHub Actions on every pull request: fmt and clippy, a check that `frontend/schema.graphql` and
`.sqlx/` are current, tests against a ParadeDB service container, frontend lint and tests, helm
lint, and PR images and chart published as `:pr<N>`.

## Conventions

- The Rust toolchain is pinned in `rust-toolchain.toml` and must match the builder image in the
  `Dockerfile`.
- Migrations are never edited after they are merged: the server refuses to start when an applied
  migration's checksum changed. Add a new one instead.
- Migration versions are creation timestamps (`cargo sqlx migrate add --source
  crates/ferret-server/migrations <name>`, current UTC), never invented dates: parallel branches
  that pick round numbers collide, and sqlx runs only one migration per version. Some migrations
  are dated ahead of the clock. While the newest version on integration/v1 or in an open PR still
  lies in the future, take the day after it, with the current UTC time of day, so a migration
  still sorts after the ones it builds on.
- Invalid state fails loudly: required config and fields stay required, no silent defaults.

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
