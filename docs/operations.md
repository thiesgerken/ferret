# Operate Ferret

Operate the index, origins and model-serving infrastructure as separate failure domains. Ferret exposes health and Activity but does not provide a database backup scheduler, HA database operator or universal repair endpoint.

## Runtime images and writable state

The server, worker, frontend and independent WhatsApp archive images are built by Nix from runtime closures without a distribution base or build environment. Releases publish Linux x64 only. All four default to UID/GID 65532. The main Compose/Helm workloads run with read-only roots, dropped capabilities and no privilege escalation; the [independent archive guide](plugins/whatsapp.md) applies the same hardening and documents its storage-owner UID override. The archive is not deployed by that stack/chart. Temporary state is explicitly writable at `/tmp`: Compose supplies a tmpfs, and Helm supplies a per-pod `emptyDir`. Workers clone repositories there; size node/container temporary storage for the repositories they process. It is disposable state, not a backup target.

The worker closure includes Git and CA certificates for outbound HTTPS, and the server includes CA certificates for outbound API/TLS clients. The frontend runs nginx on container port 8080, while the chart's frontend Service remains port 80 and public routes do not change. Do not add writable root access or run as root to fix a missing temporary mount. The upstream database, TEI, proxy and test-mail images retain their own runtime requirements; the Ferret image restrictions do not claim those services are hardened identically.

Compose remains development/demo configuration with public credentials. For x64 pull commands or native ARM Linux build/load steps, use the [quickstart](quickstart.md). Nix integration checks use a separate sandbox-local PostgreSQL/extensions stack and never migrate a developer volume. The Compose ParadeDB tag, PostgreSQL major, volume name and data path are unchanged by the image-build cutover. Do not replace an existing data directory with the test cluster's version or downgrade its extension because the test nixpkgs selects a different version.

## Health and logs

`GET /healthz` on server port 8080 executes `select 1`: 200 means database reachable, 503 means not. It is unauthenticated but need not be exposed by the reverse proxy. It does not test model inference, worker availability or origin credentials. The Helm server liveness/startup probes use TCP; readiness uses this database check, avoiding pointless server restarts during a database outage.

Readiness has its own pool with at most one connection per server replica, separate from the
application pool. Acquisition has a 250 ms deadline; the complete acquire/query check has a
750 ms deadline, below Helm's one-second readiness timeout. A saturated application pool does
not consume this capacity. Database outages, exhausted database-wide connections or a query
that misses its deadline still return 503. Budget one additional database connection per replica;
graceful server shutdown closes the readiness pool.

Homepage document/chunk/source totals use 60-second database snapshots. Embedding progress,
including its chunk denominator, backlog, rate, stored-model statistics and Activity's complete
writer-statistics snapshot use five seconds. Coverage denominators come from the same database
snapshot as their embedded/pending/failed counts, not the homepage snapshot. Snapshots are shared
within a server replica, deduplicate concurrent refreshes of the same scope and retain at most
128 scopes per cache.
Different users have separate scopes; endpoint health remains live. Refresh errors are returned
as errors and shared for five seconds, never converted to zeros or served as stale success.
Cold/expired refreshes still execute real aggregates and can be slow on a CPU-starved database.

Search's informational `documents_in_scope`/`documentsInScope` also uses a shared 60-second cache,
keyed by user and effective source roots after API-key restrictions. Concurrent refreshes share
one load; equivalent root order/duplicates reuse the same entry. This runs a narrow document
count, not the homepage summary. Only that metadata can be stale: search hits, matching totals
and facets remain uncached and fresh. The semantic lower-bound flag uses a bounded existence
check instead of counting every in-scope embedding.

```sh
# Local direct server, not the static frontend proxy route:
curl --fail-with-body http://127.0.0.1:8380/healthz
docker compose logs --since=10m server plugins

# Kubernetes release/namespace named ferret:
kubectl -n ferret logs deployment/ferret-server --since=10m
kubectl -n ferret logs deployment/ferret-plugins --since=10m
```

Logs are structured JSON with an info default; configure `RUST_LOG` or chart `server.logLevel`. Plugin run logs/errors are visible in Activity; the server keeps a 64 Ki-character tail, not an unbounded audit trail. Workers deliberately disable async-imap command tracing to avoid logging passwords. Custom plugin logs must avoid secrets and raw response bodies.

Server database connections disable ParadeDB's planner diagnostics (`paradedb.planner_warnings = off`): ordinary joins and embedding backlog counts legitimately use PostgreSQL plans rather than ParadeDB's optimized scans. This does not disable custom scans or change query planning; other database warnings and errors retain their log levels. To investigate missed ParadeDB optimizations, use a separate SQL session with `SET paradedb.planner_warnings = warning` and inspect the query with `EXPLAIN`.

