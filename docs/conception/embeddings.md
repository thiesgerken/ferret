# Embeddings and model evaluation

## TLDR

Several embedding models can be live at the same time. Chunks are model-independent, embeddings
are stored per chunk and model. The admin decides which models exist and get backfilled. Every user
evaluates them on their own data and picks their own default.

Model evaluation is a regular feature, not a one-off: switching to a new model needs evidence that
it retrieves better on my data. ferret collects that evidence from explicit "right hit" marks.

## Models

Models are configuration, not database state, because each one needs a TEI Deployment from the Helm
chart:

```yaml
embedding:
  default: qwen3-embedding-0.6b
  models:
    - name: qwen3-embedding-0.6b
      url: http://ferret-tei-qwen3:80             # queries, and the pump unless bulk_url is set
      bulk_url: http://ferret-tei-qwen3-gpu:80    # optional: the pump only
      query_prefix: "Instruct: Given a search query, retrieve relevant passages from personal notes, documents and chats\nQuery: "
      document_prefix: ""
```

At startup the server reads each model's dimension and `max_input_length` from TEI's `/info`.

- **Prefixes**: asymmetric models expect an instruction on the query side (Qwen3-Embedding) or
  `query: ` / `passage: ` markers (E5 family). Without them recall drops noticeably. The prefix is
  part of the cache key (`text_hash` covers prefix plus text), so changing it never serves stale
  vectors.
- **`bulk_url`**: online embedding (queries, incremental ingest) runs on CPU. Big backfills run on a
  GPU node that joins the cluster temporarily. With `bulk_url` set, the pump uses only that
  endpoint. If it is unreachable the pump fails loudly (error in Admin, backlog grows) instead of
  falling back to the CPU endpoint, which would slow the backfill to days without anyone noticing.
  Queries are unaffected either way. One Service over CPU and GPU pods doesn't work: HTTP keep-alive
  pins connections to pods, so the split would be random.

- **A model whose `max_input_length` is below the chunker's cap is rejected** and the server
  refuses to start. TEI would otherwise truncate chunks silently.
- **Adding a model**: the server creates its partial HNSW index and the pump starts backfilling it.
- **Removing a model**: the server drops its index and its rows. User preferences pointing to it
  are reset to the default at startup, logged as a warning.

## Storage

```sql
create table chunk_embeddings (
    chunk_id      uuid not null references chunks on delete cascade,
    model         text not null,
    -- Denormalized from chunks: pgvector's iterative scans only filter on columns of the table
    -- that holds the vector.
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
Query embeddings go through the same cache.

Search embeds the query with the requested model and searches only that model's index, so vectors
of different models are never compared.

## Chunking is shared

All models embed the same chunks. That keeps comparisons fair (same units, same BM25 half) and
storage simple. The consequences:

- The chunker's token cap (~500) must fit every configured model. Current models accept 8k tokens
  and more, so this only excludes old 512-token models.
- Chunk size can't be compared like models can. A chunker change means re-chunking all documents,
  after which every configured model re-embeds. If comparing chunkers becomes a need, `chunks` gets
  a `chunker` column (the same pattern as models).

## Embed pump

The queue is "chunks without an embedding for model X":

```sql
select c.id, c.text from chunks c
where not exists (
    select 1 from chunk_embeddings e where e.chunk_id = c.id and e.model = $1
)
order by c.ts desc
limit 64
for update of c skip locked;
```

- The pump serves the default model first, then the others. Within a model, newest chunks first.
- Each batch: look up `embedding_cache`, send the misses to that model's TEI, write
  `chunk_embeddings` and `embedding_cache` in one transaction.
- `LISTEN chunks_pending` wakes the pump after an ingest. Several pumps share the queue through
  `SKIP LOCKED`.

## Per-user model choice

- `users.default_embedding_model` (null = the configured default) decides which model the user's
  searches use, in the UI and through API keys.
- `model=` on the search API and a model switch in the search UI override it per query.
- A model can be chosen before its backfill is complete. Settings shows the coverage, so the user
  knows the semantic half is partial.
- The model switch in the search UI only appears once more than one model is configured.

v1 configures a single model, but adding one is a config change plus a TEI Deployment, and
switching to it needs nothing but the Settings dropdown once its backfill is done.

## Evaluation

### Collecting judgments

A ✓ on a hit records an explicit judgment. It carries its own query, so there is no search log.

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

Settings, tab "Models", per configured model:

- **Coverage**: the share of the user's chunks that have an embedding for this model.
- **recall@10** and **MRR** over the user's judgments, each for vector-only and for hybrid search.
  The server replays every judged query against the model and checks where the judged document
  lands. Runs on demand, results are kept until the next run.

Plus the model switch in the search UI, to compare result lists of two models by eye.

### Later

- **Click logging**: record which hit was opened after a search, as implicit judgments. Biased
  towards whatever model was active (you only click what you're shown), and it turns into a log of
  every search.
- **Interleaving** (team-draft): merge the results of two models into one list and count clicks
  per model. The fair way to compare from click data, without position bias.
- Admin view with aggregated metrics across users (counts only, no queries).
