# Embeddings and model evaluation

## TLDR

Several embedding models can be live at the same time. Chunks are model-independent, embeddings
are stored per chunk and model. The admin decides which models exist and get backfilled. Every user
picks their own default and sees how much of their data each model covers. Evaluating models on
one's own data is post-v1 (#21).

Model evaluation is meant to be a regular feature, not a one-off: switching to a new model needs
evidence that it retrieves better on my data. v1 collects that evidence from explicit "right hit"
marks. Turning them into metrics is post-v1 (#21).

## Models

Models are configuration, not database state, because each one needs a TEI Deployment from the Helm
chart:

```yaml
embedding:
  default: qwen3-embedding-0.6b
  models:
    - name: qwen3-embedding-0.6b
      url: http://ferret-tei-qwen3-embedding-0-6b:80          # queries, and the pump unless bulk_url is set
      bulk_url: http://ferret-tei-qwen3-embedding-0-6b-gpu:80 # optional: the pump only
      query_prefix: "Instruct: Given a search query, retrieve relevant passages from personal notes, documents and chats\nQuery: "
      document_prefix: ""
      min_similarity: 0.42                                   # see Similarity cut-off
```

The configuration is a YAML file the server reads from `FERRET_CONFIG_FILE` (Helm: a ConfigMap). At
startup the server reads each model's `max_input_length` from TEI's `/info` and its dimension from
embedding a probe text, since `/info` doesn't report it. The server waits up to 10 minutes for an
unreachable model (TEI downloads its model on first start), then stops. Model names end up in index
names and SQL literals, so they are limited to `[a-z0-9][a-z0-9._-]*`, ending in a letter or digit,
at most 40 characters, and no two names may differ only in `.`, `_` and `-`.

- **No silent truncation**: every embed request sets `truncate: false`, so TEI rejects an
  over-long text instead of embedding its beginning.

- **Prefixes**: asymmetric models expect an instruction on the query side (Qwen3-Embedding) or
  `query: ` / `passage: ` markers (E5 family). Without them recall drops noticeably. The prefix is
  part of the cache key (`text_hash` covers the whole input: prefix, title and text), so changing
  it never serves stale vectors.
- **`bulk_url`**: online embedding (queries, incremental ingest) runs on CPU. Big backfills run on a
  GPU node that joins the cluster temporarily. With `bulk_url` set, the pump uses only that
  endpoint. If it is unreachable the pump fails loudly (error in Admin, backlog grows) instead of
  falling back to the CPU endpoint, which would slow the backfill to days without anyone noticing.
  Queries are unaffected either way. One Service over CPU and GPU pods doesn't work: HTTP keep-alive
  pins connections to pods, so the split would be random. In the Helm chart, `tei.gpu.enabled`
  adds a GPU TEI per model (a Deployment pinned to the GPU node) and sets each model's `bulk_url`
  to it. Turning it off after the backfill removes `bulk_url`, and the server restarts with the
  CPU endpoint for the pump. The compose dev setup sets it to a second CPU TEI: a query that
  queues behind a backfill batch there would run into the 5 s query timeout.

- **A model whose `max_input_length` is below the longest embedded input is rejected** and the
  server refuses to start. TEI would otherwise truncate chunks silently. The embedded input is the
  model's `document_prefix`, the title capped at 200 characters, a blank line and the chunk (at
  most 2000 characters). The check covers title and chunk, 2202 characters, and leaves the prefix
  out. It compares them one to one with `max_input_length` tokens: Latin
  text never takes more than one token per character, so a model that accepts 2202 tokens takes
  every input.
