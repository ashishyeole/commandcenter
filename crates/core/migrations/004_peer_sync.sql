CREATE TABLE sync_heads(entity_id TEXT PRIMARY KEY,kind TEXT NOT NULL,clock TEXT NOT NULL,payload TEXT NOT NULL);
CREATE TABLE sync_events(sequence INTEGER PRIMARY KEY AUTOINCREMENT,event_id TEXT UNIQUE NOT NULL,entity_id TEXT NOT NULL,kind TEXT NOT NULL,clock TEXT NOT NULL,base_clock TEXT NOT NULL,base_payload TEXT,payload TEXT NOT NULL);
CREATE TABLE sync_seen(event_id TEXT PRIMARY KEY,digest TEXT NOT NULL);
CREATE TABLE sync_conflicts(id TEXT PRIMARY KEY,peer_id TEXT NOT NULL,entity_id TEXT NOT NULL,event TEXT NOT NULL,local_payload TEXT NOT NULL,created_at TEXT NOT NULL,resolved_at TEXT);
CREATE TABLE peer_progress(peer_id TEXT PRIMARY KEY,sent INTEGER NOT NULL DEFAULT 0,received INTEGER NOT NULL DEFAULT 0,last_sync TEXT);
PRAGMA user_version=4;
