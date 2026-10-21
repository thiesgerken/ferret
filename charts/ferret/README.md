# ferret Helm chart

Deploys [ferret](https://github.com/thiesgerken/ferret): server, plugin workers, frontend and,
by default, a bundled ParadeDB. You can use an [external Postgres](#external-postgres) instead.
Embeddings come from an OpenAI-compatible endpoint per model that you name (a
[TEI](https://github.com/huggingface/text-embeddings-inference), a proxy like LiteLLM, or a cloud
API). For a quickstart, the chart can bring one CPU TEI along (`bundledTei`).

For the infrastructure walkthrough, see [Kubernetes](../../docs/kubernetes.md). This page and
[values.yaml](values.yaml) own chart-specific installation/settings; [operations](../../docs/operations.md)
owns health, troubleshooting, upgrades and backup/restore procedures.

## Prerequisites

- [Gateway API](https://gateway-api.sigs.k8s.io/) CRDs and a Gateway to attach the `HTTPRoute` to
- For the bundled database: a ReadWriteOnce StorageClass. Backups are the cluster's job: pick a
  class with CSI snapshots (e.g. for Velero), the chart takes none itself.
- A CNI that enforces NetworkPolicies (Cilium, Calico, …). With `networkPolicy.enabled` (the
  default) the chart restricts the server's internal port to the plugin workers. Without
  enforcement, the worker secret is all that guards it.

## Install

Every model in `embedding.models` needs an endpoint (see [Embedding endpoints](#embedding-endpoints)),
the chart refuses to render without one. The quickest start is the bundled TEI:

```bash
helm install ferret ./charts/ferret --namespace ferret --create-namespace \
  --set ingress.hostname=ferret.example.com \
  --set bundledTei.enabled=true --set 'embedding.models[0].bundledTei=true'
```

Then sign in as `admin` with the generated bootstrap password:

```bash
kubectl get secret -n ferret ferret-secrets -o jsonpath='{.data.bootstrap-password}' | base64 -d
```

## Secrets

The chart generates every secret below once and keeps them across upgrades (`lookup`). Tools that render with `helm template` (Argo CD, Flux) can't
look anything up and would generate new values on every sync. There, create a Secret yourself and
set `secrets.existingSecret`:

| key | used for |
|---|---|
| `postgres-password` | the bundled database user only; not required with `postgres.external` |
| `bootstrap-password` | the first `admin`, at least 16 characters |
| `session-secret` | signing session cookies, at least 32 bytes; changing it signs everyone out |
| `worker-secret` | the plugin workers' key for the server's internal API, at least 32 bytes |
| `config-key` | encrypts plugin credentials, 32 bytes in base64 (`openssl rand -base64 32`); back it up apart from the database |

## External Postgres

Set `postgres.external` to use a database you operate instead of the bundled ParadeDB:

```yaml
postgres:
  external:
    url: postgres://ferret@db.example.com:5432/ferret?sslmode=verify-full
    passwordSecret: {name: ferret-db-app, key: password}
    caSecret: {name: ferret-db-ca, key: ca.crt} # optional
```

The presence of `external` disables the bundled PVC, Deployment and Service. All other
`postgres.*` values are ignored; omit `external` to keep the bundled database. An empty
`external: {}` is an error, not a switch back to the bundled database.

- `url` is required and becomes `FERRET_DATABASE_URL`. Do not put a password in its user info
  or query parameters: the chart refuses it to keep passwords out of the Deployment.
- `passwordSecret.name` and `key` are required. The referenced Secret must exist in the release
  namespace; its password is supplied through `PGPASSWORD`, not embedded in the URL.
- `caSecret` is optional. When set, both `name` and `key` are required, and the referenced Secret
  must exist in the release namespace. The chart mounts that key read-only as
  `/etc/ferret-postgres/ca.crt` and sets `PGSSLROOTCERT` to that file. Use `sslmode=verify-full`
  to verify the server certificate and hostname. Omit `caSecret` for a publicly trusted CA.
- `secrets.existingSecret` still supplies the application secrets in the table above, but needs
  no `postgres-password` in external mode. A newly generated application Secret omits that key.
  An existing bundled password is retained across cutover so the kept PVC remains usable for
  rollback.

### Database requirements

The database must provide ParadeDB **`pg_search` compatible with the chart's pinned 0.26.x
release** (currently 0.26.2), and **pgvector** (`vector`). Plain AWS RDS and Google Cloud SQL do
not offer `pg_search` and cannot serve as Ferret's database.

Use a database owned by the Ferret user. If that user is not a superuser, an administrator must
create `pg_search` and `vector` in that database before the server starts. Migrations use
`CREATE EXTENSION IF NOT EXISTS`, so they skip extensions already installed.

The reference setup is **CloudNativePG with a ParadeDB image**. See
[ParadeDB's CloudNativePG chart](https://paradedb.github.io/charts/) for the operator/image setup.
Provision the extensions with a CloudNativePG
[`Database` resource's `spec.extensions`](https://cloudnative-pg.io/docs/1.28/declarative_database_management/#managing-extensions-in-a-database)
(`pg_search` and `vector`, each with `ensure: present`), or through
[`spec.bootstrap.initdb.postInitApplicationSQL`](https://cloudnative-pg.io/docs/1.28/bootstrap/#executing-queries-after-initialization)
with `CREATE EXTENSION IF NOT EXISTS pg_search;` and `CREATE EXTENSION IF NOT EXISTS vector;`.
These run with administrative privileges, so the application user need not be a superuser.

Connect directly or through **session pooling**, not a transaction-mode pooler such as
PgBouncer in transaction mode. The server uses `LISTEN/NOTIFY`, advisory locks and the startup
option `paradedb.planner_warnings`; the connection must preserve session state and accept that
option.

The chart's NetworkPolicy restricts no egress, so no chart policy needs opening for the database.
Backups and migration from the bundled database are covered in
[operations](../../docs/operations.md#backups-and-restore-responsibilities).

## Embedding endpoints

Each model names the endpoint that embeds its chunks (`embeddingApi`) and the one that embeds
search queries (`queryApi`). Either one serves both when the other is missing. ferret sends
`POST {url}/embeddings` with `model` and `input` to every kind of endpoint, checks every answer
(one vector per input, the configured `dimension`, finite values, not all zero) and leaves
routing, failover and serving to the endpoint.

```yaml
embedding:
  default: qwen3-embedding-0.6b
  models:
    - id: qwen3-embedding-0.6b
      dimension: 1024
      queryPrefix: "Instruct: Given a search query, retrieve relevant passages from personal notes, documents and chats\nQuery: "
      minSimilarity: 0.42
      embeddingApi:
        url: http://litellm.ai.svc:4000/v1
        model: qwen3-embed
        tokenSecret: {name: litellm, key: token}
        concurrency: 4
      queryApi:
        url: http://tei-qwen3.ai.svc:80/v1
        model: Qwen/Qwen3-Embedding-0.6B
```

- `tokenSecret` names a key of an existing Secret. The chart hands it to the server as an
  environment variable, which goes out as `Authorization: Bearer`.
- `concurrency` (default 1) is how many embedding requests one server replica runs at once for
  the model. Only the endpoint that embeds chunks takes it.
- An endpoint that is down is a normal state: the pump tries again later, searches with that
  model answer by keyword, and Admin shows the last error. Other models aren't held up.
- A model name at the endpoint must always mean the same weights: ferret can't tell when they
  change, and vectors of another model would end up in this one's index.
- A cloud endpoint receives every chunk and every search query.

More setups (TEI, llama.cpp, LiteLLM, a cloud API) in [embeddings.md](../../docs/embeddings.md).

### The bundled TEI

`bundledTei.enabled` deploys one CPU TEI (`<release>-tei`) for exactly one model, which sets
`bundledTei: true` instead of endpoints. The chart fills in its URL and model name. `modelId`,
`maxBatchTokens`, `cacheSize`, `resources` and `image` configure it. The chart refuses to render
when no model or more than one sets the flag, or when a model sets it with the TEI off.

### ⚠️ From chart versions with `tei.*` or `externalTeis`

ferret no longer manages a TEI pool. The chart refuses to render while `tei`, `externalTeis` or a
model's `name`, `modelId`, `maxBatchTokens`, `cacheSize`, `resources` or `bulkUrl` is set.

- `name` becomes `id`. **Keep the value**: embeddings are stored under it, and a new id
  re-embeds everything.
- The per-model TEI settings move to `bundledTei` (one model), or to a TEI you deploy yourself
  and name in `embeddingApi`.
- An external TEI (a laptop's GPU) becomes the model's `embeddingApi` with `url:
  http://192.168.1.20:8080/v1`, and the in-cluster TEI its `queryApi`. Routing between several
  TEIs for one model is a proxy's job (LiteLLM).

## Read-only worker mounts

`plugins.volumes` accepts Kubernetes volume definitions (for example NFS exports or existing PVCs).
`plugins.volumeMounts` accepts `{name, mountPath}` entries and always renders `readOnly: true`.
Mounts apply to every plugin replica, never the server, frontend or database. Worker pods have no
service-account token and need no privileged mounting capability. Use the mounted directory itself
as the `files` connection root, not a subdirectory or an unmounted placeholder.

See the [files plugin setup](../../docs/plugins/files.md#mount-the-origin-first) for generic NFS/k3s
values, mount-availability checks, origin links and failure/recovery behavior. Deploy the required
volumes consistently before enabling connections; a worker missing a mount fails without sweeping.

## Uninstall

`helm uninstall` keeps the bundled database PVC and the generated Secret, so a reinstall under
the same release name finds the index and its password again. To really remove those resources
after uninstalling a bundled-database release:

```bash
helm uninstall ferret -n ferret
kubectl delete -n ferret pvc/ferret-postgres secret/ferret-secrets
```

An external database and its separately managed Secrets are not owned or deleted by this chart.

See [values.yaml](values.yaml) for everything else.
