CREATE TABLE records (id TEXT PRIMARY KEY, module TEXT NOT NULL, title TEXT NOT NULL, summary TEXT NOT NULL, status TEXT NOT NULL, pinned INTEGER NOT NULL, tags TEXT NOT NULL, links TEXT NOT NULL, fields TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL, device_id TEXT NOT NULL, revision INTEGER NOT NULL, deleted_at TEXT);
CREATE VIRTUAL TABLE records_fts USING fts5(title,summary,tags,content='records',content_rowid='rowid');
CREATE TRIGGER records_ai AFTER INSERT ON records BEGIN INSERT INTO records_fts(rowid,title,summary,tags) VALUES(new.rowid,new.title,new.summary,new.tags); END;
CREATE TRIGGER records_au AFTER UPDATE ON records BEGIN INSERT INTO records_fts(records_fts,rowid,title,summary,tags) VALUES('delete',old.rowid,old.title,old.summary,old.tags); INSERT INTO records_fts(rowid,title,summary,tags) VALUES(new.rowid,new.title,new.summary,new.tags); END;
CREATE TABLE local_bindings (project_id TEXT PRIMARY KEY, path TEXT NOT NULL, approved_at TEXT NOT NULL);
CREATE TABLE feed_cache (source TEXT PRIMARY KEY, fetched_at TEXT NOT NULL, payload TEXT NOT NULL);
CREATE TABLE vault (id TEXT PRIMARY KEY, ciphertext BLOB NOT NULL, updated_at TEXT NOT NULL);
CREATE TABLE notification_delivery (id TEXT PRIMARY KEY, delivered_at TEXT NOT NULL);
PRAGMA user_version = 2;