Activity distinguishes a **run** from an API-key **burst**. A burst groups a key's nearby writes and appears running while its last write was within ten minutes, then succeeded. It does not carry a worker lease/failure state. A run has claimed/running/succeeded/failed/lost behavior and an origin cursor. Admin all-user Activity requires an admin session.

Activity also shows the signed-in user's current content and **Estimated storage** by writer,
including Unknown writer. Its rows and percentages remain user-scoped even when an admin switches
the ingest feed to all users. Expand writer names for paths and storage figures for the
document/chunk/embedding/index and per-model breakdown. This is not a deletion-reclaim forecast:
see [storage attribution and exclusions](indexing.md#writer-statistics-and-estimated-storage).

## Failed or stuck connections

1. Read the Activity error/log and check whether the run failed or is still running. Due with no run suggests worker/internal-API/auth/capability failure; an origin authentication error suggests the plugin's credentials/address.
2. Check worker logs and registration. A worker must reach internal port 8081 with the shared PSK and public port 8080 for ingestion. Changing the public browser hostname does not automatically change these internal addresses.
3. Fix the cause in the connection or infrastructure, then use **Run now** on an enabled connection. The failed run's old successful cursor remains, and earlier writes may already be present. Puts are designed to be idempotent.
4. For a genuinely lost worker, the two-minute heartbeat lease prevents continued token writes. Claims mark expired running runs lost. Do not manually finish arbitrary run rows or delete state to hide an error.

Disabling a connection prevents new claims; it does not immediately cancel an in-flight worker. Deleting it revokes the run by removing the connection/runs and also deletes every document below its root. If credentials cannot decrypt after restoring with a missing/wrong config key, the claim path records a failed run and advances to the next scheduled slot without handing work to the worker. It does not disable the connection for that failure. Correct the key or re-enter the credentials, then use **Run now** or wait for the schedule; re-enable only if an operator disabled it. A schedule with no future slot can disable a connection independently of credential decryption.

Removing a remembered exclusion requires no live run and resets state for a full next run. That is a supported restore path for excluded documents, not a general-purpose reset button. See [connections](connections.md) and each [plugin guide](plugins/README.md).

## Search and embedding failures

- No hits: try keyword mode with a known ingested phrase, clear time/author/source filters, inspect exclusions and the latest plugin run. Empty-text documents have no chunks. Thread collapse can hide additional matching messages; the viewer navigates the thread.
- Hybrid warns `semantic_unavailable`: keyword results remain usable; check the model's query endpoint in Admin (reachable, last error), its network path, token and latency. Semantic-only returns 503 rather than pretending to search by meaning.
- Pending backlog does not move: check the model's embedding endpoint in Admin. A failing endpoint is retried with a growing pause (up to a minute), a rate-limited one after its `Retry-After`. Answers that fail ferret's checks (dimension, vector count, non-finite or zero vectors) are discarded and show as the last error.
- All chunks embedded but uncached queries fail: queries may go to a different endpoint (`query_api`) than chunks. Inspect its status in Admin instead of re-ingesting everything.
- Rejected chunks: inspect the endpoint's actual error and model/input configuration. Rejected per-chunk/model inputs are excluded from retries; there is no public mass-retry endpoint. Text/title changes create/invalidate eligible work, but do not edit user text merely to suppress an infrastructure error.

Use [embeddings](embeddings.md) for model-identity/backfill rules. Do not change weights/prefixes under an existing model id or delete embedding tables as a casual repair. To remove the stored embeddings of a model that is no longer configured, use the delete in Admin's [stored embeddings](embeddings.md#stored-embeddings-and-the-model-registry) list rather than deleting rows. Endpoint caches/weights are reconstructible; document/user/connection state is not equivalently disposable.

## Upgrades and migrations

Read release notes and back up database/configuration/secrets before upgrading. Pin compatible server, worker and frontend versions and review chart value changes. Current server startup applies embedded sqlx migrations before listening; applied checksums are validated and concurrent migration runners are serialized. Never edit an applied migration file. A failed/partial migration or checksum mismatch is a startup failure to investigate, not a reason to remove migration records.

The writer-statistics migration performs a one-time backfill over existing document, chunk and
embedding rows, and adds stored numeric size estimates plus write-time triggers. Allow for its
row updates, index maintenance, temporary disk/WAL growth and migration locks when scheduling an
upgrade of a large index. Subsequent Activity polls read numeric sizes rather than sizing full
content again. No manual backfill command or re-ingestion is needed.

The statistics-index migration builds `chunks(user_id)` and
`chunk_embeddings(user_id, model)` during startup. Normal transactional index creation can block
writes and adds disk/WAL work; allow an upgrade window for a large corpus. It requires no
re-ingestion or manual backfill. These indexes help selective user counts, not broad all-user
backlog scans; short-lived shared snapshots reduce how often those scans execute.

```sh
helm upgrade ferret ./charts/ferret -n ferret -f deployment-values.yaml
kubectl -n ferret rollout status deployment/ferret-server
kubectl -n ferret logs deployment/ferret-server --since=10m
```

An application image rollback does not reverse database schema changes. Plan rollback against the actual migration compatibility or restore a tested pre-upgrade backup. Postgres major upgrades require a compatible data migration/restore strategy; changing its image tag does not migrate an existing data directory. Verify database extension versions as well as Postgres itself.

Secret changes need distinct procedures: session-secret invalidates sessions/run signatures; worker-secret must match server/workers; changing config-key without re-encryption makes stored credentials unreadable; changing the Postgres Secret does not change the initialized database password. There is no automatic cross-secret rotation workflow.

## Backups and restore responsibilities

Back up:

- The full Postgres database, including users, sessions, keys, connection credentials/cursors/runs, documents/chunks/vectors/cache, exclusions and judgments. Search results are not an export or backup.
- Deployment values/server configuration and exact application/database image versions.
- The required runtime secrets through your secret manager, particularly **config-key in a separately protected backup**. A database dump without it cannot recover stored origin credentials.

Use your infrastructure's database-consistent backups/PITR or snapshots with proper Postgres coordination. A raw copy of a mounted live data directory is not a safe backup. The chart creates no backup schedule and documents storage snapshots as a cluster responsibility. With `postgres.external`, backups, retention and recovery belong to the external database operator, not the Ferret chart.

For a logical backup on a local disposable Compose project, use the matching database container tools and a protected backup path:

```sh
umask 077
docker compose exec -T paradedb pg_dump -U ferret -d ferret -Fc -f /tmp/ferret.dump
docker compose cp paradedb:/tmp/ferret.dump ./ferret.dump
```

Copy the dump into protected off-host storage, then remove local/container copies when your backup policy permits. Do not commit it. This is an example transport for pg_dump, not an application-managed backup service. Production operators should use their approved storage/encryption/retention tooling.

Restore into an isolated empty target first, using matching Postgres/ParadeDB/pgvector versions and tools. Create a fresh database from `template0`, not the default template: ParadeDB's default template can already contain its schemas/extensions and collide with objects in the dump. Before starting the isolated server/workers:

```sh
docker compose cp ./ferret.dump paradedb:/tmp/ferret.dump
docker compose exec -T paradedb createdb -U ferret --template=template0 ferret_restore
docker compose exec -T paradedb pg_restore -U ferret -d ferret_restore \
  --no-owner --exit-on-error /tmp/ferret.dump
```

Point the isolated server's `FERRET_DATABASE_URL` at `ferret_restore` (or set the deployment's database name accordingly). Never restore over an initialized live Ferret database. Use a compatible ParadeDB image and verify restored indexes/extensions with its supported PostgreSQL restore procedure; logical dumps containing extension/index state are version-sensitive. Restore the matching config key and other secrets before permitting workers to claim runs. First validate startup/migrations and a known keyword search, then connection credential decryption, a controlled run and model/query health. Only cut over after proving the restore. A previously lost secret may require reissuing origin credentials/API keys or signing sessions out, not a fabricated fallback.

