# Deploy and connect Kubernetes

Use the Helm chart for a real deployment, not the development Compose credentials. This walkthrough covers the operational decisions around install, network/storage and verification. The [chart README](../charts/ferret/README.md) owns chart-specific install/secret behavior, and [values.yaml](../charts/ferret/values.yaml) owns the full values reference.

## Prepare infrastructure

You need Helm, kubectl access to your target cluster, Gateway API CRDs/controller and an existing Gateway with an HTTPS listener. Provision DNS and a certificate for your hostname at that Gateway. Ferret's HTTPRoute does not create a Gateway, certificate or DNS record. When the Gateway is in another namespace, its allowedRoutes must admit the Ferret namespace; set the parent reference accordingly.

Choose a ReadWriteOnce StorageClass for the single-instance ParadeDB PVC and establish a database-consistent backup policy with your infrastructure tooling. The chart is not an HA Postgres operator. Its database Deployment uses Recreate because two Postgres processes must never open one data directory. A multi-replica server does not make the database highly available.

Use a CNI enforcing NetworkPolicy. The chart allows public server port 8080 ingress but restricts private port 8081 to this release's worker pods. This policy is **not** a complete database/frontend/TEI/egress isolation policy; add your cluster's controls as required. Never put private port 8081 behind the Gateway.

Check node architecture and capacity for TEI. The internal chart Deployment hard-codes an `amd64` node selector, so changing only its image tag does not support ARM-only clusters. On ARM-only infrastructure, disable internal TEIs and deploy native ARM TEIs separately, then list them in `externalTeis` and enable query use in Admin. First start downloads weights; plan model cache ephemeral storage/memory as well as database capacity. See [embeddings](embeddings.md).

## Choose secret management

For ordinary `helm install/upgrade`, the chart generates its Secret once and reuses values with live `lookup`. Both the generated Secret and database PVC are marked keep on uninstall. A same-name reinstall can reuse the old state.

For GitOps/render-only tooling, create a stable Secret separately and set `secrets.existingSecret`. `helm template` cannot look up the live Secret and would otherwise generate different credentials on every render. Use SOPS/Sealed Secrets/your secret manager rather than committing plaintext secrets. The exact five required keys are documented in the [chart secret contract](../charts/ferret/README.md#secrets).

For an imperative test install, this creates a separately owned Secret with independent generated values (the shell passes them as local process arguments, so use trusted administration tooling):

```sh
kubectl create namespace ferret
kubectl -n ferret create secret generic ferret-runtime \
  --from-literal=postgres-password="$(openssl rand -hex 32)" \
  --from-literal=bootstrap-password="$(openssl rand -hex 16)" \
  --from-literal=session-secret="$(openssl rand -hex 32)" \
  --from-literal=worker-secret="$(openssl rand -hex 32)" \
  --from-literal=config-key="$(openssl rand -base64 32)"
```

Back up config-key independently of the database. Changing a Kubernetes Secret does not rotate the initialized database password or re-encrypt stored connection credentials.

## Install a concrete release

Create a private deployment-values file, replacing hostname, Gateway and StorageClass with your infrastructure's names:

```yaml
secrets:
  existingSecret: ferret-runtime
ingress:
  hostname: ferret.example.com
  parentRefs:
    - name: public-gateway
      namespace: gateway-system
      sectionName: https
postgres:
  persistence:
    storageClassName: your-rwo-storage-class
```

From the repository root:

```sh
helm upgrade --install ferret ./charts/ferret -n ferret -f deployment-values.yaml
kubectl -n ferret rollout status deployment/ferret-postgres
kubectl -n ferret rollout status deployment/ferret-server
kubectl -n ferret rollout status deployment/ferret-plugins
kubectl -n ferret rollout status deployment/ferret-frontend
kubectl -n ferret get pods,pvc,httproute
kubectl -n ferret describe httproute ferret
```

Check the route's Accepted/ResolvedRefs conditions as well as pod readiness. The current route sends `/graphql`, `/mcp`, `/api`, `/auth` to the server and everything else to the static frontend. Verify `/mcp` against the [agent guide](api/mcp.md), not a legacy SSE URL.

Read the bootstrap password without storing it in a repository:

```sh
kubectl -n ferret get secret ferret-runtime \
  -o jsonpath='{.data.bootstrap-password}' | base64 -d
```

Open your HTTPS hostname and sign in as admin. Create a limited ordinary user if appropriate and a first [connection](connections.md). Inspect Activity and search a known origin title. A ready server confirms database reachability, not model download or a successful origin sync.

## External TEI and scaling

Keep default internal TEIs for query availability and add backfill capacity:

```yaml
externalTeis:
  - http://192.168.1.20:8080
```

The URL must be reachable from the **server pods**. External entries start with queries off. Toggle query/embedding use in Admin after verifying the served model and dimension. If you set `tei.internal.enabled: false`, external endpoints must cover all configured models, and you must explicitly enable query use for them in Admin. Otherwise uncached semantic queries are unavailable even if embeddings exist. GPU TEI deployment/device scheduling is your responsibility; there is no current `tei.gpu`/per-model bulkUrl setting.

Server and worker replicas coordinate using Postgres claims/locks/notifications. Workers each process their claimed runs; extra replicas provide more simultaneous connections, not multiple simultaneous runs of one connection. Keep secrets/config and images aligned across replicas.

## Inspect and recover

```sh
kubectl -n ferret logs deployment/ferret-server --since=10m
kubectl -n ferret logs deployment/ferret-plugins --since=10m
kubectl -n ferret get events --sort-by=.lastTimestamp
```

A Pending database pod usually needs PVC/scheduling investigation. A loaded frontend with failing API calls needs routing/server checks. TEI offline/unsuitable states need model/network/input-budget investigation. Use [operations](operations.md) for failure procedures, upgrades and backup/restore. Do not delete the kept PVC or Secret to “fix” an install: that crosses a data-loss boundary.

Implementation: [chart templates](../charts/ferret/templates/), [HTTPRoute](../charts/ferret/templates/httproute.yaml), [NetworkPolicy](../charts/ferret/templates/networkpolicy.yaml), [database persistence](../charts/ferret/templates/postgres.yaml), [configuration](configuration.md).
