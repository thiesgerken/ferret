-- The external_id of the document in the same source and thread this one answers. A loose
-- reference: the parent may be excluded, deleted or never indexed.
alter table documents add column parent text;
