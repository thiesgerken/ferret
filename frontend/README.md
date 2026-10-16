# ferret frontend

Web UI for ferret: Next.js (App Router, static export) with React, Tailwind CSS 4, Lucide icons and
`next-themes`, copied from carapace's frontend. Talks GraphQL to the server through urql.

## Development

```bash
pnpm install
pnpm dev
```

Open [http://localhost:3000](http://localhost:3000). `pnpm dev` proxies `/graphql`, `/api/*` and
`/auth/*` to `FERRET_SERVER_URL` (default `http://127.0.0.1:8080`), so start the server separately.

- `pnpm lint`, `pnpm test` (`node --test`, React components render into jsdom), `pnpm build`
  (static export into `out/`, served by nginx in the Docker image).
- `pnpm codegen` generates typed GraphQL documents into `src/gql/` (ignored by git) from
  `schema.graphql`, the SDL the server prints. It runs automatically before `dev`, `build` and
  `test`. Write queries with `graphql()` from `@/gql`.

## Code style

- Icons from [Lucide](https://lucide.dev/icons) (`lucide-react`).
- Tailwind utility classes, compose them with `cn()` from `@/lib/utils`.
- Components in `src/components/`.
