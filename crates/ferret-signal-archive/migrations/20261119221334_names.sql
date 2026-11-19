-- Names from signal-cli's listContacts: the contact's nickname, else its system name, else the
-- profile name. Contacts known by name or username alone have no number.
CREATE TABLE recipients_named (
    aci TEXT PRIMARY KEY,
    number TEXT,
    name TEXT
) STRICT;

INSERT INTO recipients_named (aci, number) SELECT aci, number FROM recipients;
DROP TABLE recipients;
ALTER TABLE recipients_named RENAME TO recipients;

-- A rename concerns a whole chat, so its journal entries carry no message identity.
CREATE TABLE changes_chat_scoped (
    sequence INTEGER PRIMARY KEY,
    recorded_at INTEGER NOT NULL,
    operation TEXT NOT NULL,
    chat TEXT NOT NULL,
    sent_at INTEGER,
    author TEXT,
    CHECK ((sent_at IS NULL) = (author IS NULL))
) STRICT;

INSERT INTO changes_chat_scoped (sequence, recorded_at, operation, chat, sent_at, author)
SELECT sequence, recorded_at, operation, chat, sent_at, author FROM changes;
DROP TABLE changes;
ALTER TABLE changes_chat_scoped RENAME TO changes;
