-- A user's statement that a document is the right hit for a search. It carries its own search,
-- so evaluating a model replays the judged searches without a search log.
create table relevance_judgments (
    id           uuid primary key,
    user_id      uuid not null references users on delete cascade,
    query        text not null,
    -- The search's filters in canonical form (judgments.rs), so the same search always matches
    -- the same row.
    filters      jsonb not null,
    document_id  uuid not null references documents on delete cascade,
    created_at   timestamptz not null,
    unique (user_id, query, filters, document_id)
);
