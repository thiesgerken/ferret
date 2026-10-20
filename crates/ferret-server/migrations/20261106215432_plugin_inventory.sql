create table plugin_inventory (
    connection_id uuid not null references connections on delete cascade,
    generation uuid not null references plugin_runs on delete cascade,
    external_id text not null,
    fingerprint text not null,
    indexed boolean not null,
    primary key (connection_id, generation, external_id)
);
