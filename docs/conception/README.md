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
- **Local embeddings.** DMs and private documents never leave the machine for indexing. One model
  for everything, because all documents share one embedding space.
- **No knowledge graph, no LLM fact extraction.** The sources already carry structure (senders,
  correspondents, wikilinks, tags) that goes into metadata deterministically. Facts are carapace's
  job.

## Non-goals

- Sharing documents between users (ACLs).
- Server-side text extraction (PDF, OCR, docx). Paperless already OCRs, plugins extract text.
- A built-in scheduler. Plugins run via cron, systemd timers or k8s CronJobs.
- A chat viewer with media, reactions and replies.

## Architecture

```text
 origins                 plugins (periodic)          ferret (one Rust binary)              consumers
 ───────                 ──────────────────          ────────────────────────              ─────────
 Obsidian, git     ─┐                               ┌─ ingest API ──┐
 Paperless         ─┤                               │               ▼
 email (IMAP)      ─┼─► ferret ingest <plugin> ─────┤      Postgres (pg_search, pgvector) ─► search API ─┬─► search UI
 AFFiNE            ─┤      (HTTP, like any          │               ▲                                  └─► carapace skill
 chats             ─┤       external ingester)      └─ embed worker ┘
 carapace, articles ┘                                      │
                                                           ▼
                                                    TEI (embeddings, reranker later)
```

- **Server**: axum, sqlx. Ingest API and search API.
- **Embed worker**: a tokio task inside the same binary, enabled by default (`--no-worker` to turn
  it off). Several instances are safe because work is claimed with `SKIP LOCKED`.
