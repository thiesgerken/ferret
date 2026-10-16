create table users (
    id                   uuid primary key,
    username             text not null unique check (username <> '' and username = lower(btrim(username))),
    display_name         text,
    password_hash        text not null,
    is_admin             boolean not null default false,
    enabled              boolean not null default true,
    token_version        int not null default 1,
    created_at           timestamptz not null,
    password_changed_at  timestamptz not null,
    last_login_at        timestamptz
);

create table sessions (
    id          uuid primary key,
    user_id     uuid not null references users on delete cascade,
    created_at  timestamptz not null,
    expires_at  timestamptz not null
);
