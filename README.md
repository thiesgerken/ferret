<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/brand/mark-dark.svg">
  <img src="docs/brand/mark-light.svg" alt="" width="96" height="96">
</picture>

# ferret

ferret makes everything that describes me instantly searchable: notes, documents (Paperless), email,
chats, git repos, carapace memory and saved articles. The data already lives in those systems,
ferret is an index over them: keyword and semantic search with snippets, filters and a deep link
back to the origin, for me through a search UI and for agents through the same API.

## Status

v1 ([scope](docs/conception/README.md#v1-scope)) is in progress.

## Development

```sh
docker compose up
```

starts ParadeDB, TEI with Qwen3-Embedding-0.6B, the server, a plugin worker and the UI on
<http://127.0.0.1:3380>.
Sign in as `admin` with the password `ferret-development`. On Apple Silicon, set `TEI_IMAGE=ghcr.io/huggingface/text-embeddings-inference:cpu-arm64-1.9.4` for a
native TEI build. Tests need only the database:

```sh
docker compose up -d paradedb
cargo test --workspace
```

### Demo data

```sh
scripts/demo-seed.sh
```

ingests the sample documents in [docker/demo](docker/demo) (needs `jq`) and adds a git connection
to ferret's own docs. To index a local carapace knowledge repository as well, start compose with
the demo overlay and seed with the same variable:

```sh
export CARAPACE_KNOWLEDGE_DIR=../carapace/data/knowledges/me
docker compose -f docker-compose.yml -f docker-compose.demo.yml up
scripts/demo-seed.sh
```

More commands and conventions in [AGENTS.md](AGENTS.md).

## Docs

- [Conception](docs/conception/README.md): design and v1 scope
- [Glossary](docs/glossary.md)
- [Architecture decisions](docs/adr/)

## License

[MIT](LICENSE)
