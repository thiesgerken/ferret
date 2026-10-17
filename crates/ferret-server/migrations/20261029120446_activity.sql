-- When the pump wrote the embedding, for its rate. Null for those from before it was recorded.
alter table chunk_embeddings add column embedded_at timestamptz;
alter table chunk_embeddings alter column embedded_at set default now();
create index on chunk_embeddings (model, embedded_at);

-- What a running run did so far, counted by the server as its writes arrive. The worker's
-- `stats` replace them once it finishes.
alter table plugin_runs
    add column put integer not null default 0,
    add column unchanged integer not null default 0,
    add column deleted integer not null default 0;

-- An API key's writes, grouped into bursts: a write within 10 minutes of the burst's last one
-- belongs to it, a later one starts the next.
create table api_ingests (
    id          uuid primary key,
    user_id     uuid not null references users on delete cascade,
    api_key_id  uuid not null references api_keys on delete cascade,
    started_at  timestamptz not null,
    last_at     timestamptz not null,
    put         integer not null default 0,
    unchanged   integer not null default 0,
    deleted     integer not null default 0
);
create index on api_ingests (api_key_id, last_at desc);
create index on api_ingests (user_id, started_at desc);
