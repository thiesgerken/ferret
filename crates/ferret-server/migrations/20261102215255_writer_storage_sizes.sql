-- Size content on writes, not while Activity polls. pg_column_size(record) includes the tuple
-- header and fields; add the heap line pointer. This is an estimate before final TOAST/page layout.
create function estimate_content_row_bytes() returns trigger language plpgsql as $$
begin
    new.storage_bytes = 0;
    new.storage_bytes = pg_column_size(new) + 4;
    return new;
end
$$;

alter table documents add column storage_bytes bigint not null default 0;
alter table chunks add column storage_bytes bigint not null default 0;
alter table chunks add column indexed_text_bytes bigint generated always as
    (octet_length(text)::bigint + coalesce(octet_length(title), 0)) stored;
alter table chunk_embeddings add column storage_bytes bigint not null default 0;

create trigger documents_storage_bytes before insert or update of
    text, title, url, thread, authors, meta, source, external_id, content_type on documents
    for each row execute function estimate_content_row_bytes();
create trigger chunks_storage_bytes before insert or update on chunks
    for each row execute function estimate_content_row_bytes();
create trigger embeddings_storage_bytes before insert or update on chunk_embeddings
    for each row execute function estimate_content_row_bytes();

-- One-time backfill. Future seen_at/API-key-only updates need no content-size computation.
update documents set text = text;
update chunks set text = text;
update chunk_embeddings set storage_bytes = 0;

alter table documents alter column storage_bytes drop default;
alter table chunks alter column storage_bytes drop default;
alter table chunk_embeddings alter column storage_bytes drop default;
