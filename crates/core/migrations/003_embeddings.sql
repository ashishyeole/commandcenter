CREATE TABLE embeddings (entity_id TEXT NOT NULL, model TEXT NOT NULL, updated_at TEXT NOT NULL, vector TEXT NOT NULL, PRIMARY KEY(entity_id,model));
PRAGMA user_version = 3;
