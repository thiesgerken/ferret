create table api_keys (
    id            uuid primary key,
    user_id       uuid not null references users on delete cascade,
    name          text not null check (btrim(name) <> ''),
    -- The non-secret part of the key after `fk_`, for lookup.
    prefix        text not null unique,
    -- sha256 of the secret part.
    secret_hash   bytea not null,
    scopes        text[] not null check (
        cardinality(scopes) > 0
        and scopes <@ array['search:read', 'ingest:read', 'ingest:write', 'admin:read', 'admin:write']
    ),
    -- Restricts every scope to these source paths and below, null = all sources.
    sources       text[] check (cardinality(sources) > 0),
    created_at    timestamptz not null,
    last_used_at  timestamptz,
    expires_at    timestamptz
);

create index on api_keys (user_id);
