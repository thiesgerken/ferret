-- The embedding model the user's searches use, null for the configured default. No foreign key:
-- models live in the config, and the server resets defaults to removed models at startup.
alter table users add column default_embedding_model text;
