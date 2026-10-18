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
`bundledTei` deploys one such TEI for one model.

**A self-hosted proxy.** [LiteLLM](https://docs.litellm.ai/docs/embedding/supported_embedding) in
front of several TEIs (a CPU TEI in the cluster, a laptop's GPU for a backfill) balances and fails
over between them under one model name. Ferret only sees the proxy: `url:
http://litellm:4000/v1`, `model` as the proxy names it, and the proxy's key in `token_env`.

**A cloud API.** Any OpenAI-compatible embeddings API works the same way, e.g. OpenAI's
`https://api.openai.com/v1` with `model: text-embedding-3-small`, `dimension: 1536` and the API
key in `token_env`. A 429 pauses the pump for `Retry-After`, and `concurrency` keeps the request
rate within the API's limits.

### What the endpoint must guarantee

- **A model name always means the same weights.** Ferret checks every answer's shape, not which
  model computed it: vectors from other weights would land in this model's index unnoticed and
  rank nonsense. Swapping the model behind a name needs a new `id` in ferret.
- **A cloud endpoint receives every chunk and every search query.** Restrict what reaches it with
  the sources you ingest, or keep sensitive sources on a model with a self-hosted endpoint.

## Requests and answers

Every answer is checked, and a failed check fails the whole request: nothing of it is stored.

- exactly one vector per input, with `index` covering `0..n-1` (answers may come out of order)
- every vector of the configured `dimension`
- every value finite, and no vector all zeros

The answer's `model` field isn't checked: proxies and cloud APIs name models their own way.

- **400, 413 or 422:** the endpoint refused inputs (too long, untokenizable). The pump finds the
  culprits one by one, records them in `embedding_failures` and skips them from then on, so one
  bad input does not stall every other chunk. Changing a document's text or title gives its
  chunks another chance.
- **429:** the model's pump waits for `Retry-After` (30 seconds without one). The batch size stays.
- **Unreachable, timed out or any other error:** a normal state. The pump tries again after a
  pause that doubles from 1 second up to a minute, logs the problem once per outage, and Admin
  shows it. Other models are not held up.

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

Startup rejects a cached dimension inconsistent with config, but an equal dimension does not
establish semantic compatibility. Changing the cutoff changes retrieval filtering, not vectors.

Users can select a default in Settings; it applies to their UI and API-key searches. Requests can
explicitly choose another configured model. The configured default is used without a personal
choice; startup clears personal defaults for removed models. Selection is allowed before coverage
completes, so semantic results can be partial.

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
