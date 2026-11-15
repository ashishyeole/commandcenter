CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS items (
 id TEXT PRIMARY KEY, kind TEXT NOT NULL CHECK(kind IN ('note','task','saved')),
 title TEXT NOT NULL, body TEXT NOT NULL, pinned INTEGER NOT NULL,
 priority TEXT NOT NULL, status TEXT NOT NULL, due TEXT, recurrence TEXT NOT NULL,
 url TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
 device_id TEXT NOT NULL, revision INTEGER NOT NULL, deleted_at TEXT
);
CREATE VIRTUAL TABLE IF NOT EXISTS items_fts USING fts5(title, body, content='items', content_rowid='rowid');
CREATE TRIGGER IF NOT EXISTS items_ai AFTER INSERT ON items BEGIN
 INSERT INTO items_fts(rowid,title,body) VALUES(new.rowid,new.title,new.body); END;
CREATE TRIGGER IF NOT EXISTS items_au AFTER UPDATE ON items BEGIN
 INSERT INTO items_fts(items_fts,rowid,title,body) VALUES('delete',old.rowid,old.title,old.body);
 INSERT INTO items_fts(rowid,title,body) VALUES(new.rowid,new.title,new.body); END;
CREATE TABLE IF NOT EXISTS changes (
 sequence INTEGER PRIMARY KEY AUTOINCREMENT, event_id TEXT UNIQUE NOT NULL,
 entity_id TEXT NOT NULL, device_id TEXT NOT NULL, revision INTEGER NOT NULL,
 operation TEXT NOT NULL, timestamp TEXT NOT NULL, payload TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS sync_cursors (peer_id TEXT PRIMARY KEY, sequence INTEGER NOT NULL);
PRAGMA user_version = 1;
