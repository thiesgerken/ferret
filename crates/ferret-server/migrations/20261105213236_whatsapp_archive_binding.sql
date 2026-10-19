-- Archive identity survives disposable plugin cursors, failures and exclusion resets.
alter table connections add column archive_store_id text
    check (archive_store_id is null or archive_store_id ~ '^[0-9a-f]{32}$');
