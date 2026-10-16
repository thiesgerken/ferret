-- Whether `root` is `path` or one of its ancestors, compared segment-wise: `notes` covers
-- `notes/work` but not `notes-old`. The same rule as `sources::covers` in the server.
create function source_covers(root text, path text) returns boolean
    language sql immutable strict parallel safe
    return path = root or starts_with(path, root || '/');

create table documents (
    id            uuid primary key,
    user_id       uuid not null references users on delete cascade,
    source        text not null,
    external_id   text not null,
    content_type  text not null check (content_type in ('markdown', 'plain', 'email')),
    title         text,
    url           text,
    thread        text,
    authors       text[] not null,
    created_at    timestamptz not null,
    modified_at   timestamptz not null,
    meta          jsonb not null,
    text          text not null,
    content_hash  bytea not null,
    -- Bumped on every PUT, even a no-op, for the sweep of a sync.
    seen_at       timestamptz not null,
    unique (user_id, source, external_id)
);

create table chunks (
    id            uuid primary key,
    document_id   uuid not null references documents on delete cascade,
    -- Denormalized from documents, so search filters without a join.
    user_id       uuid not null,
    source        text not null,
    content_type  text not null,
    ts            timestamptz not null,    -- the document's created_at
    ordinal       int not null,
    -- Characters (Unicode scalar values) into the document's text.
    start_offset  int not null,
    end_offset    int not null,
    text          text not null,
    text_hash     bytea not null
);

create index chunks_document_id on chunks (document_id);

-- The text three times: German- and English-stemmed, and as trigrams, which find compounds
-- ("Versicherung" in "Kfz-Versicherungsbeitrag"). The other fields let filters run in the index.
create index chunks_bm25 on chunks using paradedb (
    id,
    (text::pdb.simple('stemmer=german')),
    (text::pdb.simple('stemmer=english', 'alias=text_en')),
    (text::pdb.ngram(3, 3, 'alias=text_ngram')),
    user_id,
    (source::pdb.literal),
    ts,
    document_id
);
