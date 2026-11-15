CREATE TABLE attachments(id TEXT PRIMARY KEY,owner_id TEXT NOT NULL,payload TEXT NOT NULL);
CREATE INDEX attachments_owner ON attachments(owner_id);
CREATE VIRTUAL TABLE attachments_fts USING fts5(id UNINDEXED,title,extracted_text);
PRAGMA user_version=5;
