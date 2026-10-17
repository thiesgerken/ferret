# ferret Helm chart

Deploys [ferret](https://github.com/thiesgerken/ferret): server, plugin workers, frontend, ParadeDB
and one CPU [TEI](https://github.com/huggingface/text-embeddings-inference) per embedding model
(`embedding.models`). TEIs outside the chart, e.g. a laptop's GPU for a backfill, join through
`externalTeis`.

## Prerequisites

- [Gateway API](https://gateway-api.sigs.k8s.io/) CRDs and a Gateway to attach the `HTTPRoute` to
- A ReadWriteOnce StorageClass for the database. Backups are the cluster's job: pick a class with
  CSI snapshots (e.g. for Velero), the chart takes none itself.
- A CNI that enforces NetworkPolicies (Cilium, Calico, …). With `networkPolicy.enabled` (the
  default) the chart restricts the server's internal port to the plugin workers. Without
  enforcement, the worker secret is all that guards it.

## Install

```bash
helm install ferret ./charts/ferret --namespace ferret --create-namespace \
  --set ingress.hostname=ferret.example.com
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

## TEIs

ferret embeds with a pool of TEIs. Each works for whichever configured model it serves: the server
asks every TEI's `/info` for its model every 10 seconds and matches the model's `modelId`, and it
checks every answer for the model and dimension, so a TEI may switch models at any time.

- `tei.internal.enabled` (default): one CPU TEI per model in the cluster. It answers queries and
  embeds, in short batches so a query never waits long behind one.
- `externalTeis`: URLs of TEIs outside the chart, e.g. a laptop with a GPU in the same LAN for a
  big backfill (the first import, a new model), or a GPU TEI you deploy separately. They start
  with queries off, so searches don't depend on a laptop that may go to sleep.

```yaml
externalTeis:
  - http://192.168.1.20:8080
```

Admin switches each TEI on and off, for queries and for the embed pump, without a restart. Every
TEI that may embed pumps on its own: a fast GPU simply takes more batches than the CPU TEI. A TEI
that is down or serves a model no configured one uses shows as such in Admin and is skipped.
Remove an external TEI from the list once it's gone for good.

### ⚠️ From chart versions with `tei.gpu` or `bulkUrl`

Every model needs its `dimension` (1024 for Qwen3-Embedding-0.6B). `tei.gpu.*` and the models'
`bulkUrl` are gone, and the chart refuses to render while they're set. Deploy a GPU TEI yourself (TEI's image for your GPU generation, e.g. `89-1.9.4`, with the
model's `--model-id`) and list its Service URL in `externalTeis`. A `bulkUrl` becomes an entry of
`externalTeis` as it is. Unlike `bulk_url`, the CPU TEI keeps embedding next to it unless Admin
switches its embedding off.

## Uninstall

`helm uninstall` keeps the database PVC and the Secret, so a reinstall under the same release name
finds the index and its password again. To really remove everything:

```bash
helm uninstall ferret -n ferret
kubectl delete -n ferret pvc/ferret-postgres secret/ferret-secrets
```

See [values.yaml](values.yaml) for everything else.
