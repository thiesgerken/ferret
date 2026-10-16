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

Keep `docs/` up to date when a change alters what it describes.

## Commands

- Start everything for development: `docker compose up`
- Start only the database (for tests and `cargo run`): `docker compose up -d paradedb`
- Build: `cargo build --workspace`
- Run the server on the host: `FERRET_DATABASE_URL=postgres://ferret:ferret@localhost:5439/ferret cargo run -p ferret-server`
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
  through the `ferret-api` client, against a real ParadeDB. No mocked database.
- `#[sqlx::test(migrations = false)]` gives each test its own database, the server's startup
  migrates it. `DATABASE_URL` comes from `.cargo/config.toml` (the compose ParadeDB), an exported
  one takes precedence.
- Plugin parsers are tested against fixtures, the frontend with `node --test`.

## CI

GitHub Actions on every pull request: fmt and clippy, tests against a ParadeDB service container,
image build.

## Conventions

- The Rust toolchain is pinned in `rust-toolchain.toml` and must match the builder image in the
  `Dockerfile`.
- Migrations are never edited after they are merged: the server refuses to start when an applied
  migration's checksum changed. Add a new one instead.
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
