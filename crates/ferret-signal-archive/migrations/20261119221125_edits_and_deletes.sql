-- A deleted message stays as a tombstone (no text, no attachments) that keeps its identity
-- occupied: inserts skip existing identities, so a redelivered envelope or a later history
-- import cannot bring the message back.
ALTER TABLE messages ADD COLUMN deleted INTEGER NOT NULL DEFAULT 0;

-- Every later revision timestamp of an edited message, pointing back to its original identity.
-- Signal addresses an edit or delete to the latest revision or to any revision, not only to the
-- original send.
CREATE TABLE edits (
    chat TEXT NOT NULL,
    author TEXT NOT NULL,
    edited_at INTEGER NOT NULL,
    sent_at INTEGER NOT NULL,
    PRIMARY KEY (chat, author, edited_at),
    FOREIGN KEY (chat, sent_at, author) REFERENCES messages (chat, sent_at, author)
) STRICT, WITHOUT ROWID;
