-- Explore's default sort: newest first, paged.
create index documents_created_at on documents (user_id, created_at desc, id);