### Moving from the bundled database to an external Postgres

Use `pg_dump`/`pg_restore` and the restore procedure above; setting `postgres.external` does not
copy data. Prepare the target with the [required extensions and connection settings](../charts/ferret/README.md#database-requirements).
Stop the server and plugin workers before the final dump so no writes are lost at cutover.
Restore to an empty target and validate it in isolation as above, keeping the same application
secrets, especially `config-key`. Then set `postgres.external.url` to the restored database and
`passwordSecret` (plus `caSecret` if needed) to its credentials, and upgrade the release. The chart
removes the bundled Deployment and Service but retains its PVC; keep that PVC and the backup
until the cutover has been verified. Resume the desired server/worker replicas only after the
target is ready. The external database operator owns backups from then on.

## Data-loss boundaries

Deleting a connection or committing an intentionally empty sync deletes indexed documents under its root. Deleting a PVC loses the database, not just embeddings. Helm uninstall keeps generated Secret/PVC, but explicit deletion or external GitOps pruning can bypass that retention. Losing config-key loses decryptability of stored origin credentials. Origin deletions/permission loss can propagate on a successful plugin sweep.

Ferret is a discovery copy, but reconstructing everything from origins does not necessarily recover user accounts/keys/exclusions/judgments or deleted origin content. Document deletion cascades live derived rows but not shared embedding cache or backups; apply retention/erasure policies to those separately.

Implementation: [health routing](../crates/ferret-server/src/lib.rs), [Activity](../crates/ferret-server/src/activity.rs), [leases](../crates/ferret-server/src/runs.rs), [migrations](../crates/ferret-server/migrations/), [secret encryption](../crates/ferret-server/src/credentials.rs), [chart retention](../charts/ferret/README.md).
