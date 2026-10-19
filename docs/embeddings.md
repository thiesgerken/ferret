# Embedding models and their endpoints

Embeddings supply the meaning half of search. Ingestion and keyword search do not wait for the
document backlog. Each configured model names the OpenAI-compatible endpoint that computes its
embeddings: a TEI, a proxy such as LiteLLM, or a cloud API. Routing, failover and serving are
that endpoint's job. Ferret measures its own work: what's left and how fast it gets done.

## Configure a model

The server YAML declares each model's identity, expected dimension, query/document prefixes,
minimum cosine similarity and endpoints. The development example is
[docker/ferret.yaml](../docker/ferret.yaml).

```yaml
embedding:
  default: qwen3-embedding-0.6b
  models:
    - id: qwen3-embedding-0.6b          # stored with every embedding
      dimension: 1024
      query_prefix: "Instruct: ...\nQuery: "
      document_prefix: ""
      min_similarity: 0.42
      embedding_api:                    # embeds chunks
        url: http://litellm:4000/v1
        model: qwen3-embed
        token_env: FERRET_LITELLM_TOKEN # optional
        concurrency: 4                  # optional, default 1, per server replica
      query_api:                        # embeds search queries
        url: http://tei:80/v1
        model: Qwen/Qwen3-Embedding-0.6B
```

- `id` is stored with every embedding and names the model's index. Treat it as the vector space's
  identity: new weights or prefixes need a new id, which embeds everything again.
- A model needs `embedding_api`, `query_api` or both. When one is missing, the other serves
  both. Separate ones let queries go to a fast, close endpoint while a backfill runs elsewhere.
- `url` goes up to the version: requests go to `{url}/embeddings`. `model` is what the endpoint
  calls the model.
- `token_env` names an environment variable whose value goes out as `Authorization: Bearer`. The
  server refuses to start when it's unset or empty. The Helm chart fills it from an existing
  Secret (`tokenSecret`).
- `concurrency` is how many embedding requests one server replica runs at once for the model.
  It exists only on the endpoint that embeds chunks.

## Setups

Every endpoint gets the same request: `POST {url}/embeddings` with `model`, `input` and
`encoding_format: float`. There is no mode switch.

