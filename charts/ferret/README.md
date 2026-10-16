# ferret Helm chart

Deploys [ferret](https://github.com/thiesgerken/ferret): server, frontend and ParadeDB.

## Prerequisites

- [Gateway API](https://gateway-api.sigs.k8s.io/) CRDs and a Gateway to attach the `HTTPRoute` to
- A ReadWriteOnce StorageClass for the database. Backups are the cluster's job: pick a class with
  CSI snapshots (e.g. for Velero), the chart takes none itself.

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

The chart generates the Postgres password, the bootstrap password and the session secret once and
keeps them across upgrades (`lookup`). Tools that render with `helm template` (Argo CD, Flux) can't
look anything up and would generate new values on every sync. There, create a Secret yourself and
set `secrets.existingSecret`:

| key | used for |
|---|---|
| `postgres-password` | the database user |
| `bootstrap-password` | the first `admin`, at least 16 characters |
| `session-secret` | signing session cookies; changing it signs everyone out |

## Uninstall

`helm uninstall` keeps the database PVC and the Secret, so a reinstall under the same release name
finds the index and its password again. To really remove everything:

```bash
helm uninstall ferret -n ferret
kubectl delete -n ferret pvc/ferret-postgres secret/ferret-secrets
```

See [values.yaml](values.yaml) for everything else.