- **Adding a model**: the server creates its partial HNSW index and the pump starts backfilling it.
- **Removing a model** is not implemented yet (#21): its index and rows stay until dropped by hand.

## Storage

```sql
create table chunk_embeddings (
    chunk_id      uuid not null references chunks on delete cascade,
    model         text not null,
    -- Denormalized from chunks: pgvector's iterative scans only filter on columns of the table
    -- that holds the vector.
    document_id   uuid not null,                -- for -word
    user_id       uuid not null,
    source        text not null,
    content_type  text not null,
    ts            timestamptz not null,
    embedding     halfvec not null,             -- no fixed dimension, models differ
    primary key (chunk_id, model)
);

-- One per configured model, created by the server at startup.
create index chunk_embeddings_hnsw_qwen3_embedding_0_6b on chunk_embeddings
    using hnsw ((embedding::halfvec(1024)) halfvec_cosine_ops)
    where model = 'qwen3-embedding-0.6b';
```

`embedding_cache (text_hash, model)` stays: it saves compute when the same text appears in another
chunk, another document or another user's data, and when chunks are re-created after re-chunking.
`text_hash` is the SHA-256 of the whole embedded input: prefix, capped title and chunk text (for
queries: prefix and the query's words, without `"…"` quotes and `-word`s). Query embeddings go through the same cache.

Search embeds the query with the requested model and searches only that model's index, so vectors
of different models are never compared.

## Chunking is shared

All models embed the same chunks. That keeps comparisons fair (same units, same BM25 half) and
storage simple. The consequences:

- Every configured model must take the longest embedded input (title and chunk, 2202 characters,
  see above; the chunker caps chunks at 2000 characters, roughly 500 to 700 tokens of German
  text). Current models accept 8k tokens and more, so this only excludes models below 2202
  tokens, such as old 512-token ones.
- Chunk size can't be compared like models can. A chunker change means re-chunking all documents,
  after which every configured model re-embeds. If comparing chunkers becomes a need, `chunks` gets
  a `chunker` column (the same pattern as models).

## Embed pump

The queue is "chunks without an embedding for model X":

```sql
select id, text, title from (
    select c.id, c.text, c.title from chunks c
    where not exists (
        select 1 from chunk_embeddings e where e.chunk_id = c.id and e.model = $1
    )
    and not exists (
        select 1 from embedding_failures f where f.chunk_id = c.id and f.model = $1
    )
    order by c.ts desc
    limit 256
) candidates
where pg_try_advisory_xact_lock(hashtextextended(id::text || $1, 0))
limit $2;  -- the model's batch size, at most 32 (TEI's max_client_batch_size)
```

- The pump serves the default model first, then the others. Within a model, newest chunks first.
- Each batch: look up `embedding_cache`, send the misses to that model's TEI, write
  `chunk_embeddings` and `embedding_cache` in one transaction.
- `LISTEN chunks_pending` wakes the pump after an ingest. Several pumps share the queue through
  the advisory locks.
- A statement trigger on `chunks` sends the notification, delivered when the ingest commits. The
  pump also polls every 30 s, which picks up after outages.
- A failed batch is logged and stored in `embedding_errors` (model, error, time) until the model's
  next successful batch, for Admin. The pump keeps retrying, an unreachable `bulk_url` therefore
  shows as an error and a growing backlog.
- TEI rejecting a batch (4xx) makes the pump retry its texts one by one. Texts TEI rejects alone
  (more tokens than characters, e.g. emoji or CJK) land in `embedding_failures` (chunk, model,
  error) and leave the queue. A changed text is a new chunk and a changed title deletes the
  failure, so either gets another chance.
- Advisory locks instead of row locks: a `PUT` that changes a claimed chunk never waits for TEI.
  The final insert locks the chunks `for share` and skips those deleted meanwhile.
- Timeouts: 5 s for a query embedding (then keyword-only), 60 s for a pump batch.
- The batch size adapts per model and server: a timeout halves it (down to 1), a batch that took
  less than 30 s adds one (up to 32). TEI computes a request to the end even after the pump gave
  up on it, so on a CPU too slow for 32 long chunks, retrying the same batch would queue behind
  the abandoned one and time out again, forever. A GPU TEI stays at 32.

## Per-user model choice

- `users.default_embedding_model` (null = the configured default) decides which model the user's
  searches use, in the UI and through API keys.
- `model=` on the search API (`model` in GraphQL's `SearchInput`) and a model switch in the search
  UI override it per query. An unknown model is an error, never a silent fallback. The response
  says which model ran (`model`).
- A model can be chosen before its backfill is complete. Settings → Models shows each model's
  coverage of the user's chunks, so the user knows the semantic half is partial.
- The model switch in the search UI only appears once more than one model is configured. Its
  default option names the model that ran.
- At startup, users whose default model is no longer configured are reset to the configured
  default, each with a logged warning.

v1 configures a single model, but adding one is a config change plus a TEI Deployment, and
switching to it needs nothing but a choice in Settings once its backfill is done.

## Similarity cut-off

Every chunk has some similarity to every query, so without a floor the vector half would make each
search "match" its 100 nearest chunks, however unrelated. Each model has a `min_similarity`
(cosine, required): nearest chunks below it are dropped before the fusion. It is applied to the
100 nearest rather than inside the HNSW scan, which would otherwise keep scanning for rows that
pass, through the whole index when few do.

Where noise ends depends on the model (and its prefixes), so the value is calibrated per model:

1. Queries of two kinds, in German and English: on-topic ones whose relevant documents are known
   (the demo documents of `scripts/demo-seed.sh`, ferret's docs, notes of the knowledge repo), and
   off-topic ones the corpus has no answer to (sports, recipes, history, physics).
2. Each query runs once through the search, which caches its embedding. SQL then computes every
   embedded chunk's similarity to it, exactly rather than through HNSW.
3. The off-topic queries' best similarities show where noise ends, the relevant documents' best
   chunks where matches begin. The cut-off goes in the middle of that gap: a bigger corpus raises
   the noise (more chunks that are close by chance), and a weak match lost costs more than one
   extra hit.

**Qwen3-Embedding-0.6B: 0.42**, with the prefixes above, from 10 on-topic and 10 off-topic queries
on the compose demo (158 documents, 389 chunks: the demo documents, ferret's docs and a carapace
knowledge repo). The off-topic queries' best chunks reached 0.26 to 0.39, the relevant documents'
0.45 to 0.80. An average document sits at 0.15 to 0.30 from any query, never near 0: the model's
space is anisotropic, which is why 0 can't serve as a floor. At 0.42 no off-topic query keeps a
semantic match and every relevant document stays. On this small corpus that gap rests on a handful
of documents per query. Model evaluation (#57) re-checks the value against the user's judgments.

## Evaluation

### Collecting judgments

A ✓ on a hit records an explicit judgment. It carries its own query, so there is no search log.
A search may have several right hits, each its own judgment: recall@10 and MRR handle that.

- GraphQL only (`setJudgment(input: {search, documentId, judged})`, and `judged` on every hit of
  `search`): agents don't judge. An explicit set rather than a toggle, so a double click can't
  flip it twice.
- `query` is the query as typed, with whitespace collapsed. `filters` is the search's filters in
  canonical form: sources, authors and content types sorted without duplicates, `from`/`to` as
  absolute times. The same search in another chip order is the same judgment. `k` and `collapse`
  aren't part of it, and neither is `model`: evaluation (post-v1, #21) is to replay a judged
  search with every model. A search `search` would reject can't be judged.
- A relative time range ("Past week", `range` in GraphQL's `SearchInput`) is stored as its preset
  instead of the `from` it resolved to, so the same search tomorrow still shows its ✓ and a
  second ✓ doesn't add a row. Replay (#21) will anchor the preset at the judgment's `created_at`.

```sql
create table relevance_judgments (
    id           uuid primary key,
    user_id      uuid not null references users on delete cascade,
    query        text not null,
    filters      jsonb not null,
    document_id  uuid not null references documents on delete cascade,
    created_at   timestamptz not null,
    unique (user_id, query, filters, document_id)
);
```

### Measuring

Settings → Models, per configured model:

- **Coverage** (v1): the share of the user's chunks that have an embedding for this model.
- **recall@10** and **MRR** (post-v1, #21) over the user's judgments, each for vector-only and for
  hybrid search. The server replays every judged query against the model and checks where the
  judged document lands. Runs on demand, results are kept until the next run.
- **Similarity cut-off**: see [above](#similarity-cut-off). Once judgments exist, the same replays can check it.

Plus the model switch in the search UI, to compare result lists of two models by eye.

### Later

- **Click logging**: record which hit was opened after a search, as implicit judgments. Biased
  towards whatever model was active (you only click what you're shown), and it turns into a log of
  every search.
- **Interleaving** (team-draft): merge the results of two models into one list and count clicks
  per model. The fair way to compare from click data, without position bias.
- Admin view with aggregated metrics across users (counts only, no queries).
