create extension if not exists vector;

-- One row per chunk and embedding model, so several models can be live at once (ADR 0006).
create table chunk_embeddings (
    chunk_id      uuid not null references chunks on delete cascade,
    model         text not null,
    -- Denormalized from chunks: pgvector's iterative scans only filter on columns of the table
    -- that holds the vector.
    document_id   uuid not null,
    user_id       uuid not null,
    source        text not null,
    content_type  text not null,
    ts            timestamptz not null,
    -- No fixed dimension, models differ. Each model's partial HNSW index casts to its own, the
    -- server creates them at startup.
    embedding     halfvec not null,
    primary key (chunk_id, model)
);

-- Shared across users: the same text embedded once per model, also for queries.
create table embedding_cache (
    text_hash  bytea not null,    -- sha256 of the prefixed text
    model      text not null,
    embedding  halfvec not null,
    primary key (text_hash, model)
);

-- Chunks TEI refused to embed (a 4xx, e.g. too long for the model). The pump leaves them out, so
-- one bad chunk can't keep the others from being embedded. A changed text is a new chunk row.
create table embedding_failures (
    chunk_id  uuid not null references chunks on delete cascade,
    model     text not null,
    error     text not null,
    at        timestamptz not null,
    primary key (chunk_id, model)
);

-- The embed pump's last failure per model, until its next success.
create table embedding_errors (
    model  text primary key,
    error  text not null,
    at     timestamptz not null
);

-- The pump claims the newest chunks first.
create index chunks_ts on chunks (ts desc);

-- Wakes the embed pump after an ingest, delivered on commit.
create function notify_chunks_pending() returns trigger language plpgsql as $$
begin
    perform pg_notify('chunks_pending', '');
    return null;
end
$$;

create trigger chunks_pending after insert on chunks
    for each statement execute function notify_chunks_pending();
