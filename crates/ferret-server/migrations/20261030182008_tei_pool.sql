-- The TEIs of the config, with the switches an admin flips in the UI. Rows of TEIs that left the
-- config are removed at startup. Which model a TEI serves is discovered at runtime, not stored.
create table teis (
    url text primary key,
    enabled boolean not null,
    queries boolean not null,
    embedding boolean not null,
    -- Its last failure (pump batch or query), until its next success.
    last_error text,
    last_error_at timestamptz,
    check ((last_error is null) = (last_error_at is null))
);

-- Errors are per TEI now.
drop table embedding_errors;
