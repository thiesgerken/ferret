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
  The static export is served by non-root nginx on container port 8080 with a read-only root.
- `pnpm codegen` generates typed GraphQL documents into `src/gql/` (ignored by git) from
  `schema.graphql`, the SDL the server prints. It runs automatically before `dev`, `build` and
  `test`. Write queries with `graphql()` from `@/gql`.

## Local fonts

`next/font/local` bundles the checked Latin WOFF2 assets in `src/fonts`; builds and browsers do
not fetch Google Fonts. Geist and Geist Mono retain variable weights 100–900, and the Alegreya
wordmark retains weight 600. The existing CSS variables remain unchanged. Each font's upstream
SIL Open Font License is retained beside its asset, from
[google/fonts at 47d54ca](https://github.com/google/fonts/tree/47d54ca1ebb14fd864695f1d39dc734f39017c33/ofl).

| Asset | Exact upstream resource | SHA-256 |
| --- | --- | --- |
| `alegreya-latin-600.woff2` | [Alegreya v41, Latin 600](https://fonts.gstatic.com/s/alegreya/v41/4UacrEBBsBhlBjvfkQjt71kZfyBzPgNGKBI4-6qjkySFrt19.woff2) | `3409a7b2dd3743c9ac371f04cc5ae01e77e2ea3b834c539deee108410f8bd3d8` |
| `geist-latin-variable.woff2` | [Geist v5, Latin variable](https://fonts.gstatic.com/s/geist/v5/gyByhwUxId8gMEwcGFWNOITd.woff2) | `9b6f5ff45b278c744b5f379a2c4ecbaf858a842b8eaf82ac8d21b699ca16c608` |
| `geist-mono-latin-variable.woff2` | [Geist Mono v6, Latin variable](https://fonts.gstatic.com/s/geistmono/v6/or3nQ6H-1_WfwkMZI_qYFrcdmhHkjko.woff2) | `5f3d6ad60f29d6cb708414ec6887163d63bf197377ef5417d2483ff31ace6c3b` |

## Code style

- Icons from [Lucide](https://lucide.dev/icons) (`lucide-react`).
- Tailwind utility classes, compose them with `cn()` from `@/lib/utils`.
- Components in `src/components/`.
