# Deploy and connect Kubernetes

Use the Helm chart for a real deployment, not the development Compose credentials. This walkthrough covers the operational decisions around install, network/storage and verification. The [chart README](../charts/ferret/README.md) owns chart-specific install/secret behavior, and [values.yaml](../charts/ferret/values.yaml) owns the full values reference.

## Prepare infrastructure

You need Helm, kubectl access to your target cluster, Gateway API CRDs/controller and an existing Gateway with an HTTPS listener. Provision DNS and a certificate for your hostname at that Gateway. Ferret's HTTPRoute does not create a Gateway, certificate or DNS record. When the Gateway is in another namespace, its allowedRoutes must admit the Ferret namespace; set the parent reference accordingly.

Choose who operates the database:

- **Bundled ParadeDB (default):** choose a ReadWriteOnce StorageClass for its single-instance
  PVC and establish a database-consistent backup policy with your infrastructure tooling. The
  chart is not an HA Postgres operator. Its database Deployment uses Recreate because two
  Postgres processes must never open one data directory. A multi-replica server does not make
  the database highly available.
- **Your own Postgres:** set `postgres.external` as described in the
  [chart's external Postgres contract](../charts/ferret/README.md#external-postgres), checking its
  [database requirements](../charts/ferret/README.md#database-requirements). CloudNativePG with a
  ParadeDB image is the reference setup; you operate and back up this database separately.

Use a CNI enforcing NetworkPolicy. The chart allows public server port 8080 ingress but restricts private port 8081 to this release's worker pods. This policy is **not** a complete database/frontend/embedding-endpoint isolation policy and restricts no egress, so no chart policy needs opening for an external database. Add your cluster's controls as required. Never put private port 8081 behind the Gateway.

Decide where embeddings come from before installing: every configured model needs an OpenAI-compatible endpoint the server pods can reach (a TEI, a proxy such as LiteLLM, or a cloud API), and the chart refuses to render without one. The chart's optional `bundledTei` deploys one CPU TEI for one model; it hard-codes an `amd64` node selector, so ARM-only clusters run a native ARM TEI themselves and name it as the endpoint. A TEI's first start downloads weights; plan its cache storage and memory as well as database capacity. See [embeddings](embeddings.md).

## Choose secret management

For ordinary `helm install/upgrade`, the chart generates its Secret once and reuses values with live `lookup`. The generated Secret and bundled database PVC are marked keep on uninstall. A same-name reinstall can reuse the old state.

For GitOps/render-only tooling, create a stable Secret separately and set `secrets.existingSecret`. `helm template` cannot look up the live Secret and would otherwise generate different credentials on every render. Use SOPS/Sealed Secrets/your secret manager rather than committing plaintext secrets. The required keys are documented in the [chart secret contract](../charts/ferret/README.md#secrets); `postgres-password` is bundled-only. External database credentials come from `postgres.external.passwordSecret`, with an optional `caSecret` for TLS verification.

For an imperative test install with the bundled database, this creates a separately owned Secret with independent generated values (the shell passes them as local process arguments, so use trusted administration tooling):

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
embedding:
  default: qwen3-embedding-0.6b
  models:
    - id: qwen3-embedding-0.6b
      dimension: 1024
      queryPrefix: "Instruct: Given a search query, retrieve relevant passages from personal notes, documents and chats\nQuery: "
      minSimilarity: 0.42
      embeddingApi:
        url: http://tei-qwen3.ai.svc:80/v1
        model: Qwen/Qwen3-Embedding-0.6B
```

For a first try without an embeddings service of your own, replace the model's `embeddingApi` with `bundledTei: true` and set `bundledTei.enabled: true`.

For an external database, replace the `postgres.persistence` block with the chart's
[`postgres.external` example](../charts/ferret/README.md#external-postgres). Create or sync its
password Secret and optional CA Secret into the Ferret namespace before installing.

From the repository root:

```sh
helm upgrade --install ferret ./charts/ferret -n ferret -f deployment-values.yaml
kubectl -n ferret rollout status deployment/ferret-postgres # bundled database only
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

## Embedding capacity and scaling

Ferret sends a model's work to its endpoints and leaves capacity to them. For a big backfill, put a faster endpoint (a GPU TEI, a laptop's GPU in the LAN) behind the model's `embeddingApi` and keep `queryApi` on an always-on one, or let a proxy such as LiteLLM balance several TEIs under one model name. `concurrency` raises how many requests each server replica sends at once. The URLs must be reachable from the **server pods**, and an endpoint's token comes from an existing Secret (`tokenSecret`). GPU deployment and device scheduling are your responsibility.

Server and worker replicas coordinate using Postgres claims/locks/notifications. Workers each process their claimed runs; extra replicas provide more simultaneous connections, not multiple simultaneous runs of one connection. Keep secrets/config and images aligned across replicas.

## Inspect and recover

```sh
kubectl -n ferret logs deployment/ferret-server --since=10m
kubectl -n ferret logs deployment/ferret-plugins --since=10m
kubectl -n ferret get events --sort-by=.lastTimestamp
```

A Pending database pod usually needs PVC/scheduling investigation. A loaded frontend with failing API calls needs routing/server checks. An unreachable endpoint or one whose answers fail ferret's checks shows its last error in Admin; investigate network, token, model name and dimension. Use [operations](operations.md) for failure procedures, upgrades and backup/restore. Do not delete the kept PVC or Secret to “fix” an install: that crosses a data-loss boundary.

Implementation: [chart templates](../charts/ferret/templates/), [HTTPRoute](../charts/ferret/templates/httproute.yaml), [NetworkPolicy](../charts/ferret/templates/networkpolicy.yaml), [database persistence](../charts/ferret/templates/postgres.yaml), [configuration](configuration.md).
