<p align="center">
  <a href="https://github.com/thiesgerken/ferret/actions/workflows/release.yml"><img alt="Release workflow" src="https://img.shields.io/github/actions/workflow/status/thiesgerken/ferret/release.yml?branch=main&label=release%20build"></a>
  <a href="https://github.com/thiesgerken/ferret/releases"><img alt="Release" src="https://img.shields.io/github/v/release/thiesgerken/ferret?display_name=tag"></a>
  <a href="rust-toolchain.toml"><img alt="Rust" src="https://img.shields.io/badge/rust-stable-B7410E?logo=rust&logoColor=white"></a>
  <a href="LICENSE"><img alt="License: MIT" src="https://img.shields.io/badge/license-MIT-885629"></a>
  <a href="charts/ferret/README.md"><img alt="Helm chart" src="https://img.shields.io/badge/helm-chart-885629?logo=helm&logoColor=white"></a>
</p>

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/brand/logo-dark.svg">
    <img src="docs/brand/logo-light.svg" alt="ferret" width="360">
  </picture>
</p>
<p align="center"><strong>A personal search index over everything that describes you.</strong></p>

<p align="center">Hybrid search. Deep links to the origin. One API and MCP server for you and your agents.</p>

<p align="center">
  <a href="#-getting-started">🚀 Getting started</a>
  ·
  <a href="docs/conception/plugins.md">🔌 Plugins</a>
  ·
  <a href="docs/conception/architecture.md#apis">🤖 API</a>
  ·
  <a href="#-mcp">🧩 MCP</a>
  ·
  <a href="charts/ferret/README.md">⚓ Helm chart</a>
</p>

Your notes and documents are spread over a DMS, git repos, email, chats, agent sessions and more,
each with its own search, mostly not a good one. ferret is the **discovery layer** on top, built
agents-first: one index over all of them that answers *"what do I have about this, and where?"* in
a single query. Your agent gets the passage that matched and a link straight back to the origin,
where that source's own tools take over, instead of guessing which tool's search to try first.

## ✨ Highlights

- 🔎 **Hybrid search that speaks German and English.** BM25 with German and English stemming plus
  trigrams for compounds, fused with multilingual embeddings. Switch either half off when you know
  what you want. The UI speaks both languages too.
- 🔗 **Find it here, open it there.** Every hit deep-links to its origin: the Paperless document,
  the file in your git forge or Obsidian vault, the carapace session. ferret keeps a copy to search
  it, never to become the place you read it.
- 🔌 **Plugins for your systems, a REST API for everything else.** git repos, Gitea accounts,
  mounted filesystem directories and selected plaintext/code, Home Assistant inventory and configured behavior, Paperless, carapace, IMAP mailboxes, AFFiNE workspaces, Vikunja tasks, Karakeep bookmarks, Mealie recipes and Moneta expenses sync on a schedule.
  Anything else sends documents with a plain HTTP `PUT`.
- 🤖 **Your agent searches once.** One index over everything, so the cocktail question finds the
  recipe and the note about your home bar in the same query, and the agent knows where to dig
  deeper with that source's own tools. It's the search you use too, through the REST API or as an
  MCP server. API keys are scoped to sources and to read or write, so an agent sees exactly what
  you hand it.
- ⚡ **Embeddings from wherever you like.** Each model names an OpenAI-compatible endpoint: a TEI
  in the cluster, a LiteLLM in front of your laptop's GPU for big backfills, or a cloud API.
  Activity shows the progress.
- ☸️ **Self-hosted, Kubernetes-native.** A Helm chart, Postgres (ParadeDB and pgvector) as the only
  store, secrets generated on install.

## 💡 Motivation

