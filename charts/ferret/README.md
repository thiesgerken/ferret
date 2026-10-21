# ferret Helm chart

Deploys [ferret](https://github.com/thiesgerken/ferret): server, plugin workers, frontend and
ParadeDB. Embeddings come from an OpenAI-compatible endpoint per model that you name (a
[TEI](https://github.com/huggingface/text-embeddings-inference), a proxy like LiteLLM, or a cloud
API). For a quickstart, the chart can bring one CPU TEI along (`bundledTei`).

For the infrastructure walkthrough, see [Kubernetes](../../docs/kubernetes.md). This page and
[values.yaml](values.yaml) own chart-specific installation/settings; [operations](../../docs/operations.md)
owns health, troubleshooting, upgrades and backup/restore procedures.

## Prerequisites

- [Gateway API](https://gateway-api.sigs.k8s.io/) CRDs and a Gateway to attach the `HTTPRoute` to
- A ReadWriteOnce StorageClass for the database. Backups are the cluster's job: pick a class with
  CSI snapshots (e.g. for Velero), the chart takes none itself.
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
| `postgres-password` | the database user |
| `bootstrap-password` | the first `admin`, at least 16 characters |
| `session-secret` | signing session cookies, at least 32 bytes; changing it signs everyone out |
| `worker-secret` | the plugin workers' key for the server's internal API, at least 32 bytes |
| `config-key` | encrypts plugin credentials, 32 bytes in base64 (`openssl rand -base64 32`); back it up apart from the database |

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

`helm uninstall` keeps the database PVC and the Secret, so a reinstall under the same release name
finds the index and its password again. To really remove everything:

```bash
helm uninstall ferret -n ferret
kubectl delete -n ferret pvc/ferret-postgres secret/ferret-secrets
```

See [values.yaml](values.yaml) for everything else.
