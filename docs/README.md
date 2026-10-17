# Ferret documentation

Ferret is a personal search index. It copies text from origins, indexes it, and returns hits that lead back to those origins. Start with a small, disposable local index before connecting private data.

## Try Ferret

- [Quickstart](quickstart.md): start locally, seed documents, search, open an origin, and inspect a git run.
- [Connections](connections.md): configure scheduled plugins, inspect Activity, and manage indexed data.

## Connect data and agents

- [HTTP ingestion](ingestion.md): write documents and safely reconcile deletions.
- Plugin setup: [git](plugins/git.md), [Paperless](plugins/paperless.md), [carapace](plugins/carapace.md), [IMAP](plugins/imap.md), [AFFiNE](plugins/affine.md).
- [MCP availability and setup](api/mcp.md): the implementation currently requires the unmerged MCP change, not stock main.

## Deploy and operate

- [Helm chart](../charts/ferret/README.md): production deployment and canonical chart values.

## Understand and contribute

- [Development](development.md): repository development commands.
- [Source](../crates/): server, shared API types, and plugin workers.

## Design and background

These documents explain vocabulary, rationale, and design intent. They are not the operational API reference.

- [Conception](conception/README.md)
- [Architecture decisions](adr/)
- [Glossary](glossary.md)
- [Brand](brand/README.md)
