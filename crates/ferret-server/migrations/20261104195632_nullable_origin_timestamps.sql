-- Origin dates can be unknown independently. Search date predicates naturally exclude NULL.
alter table documents alter column created_at drop not null;
alter table documents alter column modified_at drop not null;
alter table chunks alter column ts drop not null;
alter table chunk_embeddings alter column ts drop not null;
