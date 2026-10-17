-- Thread listing and the viewer's previous/next both walk a thread in this order.
create index documents_thread on documents (user_id, source, thread, created_at, id)
    where thread is not null;
