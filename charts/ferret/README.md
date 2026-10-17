# ferret Helm chart

Deploys [ferret](https://github.com/thiesgerken/ferret): server, plugin workers, frontend, ParadeDB
and one CPU [TEI](https://github.com/huggingface/text-embeddings-inference) per embedding model
(`embedding.models`), optionally a GPU TEI per model for backfills.

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

## GPU backfills

A big backfill (the first import, a new model) is slow on the CPU TEI. With `tei.gpu.enabled`, each
model gets a second TEI on a GPU node, and the embed pump uses only that one (the model's
`bulk_url`). Queries stay on the CPU TEI.

```yaml
tei:
  gpu:
    enabled: true
    image:
      tag: "89-1.9.4"  # required: the image for your GPU generation, see values.yaml
    nodeSelector:
      nvidia.com/gpu.present: "true"
```

The GPU TEI tolerates the usual `nvidia.com/gpu:NoSchedule` taint by default (`tei.gpu.tolerations`).

The pump doesn't fall back to the CPU: while the GPU node is missing, the GPU TEI stays Pending,
Admin shows the pump's error, and the backlog grows. Disable it again once the backfill is done.
A Deployment that never becomes ready also makes `helm upgrade --wait` and GitOps health checks
(Argo CD, Flux) time out, so enable it only together with the GPU node.

A TEI outside the cluster can take the GPU TEI's place, e.g. on a laptop with a GPU in the same LAN:
set the model's `bulkUrl` (not together with `tei.gpu.enabled`). It must serve the same model, and
the pump uses only it, so remove it once the backfill is done.

```yaml
embedding:
  models:
    - name: qwen3-embedding-0.6b
      # ...
      bulkUrl: http://192.168.1.20:8080
```

## Uninstall

`helm uninstall` keeps the database PVC and the Secret, so a reinstall under the same release name
finds the index and its password again. To really remove everything:

```bash
helm uninstall ferret -n ferret
kubectl delete -n ferret pvc/ferret-postgres secret/ferret-secrets
```

See [values.yaml](values.yaml) for everything else.
