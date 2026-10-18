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

Treat a configured model name as a stable vector-space identity. Do not repoint an existing name at different weights or prefixes and assume old `chunk_embeddings` will be regenerated. New weights/preprocessing need a new model name and backfill. Startup rejects a dimension that contradicts the [model registry](#stored-embeddings-and-the-model-registry), but equal dimension does not establish semantic compatibility. Changing the cutoff changes retrieval filtering, not vectors.

Users can select a default in Settings; it applies to their UI and API-key searches. Requests can explicitly choose another configured model. The configured default is used without a personal choice; startup clears personal defaults for removed models. Selection is allowed before coverage completes, so semantic results can be partial. Removing a model from config keeps its stored embeddings for reuse if it returns; deleting them is a separate Admin action (below).

## Stored embeddings and the model registry

`embedding_models` records every model that ever embedded, configured or not, with its dimension and when it was first seen. Its migration seeded it from existing chunk vectors and cached query vectors; startup adds configured models it does not know yet. `chunk_embeddings`, `embedding_cache` and `embedding_failures` reference it, so a model's rows exist only while it is registered.

Startup refuses to start when:

- a configured model is registered with another dimension. Its HNSW index is built for one dimension, and vectors of two cannot share it. To change the dimension under the same name, remove the model from config, delete its embeddings in Admin and add it again; otherwise choose a new name.
- a configured model's index name equals another registered model's (names that differ only by `.`, `-` or `_`, e.g. `a.b` and `a-b`). Delete the other model's embeddings first or choose another name.

Admin → Embedding → Stored embeddings lists every registered model with its chunk embeddings across all users, the exact size of its HNSW index and an estimated size of its rows (count × vector size: the table is shared, so this is not exact). Models that are no longer configured are marked as such.

Only those can be deleted. Deleting a configured model is rejected: remove it from YAML and restart first, so no pump embeds for it and no personal default points at it (startup resets those). After a confirmation that names the model and its embedding count, one transaction removes its chunk embeddings, cached document and query vectors, failure records, HNSW index and registry entry. Documents, chunks, other models and user settings stay. Dropping the index blocks searches and pump batches briefly at the end of the transaction. If the model returns later, it is registered anew and backfilled from scratch. With several server replicas, delete only after every replica runs without the model: one still configured for it fails its writes on the foreign key until it is rolled.

## Failures and progress

A query embedding shares a five-second inference deadline across attempted eligible TEIs, idle ones first. Cached queries can avoid a TEI call. Hybrid explicitly falls back to keywords on embedding failure; semantic-only fails. Database errors are not hidden by that fallback.

Pump batches adapt up to 32 chunks, bounded also by TEI's advertised client batch limit. Shared query/embedding TEIs target short batches; embedding-only TEIs can use the longer batch budget (60 seconds). Transport/service failures leave work pending for later attempts. Individual TEI-refused inputs are recorded in `embedding_failures` and skipped so one bad input does not stall every other chunk. Changing text creates new chunk work; changing a title invalidates the retained chunk's embeddings/failures. There is no public “retry every rejected chunk” endpoint.

Activity/Settings show embedded, pending and rejected chunks, rate, model query/embedding availability and estimated completion. Admin can see all-user model backlogs and TEI errors/switches. ETA is a recent-throughput estimate, absent without progress, not an SLA. A completed plugin run does not mean backfill is done.

Current functionality does not include a finished model-evaluation dashboard merely because relevance judgments or conception documents discuss evaluation. Use [search mechanics](search.md), [indexing invariants](indexing.md) and the actual configuration/source when planning a model change.

Implementation: [embedding lifecycle](../crates/ferret-server/src/embeddings.rs), [TEI discovery and switches](../crates/ferret-server/src/tei_pool.rs), [HTTP client](../crates/ferret-server/src/tei.rs), [configuration validation](../crates/ferret-server/src/config.rs).
