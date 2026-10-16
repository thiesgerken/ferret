-- Upserted by workers when they register. The UI renders config forms from config_schema.
create table plugin_types (
    name           text primary key,
    config_schema  jsonb not null,
    registered_at  timestamptz not null
);

create table workers (
    id            uuid primary key,
    name          text not null,               -- pod name, informational
    plugins       text[] not null,             -- this worker only gets runs of these
    last_seen_at  timestamptz not null
);

create table connections (
    id           uuid primary key,
    user_id      uuid not null references users on delete cascade,
    plugin       text not null references plugin_types,
    -- The source root: the connection writes to it and below it, exclusively.
    source       text not null,
    config       jsonb not null,
    schedule     text not null,                -- cron expression
    enabled      boolean not null default true,
    next_run_at  timestamptz not null,
    -- Incremental cursor, written only by a successful run.
    state        jsonb not null default '{}',
    created_at   timestamptz not null,
    unique (user_id, source)
);

create table plugin_runs (
    id            uuid primary key,
    connection_id uuid not null references connections on delete cascade,
    worker_id     uuid not null references workers,
    status        text not null check (status in ('running', 'succeeded', 'failed', 'lost')),
    started_at    timestamptz not null,
    heartbeat_at  timestamptz not null,
    finished_at   timestamptz,
    stats         jsonb,
    log           text not null default '',
    error         text
);

create index plugin_runs_connection on plugin_runs (connection_id, started_at desc);
-- At most one live run per connection, whatever races the claims run into.
create unique index plugin_runs_one_running on plugin_runs (connection_id) where status = 'running';
