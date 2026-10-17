# Development

## The compose stack

```sh
docker compose up
```

starts ParadeDB, two TEIs with Qwen3-Embedding-0.6B (one for queries, one for the embed pump, so
search stays fast during a backfill), the server, a plugin worker and the UI on
<http://127.0.0.1:3380>. Sign in as `admin` with the password `ferret-development`.

On Apple Silicon, set `TEI_IMAGE=ghcr.io/huggingface/text-embeddings-inference:cpu-arm64-1.9.4`
for native TEI builds instead of emulated ones.

## Demo data

```sh
scripts/demo-seed.sh
```

ingests the sample documents in [docker/demo](../docker/demo) (needs `jq`) and adds a git
connection to ferret's own docs. To index a local carapace knowledge repository as well, start
compose with the demo overlay and seed with the same variable:

```sh
export CARAPACE_KNOWLEDGE_DIR=../carapace/data/knowledges/me
docker compose -f docker-compose.yml -f docker-compose.demo.yml up
scripts/demo-seed.sh
```

## Tests

Tests need only the database:

```sh
docker compose up -d paradedb
cargo test --workspace
```

The frontend's tests run with `pnpm test` in `frontend/`. More commands and conventions are in
[AGENTS.md](../AGENTS.md).
