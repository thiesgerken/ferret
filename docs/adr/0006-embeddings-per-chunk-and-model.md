# Embeddings per chunk and model, with denormalized filter columns

Embeddings live in their own table keyed by chunk and model, each model with a partial HNSW
index, so several models can be live at once and compared or switched without downtime. The table
repeats the filter columns of its chunk (document, user, source, content type, timestamp) because pgvector's
iterative index scans only filter on columns of the table that holds the vectors; filtering through
a join would silently return fewer than k hits. A single vector column re-embedded in place was
rejected because model comparison is a recurring need, not a one-off.
