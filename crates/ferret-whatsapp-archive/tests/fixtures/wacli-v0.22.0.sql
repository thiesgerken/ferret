-- Native offline wacli v0.22.0 archive, created with doctor without --connect.
-- sqlite3 .schema --nosys plus native migration/retrieval/change state rows.
-- FTS shadow tables are omitted because CREATE VIRTUAL TABLE recreates them.
-- Release archive SHA256: da3f528e0c65b743d451b360b9dea14d784fb209a2e9f8b8b9c00498d8f619fe.
CREATE TABLE schema_migrations (
			version INTEGER PRIMARY KEY,
			name TEXT NOT NULL,
			applied_at INTEGER NOT NULL
		);
CREATE TABLE chats (
    jid TEXT PRIMARY KEY,
    kind TEXT NOT NULL, -- dm|group|broadcast|newsletter|unknown
    name TEXT,
    last_message_ts INTEGER,
    archived INTEGER NOT NULL DEFAULT 0,
    pinned INTEGER NOT NULL DEFAULT 0,
    muted_until INTEGER NOT NULL DEFAULT 0,
    unread INTEGER NOT NULL DEFAULT 0,
    unread_count INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE contacts (
    jid TEXT PRIMARY KEY,
    phone TEXT,
    push_name TEXT,
    full_name TEXT,
    first_name TEXT,
    business_name TEXT,
    system_name TEXT,
    updated_at INTEGER NOT NULL
);
CREATE TABLE groups (
    jid TEXT PRIMARY KEY,
    name TEXT,
    owner_jid TEXT,
    created_ts INTEGER,
    is_parent INTEGER NOT NULL DEFAULT 0,
    linked_parent_jid TEXT,
    left_at INTEGER,
    updated_at INTEGER NOT NULL
);
CREATE TABLE group_participants (
    group_jid TEXT NOT NULL,
    user_jid TEXT NOT NULL,
    role TEXT,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (group_jid, user_jid),
    FOREIGN KEY (group_jid) REFERENCES groups(jid) ON DELETE CASCADE
);
CREATE TABLE contact_aliases (
    jid TEXT PRIMARY KEY,
    alias TEXT NOT NULL,
    notes TEXT,
    updated_at INTEGER NOT NULL
);
CREATE TABLE contact_tags (
    jid TEXT NOT NULL,
    tag TEXT NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (jid, tag)
);
CREATE TABLE messages (
    rowid INTEGER PRIMARY KEY AUTOINCREMENT,
    chat_jid TEXT NOT NULL,
    chat_name TEXT,
    msg_id TEXT NOT NULL,
    sender_jid TEXT,
    sender_name TEXT,
    ts INTEGER NOT NULL,
    from_me INTEGER NOT NULL,
    text TEXT,
    display_text TEXT,
    quoted_msg_id TEXT,
    quoted_sender_jid TEXT,
    is_forwarded INTEGER NOT NULL DEFAULT 0,
    forwarding_score INTEGER NOT NULL DEFAULT 0,
    reaction_to_id TEXT,
    reaction_emoji TEXT,
    media_type TEXT,
    media_caption TEXT,
    filename TEXT,
    mime_type TEXT,
    direct_path TEXT,
    media_key BLOB,
    file_sha256 BLOB,
    file_enc_sha256 BLOB,
    file_length INTEGER,
    local_path TEXT,
    downloaded_at INTEGER,
    media_unavailable_at INTEGER,
    revoked INTEGER NOT NULL DEFAULT 0,
    deleted_for_me INTEGER NOT NULL DEFAULT 0,
    deleted_at INTEGER,
    deletion_reason TEXT,
    payload_purged_at INTEGER,
    edited INTEGER NOT NULL DEFAULT 0,
    edited_ts INTEGER NOT NULL DEFAULT 0,
    buttons TEXT,
    UNIQUE(chat_jid, msg_id),
    FOREIGN KEY (chat_jid) REFERENCES chats(jid) ON DELETE CASCADE
);
CREATE INDEX idx_messages_chat_ts ON messages(chat_jid, ts);
CREATE INDEX idx_messages_ts ON messages(ts);
CREATE TABLE message_receipts (
    chat_jid TEXT NOT NULL,
    msg_id TEXT NOT NULL,
    recipient_jid TEXT NOT NULL,
    status TEXT NOT NULL, -- delivered|read|played
    ts INTEGER NOT NULL,
    PRIMARY KEY (chat_jid, msg_id, recipient_jid),
    FOREIGN KEY (chat_jid) REFERENCES chats(jid) ON DELETE CASCADE
);
CREATE INDEX idx_message_receipts_msg ON message_receipts(chat_jid, msg_id);
CREATE TABLE message_payload_purges (
    chat_jid TEXT NOT NULL,
    msg_id TEXT NOT NULL,
    purged_at INTEGER NOT NULL,
    deleted_at INTEGER NOT NULL,
    deletion_reason TEXT NOT NULL,
    PRIMARY KEY (chat_jid, msg_id)
);
CREATE TABLE message_local_media_aliases (
    chat_jid TEXT NOT NULL,
    msg_id TEXT NOT NULL,
    local_path TEXT NOT NULL,
    downloaded_at INTEGER,
    PRIMARY KEY (chat_jid, msg_id, local_path),
    FOREIGN KEY (chat_jid, msg_id) REFERENCES messages(chat_jid, msg_id) ON DELETE CASCADE
);
CREATE TABLE status_messages (
    rowid INTEGER PRIMARY KEY AUTOINCREMENT,
    msg_id TEXT NOT NULL UNIQUE,
    ts INTEGER NOT NULL,
    from_me INTEGER NOT NULL,
    sender_jid TEXT,
    sender_name TEXT,
    text TEXT,
    media_type TEXT,
    media_caption TEXT,
    filename TEXT,
    mime_type TEXT,
    direct_path TEXT,
    media_key BLOB,
    file_sha256 BLOB,
    file_enc_sha256 BLOB,
    file_length INTEGER,
    background_color TEXT,
    font INTEGER
);
CREATE INDEX idx_status_messages_ts ON status_messages(ts);
CREATE TABLE call_events (
    rowid INTEGER PRIMARY KEY AUTOINCREMENT,
    chat_jid TEXT NOT NULL,
    chat_name TEXT,
    sender_jid TEXT,
    sender_name TEXT,
    call_id TEXT NOT NULL,
    msg_id TEXT,
    event_type TEXT NOT NULL,
    direction TEXT,
    media TEXT,
    outcome TEXT,
    reason TEXT,
    call_type TEXT,
    duration_secs INTEGER NOT NULL DEFAULT 0,
    ts INTEGER NOT NULL,
    participants TEXT,
    UNIQUE(chat_jid, call_id, event_type, ts),
    FOREIGN KEY (chat_jid) REFERENCES chats(jid) ON DELETE CASCADE
);
CREATE INDEX idx_call_events_chat_ts ON call_events(chat_jid, ts);
CREATE INDEX idx_call_events_ts ON call_events(ts);
CREATE TABLE starred (
    chat_jid TEXT NOT NULL,
    msg_id TEXT NOT NULL,
    sender_jid TEXT,
    from_me INTEGER NOT NULL DEFAULT 0,
    starred_at INTEGER NOT NULL,
    PRIMARY KEY (chat_jid, msg_id)
);
CREATE INDEX idx_starred_starred_at ON starred(starred_at);
CREATE TABLE polls (
    chat_jid TEXT NOT NULL,
    msg_id TEXT NOT NULL,
    sender_jid TEXT,
    question TEXT NOT NULL,
    options_json TEXT NOT NULL,
    selectable_count INTEGER NOT NULL DEFAULT 1,
    created_ts INTEGER NOT NULL,
    PRIMARY KEY (chat_jid, msg_id)
);
CREATE INDEX idx_polls_chat_ts ON polls(chat_jid, created_ts);
CREATE TABLE message_locations (
    chat_jid TEXT NOT NULL,
    msg_id TEXT NOT NULL,
    latitude REAL NOT NULL,
    longitude REAL NOT NULL,
    name TEXT,
    address TEXT,
    is_live INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (chat_jid, msg_id)
);
CREATE TABLE poll_votes (
    chat_jid TEXT NOT NULL,
    poll_msg_id TEXT NOT NULL,
    voter_jid TEXT NOT NULL,
    vote_msg_id TEXT NOT NULL,
    selected_options_json TEXT NOT NULL,
    ts INTEGER NOT NULL,
    PRIMARY KEY (chat_jid, poll_msg_id, voter_jid)
);
CREATE INDEX idx_poll_votes_poll ON poll_votes(chat_jid, poll_msg_id);
CREATE VIRTUAL TABLE messages_fts USING fts5(
				text,
				media_caption,
				filename,
				chat_name,
				sender_name,
				display_text
			)
/* messages_fts(text,media_caption,filename,chat_name,sender_name,display_text) */;
CREATE INDEX idx_groups_linked_parent_jid ON groups(linked_parent_jid);
CREATE TABLE app_state_recovery_intents (
			id INTEGER PRIMARY KEY AUTOINCREMENT,
			collection TEXT NOT NULL
		);
CREATE INDEX idx_app_state_recovery_intents_collection
		ON app_state_recovery_intents(collection);
CREATE INDEX idx_messages_sender_jid ON messages(sender_jid);
CREATE INDEX idx_messages_quoted_sender_jid ON messages(quoted_sender_jid);
CREATE TRIGGER messages_ai AFTER INSERT ON messages WHEN new.deleted_at IS NULL BEGIN
			INSERT INTO messages_fts(rowid, text, media_caption, filename, chat_name, sender_name, display_text)
			VALUES (new.rowid, COALESCE(new.text,''), COALESCE(new.media_caption,''), COALESCE(new.filename,''), COALESCE(new.chat_name,''), COALESCE(new.sender_name,''), COALESCE(new.display_text,''));
		END;
CREATE TRIGGER messages_ad AFTER DELETE ON messages BEGIN
			DELETE FROM messages_fts WHERE rowid = old.rowid;
		END;
CREATE TRIGGER messages_au AFTER UPDATE ON messages
		WHEN old.rowid IS NOT new.rowid
			OR old.deleted_at IS NOT new.deleted_at
			OR old.text IS NOT new.text
			OR old.media_caption IS NOT new.media_caption
			OR old.filename IS NOT new.filename
			OR old.chat_name IS NOT new.chat_name
			OR old.sender_name IS NOT new.sender_name
			OR old.display_text IS NOT new.display_text
		BEGIN
			DELETE FROM messages_fts WHERE rowid = old.rowid;
			INSERT INTO messages_fts(rowid, text, media_caption, filename, chat_name, sender_name, display_text)
			SELECT new.rowid, COALESCE(new.text,''), COALESCE(new.media_caption,''), COALESCE(new.filename,''), COALESCE(new.chat_name,''), COALESCE(new.sender_name,''), COALESCE(new.display_text,'')
			WHERE new.deleted_at IS NULL;
		END;
CREATE TABLE unavailable_app_state_keys (
  account_jid TEXT NOT NULL,
  key_id BLOB NOT NULL,
  PRIMARY KEY(account_jid,key_id)
 );
CREATE TABLE retrieval_state (
 id INTEGER PRIMARY KEY CHECK(id=1), store_id TEXT NOT NULL, revision INTEGER NOT NULL DEFAULT 0,
 last_sync_at INTEGER
 );
CREATE TRIGGER retrieval_chats_insert AFTER INSERT ON chats BEGIN UPDATE retrieval_state SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER retrieval_chats_update AFTER UPDATE ON chats BEGIN UPDATE retrieval_state SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER retrieval_chats_delete AFTER DELETE ON chats BEGIN UPDATE retrieval_state SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER retrieval_contacts_insert AFTER INSERT ON contacts BEGIN UPDATE retrieval_state SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER retrieval_contacts_update AFTER UPDATE ON contacts BEGIN UPDATE retrieval_state SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER retrieval_contacts_delete AFTER DELETE ON contacts BEGIN UPDATE retrieval_state SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER retrieval_contact_aliases_insert AFTER INSERT ON contact_aliases BEGIN UPDATE retrieval_state SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER retrieval_contact_aliases_update AFTER UPDATE ON contact_aliases BEGIN UPDATE retrieval_state SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER retrieval_contact_aliases_delete AFTER DELETE ON contact_aliases BEGIN UPDATE retrieval_state SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER retrieval_starred_insert AFTER INSERT ON starred BEGIN UPDATE retrieval_state SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER retrieval_starred_update AFTER UPDATE ON starred BEGIN UPDATE retrieval_state SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER retrieval_starred_delete AFTER DELETE ON starred BEGIN UPDATE retrieval_state SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER retrieval_message_locations_insert AFTER INSERT ON message_locations BEGIN UPDATE retrieval_state SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER retrieval_message_locations_update AFTER UPDATE ON message_locations BEGIN UPDATE retrieval_state SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER retrieval_message_locations_delete AFTER DELETE ON message_locations BEGIN UPDATE retrieval_state SET revision=revision+1 WHERE id=1; END;
CREATE TABLE change_state(id INTEGER PRIMARY KEY CHECK(id=1), floor INTEGER NOT NULL DEFAULT 0, retained_count INTEGER NOT NULL DEFAULT 0, suppress INTEGER NOT NULL DEFAULT 0);
CREATE TABLE message_changes(sequence INTEGER PRIMARY KEY AUTOINCREMENT,recorded_at INTEGER NOT NULL,operation TEXT NOT NULL,chat_jid TEXT NOT NULL,msg_id TEXT NOT NULL,new_chat_jid TEXT,new_msg_id TEXT,reason TEXT NOT NULL DEFAULT '',message_ts INTEGER NOT NULL);
CREATE INDEX message_changes_recorded ON message_changes(recorded_at);
CREATE TRIGGER changes_removed AFTER DELETE ON message_changes BEGIN
 UPDATE change_state SET floor=max(floor,old.sequence),retained_count=retained_count-1 WHERE id=1; END;
CREATE TRIGGER changes_added AFTER INSERT ON message_changes BEGIN
 UPDATE change_state SET retained_count=retained_count+1 WHERE id=1;DELETE FROM message_changes WHERE recorded_at <= CAST(strftime('%s','now') AS INTEGER)-2592000;
 DELETE FROM message_changes WHERE sequence IN (
 SELECT sequence FROM message_changes ORDER BY sequence LIMIT max(0,(SELECT retained_count-100000 FROM change_state WHERE id=1))
 );END;
CREATE TRIGGER retrieval_messages_insert AFTER INSERT ON messages BEGIN UPDATE retrieval_state SET revision=revision+1 WHERE id=1;
   INSERT INTO message_changes(recorded_at,operation,chat_jid,msg_id,reason,message_ts)
   SELECT CAST(strftime('%s','now') AS INTEGER),CASE WHEN new.deleted_at IS NULL THEN 'upsert' ELSE 'delete' END,new.chat_jid,new.msg_id,COALESCE(new.deletion_reason,''),new.ts WHERE (SELECT suppress FROM change_state WHERE id=1)=0; END;
CREATE TRIGGER retrieval_messages_update AFTER UPDATE ON messages WHEN old.rowid IS NOT new.rowid OR old.chat_jid IS NOT new.chat_jid OR old.chat_name IS NOT new.chat_name OR old.msg_id IS NOT new.msg_id OR old.sender_jid IS NOT new.sender_jid OR old.sender_name IS NOT new.sender_name OR old.ts IS NOT new.ts OR old.from_me IS NOT new.from_me OR old.text IS NOT new.text OR old.display_text IS NOT new.display_text OR old.quoted_msg_id IS NOT new.quoted_msg_id OR old.quoted_sender_jid IS NOT new.quoted_sender_jid OR old.is_forwarded IS NOT new.is_forwarded OR old.forwarding_score IS NOT new.forwarding_score OR old.reaction_to_id IS NOT new.reaction_to_id OR old.reaction_emoji IS NOT new.reaction_emoji OR old.media_type IS NOT new.media_type OR old.media_caption IS NOT new.media_caption OR old.filename IS NOT new.filename OR old.mime_type IS NOT new.mime_type OR old.direct_path IS NOT new.direct_path OR old.media_key IS NOT new.media_key OR old.file_sha256 IS NOT new.file_sha256 OR old.file_enc_sha256 IS NOT new.file_enc_sha256 OR old.file_length IS NOT new.file_length OR old.local_path IS NOT new.local_path OR old.downloaded_at IS NOT new.downloaded_at OR old.media_unavailable_at IS NOT new.media_unavailable_at OR old.revoked IS NOT new.revoked OR old.deleted_for_me IS NOT new.deleted_for_me OR old.deletion_reason IS NOT new.deletion_reason OR old.payload_purged_at IS NOT new.payload_purged_at OR old.edited IS NOT new.edited OR old.buttons IS NOT new.buttons OR (old.deleted_at IS NULL) != (new.deleted_at IS NULL) BEGIN UPDATE retrieval_state SET revision=revision+1 WHERE id=1;
   INSERT INTO message_changes(recorded_at,operation,chat_jid,msg_id,new_chat_jid,new_msg_id,reason,message_ts)
   SELECT CAST(strftime('%s','now') AS INTEGER),'identity_move',old.chat_jid,old.msg_id,new.chat_jid,new.msg_id,'identity_normalization',new.ts
   WHERE (old.chat_jid IS NOT new.chat_jid OR old.msg_id IS NOT new.msg_id) AND (SELECT suppress FROM change_state WHERE id=1)=0;
   INSERT INTO message_changes(recorded_at,operation,chat_jid,msg_id,reason,message_ts)
   SELECT CAST(strftime('%s','now') AS INTEGER),CASE WHEN new.deleted_at IS NULL THEN 'upsert' ELSE 'delete' END,new.chat_jid,new.msg_id,CASE WHEN new.payload_purged_at IS NOT NULL THEN 'hard_purge' ELSE COALESCE(new.deletion_reason,'') END,new.ts WHERE (SELECT suppress FROM change_state WHERE id=1)=0 AND old.chat_jid IS new.chat_jid AND old.msg_id IS new.msg_id; END;
CREATE TRIGGER retrieval_messages_delete AFTER DELETE ON messages BEGIN UPDATE retrieval_state SET revision=revision+1 WHERE id=1;
   INSERT INTO message_changes(recorded_at,operation,chat_jid,msg_id,reason,message_ts)
   SELECT CAST(strftime('%s','now') AS INTEGER),'delete',old.chat_jid,old.msg_id,COALESCE(NULLIF(old.deletion_reason,''),'local_removal'),old.ts WHERE (SELECT suppress FROM change_state WHERE id=1)=0; END;
CREATE TRIGGER changes_payload_purge AFTER INSERT ON message_payload_purges BEGIN
 DELETE FROM message_changes WHERE chat_jid=new.chat_jid AND msg_id=new.msg_id;
 END;
CREATE TRIGGER retrieval_message_receipts_insert AFTER INSERT ON message_receipts BEGIN UPDATE retrieval_state SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER retrieval_message_receipts_update AFTER UPDATE ON message_receipts BEGIN UPDATE retrieval_state SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER retrieval_message_receipts_delete AFTER DELETE ON message_receipts BEGIN UPDATE retrieval_state SET revision=revision+1 WHERE id=1; END;
CREATE TABLE app_state_full_sync_broken (
			collection TEXT PRIMARY KEY,
			marked_at INTEGER NOT NULL
		);
INSERT INTO schema_migrations VALUES(1,'core schema',1792446158);
INSERT INTO schema_migrations VALUES(2,'messages display_text column',1792446158);
INSERT INTO schema_migrations VALUES(3,'messages fts',1792446158);
INSERT INTO schema_migrations VALUES(4,'groups left_at column',1792446158);
INSERT INTO schema_migrations VALUES(5,'messages forwarded columns',1792446158);
INSERT INTO schema_migrations VALUES(6,'messages reaction columns',1792446158);
INSERT INTO schema_migrations VALUES(7,'starred messages',1792446158);
INSERT INTO schema_migrations VALUES(8,'messages revoked column',1792446158);
INSERT INTO schema_migrations VALUES(9,'messages deleted_for_me column',1792446158);
INSERT INTO schema_migrations VALUES(10,'chat state columns',1792446158);
INSERT INTO schema_migrations VALUES(11,'group hierarchy columns',1792446158);
INSERT INTO schema_migrations VALUES(12,'contacts system_name column',1792446158);
INSERT INTO schema_migrations VALUES(13,'messages buttons column',1792446158);
INSERT INTO schema_migrations VALUES(14,'polls and poll_votes',1792446158);
INSERT INTO schema_migrations VALUES(15,'call events',1792446158);
INSERT INTO schema_migrations VALUES(16,'messages edited columns',1792446158);
INSERT INTO schema_migrations VALUES(17,'status messages',1792446158);
INSERT INTO schema_migrations VALUES(18,'chat unread count column',1792446158);
INSERT INTO schema_migrations VALUES(19,'messages quoted columns',1792446158);
INSERT INTO schema_migrations VALUES(20,'messages media_unavailable_at column',1792446158);
INSERT INTO schema_migrations VALUES(21,'message tombstone metadata',1792446158);
INSERT INTO schema_migrations VALUES(22,'message local media aliases',1792446158);
INSERT INTO schema_migrations VALUES(23,'app state recovery markers',1792446158);
INSERT INTO schema_migrations VALUES(24,'app state recovery intents',1792446158);
INSERT INTO schema_migrations VALUES(25,'message locations',1792446158);
INSERT INTO schema_migrations VALUES(26,'message identity indexes and selective fts updates',1792446158);
INSERT INTO schema_migrations VALUES(27,'repair placeholder chat activity',1792446158);
INSERT INTO schema_migrations VALUES(28,'unavailable app state keys',1792446158);
INSERT INTO schema_migrations VALUES(29,'message receipts',1792446158);
INSERT INTO schema_migrations VALUES(30,'retrieval identity, revision and sync state',1792446158);
INSERT INTO schema_migrations VALUES(31,'content-free message change journal',1792446158);
INSERT INTO schema_migrations VALUES(32,'message receipt retrieval revisions',1792446158);
INSERT INTO retrieval_state VALUES(1,'e29cbea181f4434fcb2276ebe76d9ba3',0,NULL);
INSERT INTO change_state VALUES(1,0,0,0);
