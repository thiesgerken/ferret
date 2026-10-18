# Authentication and trust boundaries

Ferret is a self-hosted personal index with per-user data isolation. The server, database operator and plugin workers are trusted infrastructure. Scoped keys limit clients; they do not make the database or worker PSK safe to hand to an untrusted party.

## Users, bootstrap and sessions

Users sign in with username/password; no registration/OIDC endpoint is implemented. Usernames normalize to trimmed lowercase and allow letters, digits, dot, underscore and hyphen. Passwords are Argon2-hashed. Admin-set/user-changed passwords require at least 12 characters.

When no enabled admin exists, startup uses `FERRET_BOOTSTRAP_PASSWORD` to create or restore `admin`; this bootstrap password needs at least 16 characters. If an enabled admin already exists, the variable is not a password reset. The UI prevents disabling/demoting the last enabled admin. New users and admin account management use GraphQL, not an invented REST CRUD interface.

Login returns a 14-day `ferret_session` JWT cookie: HttpOnly, Secure, SameSite=Lax, Path=/. Each GraphQL request checks the session database row, expiration, user enabled flag and password token version. Logout deletes that session. Password changes invalidate other sessions; a self-change keeps the current session with its remaining lifetime. API keys are separate credentials, not automatically replaced on a password change.

Production requires HTTPS. Local loopback behavior is browser/tool-specific: the seed script explicitly forwards the Secure cookie rather than trusting a plain-HTTP curl cookie jar. Use the [GraphQL example](api/graphql.md) for shell sessions.

## API keys and exact scopes

Create keys in Settings or the authenticated GraphQL mutation. Keys begin with `fk_`; the secret is shown once and stored as a hash, with a non-secret prefix for identification. Revocation/expiry and the owner's enabled/admin state are checked on use. A revoked key remains the recorded writer of documents it put.

| Scope | Current permission |
| --- | --- |
| `search:read` | search and full-document/thread reads |
| `ingest:read` | full-document/thread reads, **not** search |
| `ingest:write` | puts, explicit deletes, sync/seen/commit, and ingest reads |
| `admin:read` | current public machine admin read (`GET /api/users`) while owner is admin |
| `admin:write` | includes admin read; no public REST admin-write endpoint is currently implemented |

Write implies read only in its own scope family. `ingest:write` does not imply `search:read`; admin scopes do not bypass document-user isolation or automatically grant search/ingestion.

Keys may restrict all their operations to source roots and descendants. Omitted/null `sources` means all of the owner's sources; an explicit empty array is rejected. Admin scopes cannot carry source restrictions because admin objects have no source. Requests cannot widen a key by asking for a parent source. Ordinary keys cannot write below a connection root, and cannot sweep a root containing one.

For integrations start with the narrowest root and permission. An agent needing only discovery gets `search:read`; an ingester gets `ingest:write`. Readable full document text is included in search permission, not just snippets.

## Run tokens and worker authentication

The private worker API on 8081 takes `FERRET_WORKER_SECRET` as a bearer PSK. Worker registration supplies plugin schemas; claims receive decrypted credentials, previous state and a run token. A worker PSK therefore crosses user boundaries through the work it can claim and must not be exposed by the proxy. A NetworkPolicy/firewall is part of the deployment boundary.

Run tokens are signed JWTs with a separate run audience/type, owner and source. They grant `ingest:write` only below the connection root. Each request also checks that the run is running, its lease is fresh and its user is enabled. The token has a 24-hour hard lifetime; heartbeats maintain the two-minute lease but do not extend that maximum. Finish, loss or connection deletion ends writes. The worker PSK is not a public ingestion key; session JWTs are not accepted as run tokens.

## Encrypted origin credentials

Plugin JSON Schema marks top-level secret properties `writeOnly`. They are split out of normal config, encrypted using XChaCha20-Poly1305 under `FERRET_CONFIG_KEY` (32 random bytes, base64), and bound to the connection/user ids as authenticated data. GraphQL never returns them. Only a claimed run receives merged plaintext config.

Non-secret config changes require re-entry of existing secrets, preventing hidden credential reuse against a newly chosen origin. Merely changing schedule/enabled does not. Back up the encryption key separately from the database: losing it makes stored origin credentials unreadable. There is no online key-rotation/re-encryption workflow in the API; changing the environment key alone is not rotation.

## Privacy consequences

An origin token determines what the plugin can copy. Once copied, all documents belong to that Ferret user and its authorized keys; Ferret does not continuously evaluate the origin's ACLs on search. Humans still authenticate separately when opening an origin URL. Permission loss at an origin only removes indexed copies when the plugin successfully reconciles a listing that no longer contains them.

Text is stored in Postgres and sent to configured TEIs. Cached embeddings are shared across users and are not deleted by a document cascade; deleting a model that is no longer configured in Admin ([stored embeddings](embeddings.md#stored-embeddings-and-the-model-registry)) removes all of its cached vectors. Database backups and run logs are additional retention surfaces. Avoid logging credentials or sensitive raw responses in plugins; logs are visible in the UI. This is an implementation description, not an independent security audit.

See [API surfaces](api/README.md), [connections](connections.md), [embedding privacy](embeddings.md). Implementation: [sessions](../crates/ferret-server/src/auth.rs), [users](../crates/ferret-server/src/users.rs), [API keys](../crates/ferret-server/src/api_keys.rs), [authorization](../crates/ferret-server/src/api_auth.rs), [run authentication](../crates/ferret-server/src/runs.rs), [encryption](../crates/ferret-server/src/credentials.rs).
