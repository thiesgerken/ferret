-- Documents and threads a user removed from the index for good: their connection's PUTs for
-- them are dropped. Exactly one of external_id and thread is set.
create table exclusions (
    id             uuid primary key,
    connection_id  uuid not null references connections on delete cascade,
    source         text not null,
    external_id    text,
    thread         text,
    created_at     timestamptz not null,
    check ((external_id is null) <> (thread is null))
);

create unique index exclusions_document on exclusions (connection_id, source, external_id)
    where external_id is not null;
create unique index exclusions_thread on exclusions (connection_id, source, thread)
    where thread is not null;
