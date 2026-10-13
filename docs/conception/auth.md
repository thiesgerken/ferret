# Users and auth

## TLDR

A small built-in system, modelled on carapace so both behave the same and can move to a shared
identity provider (e.g. Keycloak) later:

- **Users** with password login. The browser session is a signed JWT in an HttpOnly cookie, checked
  against a server-side session record on every request.
- **API keys** with scopes for agents, scripts and external ingesters.
- **Plugin run tokens**: short-lived, issued per plugin run, scoped to one source.
- **A worker PSK** for the internal API that plugin workers use to claim runs.
- **Bootstrap** of the first admin from a Kubernetes Secret, no CLI step after deployment.

All state lives in ferret's Postgres. The server itself is stateless.

Each API accepts exactly one kind of credential:

| API | Credential |
|---|---|
| GraphQL `/graphql` (UI) | session cookie |
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

The Helm chart takes the password from a Secret (`existingSecret`, or generated once and kept
across upgrades via `lookup`, like carapace's Postgres password).

## Browser sessions

Same flow as carapace:

1. `POST /api/auth/login` verifies the password and creates a `sessions` row.
2. The response sets the `ferret_session` HttpOnly, `Secure`, `SameSite=Lax` cookie holding an
   HS256 JWT: `iss`, `aud`, `typ = session`, `sub` (user id), `sid`, `iat`, `exp`, `ver`
   (`token_version`).
3. Every request verifies the signature, then checks that the session row exists and that the user
   is enabled and its `token_version` matches `ver`. Logout and disabling a user take effect
   immediately, not at cookie expiry.
4. `POST /api/auth/logout` deletes the session row and the cookie.
5. Changing a password increments `token_version`, which invalidates all of the user's cookies.

```sql
create table sessions (
    id          uuid primary key,              -- the JWT's sid
    user_id     uuid not null references users on delete cascade,
    created_at  timestamptz not null,
    expires_at  timestamptz not null
);
```

The signing secret comes from `FERRET_SESSION_SECRET` (Helm: generated once and kept via `lookup`,
or `existingSecret`). The server refuses to start without it. Rotating it logs everyone out, which
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
- Sent as `Authorization: Bearer fk_...`.

```sql
create table api_keys (
    id            uuid primary key,
    user_id       uuid not null references users on delete cascade,
    name          text not null,               -- 'paperless plugin', 'carapace kb-search'
    prefix        text not null unique,
    secret_hash   text not null,
    scopes        text[] not null,             -- e.g. {'search:read', 'ingest:write'}
    sources       text[],                      -- restricts ingest:write to these sources, null = all
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
| `ingest` | list own sources and syncs | PUT and DELETE documents, start and commit syncs |
| `admin` | list users | manage users. Requires `is_admin` on the owner, and stops working as soon as the owner loses it |

`sources` binds an ingest key to its sources. A leaked key for one ingester can't overwrite or sweep
chats.

Typical keys:

- **An external ingester** (a script outside ferret): `ingest:write`, `sources = {articles}`.
- **carapace's `kb-search` skill**: `search:read`, nothing else.

A key acts as its owner: it only ever sees and writes that user's documents.

## Plugin run tokens

Built-in plugins don't use API keys. When a worker claims a run ([architecture.md](architecture.md#plugin-orchestration)),
the server issues an HS256 JWT signed with `FERRET_SESSION_SECRET`: `typ = plugin_run`, `sub` (the
instance's user), `run` (run id), `source`, `exp` (run timeout).

- It grants `ingest:write` on exactly that source of that user, nothing else.
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
A fresh install needs no manual step: log in as `admin`, activate plugins in Sources.

## Later

- **OIDC** (e.g. a Keycloak shared with carapace): login via the provider, local users matched by
  `sub` or username. API keys stay local.
- Declarative provisioning of users and API keys (e.g. from Helm values), for GitOps setups.
- Per-key IP restrictions or rate limits, if a key ever leaks in practice.
