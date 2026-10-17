-- The document's title on every chunk, so a query word may match the title instead of the
-- chunk's text, and the embedded input carries the title as context.
alter table chunks add column title text;
update chunks c set title = d.title from documents d where d.id = c.document_id;

-- The title three times, like the text.
drop index chunks_bm25;
create index chunks_bm25 on chunks using paradedb (
    id,
    (text::pdb.simple('stemmer=german')),
    (text::pdb.simple('stemmer=english', 'alias=text_en')),
    (text::pdb.ngram(3, 3, 'alias=text_ngram')),
    (title::pdb.simple('stemmer=german')),
    (title::pdb.simple('stemmer=english', 'alias=title_en')),
    (title::pdb.ngram(3, 3, 'alias=title_ngram')),
    user_id,
    (source::pdb.literal),
    ts,
    document_id
);

-- A changed title re-embeds the document's chunks, so it wakes the pump like new chunks do.
drop trigger chunks_pending on chunks;
create trigger chunks_pending after insert or update of title on chunks
    for each statement execute function notify_chunks_pending();

-- Embedded without their title.
delete from chunk_embeddings e using chunks c where c.id = e.chunk_id and c.title is not null;
