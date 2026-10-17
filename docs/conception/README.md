# ferret: conception

## TLDR

ferret makes everything that describes me instantly searchable: notes (Obsidian, AFFiNE), documents
(Paperless), email, chats, git repos, carapace memory and saved articles. The data already lives in
those systems. ferret is an **index** over them. Its job is to find the information and tell me
where to look. For chats without a usable origin (WhatsApp, Signal exports) ferret also holds the
only copy.

It has two consumers:

1. **Me**, through a search UI: keyword and semantic search over all sources, with snippets,
   filters and a deep link back to the origin.
2. **Agents** (carapace first), through the same search API, wrapped in a skill.

## Principles

- **The keyword half is at least as valuable as the semantic half.** Invoice numbers, IBANs, names
  and license plates are where vector search fails. Keyword search uses real BM25, not Postgres
  `ts_rank`.
- **Ingestion is fast and dumb.** An ingest call never waits for embeddings. Keyword search works
  as soon as the call returns, semantic search follows shortly after.
- **One store.** Postgres with ParadeDB `pg_search` (BM25) and pgvector. No separate search engine,
  no message broker.
- **Local embeddings.** DMs and private documents never leave the machine for indexing. Vectors
  are only ever compared within one embedding model, several models can be live side by side.
- **No knowledge graph, no LLM fact extraction.** The sources already carry structure (senders,
  correspondents, wikilinks, tags) that goes into metadata deterministically. Facts are carapace's
  job.

## Non-goals

- Sharing documents between users (ACLs).
- Server-side text extraction (PDF, OCR, docx). Paperless already OCRs, plugins extract text.
- A chat viewer with media, reactions and replies.

## Architecture

See [architecture.md](architecture.md). In short:

- **ferret-server** (Rust: axum, sqlx, juniper) is the only component with database access. GraphQL
  for the UI, REST for machines, an internal API for plugin workers. It schedules plugin runs and
  pumps chunks to the embedder.
