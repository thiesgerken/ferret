create table syncs (
    id            uuid primary key,
    user_id       uuid not null references users on delete cascade,
    -- The root swept at the commit: this source and everything below it.
    source        text not null,
    started_at    timestamptz not null,
    committed_at  timestamptz
);
