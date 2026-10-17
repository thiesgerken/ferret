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

-- Title changes re-embed, so updates that set the title wake the pump too.
-- The embedding cache's text_hash covers the whole embedded input from now on: prefix, title and
-- text.
drop trigger chunks_pending on chunks;
create trigger chunks_pending after insert or update of title on chunks
    for each statement execute function notify_chunks_pending();

-- Embedded, or rejected, without their title.
delete from chunk_embeddings e using chunks c where c.id = e.chunk_id and c.title is not null;
delete from embedding_failures f using chunks c where c.id = f.chunk_id and c.title is not null;
