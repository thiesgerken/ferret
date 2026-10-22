# Quickstart

This walkthrough uses the development Compose stack and disposable demo data. Do not expose it to the internet: its admin password, worker secret, session secret, database password, and configuration encryption key are public development values. Use the [Helm chart](../charts/ferret/README.md) for deployment.

## Prerequisites

- Install Git, Docker with Compose v2.24.4 or newer, Bash, curl, and jq. Run commands from the repository root.
- Published images support Linux x64 only; x64 users do not need Nix.
- The first start downloads model weights. Allow disk space, memory, registry/Hugging Face network access, and time.
- A healthy database does not mean the embedding model is ready.

### x64: pull published images

```sh
docker compose pull
```

`latest` is the last release and may not match the checkout's configuration; set `FERRET_IMAGE_TAG` to the matching release/PR tag, or use [local images](development.md#compose-development).

### Apple Silicon: build native images

Install [Determinate Nix](https://docs.determinate.systems/) and authorize its Native Linux Builder with a free FlakeHub account:

```sh
determinate-nixd auth login
nix build .#packages.aarch64-linux.{server,plugins,frontend}-image --out-link result-image
for image in result-image*; do docker load -i "$image"; done
export COMPOSE_FILE=docker-compose.yml:docker-compose.local.yml:docker-compose.arm.yml
```

Keep `COMPOSE_FILE` set for subsequent commands. The overlays select local images without pulling replacements and native ARM supporting services.

`tei` computes Qwen/Qwen3-Embedding-0.6B. A second model, bge-m3, is configured too, but its TEI (`tei-bge`) starts only with `docker compose up -d --scale tei-bge=1 tei-bge`: until then Admin shows its endpoint as unreachable.

## Start and sign in

Check that ports 3380, 8380 and 5439 are free. If you already run Ferret, use a separate checkout, Compose project, volumes and port overrides instead of seeding your existing index.

```sh
docker compose up -d
docker compose ps
docker compose logs -f server tei
```

Open <http://127.0.0.1:3380>. Sign in as `admin` with `ferret-development`. On an empty database the server bootstraps this admin; changing the environment password does not reset an existing account. The proxy routes browser requests to the frontend and server. Port 8380 is the direct server API, not the web app.

## Seed and find a known document

```sh
scripts/demo-seed.sh
```

The script signs in, creates a temporary ingestion key for `demo`, puts every JSON document in `docker/demo`, revokes the key, and creates a git connection named **ferret docs** at source `ferret`. Re-running updates the sample documents and skips an existing connection at that source. Expected per-file outcomes are `created` on a fresh index, and `unchanged` when nothing changed.

In the search UI enter `Brocken`. Turn **Semantic** off while leaving **Keyword** on to make the first result independent of model loading. Filter to source `demo/chat` if needed. A hit contains **“And a Brocken hike on Saturday if the weather holds?”**, authored by Anna. The demo groups this message with other messages in the `harz-trip` thread. Search collapses matching messages into the best hit; open its title and use the thread outline (beside the message on a wide screen, above it on a narrow one) to read the others. For a thread that branches, open any mail in `demo/mail` (search `offsite`): its outline shows who answered whom.

Click the hit title (the external id when no title exists) to open Ferret's document viewer at the matching passage. This sample has no `url`, so the viewer is its origin. Ferret stores searchable text, not the original application's attachments or access controls.

## Inspect a real git connection

Open **Connections**. The seed script has already created a real public-git connection using:

```json
{
  "remote": "https://github.com/thiesgerken/ferret.git",
  "include": ["README.md", "docs/**"],
  "url_template": "https://github.com/thiesgerken/ferret/blob/{commit}/{path}"
}
```

Its schedule is `0 * * * *`; creation makes the first run due immediately. If creating it manually instead, select plugin `git`, choose a name and icon, use a new source root such as `my-ferret`, and enter these fields in the generated form. Do not create two connections at the same or overlapping roots.

Open **Activity**, select this connection's run, and inspect status, log and put/unchanged/deleted counts. A successful run clones a commit and reports the full sync. If it stays due, check `docker compose logs plugins`: a worker must register `git` and claim the run. If cloning fails, check worker network access and the run error. **Run now** schedules another attempt; it is not a browser-side clone.

After the run succeeds, search `discovery` in keyword mode and filter to source `ferret`. Find the repository README and use **Open in origin** (the external-link icon next to the title) to open GitHub at the indexed commit. The title opens Ferret's local viewer. Confirm the origin URL contains `/blob/<commit>/README.md`, not a fabricated local path. The [git guide](plugins/git.md) explains private repositories and custom origins.

## Next steps

- [Connections](connections.md) and the individual plugin guides explain how to connect your own data with minimal origin credentials.
- [HTTP ingestion](ingestion.md) provides a complete write and reconciliation example.
- [MCP](api/mcp.md) connects an agent with a source-scoped search key.
- [Kubernetes deployment](kubernetes.md) and the [Helm chart](../charts/ferret/README.md) replace the insecure local configuration.
- [Development](development.md) covers host tooling and tests.

Stop this development project with `docker compose stop`. Its named volumes retain the database and model cache. Removing the database volume deletes your index and users; never run volume-removal commands against a project you intend to keep.

Implementation: [Compose](../docker-compose.yml), [seed script](../scripts/demo-seed.sh), [sample message](../docker/demo/chat-harz-4.json), [proxy routing](../docker/traefik-dynamic.yml).