- **ferret-plugins** workers pull runs via long polling and ingest through the public REST API.
- **TEI** (Hugging Face [text-embeddings-inference](https://github.com/huggingface/text-embeddings-inference))
  computes embeddings, and later also serves the reranker.
- **frontend**: Next.js, stack and look copied from carapace.

## Data model

Two layers with different durability:

- **`documents` is durable.** For chat exports it is the only copy. It gets backed up.
- **`chunks` and embeddings are derived.** A chunker change re-chunks from `documents`, a new model
  backfills from `chunks`. Never a re-ingest.

```sql
create table documents (
    id            uuid primary key,
    user_id       uuid not null,
    source        text not null,           -- a path: 'notes', 'paperless', 'carapace/facts', ...
    external_id   text not null,           -- stable id given by the ingester
    content_type  text not null,           -- 'markdown', 'plain', 'email': chunker and viewer hint
    title         text,
    url           text,                    -- deep link to the origin, null = ferret's viewer is the origin
    thread        text,                    -- groups documents into a sequence: chat channel, email thread
    authors       text[] not null,
    created_at    timestamptz not null,
    modified_at   timestamptz not null,
    meta          jsonb not null,
    text          text not null,
    content_hash  bytea not null,
    seen_at       timestamptz not null,    -- bumped on every PUT, even a no-op (see sync sweep)
    api_key_id    uuid references api_keys,  -- the key that last put it, null for a plugin run's
    unique (user_id, source, external_id)
);

create table chunks (
    id            uuid primary key,
    document_id   uuid not null references documents on delete cascade,
    -- Denormalized from documents, so BM25 and vector search filter without a join.
    user_id       uuid not null,
    source        text not null,
    content_type  text not null,
    ts            timestamptz not null,    -- the document's created_at
    title         text,                    -- the document's, searched and embedded with the text
    ordinal       int not null,
    start_offset  int not null,            -- into the rendered document text
    end_offset    int not null,
    text          text not null,
    text_hash     bytea not null
);

create table syncs (
    id            uuid primary key,
    user_id       uuid not null,
    source        text not null,
    started_at    timestamptz not null,
    committed_at  timestamptz
);
```

Embeddings live in `chunk_embeddings`, one row per chunk and model, so several models can be live
at once. See [embeddings.md](embeddings.md).

Indexes: BM25 (`pg_search`) on `chunks.text` and `chunks.title`, one partial HNSW index per model
on `chunk_embeddings`, `documents (user_id, source, thread, created_at, id)` (partial, where
`thread` is set) for thread navigation.

## Sources

Source names are paths (`carapace/facts`, `mail/zoho`). A connection (a plugin configured by a
user) owns a root and may write into any source below it ([plugins.md](plugins.md#contract)).

- Filters match by path prefix, segment-wise: `source=carapace` finds `carapace/facts` and
  `carapace/sessions`, but not `carapace-old`. `source=` may be repeated.
- Syncs, sweeps and credentials (run tokens, API keys) are scoped to a path and everything below it.
- **Writer**: the connection or API key that put a document, shown with a name and an icon
  instead of the bare path. A document below a connection's root was put by that connection, any
  other by the API key that last put it (`documents.api_key_id`), revoked keys included. Search
  hits, documents (viewer and `GET /api/documents`) and Explore's rows carry their `writer`. The source facets of search and Explore carry each source's, so the
  UI can group sources by writer: the connection covering it, else the one connection below it or
  the key of its most recently modified document that has one, and none for a root over both a
  connection's documents and a key's. Connections and keys have a name and a lucide icon name,
  chosen by the user with a per-plugin default (git `git-branch`, carapace `brain`, Paperless
  `file-text`, API keys `key-round`).

## Ingest API

```text
PUT    /api/documents                           upsert by (source, external_id), idempotent
                                                -> {outcome: created | updated | unchanged, id}
                                                   or {outcome: excluded}, nothing stored
DELETE /api/documents?source=&external_id=      204, or 404 if there is no such document
POST   /api/syncs                {source}      -> {sync_id}
POST   /api/syncs/{id}/seen      {source, external_ids}
                                                mark existing documents as seen, without content
                                                -> {seen: number of them that exist}, 409 if already committed
POST   /api/syncs/{id}/commit                   sweep documents below the sync's source not seen in it
                                                -> {deleted}, 409 if already committed
GET    /api/documents/{id}                      full document, for context display
GET    /api/documents?source=&thread=            documents of a thread, ordered by created_at, then id
                                                -> {documents}, full documents as above
```

`seen` names the source of its external ids: a sync over `carapace` covers `carapace/facts` and
`carapace/sessions`, whose external ids may repeat. It must be the sync's source or below it.

Source and external id travel in the body or query, never in the URL path: both may contain `/`
(source paths, file paths), which path routing and proxies handle badly.

Body of a PUT:

```json
{
  "source": "notes",
  "external_id": "projects/ferret.md",
  "content_type": "markdown",
  "title": "...",
  "text": "...",
  "url": "obsidian://open?vault=main&file=...",
  "thread": null,
  "authors": ["..."],
  "created_at": "...",
  "modified_at": "...",
  "meta": {"tags": ["..."]}
}
```

Everything is a document. **The ingesting client decides what a document is**, the server only
chunks it. There is no chat-specific structure, see [Chats](#chats).

### Validation

- `url` is optional. Without one, ferret's viewer is the origin (e.g. WhatsApp and Signal exports,
  which have no message deep links).
- `url` must not use the `javascript:`, `data:` or `vbscript:` scheme (case-insensitive, leading
  spaces and control characters and inner tabs or line breaks ignored, as browsers read it): a
  `400`. A denylist rather than an allowlist, because origins use custom schemes (`obsidian:`,
  `mid:`, `zotero:`). The UI applies the same rule before rendering any `url` as a link.
- `title` and `thread` are optional too, `meta` defaults to `{}`. Everything else is required.
- Missing required fields and unknown fields (a typo like `titel`) are a 4xx, never a silent
  default. `source` must be a well-formed path, `external_id` must not be empty.

### What a PUT does

All in one transaction:

1. Hash the document (text plus metadata).
2. **Same `content_hash` as stored**: bump `seen_at`, done.
3. **Changed**: chunk, then diff chunks by `text_hash`. Unchanged chunks keep their embedding
   unless the title changed (it is part of the embedded input). New chunks are left to the embed
   pump, which takes cached embeddings from `embedding_cache` and sends only misses to TEI.
4. Commit. The BM25 index updates transactionally, so keyword search sees the document now.

Plugins can therefore resend everything on every run without cost.

## Chats

Chat plugins cut conversations into segments, each a regular `plain` document with
`thread = {channel}`. See [plugins.md](plugins.md#chats).

## Embedding models

Several models can be live at the same time, each backfilled by the embed pump. Every user picks
their own default in Settings. Evaluating the models on one's own data follows after v1 (#21).
See [embeddings.md](embeddings.md).

## Removing stale content

Depends on whether a source knows its full state:

- **Origins that report deletions** (git diff): explicit `DELETE`.
- **Origins that can list what exists** (Paperless, IMAP accounts, a first full git ingest): start a
  sync, `PUT` changed documents and `seen` the unchanged ones, commit. The commit deletes the
  documents below the sync's source with `seen_at < started_at`. If a run crashes before the
  commit, nothing is deleted.
- **Append-only origins** (chat exports, articles): never commit a sync. Explicit `DELETE` only.

## Chunking

Server-side, per content type. [`text-splitter`](https://crates.io/crates/text-splitter) does the
Markdown-aware splitting. Chunks hold 1000 to 2000 characters: characters rather than model tokens,
because ~2000 characters stay far below every embedding model's input limit, and counting tokens
would tie the chunker to one model's tokenizer.

| content type | strategy |
|---|---|
| `markdown` | by heading, then paragraph, capped at 2000 characters |
| `plain` | by paragraph, then line, capped at 2000 characters |
| `email` | like `plain`. The plugin strips quoted replies, the quoted mail is its own document |

Every chunk keeps its offsets into the document's text, counted in characters (Unicode scalar
values), so a hit shows both the localized snippet and, via `GET /api/documents/{id}`, the full
context.

The document's title belongs to every chunk: Paperless titles, mail subjects and note headings
often carry the most specific words, which the chunk's text may lack. A query word may match the
title instead of the text (see Search), and the embedding model gets `{title}\n\n{text}` (after the
model's prefix, the title capped at 200 characters) as input. The stored chunk text stays the text alone, so offsets and snippets
don't change.

## Search

```text
GET /api/search?q=&source=&content_type=&author=&from=&to=&model=&k=&offset=&collapse=&mode=
```

- **Hybrid**: the 1000 best BM25 chunks and the 100 nearest chunks by vector, fused with reciprocal
  rank fusion (k = 60: a chunk scores 1 / (60 + rank) in each half) in one SQL query. The vector
  half takes the 100 nearest chunks and drops those less similar to the query than the model's
  `min_similarity` ([embeddings.md](embeddings.md#similarity-cut-off)), so an unrelated document
  doesn't "match a little". It uses the user's default model unless `model=` says otherwise.
  Filters, `-word` and `"phrases"` apply to both halves, the every-word rule only to BM25. A chunk
  without an embedding yet (backfill) is found by keyword only. If the query can't be embedded
  within 5 s (TEI down, stuck or broken), the search answers with the keyword half alone and says
  so: `semantic_unavailable` (REST) and `semanticUnavailable` (GraphQL) carry the reason, `null`
  when the semantic half ran, the UI shows a notice above the hits, and the server logs a warning.
  Keyword search is complete on its own, a restarting TEI shouldn't make ferret useless.
- **Mode**: `mode=hybrid` (the default) runs both halves as above. `keyword` runs BM25 alone and
  doesn't embed the query, so it never touches TEI. `semantic` runs the vector half alone with the
  model's cut-off, and fails with 503 (GraphQL: an `UNAVAILABLE` error) when the query can't be
  embedded: falling back to keywords would answer a different question than the one asked. The
  ranking, `total`, the facets, paging and `total_is_lower_bound` all come from the halves that
  ran, and `semantic_unavailable` is only ever set in hybrid mode. Judgments ignore the mode.
- **Filters**: `source` by path prefix, `content_type`, `author`, `from`/`to`. No query language.
  Equality filters on `meta` keys (`meta.<key>=<value>`, with a GIN index on `documents.meta`)
  are post-v1 (#21).
- **Filtered vector search** needs pgvector's iterative index scans
  (`hnsw.iterative_scan = relaxed_order`), otherwise filters silently cut recall below k. Iterative
  scans only filter on columns of the vector's own table, hence the denormalized filter columns.
  The author filter is rare and selective and stays a join.
- **Languages**: German and English, often mixed in one text. Chunk text and the document's title
  are each indexed three times in `pg_search`: German-stemmed, English-stemmed, and trigrams (for
  compounds: "Versicherung" must find "Kfz-Versicherungsbeitrag"). Every query word must match the
  chunk's text or title in at least one of the three: German or English stem, or all of its
  trigrams. So the words of one query may match in different fields: `Versicherung Rechnungen`
  finds "Kfz-Versicherungsbeitrag … Rechnung", and `Versicherung 312` finds a bill titled
  "HUK Kfz-Versicherung Beitragsrechnung" whose text says "312 Euro". No
  language detection. No typo tolerance in v1: the trigram match needs all trigrams, so a
  misspelled word doesn't match (follow-up in #21).
- **Query syntax**: every word must match. `"exact phrase"` must appear in the chunk as a
  substring, ignoring case and whitespace differences (so it also matches inside longer words).
  `-word` drops every document containing it in its text or title, compounds included. Phrases
  are matched on the chunk's text only. All
  three go into the BM25 half, the vector half gets the words without operators. Filters are not
  part of the query text, the UI sets them as chips.
- **Hits are documents.** Ranking happens on chunks (RRF over chunks), then aggregates per
  document by its best chunk, which is shown as the highlighted snippet. The model evaluation
  after v1 (#21) counts recall@10 and MRR at the document level.
- **Time filter**: `from`/`to` filter on `created_at`, `from` inclusive, `to` exclusive.
  `created_at` is shown in the hit, so the date matches what the filter works on. The UI's presets
  ("Past week") go to GraphQL as `range`, resolved against the server's clock.
- **Repeated filters**: `source=`, `content_type=` and `author=` may repeat, a document matches any
  of them.
- **Result**: best chunk text with highlights, offsets, document id, source, content type, title,
  authors, timestamps, `url`, `thread`. The chunk text comes as parts, each marked as highlighted
  or not, so clients need no offset arithmetic. Highlights mark the chunk's words that equal a query
  word, share its German or English stem, or contain it (compounds, for query words of three or
  more characters). Each hit carries the best BM25 score among the document's chunks in the keyword
  candidates (`keyword_score`) and the best cosine similarity to the query among its chunks in the
  vector candidates above the cut-off (`similarity`), each null when none of its chunks is (or
  without the semantic half). The fused score stays internal. Next to the hits, `model`
  names the embedding model the semantic half used (or would have, when `semantic_unavailable`
  says it couldn't run).
- **Counts and timing**: `total` counts the matching documents after the cut-off, exclusions and
  collapsing, not only the `k` returned. `total_is_lower_bound` says it may be short: the keyword
  half filled its 1000 candidates, or all of the 100 nearest chunks passed the cut-off while the
  scope holds more embedded chunks than came back (HNSW is approximate and may return fewer
  neighbours, so their number alone doesn't tell). False means no sign of more, not a guarantee:
  the approximate, `relaxed_order` vector search may miss a chunk above the cut-off.
  `documents_in_scope` counts the caller's documents in the selected sources (all when none are
  selected), regardless of the other filters. `took_ms` is the search's time on the server.
- **Facets**: `facets` counts the matches per source and for the 20 most frequent (and the selected)
  authors, like `total`. Each facet applies every filter except its own, so the other sources stay
  visible while one is selected, and a selected root that is a parent of the stored sources (`mail`
  over `mail/zoho`) gets its covering count. A collapsed thread counts for every author of its
  matching documents, as the author filter keeps a thread in which one of them is theirs.
  `are_lower_bounds` means what `total_is_lower_bound` means.
- **Size and paging**: `k` hits, 10 by default, at most 100, aggregated from the fused candidate
  chunks. `offset` skips that many hits of the ranking for the next page, and `has_more` says
  whether another page exists, up to the candidate limits. Ties rank by id (except at the vector
  half's candidate limit, which HNSW decides approximately), so pages of an unchanged index
  neither overlap nor skip. The index may change between two pages; there are no snapshots
  across requests, the UI drops a document it has already shown, and a skipped hit is accepted.
- **Cost**: one statement returns the page together with every match (`total`, the facets),
  only the page's rows with their chunk text. Each facet whose filter is set takes one more run of
  the hybrid query, so a search runs it one to three times, all in one repeatable-read
  transaction. On the compose demo (389 chunks) the median `took_ms` is 8 ms, and 16 ms with a
  source filter.
- **Collapsing per thread**: hits from the same `thread` collapse into the best one plus "n more in
  this thread", like a web search collapses hits per domain. It catches quoted text that survived
  email stripping and consecutive chat segments. A thread is a `(source, thread)` pair. Collapsing
  happens before `k`, so `k` counts threads. The hit's `collapsed` says how many more documents of
  its thread matched, and `collapse=false` turns it off.

## Users

Each document belongs to exactly one user. Search always filters by the caller's user. The
embedding cache is shared across users: a cache hit only saves compute for text the caller supplied
themselves.

Login, sessions, API keys and scopes: see [auth.md](auth.md).

## Deployment

Kubernetes-native: a Helm chart (server, plugin workers, frontend, TEI, Postgres) is the
deployment, as in carapace. docker-compose exists for local development only. Details in
[architecture.md](architecture.md#deployment).

## Consumers

- **UI**: a Google-like search page, plus Explore, Activity, Connections, Settings and Admin
  ([architecture.md](architecture.md#frontend)). Map and topics come later.
- **Activity**: one feed of what was written, newest first: plugin runs and bursts of an API
  key's writes (those within 10 minutes of each other), each with its writer, status and counts
  of documents put, unchanged and deleted. API activity is grouped by time, not by sync: tools
  like the demo seed write without one. The server counts a running run's writes as they
  arrive (no worker change), a finished run shows its worker's stats. Users see their own, admins
  may ask for everyone's. Next to it the embedding progress per model
  ([embeddings.md](embeddings.md#progress)).
- **Agents** connect to the MCP server at `/mcp` ([architecture.md](architecture.md#apis)) with an
  API key restricted to the sources they may see.
- **carapace**: the MCP server, or a search skill in the knowledge repo that calls the search API,
  with an API key restricted to the sources the agent may see. ferret's domain goes on the sandbox proxy allowlist,
  the key comes from the password manager, the sentinel gates the call. No carapace code.

## Privacy and security

- **Third parties' text reaches the LLM provider** of every agent turn that reads search results.
  Mitigations: per-chat denylists at ingestion (never ingested, never leaked), and deciding which
  agent sessions get the skill at all.
- **Prompt injection**: anyone who messages me can plant text an agent will read. Search output
  labels every hit with source and author and delimits the text as data.
- **Credentials are scoped**: an agent's API key can only search, a plugin run's token can only
  ingest below its own source root ([auth.md](auth.md)). Plugin credentials are encrypted at rest.

## Backups

Infrastructure's job, not the chart's: Velero snapshots the Postgres PVC (CSI snapshots on Ceph
RBD, which are atomic, so Postgres recovers from them like from a crash). The volume must
therefore sit on a storage class with CSI snapshots, not on a file-system backup. For chat exports
`documents` is the only copy: keep the raw export zips until the importer has proven itself on real
data. `FERRET_CONFIG_KEY` lives in a Secret and must be backed up with the cluster's Secrets,
otherwise plugin credentials have to be re-entered after a restore. Media (photos, voice notes) is
not stored in ferret.

## Plugins

v1 ships **git**, **Paperless** and **carapace**. Email, chats, AFFiNE and saved articles are on
the roadmap. Details in [plugins.md](plugins.md).

## Embedding model choice

- **Default: Qwen3-Embedding-0.6B** (June 2025, Apache 2.0, 1024 dimensions, 32k context,
  multilingual, query instruction required).
- **A second model after v1**, for the model evaluation (#21) to compare against. Candidates to
  check (license, German quality, TEI support): jina-embeddings-v5-text-small, Granite Embedding
  Multilingual R2, DenseOn (all 2026). bge-m3 (January 2024) is the fallback if none of them runs
  in TEI.
- **Hardware**: CPU for queries and incremental ingest, a GPU (gaming PC, laptop) for backfills,
  listed in the chart's `externalTeis` ([embeddings.md](embeddings.md#the-tei-pool)).

## v1 scope

- **Server**: ingest API (documents, syncs, `seen`, exclusions), search (hybrid, one hit per
  document, collapsed per thread), GraphQL for the UI, auth (users, sessions, API keys restricted
  by source, run tokens, worker PSK), the MCP server for agents, embed pump, connections with
  scheduling over the internal API.
- **Embeddings**: one configured model (Qwen3-Embedding-0.6B). Adding a second one is a config
  change, switching the default is a choice in Settings.
- **Plugins**: git, Paperless, carapace (Markdown mirror).
- **UI** (English only): login, search with viewer and ✓ judgments, Explore (table with facet
  counts), Connections (config, schedule, runs, exclusions, disable, delete), Settings (account:
  change your own password (#54), API keys, default model with coverage), Admin (users, workers, backlog).
- **Ops**: Helm chart, docker-compose for development, CI and releases as in carapace (images
  `ferret`, `ferret-plugins`, `ferret-frontend`, chart as OCI on ghcr). Structured JSON logs, no
  `/metrics` endpoint.
- **Follow-up issues (#21)**: model evaluation (recall@10 and MRR per model from the judgments in
  Settings), UI i18n (German and English), model removal (dropping a removed model's index and
  embeddings), a host allowlist for plugin targets, and typo tolerance in keyword search.

## Sizing

Rough corpus: ~40M tokens (50k mails, 500k chat messages, 5k Paperless documents, notes, a few
thousand articles), ~300k chunks.

- Embedding the full corpus once locally (Qwen3-Embedding-0.6B or bge-m3): a few hours on Apple
  Silicon, under one hour on a gaming GPU, days on a CPU-only server. Incremental updates are
  negligible.
- Storage: ~600 MB of `halfvec(1024)` per live model, plus its HNSW index.

## Later

- Local reranker (e.g. `bge-reranker-v2-m3` via TEI) over the fused top 50.
- Map in Explore: UMAP via Apple Embedding Atlas over all documents, coloured by source. Needs a
  periodic projection job.
- Topics and salience over carapace facts: BERTopic on the existing embeddings, plus custom
  salience (distinct sessions, spread, recency, a floor for durable facts).

## Open questions

None for v1 at the moment. Plugin-specific details are settled when the plugin is built.

## Naming

The binary and CLI are `ferret`, the images `ferret`, `ferret-plugins` and `ferret-frontend` on
ghcr, under the repository owner's namespace. The crates (`ferret-server`, `ferret-plugins`,
`ferret-api`) are not published, so the taken `ferret` on crates.io doesn't matter. FerretDB shares
the name in the self-hosting world, the owner namespace keeps the images apart.
