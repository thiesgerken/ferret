-- A null thread retains the subtree sweep. A set thread limits it to one exact source/thread.
alter table syncs add column thread text;

update relevance_judgments set filters = filters || '{"threads": []}'::jsonb;
