# Quickstart

This walkthrough uses the development Compose stack and disposable demo data. Do not expose it to the internet: its admin password, worker secret, session secret, database password, and configuration encryption key are public development values. Use the [Helm chart](../charts/ferret/README.md) for deployment.

## Prerequisites

Install Git, Docker with Compose v2.24.4 or newer, Bash, curl, and jq. Run commands at the repository root. Compose is the primary development/demo environment. Published Ferret images support Linux x64 only; x64 users do not need Nix. The supporting services use ordinary upstream images. Allow disk space, memory, registry/Hugging Face network access, and time for the first model download. A healthy database alone does not mean the embedding model is ready.

### x64: pull published images

```sh
docker compose pull
docker compose up -d
```

Set `FERRET_IMAGE_TAG` to a release version or PR tag to select matching server, worker and frontend images. These images contain runtime closures, not compilers or package managers.

### Apple Silicon: build native ARM Linux images

Install [Determinate Nix](https://docs.determinate.systems/) and sign up for a free FlakeHub account. `determinate-nixd auth login` authorizes the Native Linux Builder; `determinate-nixd status` reports the login. This managed ARM Linux build VM is separate from Docker Desktop's ARM Linux runtime VM. Do not enable x86 emulation or select `x86_64-linux` outputs on this path.

```sh
determinate-nixd status
nix build --out-link result-server-image .#packages.aarch64-linux.server-image
nix build --out-link result-plugins-image .#packages.aarch64-linux.plugins-image
nix build --out-link result-frontend-image .#packages.aarch64-linux.frontend-image
docker load --input result-server-image
docker load --input result-plugins-image
docker load --input result-frontend-image
export COMPOSE_FILE=docker-compose.yml:docker-compose.arm.yml
docker compose up -d
```

The ARM overlay selects the local `:local` Ferret images with `pull_policy: never` and explicitly selects `linux/arm64` for every service, including native TEI. A missing local image fails instead of pulling an x64 replacement. Keep `COMPOSE_FILE` set for subsequent commands on this page. See [development](development.md) for builder filesystem constraints and checks.

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

In the search UI enter `Brocken`. Turn **Semantic** off while leaving **Keyword** on to make the first result independent of model loading. Filter to source `demo/chat` if needed. A hit contains **“And a Brocken hike on Saturday if the weather holds?”**, authored by Anna. The demo groups this message with other messages in the `harz-trip` thread. Search collapses matching messages into the best hit; open its title and use the viewer's previous/next thread links to read neighboring messages.

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
