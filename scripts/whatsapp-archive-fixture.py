#!/usr/bin/env python3
"""Keep the offline WAL fixture writer in the archive reader's Linux kernel."""

import json
from pathlib import Path
import sqlite3
import sys

schema, database = map(Path, sys.argv[1:])
writer = sqlite3.connect(database)
try:
    assert writer.execute("PRAGMA journal_mode=WAL").fetchone()[0] == "wal"
    writer.execute("PRAGMA wal_autocheckpoint=0")
    writer.executescript(schema.read_text())
    writer.execute("INSERT INTO chats(jid,kind,name) VALUES('1@s.whatsapp.net','dm','Offline fixture')")
    writer.execute(
        "INSERT INTO messages(chat_jid,msg_id,ts,from_me,text,direct_path,media_key,local_path) "
        "VALUES('1@s.whatsapp.net','first',1,0,'Offline archive smoke','privatepath',x'010203','/private/media')"
    )
    writer.commit()
    for suffix in ("", "-wal", "-shm"):
        Path(str(database) + suffix).chmod(0o644)
    print(json.dumps({"state": "ready"}), flush=True)
    if json.loads(sys.stdin.readline()) != {"command": "append"}:
        raise ValueError("Expected fixture append command")
    writer.execute(
        "INSERT INTO messages(chat_jid,msg_id,ts,from_me,text) "
        "VALUES('1@s.whatsapp.net','second',2,0,'Live WAL follow-up')"
    )
    writer.commit()
    print(json.dumps({"state": "append"}), flush=True)
    if json.loads(sys.stdin.readline()) != {"command": "close"}:
        raise ValueError("Expected fixture close command")
    writer.close()
    print(json.dumps({"state": "close"}), flush=True)
finally:
    writer.close()
