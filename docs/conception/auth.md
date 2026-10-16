# Users and auth

## TLDR

A small built-in system, modelled on carapace so both behave the same and can move to a shared
identity provider (e.g. Keycloak) later:

- **Users** with password login. The browser session is a signed JWT in an HttpOnly cookie, checked
  against a server-side session record on every request.
- **API keys** with scopes for agents, scripts and external ingesters.
- **Plugin run tokens**: short-lived, issued per plugin run, scoped to one source root.
- **A worker PSK** for the internal API that plugin workers use to claim runs.
- **Bootstrap** of the first admin from a Kubernetes Secret, no CLI step after deployment.

All state lives in ferret's Postgres. The server itself is stateless.

The path decides the credential, each API accepts exactly one kind:

| API | Credential |
|---|---|
| GraphQL `/graphql` (UI) | session cookie |
| `/auth/...` (login, logout) | username and password, exchanged for the session cookie |
| REST `/api/...` | API key or plugin run token, as bearer |
| REST `/internal/...` (own port) | worker PSK, as bearer |

## Users

```sql
create table users (
    id                   uuid primary key,
    username             text not null unique,   -- lowercase, hand-picked, stable
    display_name         text,
    password_hash        text not null,          -- argon2id
    is_admin             boolean not null default false,
    enabled              boolean not null default true,
    token_version        int not null default 1,
    created_at           timestamptz not null,
    password_changed_at  timestamptz not null,
    last_login_at        timestamptz,
    default_embedding_model text           -- null = configured default, see embeddings.md
);
```

Every owned record (`documents`, `syncs`, `api_keys`, `sessions`) has a non-null `user_id`. Records
without an owner are invalid.

Admins manage users in the UI (create, change password, enable or disable). There is no separate
admin portal.

## Bootstrap

If no enabled admin exists at startup, the server creates `admin` with the password from
`FERRET_BOOTSTRAP_PASSWORD`. If the variable is missing or shorter than 16 characters while the
bootstrap is needed, the server exits with an error. Once an admin exists, the variable is ignored.
If a user `admin` exists but isn't an enabled admin, the bootstrap turns it back into one with the
variable's password: locked out of every admin account, setting the variable is the way back in.

