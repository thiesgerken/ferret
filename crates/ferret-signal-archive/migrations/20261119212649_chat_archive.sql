CREATE TABLE archive (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    store_id TEXT NOT NULL,
    last_received_at INTEGER
) STRICT;

INSERT INTO archive (id, store_id) VALUES (1, lower(hex(randomblob(16))));

CREATE TABLE chats (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN ('direct', 'group', 'note_to_self')),
    name TEXT
) STRICT;

-- Phone numbers as signal-cli last reported them; a number can change while the ACI stays.
CREATE TABLE recipients (
    aci TEXT PRIMARY KEY,
    number TEXT NOT NULL
) STRICT;

-- The primary key is the message's Signal identity and, in this order, the messages keyset.
CREATE TABLE messages (
    chat TEXT NOT NULL REFERENCES chats (id),
    sent_at INTEGER NOT NULL,
    author TEXT NOT NULL,
    from_me INTEGER NOT NULL,
    text TEXT,
    attachments TEXT NOT NULL,
    edited INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (chat, sent_at, author)
) STRICT, WITHOUT ROWID;

-- The change journal, never pruned.
CREATE TABLE changes (
    sequence INTEGER PRIMARY KEY,
    recorded_at INTEGER NOT NULL,
    operation TEXT NOT NULL,
    chat TEXT NOT NULL,
    sent_at INTEGER NOT NULL,
    author TEXT NOT NULL
) STRICT;
