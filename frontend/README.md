# ferret frontend

Web UI for ferret: Next.js (App Router, static export) with React, Tailwind CSS 4, Lucide icons and
`next-themes`, copied from carapace's frontend. Talks GraphQL to the server through urql.

The circular avatar in the top-right corner shows the first letter of the user's display name
(or username when no display name is set). It opens an account menu grouped into Discover
(Search and Explore) and Manage (Activity, Connections, Settings and, for admins, Admin).
Below the navigation, a row contains the `thiesgerken/ferret` repository link with the version
on its right. The repository link opens in a new tab. The account group at the bottom shows
the signed-in user's name and sign-out action.

The menu footer displays the running server's version. Hover over it to see both frontend and
server versions; a warning appears when they differ. The frontend version is bundled from
`package.json`, and the authenticated GraphQL `version` query reports the server's Cargo package
version. The release pipeline updates both version sources together.

## Development

Compose is the primary environment; see the repository [quickstart](../docs/quickstart.md).
For optional fast native iteration, run `nix develop` at the repository root before the commands
below. It provides Node 24 and the exact pnpm selected by `packageManager`, including native macOS
tooling on Apple Silicon.

```bash
pnpm install --frozen-lockfile
pnpm dev
```

Open [http://localhost:3000](http://localhost:3000). `pnpm dev` proxies `/graphql`, `/api/*` and
`/auth/*` to `FERRET_SERVER_URL`. The default, `http://127.0.0.1:8380`, is the server from the
repository's docker-compose.

- `pnpm lint`, `pnpm test` and `pnpm build` are convenient iteration commands. The authoritative
  sandboxed lint/test/codegen/production-build/type-check definitions live in the root Nix flake.
  Image runtime restrictions are documented in the [chart README](../charts/ferret/README.md#runtime-security-and-images).
- `pnpm codegen` generates typed GraphQL documents into `src/gql/` (ignored by git) from
  `schema.graphql`, the SDL the server prints. It runs automatically before `dev`, `build` and
  `test`. Write queries with `graphql()` from `@/gql`.

## Local fonts

`next/font/local` bundles Latin WOFF2 assets without Google Fonts requests: Geist and Geist Mono
at variable weights 100–900, Alegreya at weight 600. Vendored from
[google/fonts@47d54ca](https://github.com/google/fonts/tree/47d54ca1ebb14fd864695f1d39dc734f39017c33/ofl)
(OFL), notices in `public/fonts`.

## Code style

- Icons from [Lucide](https://lucide.dev/icons) (`lucide-react`).
- Tailwind utility classes, compose them with `cn()` from `@/lib/utils`.
- Components in `src/components/`.
