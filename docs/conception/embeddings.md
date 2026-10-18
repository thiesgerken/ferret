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

Models and TEIs are configuration, not database state, because the Helm chart deploys them:

```yaml
embedding:
  default: qwen3-embedding-0.6b
  models:
    - name: qwen3-embedding-0.6b
      model_id: Qwen/Qwen3-Embedding-0.6B    # what a TEI serving it reports in /info
      dimension: 1024                        # of its vectors
      query_prefix: "Instruct: Given a search query, retrieve relevant passages from personal notes, documents and chats\nQuery: "
      document_prefix: ""
      min_similarity: 0.42                   # see Similarity cut-off
  teis:
    - url: http://ferret-tei-qwen3-embedding-0-6b:80
    - url: http://192.168.1.20:8080          # e.g. a laptop's GPU in the LAN
      external: true                         # starts with queries off
```

The configuration is a YAML file the server reads from `FERRET_CONFIG_FILE` (Helm: a ConfigMap).
Model names end up in index names and SQL literals, so they are limited to `[a-z0-9][a-z0-9._-]*`,
ending in a letter or digit, at most 40 characters, and no two names may differ only in `.`, `_`
and `-`. No two models may share a `model_id`.

### The TEI pool

A TEI isn't tied to a model: every TEI works for whichever configured model it currently serves.
A laptop is listed once and helps with whatever model it has loaded.

- **Discovery**: at startup and every 10 seconds, the server asks every TEI's `/info` for its
  `model_id` and matches it with the configured models. A TEI that doesn't answer is offline, one
  serving a model no configured one has is unused (Admin shows its model), one whose
  `max_input_length` is too short is unsuitable (see below). A TEI that switches models moves to
  the other model with the next look. TEIs that are down don't keep the server from starting:
  searches answer by keyword until a TEI serves the model.
