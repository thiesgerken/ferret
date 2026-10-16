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

starts ParadeDB, TEI with Qwen3-Embedding-0.6B and the server on <http://localhost:8080>. On Apple
Silicon, set `TEI_IMAGE=ghcr.io/huggingface/text-embeddings-inference:cpu-arm64-1.9.4` for a
native TEI build. Tests need only the database:

```sh
docker compose up -d paradedb
cargo test --workspace
```

More commands and conventions in [AGENTS.md](AGENTS.md).

## Docs

- [Conception](docs/conception/README.md): design and v1 scope
- [Glossary](docs/glossary.md)
- [Architecture decisions](docs/adr/)

## License

[MIT](LICENSE)