My personal agent ([carapace](https://github.com/thiesgerken/carapace)) often spends considerable
amounts of time, tokens and tool calls just to find out *where* to find stuff. And sometimes it
doesn't find it at all, even if the info is there. Sure, it has access to my recipes, DMs,
documents, emails, … and each of those has some kind of search function that might be good or not.
Mostly not. And even if: searching all of those in parallel is neither efficient nor what the agent
usually does. It either stops after the first okay-ish search result or finds nothing after
searching the wrong sources.

Example: searching for cocktail ideas, it goes immediately into my recipe tool, but does not think
about checking the document that says what I like and what is in my home bar. Sure, I can add a
skill or put explicit instructions into the agent's system prompt, but that is also not efficient,
cumbersome, and only works after I got frustrated at least once.

What I wanted was a "discovery tool": the agent gets a quick overview of what I already have
scattered around about a topic, and then uses dedicated skills to fetch more information from the
respective sources.

## 📸 Screenshots

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/assets/screenshots/search-dark.png">
    <img src="docs/assets/screenshots/search.png" width="1000">
  </picture>
</p>

<p align="center"><em>Results with filters by source and author, relevance bars for both halves of the search, and a snippet that opens the document at the match.</em></p>

<br>

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/assets/screenshots/activity-dark.png">
    <img src="docs/assets/screenshots/activity.png" width="1000">
  </picture>
</p>

<p align="center"><em>Activity: what's indexed and embedded, and every sync as it happens.</em></p>

## 🔌 Sources

| plugin | indexes | a hit links to |
| --- | --- | --- |
| git | Markdown and text files of a repository, e.g. an Obsidian vault | the file in your forge or vault, at its commit |
| Gitea | default-branch files and open issue/PR discussions across account-associated repositories | the file, issue, PR, comment or review in Gitea |
| Paperless | documents with their OCR text | the document in Paperless |
| carapace | facts, skills and archived sessions of a knowledge repository | the file or session in carapace |
| IMAP | a mailbox, with its threads kept together | the mail, e.g. as a `mid:` link |
| AFFiNE | the docs of a workspace | the doc in AFFiNE |
| Vikunja | tasks and complete comment discussions, including archived projects | the task in Vikunja |
| Karakeep | saved text and annotations, including uploaded PDFs/images | the bookmark in Karakeep |
| Mealie | group-wide recipes with attached notes and comments, across households | the recipe in Mealie |
| Moneta | every visible expense, including income, transfers and labelled templates | the expense editor in Moneta |
| Home Assistant | devices, areas, eligible entities and faithful automation/script definitions | inventory or loaded-definition pages (ordinary registry-less entities use instance home) |
| Files | immediate-child directory listings and complete selected plaintext/code files from read-only mounts | independently configured origin URLs, including NFS |

Everything else goes through the [ingest API](docs/conception/README.md#ingest-api): a `PUT` per
document with an API key, from a script, a cron job or an agent.

## 🧩 MCP

ferret speaks MCP (Streamable HTTP) at `/mcp`, with an API key with `search:read` as the bearer
token. Its tools are `search`, `get_document` and `list_sources`. In Claude Code, for example:

```sh
claude mcp add --transport http ferret https://ferret.example.com/mcp \
  --header "Authorization: Bearer fk_…"
```

## 🚀 Getting started

Follow the [quickstart](docs/quickstart.md) to start the development stack, sign in, seed sample
documents, search a known phrase, and open its origin. It includes Apple Silicon instructions and
shows how to inspect a real git connection in Activity.

The Compose stack is for local development and demos, not deployment. For a real installation
follow the [Kubernetes walkthrough](docs/kubernetes.md) and [Helm chart](charts/ferret/README.md).

## Documentation

Start at the [documentation index](docs/README.md), or go directly to:

- [Search](docs/search.md): query syntax, filters, ranking and origin links.
- [Connections](docs/connections.md): schedules, runs, exclusions and data ownership.
- [HTTP ingestion](docs/ingestion.md): document identity, upserts and safe deletion reconciliation.
- Plugin setup: [files](docs/plugins/files.md), [git](docs/plugins/git.md), [Gitea](docs/plugins/gitea.md), [Paperless](docs/plugins/paperless.md),
  [carapace](docs/plugins/carapace.md), [IMAP](docs/plugins/imap.md), [AFFiNE](docs/plugins/affine.md),
  [Vikunja](docs/plugins/vikunja.md), [Karakeep](docs/plugins/karakeep.md), [Mealie](docs/plugins/mealie.md), [Moneta](docs/plugins/moneta.md), [Home Assistant](docs/plugins/homeassistant.md).
- [MCP setup](docs/api/mcp.md): connect agents to search, full-document reads and source discovery.
- [API guide](docs/api/README.md): REST and GraphQL contracts and credential choices.
- [Architecture](docs/architecture.md): components, data flows and subsystem references.
- [Configuration](docs/configuration.md) and [operations](docs/operations.md): deploy, diagnose,
  upgrade and back up the index.
- [Development](docs/development.md): local development and tests.

Design background remains in [conception](docs/conception/README.md),
[ADRs](docs/adr/), the [glossary](docs/glossary.md), and [brand](docs/brand/README.md).

## Remarks

- Mandatory AI disclaimer: of course I use AI for coding, everything else just doesn't make sense.
  ferret is built by a small team of agents (developers, a reviewer and an orchestrator) working
  through pull requests and review threads. The architecture and design decisions are my own, and
  I review what lands. I do have over 20 years of experience in coding without AI.
- I made this for me! And because making stuff is fun, and sharing it too.
- ferret is pre-1.0. Expect breaking changes before `1.0.0`.

## License

[MIT](LICENSE)
