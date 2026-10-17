-- Icons are lucide names as the UI's DynamicIcon takes them, see icons.rs.
alter table connections add column name text, add column icon text;
update connections set
    name = source,
    icon = case plugin
        when 'git' then 'git-branch'
        when 'carapace' then 'brain'
        when 'paperless' then 'file-text'
        else 'plug'
    end;
alter table connections
    alter column name set not null,
    alter column icon set not null,
    add check (btrim(name) <> ''),
    add check (icon ~ '^[a-z0-9]+(-[a-z0-9]+)*$' and length(icon) <= 64);

alter table api_keys
    add column icon text not null default 'key-round'
        check (icon ~ '^[a-z0-9]+(-[a-z0-9]+)*$' and length(icon) <= 64),
    -- A revoked key stays as the origin of the documents it put.
    add column revoked_at timestamptz;
alter table api_keys alter column icon drop default;

-- The key that last put the document, null for a plugin run's.
alter table documents add column api_key_id uuid references api_keys on delete set null;
create index on documents (api_key_id);
