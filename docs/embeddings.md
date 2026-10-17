# Embedding models and the TEI pool

Embeddings supply the meaning half of search. Ingestion and keyword search do not wait for the document backlog. A server runs one pump loop per configured TEI; the TEI only serves inference.

## Configure models and serving endpoints

The server YAML declares model identity, expected dimension, query/document prefixes, minimum cosine similarity and TEI URLs. The development example is [docker/ferret.yaml](../docker/ferret.yaml). Its default is `qwen3-embedding-0.6b`, model id `Qwen/Qwen3-Embedding-0.6B`, dimension 1024 and cutoff 0.42. These are configuration values, not built-in defaults that appear without a file.

TEIs are not statically assigned a model in Ferret config. Discovery polls `/info`, matches its model id to a configured model, and checks suitability. Start TEI without a custom `--served-model-name`: the returned model identity must remain checkable. Its input budget must cover the title/chunk input; the current check compares max input tokens against the maximum character count conservatively. Every returned vector is checked against the configured dimension/model identity.

To add serving capacity for an existing model, add the worker-reachable TEI URL to YAML, restart the server and verify its discovered state in Admin/Activity. An external URL must be routable from the server, not only your browser. Protect the network path: embedding text is sent to that endpoint. TEI URLs carry no Ferret-side bearer credential configuration.

## Query and embedding eligibility

Admin controls three persisted switches per URL: enabled, queries and embedding. New in-deployment TEIs default to all on. New `external: true` endpoints default to enabled/embedding on and queries off so search does not depend on a sleeping laptop. Changing the `external` flag does not overwrite switches already persisted for that URL. Removed URLs are removed from the pool table at startup.

Switch changes apply immediately on the answering process and are picked up by others within the ten-second discovery poll. Disabling embedding stops new claims for that TEI; an already-running batch may finish. Query eligibility and embedding eligibility are separate: a model can be fully embedded yet currently unable to answer uncached queries, or query-capable with a partial document corpus.

Discovery distinguishes pending, offline, unused (unconfigured model id), unsuitable and serving. A serving endpoint still needs its switches enabled for the intended use. `/info` is the Compose readiness probe because `/health` can queue behind inference work.

## Backfill and cache

Adding a configured model creates its partial HNSW index and makes existing chunks pending for that model automatically. The pump claims recent pending chunks using advisory locks, so concurrent replicas skip claimed work and document ingestion does not wait for TEI. Notifications wake it; idle polling recovers missed notifications.

Document input is document prefix plus at most 200 title characters, two newlines, and chunk text (or just prefix/text without a title). Queries use the query prefix and parsed positive query words. Input/model hashes allow shared cache reuse across users. A model's vectors stay separate from other models' vectors.

Treat a configured model name as a stable vector-space identity. Do not repoint an existing name at different weights or prefixes and assume old `chunk_embeddings` will be regenerated. New weights/preprocessing need a new model name and backfill. Startup rejects an existing cached dimension inconsistent with config, but equal dimension does not establish semantic compatibility. Changing the cutoff changes retrieval filtering, not vectors.

Users can select a default in Settings; it applies to their UI and API-key searches. Requests can explicitly choose another configured model. The configured default is used without a personal choice; startup clears personal defaults for removed models. Selection is allowed before coverage completes, so semantic results can be partial. Removing a model from config does not constitute a documented cache-erasure operation.

## Failures and progress

A query embedding shares a five-second inference deadline across attempted eligible TEIs, idle ones first. Cached queries can avoid a TEI call. Hybrid explicitly falls back to keywords on embedding failure; semantic-only fails. Database errors are not hidden by that fallback.

Pump batches adapt up to 32 chunks, bounded also by TEI's advertised client batch limit. Shared query/embedding TEIs target short batches; embedding-only TEIs can use the longer batch budget (60 seconds). Transport/service failures leave work pending for later attempts. Individual TEI-refused inputs are recorded in `embedding_failures` and skipped so one bad input does not stall every other chunk. Changing text creates new chunk work; changing a title invalidates the retained chunk's embeddings/failures. There is no public “retry every rejected chunk” endpoint.

Activity/Settings show embedded, pending and rejected chunks, rate, model query/embedding availability and estimated completion. Admin can see all-user model backlogs and TEI errors/switches. ETA is a recent-throughput estimate, absent without progress, not an SLA. A completed plugin run does not mean backfill is done.

Current functionality does not include a finished model-evaluation dashboard merely because relevance judgments or conception documents discuss evaluation. Use [search mechanics](search.md), [indexing invariants](indexing.md) and the actual configuration/source when planning a model change.

Implementation: [embedding lifecycle](../crates/ferret-server/src/embeddings.rs), [TEI discovery and switches](../crates/ferret-server/src/tei_pool.rs), [HTTP client](../crates/ferret-server/src/tei.rs), [configuration validation](../crates/ferret-server/src/config.rs).
