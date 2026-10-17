# Postgres as the only store

ferret keeps documents, chunks, BM25 index (ParadeDB `pg_search`), vectors (pgvector), users and
plugin state in one Postgres. A dedicated vector database (Qdrant) or search engine
(Elasticsearch) was rejected: at personal scale (hundreds of thousands of chunks) pgvector's HNSW is
enough, ingestion and its keyword index update share one transaction (embeddings follow from the
embed pump), filters and joins are plain SQL, and the same database is the archive for content
without another copy (chat exports). Keyword search uses real BM25 rather than Postgres'
`ts_rank`, which has no IDF.
