-- Unstemmed text and title, for typo-tolerant matching. pg_search runs a field's tokenizer on the
-- query before its fuzzy match, so stemmed fields miss typos in suffixes the stemmer would strip
-- (`kündigng` stays whole, two edits from the stem `kundig`) and give short stems huge distance-1
-- neighbourhoods.
drop index chunks_bm25;
create index chunks_bm25 on chunks using paradedb (
    id,
    (text::pdb.simple('stemmer=german')),
    (text::pdb.simple('stemmer=english', 'alias=text_en')),
    (text::pdb.ngram(3, 3, 'alias=text_ngram')),
    (text::pdb.simple('alias=text_plain')),
    (title::pdb.simple('stemmer=german')),
    (title::pdb.simple('stemmer=english', 'alias=title_en')),
    (title::pdb.ngram(3, 3, 'alias=title_ngram')),
    (title::pdb.simple('alias=title_plain')),
    user_id,
    (source::pdb.literal),
    ts,
    document_id
);
