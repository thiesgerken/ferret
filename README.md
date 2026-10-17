<p align="center">
  <a href="https://github.com/thiesgerken/ferret/actions/workflows/ci.yml"><img alt="CI" src="https://img.shields.io/github/actions/workflow/status/thiesgerken/ferret/ci.yml"></a>
  <a href="rust-toolchain.toml"><img alt="Rust 1.99" src="https://img.shields.io/badge/rust-1.99-B7410E?logo=rust&logoColor=white"></a>
  <a href="LICENSE"><img alt="License: MIT" src="https://img.shields.io/github/license/thiesgerken/ferret?color=885629"></a>
  <a href="charts/ferret/README.md"><img alt="Helm chart" src="https://img.shields.io/badge/helm-chart-885629?logo=helm&logoColor=white"></a>
</p>

<h3 align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/brand/mark-dark.svg">
    <img src="docs/brand/mark-light.svg" alt="ferret logo" width="140" height="140">
  </picture><br>
  ferret
</h3>
<p align="center"><strong>A personal search index over everything that describes me.</strong></p>

<p align="center">Keyword and semantic search. Deep links to the origin. One API for me and my agents.</p>

<p align="center">
  <a href="docs/conception/README.md">💡 Conception</a>
  ·
  <a href="docs/glossary.md">📖 Glossary</a>
  ·
  <a href="docs/adr/">🧭 Decisions</a>
  ·
  <a href="charts/ferret/README.md">⚓ Helm Chart</a>
</p>

ferret makes everything that describes me instantly searchable: notes, documents (Paperless), email,
chats, git repos, carapace memory and saved articles. The data already lives in those systems,
ferret is an index over them: keyword and semantic search with snippets, filters and a deep link
back to the origin, for me through a search UI and for agents through the same API.

## 🚧 Status

v1 ([scope](docs/conception/README.md#v1-scope)) is in progress.

## 🛠️ Development

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

More commands and conventions in [AGENTS.md](AGENTS.md).

## 📚 Docs

- [Conception](docs/conception/README.md): design and v1 scope
- [Glossary](docs/glossary.md)
- [Architecture decisions](docs/adr/)

## 📄 License

[MIT](LICENSE)