- **Embedder**: Hugging Face [text-embeddings-inference](https://github.com/huggingface/text-embeddings-inference)
  (TEI), called over HTTP. It also serves rerankers.
- **Plugins**: subcommands of the same binary (`ferret ingest paperless --config ...`). They talk to
  the HTTP API like any third-party ingester, so the API is dogfooded.

## Data model

Two layers with different durability:

- **`documents` is durable.** For chat exports it is the only copy. It gets backed up.
- **`chunks` and embeddings are derived.** A change of model or chunker is a rebuild job over
  `documents`, never a re-ingest.

```sql
create table documents (
    id            uuid primary key,
    user_id       uuid not null,
    source        text not null,           -- 'obsidian-main', 'paperless', 'whatsapp', ...
    external_id   text not null,           -- stable id given by the ingester
    content_type  text not null,           -- 'markdown', 'plain', 'email', 'chat'
    title         text,
    url           text,                    -- required unless content_type = 'chat'
    authors       text[] not null,
    created_at    timestamptz not null,
    modified_at   timestamptz not null,
    meta          jsonb not null default '{}',
    body          jsonb not null,          -- text, or structured messages for chats
    content_hash  bytea not null,
    seen_at       timestamptz not null,    -- bumped on every PUT, even a no-op (see sync sweep)
    unique (user_id, source, external_id)
);

create table chunks (
    id           uuid primary key,
    document_id  uuid not null references documents on delete cascade,
    ordinal      int not null,
    start_offset int not null,             -- into the rendered document text
    end_offset   int not null,
    text         text not null,
    text_hash    bytea not null,
    message_ids  text[],                   -- chats only: messages covered by this chunk
    embedding    halfvec(1024)             -- null = waiting for the embed worker
);

-- Saves compute across documents and users. Keyed by model, so a model change starts empty.
create table embedding_cache (
    text_hash  bytea not null,
    model      text not null,
    embedding  halfvec(1024) not null,
    primary key (text_hash, model)
);

create table syncs (
    id            uuid primary key,
    user_id       uuid not null,
    source        text not null,
    started_at    timestamptz not null,
    committed_at  timestamptz
);
```

Indexes: BM25 (`pg_search`) on `chunks.text`, HNSW on `chunks.embedding`, a partial index on
`chunks (id) where embedding is null` for the worker queue, GIN on `documents.meta`.

`halfvec` halves storage compared to `vector` with no measurable recall loss. The vector is copied
into the chunk row although the cache has it too: search then stays on one table.

## Ingest API

```text
PUT    /sources/{source}/documents/{external_id}    upsert, idempotent
DELETE /sources/{source}/documents/{external_id}
POST   /sources/{source}/syncs                      -> {sync_id}
POST   /sources/{source}/syncs/{sync_id}/commit     sweep documents not seen in this sync
GET    /documents/{id}                              full document, for context display
```

Body of a PUT (non-chat):

```json
{
  "content_type": "markdown",
  "title": "...",
  "text": "...",
  "url": "obsidian://open?vault=main&file=...",
  "authors": ["..."],
  "created_at": "...",
  "modified_at": "...",
  "meta": {"tags": ["..."]}
}
```

Chats are structured, one document per conversation and day (`external_id = {channel}:{date}`):

```json
{
  "content_type": "chat",
  "channel": "...",
  "messages": [{"id": "...", "sender": "...", "sent_at": "...", "body": "..."}]
}
```

When a new message arrives, the plugin sends the whole day again. The hash diff below makes that
cheap.

### Validation

- `url` is required for `markdown`, `plain` and `email`, and rejected for `chat`. Chats have no
  usable origin to link to (WhatsApp and Signal have no message deep links). A chat hit links to
  ferret's own conversation view instead.
- Missing required fields are a 4xx, never a silent default.

### What a PUT does

All in one transaction:

1. Render the document text (chats: one line per message) and hash it.
2. **Same `content_hash` as stored**: bump `seen_at`, done.
3. **Changed**: chunk, then diff chunks by `text_hash`. Unchanged chunks keep their embedding. New
   chunks look up `embedding_cache` and stay `null` on a miss.
4. Commit. The BM25 index updates transactionally, so keyword search sees the document now.

Plugins can therefore resend everything on every run without cost.

## Embed worker

The queue is `chunks where embedding is null`. No queue table, no Kafka.

```sql
select id, text from chunks
where embedding is null
order by id
limit 64
for update skip locked;
```

Embed the batch via TEI, write `chunks.embedding` and `embedding_cache`, commit. `LISTEN/NOTIFY`
wakes idle workers after an ingest.

## Removing stale content

Depends on whether a source knows its full state:

- **Full-state sources** (git, Obsidian, Paperless, AFFiNE, IMAP folders): start a sync, PUT
  everything, commit. The commit deletes the source's documents with `seen_at < started_at`. If a
  run crashes before the commit, nothing is deleted.
- **Append-only sources** (chats, articles): never commit a sync. Explicit `DELETE` only.

## Chunking

Server-side, per content type. [`text-splitter`](https://crates.io/crates/text-splitter) does the
token-aware and Markdown-aware splitting.

| content type | strategy |
|---|---|
| `markdown` | by heading, then paragraph, capped by tokens |
| `plain` | by paragraph, capped by tokens |
| `email` | quoted reply chains stripped (the quoted mail is its own document) |
| `chat` | windows split on gaps over ~30 min, capped at ~500 tokens, prefixed with channel name and participants |

Every chunk keeps its offsets into the rendered text, so a hit shows both the localized snippet and,
via `GET /documents/{id}`, the full context.

## Search

```text
GET /search?q=&source=&content_type=&author=&from=&to=&meta.<key>=<value>&k=
```

- **Hybrid**: BM25 top N and vector top N, fused with reciprocal rank fusion (k = 60) in one SQL
  query.
- **Filters**: core columns plus equality on `meta` keys. No query language.
- **Filtered vector search** needs pgvector's iterative index scans
  (`hnsw.iterative_scan = relaxed_order`), otherwise filters silently cut recall below k.
- **German compounds**: "Versicherung" must find "Kfz-Versicherungsbeitrag". Index chunk text twice
  in `pg_search`: a stemmed field and an ngram field.
- **Result**: chunk text with highlights, offsets, document id, source, content type, title,
  authors, timestamps, `url`. Chat hits carry `channel` and `message_id` instead of `url`.

## Users

Each document belongs to exactly one user. Search always filters by the caller's user. The
embedding cache is shared across users: a cache hit only saves compute for text the caller supplied
themselves.

## Consumers

- **Search UI**: search first. Map and topics come later.
- **carapace**: a `kb-search` skill in the knowledge repo that calls the search API. The KB domain
  goes on the sandbox proxy allowlist, the token comes from the password manager, the sentinel gates
  the call. No carapace code.

## Privacy and security

- **Third parties' text reaches the LLM provider** of every agent turn that reads search results.
  Mitigations: per-chat denylists at ingestion (never ingested, never leaked), and deciding which
  agent sessions get the skill at all.
- **Prompt injection**: anyone who messages me can plant text an agent will read. Search output
  labels every hit with source and author and delimits the text as data.
- **Tokens are scoped**: an agent's token can only search, an ingester's token can only ingest into
  its sources.

## Backups

`documents` is backed up like any primary data store (pg_dump on a schedule, or pgBackRest / CNPG
barman). For chat exports it is the archive. Keep the raw export zips until the importer has proven
itself on real data. Media (photos, voice notes) is not stored in ferret.

## Plugins, in order of value per effort

1. Obsidian and git (files on disk)
2. Paperless (REST API, OCR text included)
3. carapace (memory mirror in the knowledge repo)
4. Email (IMAP)
5. AFFiNE (export or API, to be checked)
6. Chats (WhatsApp and Signal exports, possibly Matrix with mautrix bridges for live ingestion)
7. Saved articles (Karakeep or Wallabag), their own source, so search can include or exclude them

Only what I read or saved goes in. No news firehose.

## Sizing

Rough corpus: ~40M tokens (50k mails, 500k chat messages, 5k Paperless documents, notes, a few
thousand articles), ~300k chunks.

- Embedding the full corpus once locally (Qwen3-Embedding-0.6B or bge-m3): a few hours on Apple
  Silicon, under one hour on a gaming GPU, days on a CPU-only server. Incremental updates are
  negligible.
- Storage: ~600 MB of `halfvec(1024)` plus the HNSW index.

## Later

- Local reranker (e.g. `bge-reranker-v2-m3` via TEI) over the fused top 50.
- Map: Apple Embedding Atlas over all chunks, coloured by source.
- Topics and salience over carapace facts: BERTopic on the existing embeddings, plus custom
  salience (distinct sessions, spread, recency, a floor for durable facts).

## Open questions

1. **Users and auth**: who besides me? How are users and tokens managed (static config, OIDC)?
2. **Embedding model**: Qwen3-Embedding-0.6B vs bge-m3. Run a recall test on real German and
   English chat windows and Paperless documents before the first full ingest.
3. **Chat windowing**: gap and size parameters, and whether a window may cross the day-document
   boundary (it can't with one document per day).
4. **Email**: one document per message or per thread? How to strip quotes reliably?
5. **AFFiNE**: is there a usable API or export for pages?
6. **carapace**: parse the rendered Markdown mirror, or have carapace write a JSON file next to each
   mirror file?
7. **Rebuild procedure** after a model change: re-embed in place (search degrades during the
   rebuild) or into a second column and switch?

## Naming

The binary and CLI are `ferret`. The crate name `ferret` is taken on crates.io and FerretDB
occupies the name in the self-hosting world, so crate and container image need a qualified name
(`ferret-search`, to be checked).
