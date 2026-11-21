-- Set once signal-cli no longer lists the adopted account: its linked device was removed.
ALTER TABLE archive ADD COLUMN unlinked INTEGER NOT NULL DEFAULT 0;