- **Every answer is checked**: embeddings go through TEI's OpenAI-compatible `/v1/embeddings`,
  whose answer names the model that computed them (TEI's `served_model_name`). An answer from
  another model than discovered, with another dimension than the model's, or with a vector missing
  is discarded, never written, and the TEI is asked again in the background. A TEI started with
  `--served-model-name` is unsuitable: under a fixed name, its answers couldn't show that it
  loaded another model meanwhile. Vectors of two models therefore don't mix as TEIs come and go
  (two models of one dimension under the same model id would, which no config allows). The route
  normalizes like `/embed`.
- **Dimension**: configured per model, since `/info` doesn't report it. The server creates the
  model's partial HNSW index for it at startup, and refuses to start when the
  [model registry](#model-registry) knows the model with another one.
- **Switches** (Admin, table `teis`): per TEI, on/off, queries, embedding. New TEIs of the
  deployment get all three, external ones (`external: true`) no queries, so searches don't depend
  on a laptop that may go to sleep. A change takes effect at once on the server that made it,
  within 10 seconds on the others, without a restart. Rows of TEIs that left the config go at
  startup.
- **Queries** go to an enabled query TEI of the model, one without a pump batch in flight first,
  then the others in config order, until one answers within the 5 s query timeout.
- **The pump** runs one loop per TEI that may embed, for the model it serves. A faster TEI simply
  claims more batches. On a TEI that also answers queries, batches stay short (about 2 s), so a
  query never waits long behind one.

- **No silent truncation**: a TEI started without `--auto-truncate` (the default) rejects an
  over-long text instead of embedding its beginning. With it, the length check below still keeps
  every input within the limit.

- **Prefixes**: asymmetric models expect an instruction on the query side (Qwen3-Embedding) or
  `query: ` / `passage: ` markers (E5 family). Without them recall drops noticeably. The prefix is
  part of the cache key (`text_hash` covers the whole input: prefix, title and text), so changing
  it never serves stale vectors.
- **Backfills**: big ones (the first import, a new model) run on a GPU TEI listed as external,
  e.g. a laptop's. It embeds next to the CPU TEI, taking most batches. Switching the CPU TEI's
  embedding off in Admin leaves it all to the GPU and keeps the CPU for queries. One Service over
  CPU and GPU pods wouldn't work: HTTP keep-alive pins connections to pods, so the split would be
  random. The compose dev setup lists a second CPU TEI (`tei-bulk`) as external, to show both
  kinds.

- **A TEI whose `max_input_length` is below the longest embedded input is unsuitable**: it serves
  the model but isn't used. TEI would otherwise reject or truncate long chunks. The embedded input is the
  model's `document_prefix`, the title capped at 200 characters, a blank line and the chunk (at
  most 2000 characters). The check covers title and chunk, 2202 characters, and leaves the prefix
  out. It compares them one to one with `max_input_length` tokens: Latin
  text never takes more than one token per character, so a model that accepts 2202 tokens takes
  every input.
- **Adding a model**: the server creates its partial HNSW index, and once a TEI serves it, the
  pump backfills it.
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

### Model registry

A model's embeddings stay when it leaves the config, so they are reused if it comes back. The
registry keeps track of them:

```sql
create table embedding_models (
    id          text primary key,   -- the `model` stored with every embedding
    dimension   integer not null,
    first_seen  timestamptz not null default now()
);
```

- **Every model that ever embedded** has a row, configured or not. The migration that created the
  table seeded it from `chunk_embeddings` and `embedding_cache`, at startup the server adds every
  configured model it doesn't know yet.
- **The dimension is fixed.** The server refuses to start when a configured model is known with
  another dimension: its HNSW index is built for one, and its vectors couldn't mix. Changing it
  means removing the model from the config, deleting its embeddings and adding it again (or
  another `name`). It equally refuses a model whose index name a stored model already has (`a.b`
  and `a-b`).
- **Admin → Embedding** lists every registered model: its chunk embeddings across users, the size
  of its HNSW index (exact) and of its vectors in `chunk_embeddings` (estimated from their count,
  the table is shared). Models that aren't configured are marked as such.
- **Deleting** is for models that aren't configured: a configured one has to leave the config
  first, so nothing embeds for it and no user's default is it. After a confirmation, it removes the
  model's registry entry, its HNSW index and, through foreign keys, its chunk embeddings, cache
  entries and failure records, in one transaction under the advisory lock that also guards index
  creation. Documents, chunks and other models stay. If the model comes back later, the pump
  embeds everything again.
- **Rows need a registry entry** (foreign keys on `model` in `chunk_embeddings`, `embedding_cache`
  and `embedding_failures`). A replica still configured for a deleted model, during a rolling
  deploy, fails to write instead of leaving rows no entry accounts for.

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
limit $2;  -- the TEI's batch size, at most 32 and TEI's max_client_batch_size
```

- Each TEI's loop claims chunks of the model the TEI serves, newest first.
- Each batch: look up `embedding_cache`, send the misses to the TEI, write `chunk_embeddings` and
  `embedding_cache` in one transaction.
- `LISTEN chunks_pending` wakes the loops after an ingest, so does a TEI that changed. All loops of
  all replicas share the queue through the advisory locks.
- A statement trigger on `chunks` sends the notification, delivered when the ingest commits. The
  pump also polls every 30 s, which picks up after outages.
- A failed batch or query is logged and stored with the TEI (`teis.last_error`) until its next
  success, for Admin. The pump keeps retrying while the TEI is listed as serving.
- TEI rejecting a batch (413 too long, 422 untokenizable) makes the pump retry its texts one by
  one. Texts TEI rejects alone
  (more tokens than characters, e.g. emoji or CJK) land in `embedding_failures` (chunk, model,
  error) and leave the queue. A changed text is a new chunk and a changed title deletes the
  failure, so either gets another chance.
- Advisory locks instead of row locks: a `PUT` that changes a claimed chunk never waits for TEI.
  The final insert locks the chunks `for share` and skips those deleted meanwhile.
- Timeouts: 5 s for a query embedding (then keyword-only), 60 s for a pump batch.
- The batch size adapts per TEI and server: a timeout, or a batch that took longer than its target,
  halves it (down to 1), a batch that took less than half the target adds one (up to 32). The
  target is the 60 s timeout, and about 2 s on a TEI that also answers queries. TEI computes a
  request to the end even after the pump gave up on it, so on a CPU too slow for 32 long chunks,
  retrying the same batch would queue behind the abandoned one and time out again, forever. A GPU
  TEI stays at 32.

### Progress

What the Activity page shows per model:

- **Backlog**: the user's chunks still pending and those TEI refused (Admin: per source, across
  users).
- **Rate**: embeddings written in the last 10 minutes (`chunk_embeddings.embedded_at`), per
  minute, across users: the pump is shared, so every user sees it. It's read from the database,
  so it holds across server processes and restarts. ETA is the backlog divided by it, optimistic
  for one user while others' chunks wait too. Admins also get the global embedded and total
  chunks.
- **Batch size**: the pump's current one, per server process (it adapts per process).
- **TEI** (Admin): per configured URL (queries, pump, or both), TEI's own figures from its
  Prometheus `/metrics`, read on demand at most every 3 s and kept for a minute: queue size,
  average batch size, inputs embedded per minute, failed requests, and the share of the minute
  spent in inference as a CPU proxy. TEI records each input of a batch with the batch's
  inference time, so that share divides the inference time by the average batch size, which is
  approximate when batch sizes vary. A TEI restart resets its counters, the samples before it
  are dropped. The
  pump's TEI also gets an ETA from its throughput. No Prometheus server is involved.

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
