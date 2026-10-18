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

Models and their endpoints are configuration, not database state, because the Helm chart (or
whoever deploys ferret) decides them:

```yaml
embedding:
  default: qwen3-embedding-0.6b
  models:
    - id: qwen3-embedding-0.6b                # stored with every embedding
      dimension: 1024                         # of its vectors
      query_prefix: "Instruct: Given a search query, retrieve relevant passages from personal notes, documents and chats\nQuery: "
      document_prefix: ""
      min_similarity: 0.42                    # see Similarity cut-off
      embedding_api:                          # computes the chunks' embeddings
        url: http://litellm:4000/v1
        model: qwen3-embed                    # what the endpoint calls the model
        token_env: FERRET_LITELLM_TOKEN       # optional, a bearer token from the environment
        concurrency: 4                        # optional, requests at once per server replica
      query_api:                              # computes the queries' embeddings
        url: http://tei:80/v1
        model: Qwen/Qwen3-Embedding-0.6B
```

The configuration is a YAML file the server reads from `FERRET_CONFIG_FILE` (Helm: a ConfigMap).
Model ids end up in index names and SQL literals, so they are limited to `[a-z0-9][a-z0-9._-]*`,
ending in a letter or digit, at most 40 characters, and no two ids may differ only in `.`, `_` and
`-`.

### Endpoints

ferret doesn't manage the servers that compute embeddings. Each model names the OpenAI-compatible
endpoint that computes them: a TEI, a proxy such as LiteLLM, or a cloud API. Routing, failover and
serving are that endpoint's job, ferret only measures its own work.

- **Two roles**: `embedding_api` embeds chunks, `query_api` embeds search queries. A model needs
  at least one, and the one that is set serves both when the other is missing. This way, queries go
  to a small, always-on endpoint while a backfill runs elsewhere. `concurrency` (default 1) only
  exists on the endpoint that embeds chunks.
- **One request shape**: every endpoint gets `POST {url}/embeddings` with `model`, `input` and
  `encoding_format: float`, plus `Authorization: Bearer` when the endpoint has a `token_env`. The
  server reads the token at startup and refuses to start when the variable is unset or empty,
  since a typo would otherwise send requests without it.
- **Every answer is checked**: exactly one vector per input, `index` covering 0..n-1, every vector
  of the configured dimension, finite and with a norm above 0. An answer that fails a check is
  discarded as a whole, nothing of it is written. The answer's `model` field isn't checked:
  proxies and cloud APIs name models their own way.
- **Not ferret's job**: verifying that the endpoint serves the right weights. A model name at the
  endpoint must always mean the same weights, since vectors of another model would end up in this
  one's index. ferret can't detect a swap, so the deployment has to guarantee it.
- **Dimension**: configured per model, since endpoints don't report it. The server creates the
  model's partial HNSW index for it at startup, and refuses to start when the model's stored
  embeddings have another one.
- **Endpoints aren't contacted at startup**: one that is down doesn't keep the server from
  starting. An unreachable endpoint is a normal state: searches with its model answer by keyword,
  the pump backs off, and other models aren't held up.
- **Queries** go to the model's query endpoint, with a 5 s timeout. When it fails, a hybrid search
  answers by keyword and says so, a semantic-only search fails.
- **A cloud endpoint receives every chunk and every search query**: the privacy boundary moves
  with the endpoint.

- **No silent truncation**: a TEI started without `--auto-truncate` (the default) rejects an
  over-long text instead of embedding its beginning. Such inputs land in `embedding_failures`
  (see below).

- **Prefixes**: asymmetric models expect an instruction on the query side (Qwen3-Embedding) or
  `query: ` / `passage: ` markers (E5 family). Without them recall drops noticeably. The prefix is
  part of the cache key (`text_hash` covers the whole input: prefix, title and text), so changing
  it never serves stale vectors.
