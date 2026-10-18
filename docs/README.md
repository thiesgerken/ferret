# Ferret documentation

Ferret is a personal search index. It copies text from origins, indexes it, and returns hits that lead back to those origins. Start with a small, disposable local index before connecting private data.

## Try Ferret

- [Quickstart](quickstart.md): start locally, seed documents, search, open an origin, and inspect a git run.
- [Search](search.md): UI controls, query syntax, filters, ranking, threads and origin links.
- [Connections](connections.md): configure scheduled plugins, inspect Activity, and manage indexed data.

## Connect data and agents

- [HTTP ingestion](ingestion.md): write documents and safely reconcile deletions.
- [API guide](api/README.md): choose [REST](api/rest.md), [GraphQL](api/graphql.md) or [MCP](api/mcp.md), with the right credentials.
- Plugin setup: [git](plugins/git.md), [Gitea](plugins/gitea.md), [Paperless](plugins/paperless.md), [carapace](plugins/carapace.md), [IMAP](plugins/imap.md), [AFFiNE](plugins/affine.md), [Vikunja](plugins/vikunja.md), [Karakeep](plugins/karakeep.md).
- [MCP setup](api/mcp.md): connect agents with source-scoped search keys.

## Deploy and operate

- [Configuration](configuration.md): environment, server YAML and startup/live settings.
- [Kubernetes](kubernetes.md): Gateway/storage/secret decisions and deployment walkthrough.
- [Operations](operations.md): health, troubleshooting, upgrades and backup/restore boundaries.
- [Helm chart](../charts/ferret/README.md): production deployment and canonical chart values.
- [Authentication and privacy](auth.md): bootstrap, scopes, worker trust and encrypted credentials.

## Understand and contribute

- [Architecture](architecture.md): components, data flows and public/internal boundaries.
- [Indexing](indexing.md): documents, chunks, storage and derived-data invariants.
- [Embeddings](embeddings.md): models, their endpoints (TEI, proxy, cloud), backfills, stored embeddings and failure behavior.
- [Plugin implementation](plugins/README.md): schemas, registration, claims and safe run lifecycles.
- [Development](development.md): repository development commands.
- [Source](../crates/): server, shared API types, and plugin workers.

## Design and background

These documents explain vocabulary, rationale, and design intent. They are not the operational API reference.

- [Conception](conception/README.md)
- [Architecture decisions](adr/)
- [Glossary](glossary.md)
- [Brand](brand/README.md)
