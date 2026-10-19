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

```bash
pnpm install
pnpm dev
```

Open [http://localhost:3000](http://localhost:3000). `pnpm dev` proxies `/graphql`, `/api/*` and
`/auth/*` to `FERRET_SERVER_URL`. The default, `http://127.0.0.1:8380`, is the server from the
repository's docker-compose.

- `pnpm lint`, `pnpm test` (`node --test`, React components render into jsdom), `pnpm build`
  (static export into `out/`, served by nginx in the Docker image).
- `pnpm codegen` generates typed GraphQL documents into `src/gql/` (ignored by git) from
  `schema.graphql`, the SDL the server prints. It runs automatically before `dev`, `build` and
  `test`. Write queries with `graphql()` from `@/gql`.

## Code style

- Icons from [Lucide](https://lucide.dev/icons) (`lucide-react`).
- Tailwind utility classes, compose them with `cn()` from `@/lib/utils`.
- Components in `src/components/`.