- **Backfills**: big ones (the first import, a new model) go to whatever endpoint the model's
  `embedding_api` names, e.g. a GPU TEI or a proxy in front of several, with `concurrency` to keep
  it busy. Queries stay on the `query_api`, so searches don't wait behind the backfill.
- **Deployment**: the Helm chart deploys no TEI by default. `bundledTei` deploys one CPU TEI for
  exactly one model, for a quickstart. The compose dev setup runs a Qwen3 TEI (`tei`) and a bge-m3
  one (`tei-bge`) scaled to 0, so the second model shows an unreachable endpoint until it's scaled
  up.

- **The longest embedded input** is the model's `document_prefix`, the title capped at 200
  characters, a blank line and the chunk (at most 2000 characters): title and chunk come to 2202
  characters. Latin text never takes more than one token per character, so a model that accepts
  2202 tokens takes every input. ferret doesn't ask the endpoint for its limit, an input it
  rejects is recorded and skipped.
- **Adding a model**: the server creates its partial HNSW index, and the pump backfills it through
  the model's endpoint.
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
limit $2;  -- the model's batch size, at most 32
```

- Each model has `concurrency` loops per server replica, which claim its chunks newest first.
- Each batch: look up `embedding_cache`, send the misses to the model's `embedding_api`, write
  `chunk_embeddings` and `embedding_cache` in one transaction.
- `LISTEN chunks_pending` wakes the loops after an ingest. All loops of all replicas share the
  queue through the advisory locks, so no chunk is embedded twice.
- A statement trigger on `chunks` sends the notification, delivered when the ingest commits. The
  pump also polls every 30 s, which picks up after outages.
- A failed batch (unreachable, an error status, an answer that fails a check) leaves its chunks
  pending, and the loop tries again after a pause that doubles from 1 s to 60 s. New chunks don't
  cut the pause short: a burst of ingests must not hammer an endpoint that is down. The endpoint
  logs the first failure of an outage only, and keeps its last error and whether it answered at
  all for Admin, in memory per server process, until its next success.
- A 429 makes the loop wait for `Retry-After` (30 s without one). The endpoint is busy, not slow,
  so the batch size stays.
- The endpoint rejecting a batch (400, 413 too long, 422 untokenizable) makes the pump retry its
  texts one by one. Texts it rejects alone (more tokens than characters, e.g. emoji or CJK) land
  in `embedding_failures` (chunk, model, error) and leave the queue. A changed text is a new chunk
  and a changed title deletes the failure, so either gets another chance.
- Advisory locks instead of row locks: a `PUT` that changes a claimed chunk never waits for the
  endpoint. The final insert locks the chunks `for share` and skips those deleted meanwhile.
- Timeouts: 5 s for a query embedding (then keyword-only), 60 s for a pump batch.
- The batch size adapts per model and server: a timeout, or a batch that took longer than its
  target, halves it (down to 1), a batch that took less than half the target adds one (up to 32).
  The target is the 60 s timeout, and about 2 s when one endpoint embeds chunks and queries, so a
  query never waits long behind a batch. A TEI computes a request to the end even after the pump
  gave up on it, so on a CPU too slow for 32 long chunks, retrying the same batch would queue
  behind the abandoned one and time out again, forever. A GPU endpoint stays at 32.

### Progress

What the Activity page shows per model:

- **Backlog**: the user's chunks still pending and those the endpoint rejected (Admin: per source,
  across users).
- **Rate**: embeddings written in the last 10 minutes (`chunk_embeddings.embedded_at`), per
  minute, across users: the pump is shared, so every user sees it. It's read from the database,
  so it holds across server processes and restarts. ETA is the backlog divided by it, optimistic
  for one user while others' chunks wait too. Admins also get the global embedded and total
  chunks.
- **Endpoints** (Admin, and admins on Activity): each model's endpoints with their URL and model
  name, whether the last request reached them, and the last error with its time. As the answering
  server process saw them: the state lives in memory. Hardware and inference metrics are the
  endpoint's business, not ferret's.

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

v1 configures a single model, but adding one is a config change naming its endpoint, and
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