The Helm chart takes the password from a Secret (`existingSecret`, or generated once and kept
across upgrades via `lookup`, like carapace's Postgres password).

## Browser sessions

Same flow as carapace:

1. `POST /auth/login` with `{"username", "password"}` verifies the password and creates a
   `sessions` row. The username is trimmed and lowercased. An unknown user, a wrong password and a
   disabled user all get the same `401 {"error": "invalid username or password"}`, and an unknown
   user still costs a password hash, so neither answer nor timing reveals which usernames exist.
2. The `204` response sets the `ferret_session` cookie (`HttpOnly; Secure; SameSite=Lax; Path=/`)
   holding an HS256 JWT: `iss = ferret`, `aud = ferret-web`, `typ = session`, `sub` (user id),
   `sid`, `iat`, `exp`, `ver` (`token_version`). It has no body: the UI reads the user from
   GraphQL's `me`, so there is one user shape.
3. Sessions last 14 days from login, fixed: no sliding renewal, no setting. The cookie's `Max-Age`
   and the row's `expires_at` match.
4. Every GraphQL request verifies the signature, then checks that the session row exists and hasn't
   expired, and that the user is enabled and its `token_version` matches `ver`. Logout and disabling
   a user take effect immediately, not at cookie expiry. Without a valid session the answer is HTTP
   `401`, not a GraphQL error, so the UI can key off the status.
5. `POST /auth/logout` deletes the session row, if the cookie names one, and clears the cookie
   (`Max-Age=0`). It always answers `204`.
6. Changing a password increments `token_version`, which invalidates all of the user's cookies.

**CSRF**: `/graphql` takes `POST` only, and `/graphql` and `/auth/login` accept only
`Content-Type: application/json` (anything else is a `415`). Cross-site HTML forms can't send JSON
without a CORS preflight, and `SameSite=Lax` keeps the cookie off cross-site `POST`s anyway. `GET`
is excluded because `Lax` cookies do ride along on cross-site top-level navigations.

**Errors**: every rejected request, including a malformed body (`400`, `415`, `422`), is answered
with `{"error": "<message>"}`.

```sql
create table sessions (
    id          uuid primary key,              -- the JWT's sid
    user_id     uuid not null references users on delete cascade,
    created_at  timestamptz not null,
    expires_at  timestamptz not null
);
```

The signing secret comes from `FERRET_SESSION_SECRET` (Helm: generated once and kept via `lookup`,
or `existingSecret`). The server refuses to start without it, or with one shorter than 32 bytes. Rotating it logs everyone out, which
is the intended effect.

The JWT is redundant with the session lookup today. It stays because the claims (`iss`, `aud`,
`sub`) are what an OIDC provider issues as well, so moving to Keycloak later replaces the issuer,
not the request handling.

## API keys

Same format and storage as carapace's `ck_` keys:

- The full key is `fk_<8 hex>.<32 random bytes, urlsafe base64>`. The part before the dot is a
  non-secret lookup prefix, which also makes leaked keys easy to spot in logs and secret scanners.
- Stored as a sha256 hash. That is enough for high-entropy random keys, argon2 would only add
  latency to every request.
- Validation looks the row up by prefix, compares hashes in constant time, and hashes even when the
  prefix is unknown, so timing doesn't reveal valid prefixes.
- Shown once at creation. `last_used_at` is updated at most once a minute per key.
- Sent as `Authorization: Bearer fk_...`. A missing, unknown, revoked or expired key, or one whose
  owner is disabled, gets `401`. A valid key without the needed scope gets `403`.
- Revoking deletes the key. An expired key stays listed until its owner revokes it.

```sql
create table api_keys (
    id            uuid primary key,
    user_id       uuid not null references users on delete cascade,
    name          text not null,               -- 'paperless plugin', 'carapace search skill'
    prefix        text not null unique,
    secret_hash   bytea not null,              -- sha256 of the part after the dot
    scopes        text[] not null,             -- e.g. {'search:read', 'ingest:write'}
    sources       text[],                      -- restricts every scope to these source paths and below, null = all
    created_at    timestamptz not null,
    last_used_at  timestamptz,
    expires_at    timestamptz
);
```

### Scopes

`<scope>:<access>`, where `write` implies `read`, as in carapace.

| scope | read | write |
|---|---|---|
| `search` | search, `GET /documents/{id}`, conversation views | |
| `ingest` | read documents below its sources, list syncs | PUT and DELETE documents, start, `seen` and commit syncs |
| `admin` | list users (`GET /api/users`) | manage users. Requires `is_admin` on the owner, and stops working as soon as the owner loses it |

`sources` restricts a key to source paths and everything below them, for reading, searching and
writing alike, enforced by the server. A leaked key for one ingester can't overwrite or sweep
chats, and an agent's key only finds what it was given. Sources owned by a connection are off-limits to API keys
([architecture.md](architecture.md#source-ownership)).

Typical keys:

- **An external ingester** (a script outside ferret): `ingest:write`, `sources = {articles}`.
- **carapace's search skill**: `search:read`, `sources = {carapace, notes, paperless}`. Mail and chats
  stay invisible to it, and so never reach its LLM provider.

A key acts as its owner: it only ever sees and writes that user's documents.

## Plugin run tokens

Built-in plugins don't use API keys. When a worker claims a run ([architecture.md](architecture.md#plugin-orchestration)),
the server issues an HS256 JWT signed with `FERRET_SESSION_SECRET`: `typ = plugin_run`, `sub` (the
connection's user), `run` (run id), `source` (the connection's source root), `exp` (run timeout).

- It grants `ingest:write` on that source root and everything below it, for that user, nothing
  else.
- On every request the server also checks that the run is still `running`. Finishing or losing a
  run ends the token immediately.
- Users never see or manage these tokens. Activating a plugin in the UI is all it takes.

## Worker PSK

Plugin workers authenticate to the internal API with `FERRET_WORKER_SECRET`, a pre-shared key
mounted into server and workers from the same Secret (generated once by Helm via `lookup`, or
`existingSecret`).

- With it a worker can register, claim runs, send heartbeats and finish runs. A claimed run carries
  decrypted plugin credentials, so the PSK is the most sensitive secret after the database
  password and `FERRET_CONFIG_KEY`.
- The internal API listens on its own port, which the Ingress doesn't expose and a NetworkPolicy
  restricts to the worker pods. Running a worker outside the cluster is possible but not the
  default.
- Rotation: change the Secret, restart server and workers.

## Kubernetes

ferret is Kubernetes-native: a Helm chart is the deployment, docker-compose exists for development
only. All auth secrets (`FERRET_BOOTSTRAP_PASSWORD`, `FERRET_SESSION_SECRET`,
`FERRET_WORKER_SECRET`) come from Secrets generated once via `lookup`, or from `existingSecret`.
A fresh install needs no manual step: log in as `admin`, add connections.

## Later

- **OIDC** (e.g. a Keycloak shared with carapace): login via the provider, local users matched by
  `sub` or username. API keys stay local.
- Declarative provisioning of users and API keys (e.g. from Helm values), for GitOps setups.
- Per-key IP restrictions or rate limits, if a key ever leaks in practice.
