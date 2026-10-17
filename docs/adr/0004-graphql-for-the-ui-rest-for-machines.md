# GraphQL for the UI, REST for machines

The frontend talks GraphQL (session cookie), plugins, external ingesters and agents talk REST (API
key or run token), plugin workers talk to an internal REST API on its own port (pre-shared key).
The UI reads nested, filtered data where GraphQL shines; machine clients want an idempotent `PUT`
per document, a plain `GET` for search and scopes that map onto routes, which per-field scope
checks in GraphQL would complicate. Each API accepts exactly one kind of credential, and the path
decides which: the session cookie is only ever obtained at `/auth/login`, accepted at `/graphql`,
and read by `/auth/logout` to end it, so `/api` stays bearer-only and never has to choose between a
cookie and a key.
