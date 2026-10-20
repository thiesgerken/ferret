# Local embedding maps

Use [Apple Embedding Atlas](https://apple.github.io/embedding-atlas/) to browse a snapshot of
one Ferret user's documents, colored by source, with source/type/author filters and text previews.
This is a standalone operator tool, not an Explore feature or a deployed service.

`atlas.py` has three commands:

- `export`: read Postgres through `FERRET_DATABASE_URL`, project locally, save a snapshot.
- `export-kubernetes`: do the same through a temporary localhost-only `kubectl port-forward`.
- `serve`: reopen a snapshot without database access or recalculating the projection.

## Prerequisites

Run from the repository root on macOS or Linux. Install [uv](https://docs.astral.sh/uv/).
`uv run` installs Python 3.12 and isolated dependencies from the script's inline metadata,
including Embedding Atlas 0.26.0. The first run downloads a substantial Python environment,
including Atlas's machine-learning dependencies; subsequent runs reuse it. No embedding model
weights or new embeddings are needed.

The database needs Ferret's `users`, `documents`, `chunks`, `chunk_embeddings` and
`embedding_models` tables and pgvector. Use direct database credentials, not a Ferret API key:
the HTTP APIs do not export stored embeddings. Select one existing username and model ID.
At least four documents must have all their chunks embedded in that model.

## Export from the local Compose database

```sh
FERRET_DATABASE_URL=postgres://ferret:ferret@127.0.0.1:5439/ferret \
  uv run scripts/embedding-atlas/atlas.py export \
    --user admin --model qwen3-embedding-0.6b \
    --output .atlas-prototype/local

uv run scripts/embedding-atlas/atlas.py serve .atlas-prototype/local
```

Open <http://127.0.0.1:5055>. Stop the viewer with Ctrl+C. Reopening the same snapshot does not
contact Postgres. To refresh, export to a **new** directory and serve that directory. The tool
refuses to overwrite an existing directory, including an incomplete previous export.

For a different Postgres installation, set `FERRET_DATABASE_URL` through your normal secret
handling rather than putting passwords in shell history. Libpq connection strings and URLs are
both accepted. Prefer a database role limited to the required reads when available.

## Export from Kubernetes

This convenience command matches Ferret's Helm-managed Postgres deployment: one Postgres
container, port 5432, with `POSTGRES_USER`, `POSTGRES_DB` and `POSTGRES_PASSWORD` environment
variables. It requires an authenticated `kubectl` and permission to get the deployment/pods,
port-forward to the database pod, and exec in it. It does not create credentials or change RBAC.

Replace these example context, namespace, deployment and username values with your deployment's:

```sh
uv run scripts/embedding-atlas/atlas.py export-kubernetes \
  --context my-cluster --namespace ferret --deployment ferret-postgres \
  --user alice --model qwen3-embedding-0.6b \
  --output .atlas-prototype/cluster

uv run scripts/embedding-atlas/atlas.py serve .atlas-prototype/cluster --port 5056
```

Open <http://127.0.0.1:5056>. The helper chooses a free local database port, captures the pod's
credentials in process memory without printing or saving them, and reads only the selected
user/model. The database connection and tunnel close **before** local projection begins, also
on export failure. It does not change your current Kubernetes context. For an external database,
multiple-container pod, `_FILE` credentials or a nonstandard deployment, use `export` with your
own connection/tunnel instead.

## Snapshot files

Each export writes a new directory with owner-only permissions:

| File | Contents |
| --- | --- |
| `documents.parquet` | Full document text, metadata, document IDs, model ID and 2D coordinates |
| `state.json` | Atlas's default layout, source coloring and filters, user/model/coverage heading |
| `snapshot.json` | Username, model, completion time, included/omitted counts and per-source counts |

High-dimensional vectors and database credentials are not saved. Keep exports under the
repository's git-ignored `.atlas-prototype/` directory. If you choose another location, ensure
it is not committed, synced or backed up somewhere inappropriate. An export failure can leave
an incomplete directory; inspect and remove that directory before reusing its name.

The viewer runs DuckDB queries in the **local Atlas process**, not on Ferret's database, so the
browser does not need to load the entire snapshot. It still receives whatever text/metadata it
browses. Filters keep the existing coordinates; selection does not rerun UMAP.

## Data access and limitations

- **Private, unauthenticated local service.** Atlas binds only to `127.0.0.1`; CORS and MCP are
  not enabled. Do not expose it through a public bind address, reverse proxy or shared tunnel.
  Anyone with access to this local service can read the exported user's data. It does not enforce
  Ferret sessions or source-scoped API keys.
- **Read-only does not mean zero production load.** Export uses one repeatable-read, read-only
  transaction with a 60-second timeout per SQL statement. Aggregating the selected user's
  embeddings consumes database CPU/I/O. There are no database writes, migrations, embedding
  endpoint calls or deployment changes. UMAP runs locally after the transaction closes.
- **One dot per fully embedded document.** The vector is the normalized mean of normalized chunk
  vectors from one model. Empty documents and documents with missing chunk embeddings are omitted;
  coverage is reported. Failed/pending embeddings are not fabricated or silently substituted.
- **A map, not a similarity metric.** Mixed-topic documents can be blurred by averaging. UMAP uses
  cosine distance, up to 15 neighbors, and seed 42. Distant island spacing and automatic labels
  are exploratory, not trustworthy topic classifications. Adding/removing documents can rearrange
  the map even with a fixed seed.
- **An explicit snapshot.** Updates, deletions and permission changes in Ferret do not update an
  existing export. Refresh explicitly and delete old copies according to your retention needs.
  The entire selected corpus is loaded into local memory for projection; there is no sampling
  or streaming mode. Large corpora may require substantial RAM and projection time.
- **Local copies beyond Parquet.** Atlas can cache derived data under the operating system's
  user cache directory for `embedding_atlas`, and browsers may cache data. Stop the viewer before
  removing a snapshot; remove Atlas/browser caches too when erasing local copies. Atlas's own
  export actions can create additional private files.

Neither `serve` nor browser reloads refresh a snapshot. Keep any current viewer running while
exporting a new snapshot if you want to compare layouts.