**A TEI directly.** [text-embeddings-inference](https://github.com/huggingface/text-embeddings-inference)
serves the OpenAI route at `/v1/embeddings`, so `url: http://tei:80/v1` with the Hugging Face
model id as `model`. Its `--max-batch-tokens` caps the input length, which must stay at 2202
tokens or more (200 title characters, two newlines and a chunk of up to 2000 characters): TEI
truncates longer inputs by default, which would embed only a chunk's beginning. The chart's
`bundledTei` deploys one such TEI for one model. TEI runs nearly every embedding model on Hugging
Face, which makes it the default, but not the fastest choice for every model.

**llama.cpp directly.** For a model published as a GGUF (Qwen3-Embedding ships official ones),
[llama.cpp](https://github.com/ggml-org/llama.cpp)'s `llama-server` serves the same
`/v1/embeddings` route and can be much faster. Measured for Qwen3-Embedding-0.6B with f16 weights
on both, with batches of 32 chunks:

| Hardware | TEI 1.9.4 | llama.cpp b11723 |
|---|---|---|
| Apple M5 Max (Metal) | ~1,000 tokens/s | ~3,600 to 4,100 tokens/s |
| 5-core x86 node (CPU) | 78 tokens/s, 269 ms per query | 224 tokens/s, 156 ms per query |

On the x86 node llama.cpp also needed 1 to 2 GiB of memory against TEI's 2.6 GiB. The model
decides the server flags, e.g. for Qwen3-Embedding:

```sh
llama-server -m Qwen3-Embedding-0.6B-f16.gguf --embedding --pooling last \
  --ctx-size 8192 --parallel 2 --batch-size 4096 --ubatch-size 4096 --host 0.0.0.0 --port 8080
```

- `--pooling` must match the model: `last` for Qwen3-Embedding, often `cls` or `mean` for others.
- An embedding input must fit into one micro-batch, so `--ubatch-size` and each slot's context
  (`--ctx-size` / `--parallel`) need at least 2202 tokens, like TEI's `--max-batch-tokens`.
- Every slot reserves KV cache for its whole context, about 0.1 MiB per token for
  Qwen3-Embedding-0.6B. A large `--ctx-size` copied from a workstation gets a small container
  OOM-killed.

**A self-hosted proxy.** [LiteLLM](https://docs.litellm.ai/docs/embedding/supported_embedding) in
front of several servers (a CPU server in the cluster, a laptop's GPU for a backfill) balances and
fails over between them under one model name. Ferret only sees the proxy: `url:
http://litellm:4000/v1`, `model` as the proxy names it, and the proxy's key in `token_env`.

**A cloud API.** Any OpenAI-compatible embeddings API works the same way, e.g. OpenAI's
`https://api.openai.com/v1` with `model: text-embedding-3-small`, `dimension: 1536` and the API
key in `token_env`. A 429 pauses the pump for `Retry-After`, and `concurrency` keeps the request
rate within the API's limits.

### What the endpoint must guarantee

- **A model name always means the same weights.** Ferret checks every answer's shape, not which
  model computed it: vectors from other weights would land in this model's index unnoticed and
  rank nonsense. Swapping the model behind a name needs a new `id` in ferret.
- **Different servers behind one name agree.** The same weights in another server (TEI and
  llama.cpp, CPU and GPU, f32 and f16) give close but not bit-identical vectors, and a quantized
  GGUF (e.g. Q8_0) is not the same weights. Before mixing servers for one model, embed the same
  inputs through both: real chunks and queries with the model's prefix should reach a cosine
  similarity of at least 0.9999 each, and rank a handful of chunks the same. TEI and llama.cpp
  with the f16 weights of Qwen3-Embedding-0.6B reach 0.99997.
- **A cloud endpoint receives every chunk and every search query.** Every model embeds every
  chunk of every source, so a cloud endpoint for any model sees all of them, mail and chats
  included. When a source must not leave, every model needs a self-hosted endpoint.

## Requests and answers

Every answer is checked, and a failed check fails the whole request: nothing of it is stored.

- exactly one vector per input, with `index` covering `0..n-1` (answers may come out of order)
- every vector of the configured `dimension`
- every value finite, and no vector all zeros

The answer's `model` field isn't checked: proxies and cloud APIs name models their own way.

- **400, 413 or 422:** the endpoint refused inputs (too long, untokenizable). The pump finds the
  culprits one by one, records them in `embedding_failures` and skips them from then on, so one
  bad input does not stall every other chunk. Changing a document's text or title gives its
  chunks another chance. When every input of a batch of several is refused, the endpoint is at
  fault (a proxy answers a mistyped model name with a 400), and the batch fails like below
  instead.
- **429:** every pump loop of the model waits for `Retry-After` (30 seconds without one). The
  batch size stays.
- **Unreachable, timed out or any other error:** a normal state. The pump tries again after a
  pause that doubles from 1 second up to a minute, logs the problem when its kind changes, and
  Admin shows it. Other models are not held up.

An endpoint failure rolls back the batch's claim transaction before the pump retries, releasing
its advisory locks. Pending chunks can be claimed again without another ingestion notification;
retries still honor `Retry-After` or the normal error backoff.

## Backfill and cache

Adding a configured model creates its partial HNSW index and makes existing chunks pending for
that model automatically. Each model has `concurrency` pump loops per server replica. They claim
recent pending chunks with advisory locks, so replicas and loops skip each other's work and
document ingestion never waits for an endpoint. Notifications wake them; idle polling recovers
missed notifications.

A batch takes up to 32 chunks and adapts to how long the endpoint needs: short batches (2 seconds)
when one endpoint embeds chunks and queries, so a query never waits long behind one, otherwise up
to the 60-second batch timeout. A timeout halves the batch size.

Document input is document prefix plus at most 200 title characters, two newlines, and chunk text
(or just prefix/text without a title). Queries use the query prefix and parsed positive query
words. Input/model hashes allow shared cache reuse across users. A model's vectors stay separate
from other models' vectors.

Startup rejects a dimension that contradicts the
[model registry](#stored-embeddings-and-the-model-registry), but an equal dimension does not
establish semantic compatibility. Changing the cutoff changes retrieval filtering, not vectors.

Users can select a default in Settings; it applies to their UI and API-key searches. Requests can
explicitly choose another configured model. The configured default is used without a personal
choice; startup clears personal defaults for removed models. Selection is allowed before coverage
completes, so semantic results can be partial. Removing a model from config keeps its stored
embeddings for reuse if it returns; deleting them is a separate Admin action (below).

## Stored embeddings and the model registry

`embedding_models` records every model that ever embedded, configured or not, with its dimension
and when it was first seen. Its migration seeded it from existing chunk vectors and cached query
vectors; startup adds configured models it does not know yet. `chunk_embeddings`,
`embedding_cache` and `embedding_failures` reference it, so a model's rows exist only while it is
registered.

Startup refuses to start when:

- a configured model is registered with another dimension. Its HNSW index is built for one
  dimension, and vectors of two cannot share it. To change the dimension under the same `id`,
  remove the model from config, delete its embeddings in Admin and add it again; otherwise choose
  a new `id`.
- a configured model's index name equals another registered model's (ids that differ only by `.`,
  `-` or `_`, e.g. `a.b` and `a-b`). Delete the other model's embeddings first or choose another
  `id`.

Admin → Embedding → Stored embeddings lists every registered model with its chunk embeddings
across all users, the exact size of its HNSW index and an estimated size of its rows (count ×
vector size: the table is shared, so this is not exact). Models that are no longer configured are
marked as such.

Stored counts and sizes share a five-second snapshot on each server replica with Admin's model
progress. The confirmation's displayed embedding count is a snapshot, not a deletion limit:
the mutation checks the live registry/configuration and removes all of that model's data in its
transaction. A successful deletion invalidates stored-model and writer-statistics snapshots on
the answering replica; other replicas expire theirs within five seconds.

Only those can be deleted. Deleting a configured model is rejected: remove it from YAML and restart
first, so no pump embeds for it and no personal default points at it (startup resets those). After
a confirmation that names the model and its embedding count, one transaction removes its chunk
embeddings, cached document and query vectors, failure records, HNSW index and registry entry.
Documents, chunks, other models and user settings stay. Dropping the index blocks searches and
pump batches briefly at the end of the transaction. If the model returns later, it is registered
anew and backfilled from scratch. With several server replicas, delete only after every replica
runs without the model: one still configured for it fails its writes on the foreign key until it
is rolled.

## Failures and progress

A search waits up to five seconds for its query embedding. Cached queries need no request.
Hybrid falls back to keywords when the query endpoint fails; semantic-only fails. Database errors
are not hidden by that fallback.

Activity and Settings show embedded, pending and rejected chunks, rate, whether the model's
endpoints answered their last request, and estimated completion. Admin shows every model's
backlog per source and its endpoints: whether each answered at all, and its last error with the
time. Both come from the answering server process's own requests. ETA is a recent-throughput
estimate, absent without progress, not an SLA. A completed plugin run does not mean backfill is
done.

Database progress, per-source backlog and rate snapshots are shared for five seconds per server
replica; user progress is scoped by user and model. Concurrent requests for the same scope share
one refresh. Each user's embedded/pending/failed counts partition one SQL snapshot; their sum is
the coverage denominator, without a separate chunk-count query. Admin's stored counts and
source backlogs share one repeatable-read snapshot, and their sum provides its denominator.
Thus coverage totals refresh with progress after five seconds, while homepage totals remain
independently cached for 60 seconds. Activity's complete writer-statistics snapshot is also
shared for five seconds, including its global allocation denominators. Endpoint reachability and
last errors are read live, not frozen in these database snapshots. Failed refreshes return errors
(shared for five seconds), not zero counts or stale success. These are cached exact aggregates,
not write-time running counters.

**Test** on a model in Admin embeds a short text through each of its endpoints, with the document
prefix for chunks and the query prefix for queries, checks the answer like any other and shows the
dimension and duration, or the error. A test stores nothing and leaves the endpoint's status alone,
so it also works while the pump backs off.

## ⚠️ From versions with a TEI pool

Earlier versions managed a pool of TEIs (`embedding.teis`, switches in Admin, `externalTeis` and
`tei.*` in the chart). The server refuses that config, and the chart those values.

1. Rename each model's `name` to `id`. **Keep the value**: embeddings are stored under it, so the
   same id reuses every embedding, and a new one embeds everything again.
2. Drop `model_id`, and give each model an `embedding_api` (and a `query_api` if queries should
   go elsewhere) with `url: http://<tei>/v1` and the Hugging Face model id as `model`.
3. Drop `embedding.teis`. An in-cluster TEI becomes the model's endpoint. Several TEIs for one
   model (a laptop's GPU next to the cluster's CPU) go behind a proxy such as LiteLLM, or the
   laptop becomes `embedding_api` and the cluster's TEI `query_api`.
4. In the chart, `tei.internal.enabled` becomes `bundledTei` for one model, or a TEI you deploy
   yourself. See the [chart README](../charts/ferret/README.md#-from-chart-versions-with-tei-or-externalteis).

The `teis` table and its switches go with the upgrade.

Implementation: [embedding lifecycle](../crates/ferret-server/src/embeddings.rs),
[endpoint client and answer checks](../crates/ferret-server/src/embedding_api.rs),
[configuration validation](../crates/ferret-server/src/config.rs).
