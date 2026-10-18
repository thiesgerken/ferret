-- Every embedding model that ever embedded, configured or not. The config says which models are
-- in use. This remembers their dimension, so a model can't come back with another one (its HNSW
-- index is built for one), and lets Admin list and delete what models left the config left behind.
create table embedding_models (
    -- The `model` of chunk_embeddings, embedding_cache and embedding_failures.
    id          text primary key,
    dimension   integer not null check (dimension > 0),
    first_seen  timestamptz not null default now()
);

-- Query embeddings count too: a model may have cached some before it embedded any chunk. A model
-- with vectors of two dimensions violates the primary key and fails the migration, as it should.
insert into embedding_models (id, dimension, first_seen)
select model, dimension, coalesce(min(embedded_at), now())
from (
    select model, vector_dims(embedding) as dimension, embedded_at from chunk_embeddings
    union all
    select model, vector_dims(embedding), null from embedding_cache
) stored
group by model, dimension;
