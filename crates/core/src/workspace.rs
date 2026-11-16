use crate::{err, now, Repository, Result};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

pub const MODULES: &[&str] = &[
    "projects",
    "robotics",
    "agriculture",
    "poker",
    "crm",
    "companies",
    "learning",
    "ideas",
    "content",
    "contacts",
    "events",
    "assets",
    "watchlists",
    "mcp",
    "missions",
    "sessions",
    "notifications",
];
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub id: String,
    pub module: String,
    pub title: String,
    pub summary: String,
    pub status: String,
    pub pinned: bool,
    pub tags: Vec<String>,
    pub links: Vec<String>,
    pub fields: BTreeMap<String, String>,
    pub created_at: String,
    pub updated_at: String,
    pub device_id: String,
    pub revision: i64,
    pub deleted_at: Option<String>,
}
impl Record {
    pub fn new(module: &str, title: &str, device: &str) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            module: module.into(),
            title: title.into(),
            summary: String::new(),
            status: "draft".into(),
            pinned: false,
            tags: vec![],
            links: vec![],
            fields: BTreeMap::new(),
            created_at: now(),
            updated_at: now(),
            device_id: device.into(),
            revision: 1,
            deleted_at: None,
        }
    }
    pub fn validate(&self) -> Result<()> {
        Uuid::parse_str(&self.id).map_err(err)?;
        Uuid::parse_str(&self.device_id).map_err(err)?;
        if !MODULES.contains(&self.module.as_str())
            || self.title.trim().is_empty()
            || self.title.len() > 500
            || self.summary.len() > 100_000
            || self.status.len() > 80
            || self.revision < 1
        {
            return Err("Invalid workspace record".into());
        }
        if self.tags.len() > 40
            || self.tags.iter().any(|s| s.len() > 80)
            || self.links.len() > 200
            || self.fields.len() > 60
            || self
                .fields
                .iter()
                .any(|(k, v)| k.len() > 80 || v.len() > 20000)
        {
            return Err("Record exceeds workspace limits".into());
        }
        for id in &self.links {
            Uuid::parse_str(id).map_err(err)?;
        }
        chrono::DateTime::parse_from_rfc3339(&self.created_at).map_err(err)?;
        chrono::DateTime::parse_from_rfc3339(&self.updated_at).map_err(err)?;
        if let Some(d) = &self.deleted_at {
            chrono::DateTime::parse_from_rfc3339(d).map_err(err)?;
        }
        Ok(())
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Change {
    pub event_id: String,
    pub entity_id: String,
    pub device_id: String,
    pub revision: i64,
    pub operation: String,
    pub timestamp: String,
    pub payload: String,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortableWorkspace {
    #[serde(default)]
    pub discovery_sources: Option<serde_json::Value>,
    #[serde(default)]
    pub attachments: Vec<crate::attachments::Attachment>,
    pub records: Vec<Record>,
    pub history: Vec<Change>,
    #[serde(default)]
    pub preferences: Option<serde_json::Value>,
    #[serde(default)]
    pub feeds: Vec<CachedFeed>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct CachedFeed {
    pub source: String,
    pub fetched_at: String,
    pub items: serde_json::Value,
}
impl Repository {
    pub fn records(&self, query: &str, deleted: bool) -> Result<Vec<Record>> {
        let term = query
            .split_whitespace()
            .map(|s| format!("\"{}\"*", s.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(" AND ");
        let sql = format!(
            "SELECT r.* FROM records r {} WHERE {} {} ORDER BY pinned DESC,updated_at DESC",
            if term.is_empty() {
                ""
            } else {
                "JOIN records_fts ON records_fts.rowid=r.rowid"
            },
            if term.is_empty() {
                "?1=''"
            } else {
                "records_fts MATCH ?1"
            },
            if deleted {
                ""
            } else {
                "AND deleted_at IS NULL"
            }
        );
        let mut stmt = self.conn.prepare(&sql).map_err(err)?;
        let rows = stmt
            .query_map([term], |r| {
                let tags: String = r.get(6)?;
                let links: String = r.get(7)?;
                let fields: String = r.get(8)?;
                Ok(Record {
                    id: r.get(0)?,
                    module: r.get(1)?,
                    title: r.get(2)?,
                    summary: r.get(3)?,
                    status: r.get(4)?,
                    pinned: r.get(5)?,
                    tags: serde_json::from_str(&tags).unwrap_or_default(),
                    links: serde_json::from_str(&links).unwrap_or_default(),
                    fields: serde_json::from_str(&fields).unwrap_or_default(),
                    created_at: r.get(9)?,
                    updated_at: r.get(10)?,
                    device_id: r.get(11)?,
                    revision: r.get(12)?,
                    deleted_at: r.get(13)?,
                })
            })
            .map_err(err)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(err)
    }
    pub fn save_record(&mut self, mut record: Record) -> Result<Record> {
        record.title = record.title.trim().into();
        if record.module == "watchlists"
            && record.fields.get("monitor").map(String::as_str) == Some("enabled")
        {
            let field = |k: &str| record.fields.get(k).map(String::as_str).unwrap_or("");
            if !["stars", "downloads"].contains(&field("metric"))
                || !["at_least", "at_most"].contains(&field("operator"))
                || !field("target")
                    .parse::<f64>()
                    .is_ok_and(|n| n.is_finite() && n >= 0.0)
            {
                return Err("Enabled watchlists require stars/downloads, at_least/at_most and a finite nonnegative numeric target".into());
            }
            let url = url::Url::parse(field("source"))
                .map_err(|_| "Watchlist source must be an exact discovery URL")?;
            let host = if field("metric") == "stars" {
                "github.com"
            } else {
                "huggingface.co"
            };
            if url.scheme() != "https"
                || url.host_str() != Some(host)
                || !url.username().is_empty()
                || url.password().is_some()
            {
                return Err(
                    "Watchlist metric must match its GitHub or Hugging Face HTTPS source".into(),
                );
            }
        }

        if let Some(owner) = record
            .fields
            .get("workspace_owner")
            .filter(|id| !id.is_empty())
            .cloned()
        {
            let records = self.records("", false)?;
            let parent = records
                .iter()
                .find(|r| r.id == owner)
                .ok_or("Workspace owner is unavailable; clear or replace it")?;
            if !["projects", "companies"].contains(&parent.module.as_str()) {
                return Err("Owner must be a project or company".into());
            }
            let mut seen = std::collections::HashSet::from([record.id.clone()]);
            let mut current = Some(owner.clone());
            while let Some(id) = current {
                if !seen.insert(id.clone()) {
                    return Err("Workspace ownership cannot form a cycle".into());
                }
                current = records
                    .iter()
                    .find(|r| r.id == id)
                    .and_then(|r| r.fields.get("workspace_owner"))
                    .filter(|s| !s.is_empty())
                    .cloned();
            }
            if !record.links.contains(&owner) {
                record.links.push(owner);
            }
        }
        record.validate()?;
        let tx = self.conn.transaction().map_err(err)?;
        let old: Option<(i64, String, String)> = tx
            .query_row(
                "SELECT revision,module,created_at FROM records WHERE id=?1",
                [&record.id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(err)?;
        if let Some((revision, module, created)) = old {
            if revision != record.revision || module != record.module {
                return Err("Record changed; reload before saving".into());
            }
            record.revision += 1;
            record.created_at = created;
        } else {
            record.revision = 1;
            record.created_at = now();
        }
        record.updated_at = now();
        record.device_id = self.device_id.clone();
        write_record(&tx, &record)?;
        tx.commit().map_err(err)?;
        Ok(record)
    }
    pub fn history(&self) -> Result<Vec<Change>> {
        self.history_limit(-1)
    }
    pub fn recent_history(&self) -> Result<Vec<Change>> {
        self.history_limit(300)
    }
    fn history_limit(&self, limit: i64) -> Result<Vec<Change>> {
        let mut stmt=self.conn.prepare("SELECT event_id,entity_id,device_id,revision,operation,timestamp,payload FROM changes ORDER BY sequence DESC LIMIT ?1").map_err(err)?;
        let rows = stmt
            .query_map([limit], |r| {
                Ok(Change {
                    event_id: r.get(0)?,
                    entity_id: r.get(1)?,
                    device_id: r.get(2)?,
                    revision: r.get(3)?,
                    operation: r.get(4)?,
                    timestamp: r.get(5)?,
                    payload: r.get(6)?,
                })
            })
            .map_err(err)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(err)
    }
    pub fn portable_workspace(&self) -> Result<PortableWorkspace> {
        Ok(PortableWorkspace {
            attachments: self.attachments(None, "", true)?,
            records: self.records("", true)?,
            history: self.history()?,
            preferences: self
                .setting("preferences")?
                .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                .map(|v| {
                    let mut clean = serde_json::Map::new();
                    for key in [
                        "quiet_start",
                        "quiet_end",
                        "enabled_modules",
                        "dashboard_widgets",
                        "interests",
                        "backup_reminder_days",
                        "ui_scale",
                        "theme",
                        "high_contrast",
                        "reduce_motion",
                    ] {
                        if let Some(value) = v.get(key) {
                            clean.insert(key.into(), value.clone());
                        }
                    }
                    serde_json::Value::Object(clean)
                }),
            discovery_sources: self.portable_discovery_sources()?,
            feeds: self.feeds()?,
        })
    }
    pub fn import_workspace(
        &mut self,
        items: &[crate::Item],
        workspace: &PortableWorkspace,
    ) -> Result<usize> {
        self.import_snapshot(items, workspace, &[])
    }
    pub fn import_snapshot(
        &mut self,
        items: &[crate::Item],
        workspace: &PortableWorkspace,
        vault: &[(String, Vec<u8>)],
    ) -> Result<usize> {
        for item in items {
            item.validate()?;
        }
        for record in &workspace.records {
            record.validate()?;
        }
        for event in &workspace.history {
            Uuid::parse_str(&event.event_id).map_err(err)?;
            Uuid::parse_str(&event.entity_id).map_err(err)?;
            Uuid::parse_str(&event.device_id).map_err(err)?;
            chrono::DateTime::parse_from_rfc3339(&event.timestamp).map_err(err)?;
            if event.payload.len() > 250_000
                || !["upsert", "delete"].contains(&event.operation.as_str())
            {
                return Err("Invalid history event".into());
            }
        }
        validate_portable_extras(workspace)?;
        for a in &workspace.attachments {
            a.validate()?;
        }
        let tx = self.conn.transaction().map_err(err)?;
        if let Some(preferences) = &workspace.preferences {
            tx.execute(
                "INSERT OR IGNORE INTO settings(key,value) VALUES('preferences',?1)",
                [preferences.to_string()],
            )
            .map_err(err)?;
        }
        if let Some(sources) = &workspace.discovery_sources {
            tx.execute(
                "INSERT OR IGNORE INTO settings(key,value) VALUES('discovery_hub_streams',?1)",
                [sources.to_string()],
            )
            .map_err(err)?;
        }
        for feed in &workspace.feeds {
            tx.execute(
                "INSERT OR IGNORE INTO feed_cache VALUES(?1,?2,?3)",
                params![feed.source, feed.fetched_at, feed.items.to_string()],
            )
            .map_err(err)?;
        }
        let mut count = 0;
        for i in items {
            let exists: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM items WHERE id=?1)",
                    [&i.id],
                    |r| r.get(0),
                )
                .map_err(err)?;
            if !exists {
                crate::write(&tx, i)?;
                count += 1;
            }
        }
        for r in &workspace.records {
            let exists: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM records WHERE id=?1)",
                    [&r.id],
                    |row| row.get(0),
                )
                .map_err(err)?;
            if !exists {
                write_record(&tx, r)?;
                count += 1;
            }
        }
        for a in &workspace.attachments {
            count += crate::attachments::insert(&tx, a)?;
        }
        for e in &workspace.history {
            tx.execute("INSERT OR IGNORE INTO changes(event_id,entity_id,device_id,revision,operation,timestamp,payload) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![e.event_id,e.entity_id,e.device_id,e.revision,e.operation,e.timestamp,e.payload]).map_err(err)?;
        }
        for (id, bytes) in vault {
            check_owner(&tx, id, "vault")?;
            Uuid::parse_str(id).map_err(err)?;
            count += tx
                .execute(
                    "INSERT OR IGNORE INTO vault VALUES(?1,?2,?3)",
                    params![id, bytes, now()],
                )
                .map_err(err)?;
        }
        tx.commit().map_err(err)?;
        Ok(count)
    }
    pub fn setting(&self, key: &str) -> Result<Option<String>> {
        self.conn
            .query_row("SELECT value FROM settings WHERE key=?1", [key], |r| {
                r.get(0)
            })
            .optional()
            .map_err(err)
    }
    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute("INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![key,value]).map_err(err)?;
        Ok(())
    }
    pub fn bind_folder(&self, id: &str, path: &str) -> Result<()> {
        if !self
            .records("", false)?
            .iter()
            .any(|r| r.module == "projects" && r.id == id)
        {
            return Err("Project not found".into());
        }
        self.conn.execute("INSERT INTO local_bindings VALUES(?1,?2,?3) ON CONFLICT(project_id) DO UPDATE SET path=excluded.path,approved_at=excluded.approved_at",params![id,path,now()]).map_err(err)?;
        Ok(())
    }
    pub fn folder(&self, id: &str) -> Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT path FROM local_bindings WHERE project_id=?1",
                [id],
                |r| r.get(0),
            )
            .optional()
            .map_err(err)
    }
    pub fn revoke_folder(&self, id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM local_bindings WHERE project_id=?1", [id])
            .map_err(err)?;
        Ok(())
    }
    pub fn remove_cached_feed(&self, source: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM feed_cache WHERE source=?1", [source])
            .map_err(err)?;
        Ok(())
    }
    pub fn cache_feed(&self, source: &str, items: &serde_json::Value) -> Result<CachedFeed> {
        let fetched_at = now();
        self.conn.execute("INSERT INTO feed_cache VALUES(?1,?2,?3) ON CONFLICT(source) DO UPDATE SET fetched_at=excluded.fetched_at,payload=excluded.payload",params![source,fetched_at,items.to_string()]).map_err(err)?;
        Ok(CachedFeed {
            source: source.into(),
            fetched_at,
            items: items.clone(),
        })
    }
    pub fn feeds(&self) -> Result<Vec<CachedFeed>> {
        let mut stmt = self
            .conn
            .prepare("SELECT source,fetched_at,payload FROM feed_cache")
            .map_err(err)?;
        let rows = stmt
            .query_map([], |r| {
                let payload: String = r.get(2)?;
                Ok(CachedFeed {
                    source: r.get(0)?,
                    fetched_at: r.get(1)?,
                    items: serde_json::from_str(&payload).unwrap_or_default(),
                })
            })
            .map_err(err)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(err)
    }
    pub fn vault_rows(&self) -> Result<Vec<(String, Vec<u8>)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id,ciphertext FROM vault")
            .map_err(err)?;
        let rows = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(err)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(err)
    }
    pub fn vault_ciphertext(&self, id: &str) -> Result<Option<Vec<u8>>> {
        self.conn
            .query_row("SELECT ciphertext FROM vault WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .optional()
            .map_err(err)
    }
    pub fn save_vault_if_unchanged(
        &mut self,
        id: &str,
        ciphertext: &[u8],
        expected: Option<&[u8]>,
    ) -> Result<()> {
        Uuid::parse_str(id).map_err(err)?;
        let tx = self.conn.transaction().map_err(err)?;
        check_owner(&tx, id, "vault")?;
        let actual: Option<Vec<u8>> = tx
            .query_row("SELECT ciphertext FROM vault WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .optional()
            .map_err(err)?;
        if actual.as_deref() != expected {
            return Err("Private record changed; reload before saving".into());
        }
        tx.execute("INSERT INTO vault VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET ciphertext=excluded.ciphertext,updated_at=excluded.updated_at",params![id,ciphertext,now()]).map_err(err)?;
        tx.commit().map_err(err)
    }
    pub fn save_vault(&self, id: &str, ciphertext: &[u8]) -> Result<()> {
        check_owner(&self.conn, id, "vault")?;
        Uuid::parse_str(id).map_err(err)?;
        self.conn.execute("INSERT INTO vault VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET ciphertext=excluded.ciphertext,updated_at=excluded.updated_at",params![id,ciphertext,now()]).map_err(err)?;
        Ok(())
    }
    pub fn save_vault_batch(&mut self, rows: &[(String, Vec<u8>)]) -> Result<()> {
        let tx = self.conn.transaction().map_err(err)?;
        for (id, ciphertext) in rows {
            check_owner(&tx, id, "vault")?;
            Uuid::parse_str(id).map_err(err)?;
            tx.execute(
                "INSERT INTO vault VALUES(?1,?2,?3)",
                params![id, ciphertext, now()],
            )
            .map_err(err)?;
        }
        tx.commit().map_err(err)
    }
    pub fn recent_deliveries(&self) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM notification_delivery ORDER BY delivered_at DESC LIMIT 128")
            .map_err(err)?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0)).map_err(err)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(err)
    }
    pub fn delivery_exists(&self, id: &str) -> Result<bool> {
        self.conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM notification_delivery WHERE id=?1)",
                [id],
                |r| r.get(0),
            )
            .map_err(err)
    }
    pub fn mark_delivered(&self, id: &str) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR IGNORE INTO notification_delivery VALUES(?1,?2)",
                params![id, now()],
            )
            .map_err(err)?;
        Ok(())
    }
}
pub(crate) fn write_record(tx: &rusqlite::Transaction<'_>, r: &Record) -> Result<()> {
    check_owner(tx, &r.id, "record")?;
    tx.execute("INSERT INTO records VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14) ON CONFLICT(id) DO UPDATE SET title=excluded.title,summary=excluded.summary,status=excluded.status,pinned=excluded.pinned,tags=excluded.tags,links=excluded.links,fields=excluded.fields,updated_at=excluded.updated_at,device_id=excluded.device_id,revision=excluded.revision,deleted_at=excluded.deleted_at",params![r.id,r.module,r.title,r.summary,r.status,r.pinned,serde_json::to_string(&r.tags).map_err(err)?,serde_json::to_string(&r.links).map_err(err)?,serde_json::to_string(&r.fields).map_err(err)?,r.created_at,r.updated_at,r.device_id,r.revision,r.deleted_at]).map_err(err)?;
    tx.execute("INSERT INTO changes(event_id,entity_id,device_id,revision,operation,timestamp,payload) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![Uuid::new_v4().to_string(),r.id,r.device_id,r.revision,if r.deleted_at.is_some(){"delete"}else{"upsert"},now(),serde_json::to_string(r).map_err(err)?]).map_err(err)?;
    Ok(())
}
impl Repository {
    pub fn reset(&mut self) -> Result<()> {
        self.conn
            .execute_batch("PRAGMA secure_delete=ON;")
            .map_err(err)?;
        let tx = self.conn.transaction().map_err(err)?;
        tx.execute_batch("INSERT INTO items_fts(items_fts) VALUES('delete-all'); INSERT INTO records_fts(records_fts) VALUES('delete-all'); DELETE FROM attachments; DELETE FROM attachments_fts; DELETE FROM items; DELETE FROM records; DELETE FROM changes; DELETE FROM local_bindings; DELETE FROM feed_cache; DELETE FROM vault; DELETE FROM notification_delivery; DELETE FROM sync_cursors; DELETE FROM sync_heads; DELETE FROM sync_events; DELETE FROM sync_seen; DELETE FROM sync_conflicts; DELETE FROM peer_progress; DELETE FROM embeddings; DELETE FROM settings WHERE key != 'device_id';").map_err(err)?;
        tx.commit().map_err(err)?;
        self.conn
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE); VACUUM;")
            .map_err(err)?;
        Ok(())
    }
}
impl Repository {
    pub fn put_embedding(
        &self,
        id: &str,
        model: &str,
        updated_at: &str,
        vector: &[f32],
    ) -> Result<()> {
        if vector.is_empty() || vector.len() > 8192 || vector.iter().any(|v| !v.is_finite()) {
            return Err("Invalid embedding vector".into());
        }
        self.conn.execute("INSERT INTO embeddings VALUES(?1,?2,?3,?4) ON CONFLICT(entity_id,model) DO UPDATE SET updated_at=excluded.updated_at,vector=excluded.vector",params![id,model,updated_at,serde_json::to_string(vector).map_err(err)?]).map_err(err)?;
        Ok(())
    }
    pub fn embeddings(&self, model: &str) -> Result<Vec<(String, String, Vec<f32>)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT entity_id,updated_at,vector FROM embeddings WHERE model=?1")
            .map_err(err)?;
        let rows = stmt
            .query_map([model], |r| {
                let raw: String = r.get(2)?;
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    serde_json::from_str(&raw).unwrap_or_default(),
                ))
            })
            .map_err(err)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(err)
    }
}

pub(crate) fn check_owner(conn: &rusqlite::Connection, id: &str, kind: &str) -> Result<()> {
    let owner:Option<String>=conn.query_row("SELECT 'item' FROM items WHERE id=?1 UNION ALL SELECT 'record' FROM records WHERE id=?1 UNION ALL SELECT 'vault' FROM vault WHERE id=?1 UNION ALL SELECT 'attachment' FROM attachments WHERE id=?1 LIMIT 1",[id],|r|r.get(0)).optional().map_err(err)?;
    if owner.as_deref().is_some_and(|owner| owner != kind) {
        return Err("Record ID belongs to a different entity type".into());
    }
    Ok(())
}

impl Repository {
    /// Create tasks and link them to the parent in one transaction; stale parents cannot duplicate work.
    pub fn plan_tasks(
        &mut self,
        id: &str,
        revision: i64,
        titles: &[String],
        due: Option<String>,
    ) -> Result<usize> {
        if titles.is_empty() || titles.len() > 30 {
            return Err("Choose between 1 and 30 tasks".into());
        }
        let mut parent = self
            .records("", false)?
            .into_iter()
            .find(|r| r.id == id)
            .ok_or("Workspace no longer exists")?;
        if parent.revision != revision {
            return Err("Workspace changed; reload before creating tasks".into());
        }
        let mut tasks = vec![];
        for title in titles {
            let item = crate::Item {
                id: Uuid::new_v4().to_string(),
                kind: "task".into(),
                title: title.trim().into(),
                body: format!("From workspace: {}\nRecord: {}", parent.title, parent.id),
                pinned: false,
                priority: "normal".into(),
                status: "active".into(),
                due: due.clone(),
                recurrence: "none".into(),
                url: None,
                created_at: now(),
                updated_at: now(),
                device_id: self.device_id.clone(),
                revision: 1,
                deleted_at: None,
            };
            item.validate()?;
            parent.links.push(item.id.clone());
            tasks.push(item);
        }
        parent.revision += 1;
        parent.updated_at = now();
        parent.device_id = self.device_id.clone();
        parent.validate()?;
        let tx = self.conn.transaction().map_err(err)?;
        for item in &tasks {
            crate::write(&tx, item)?;
        }
        write_record(&tx, &parent)?;
        tx.commit().map_err(err)?;
        Ok(tasks.len())
    }
}
impl Repository {
    pub fn save_workspace_document(
        &mut self,
        id: &str,
        revision: i64,
        title: String,
        body: String,
    ) -> Result<Record> {
        let mut parent = self
            .records("", false)?
            .into_iter()
            .find(|r| r.id == id)
            .ok_or("Workspace no longer exists")?;
        if parent.revision != revision {
            return Err("Workspace changed; reload before saving draft".into());
        }
        let item = crate::Item {
            id: Uuid::new_v4().to_string(),
            kind: "note".into(),
            title,
            body,
            pinned: false,
            priority: "normal".into(),
            status: "active".into(),
            due: None,
            recurrence: "none".into(),
            url: None,
            created_at: now(),
            updated_at: now(),
            device_id: self.device_id.clone(),
            revision: 1,
            deleted_at: None,
        };
        item.validate()?;
        parent.links.push(item.id.clone());
        parent.revision += 1;
        parent.updated_at = now();
        parent.device_id = self.device_id.clone();
        parent.validate()?;
        let tx = self.conn.transaction().map_err(err)?;
        crate::write(&tx, &item)?;
        write_record(&tx, &parent)?;
        tx.commit().map_err(err)?;
        Ok(parent)
    }
}

pub fn validate_portable_extras(workspace: &PortableWorkspace) -> Result<()> {
    if workspace.attachments.len() > 1000
        || workspace
            .attachments
            .iter()
            .map(|a| a.content_base64.len())
            .sum::<usize>()
            > 64 * 1024 * 1024
    {
        return Err("Attachment archive limits exceeded".into());
    }

    if let Some(preferences) = &workspace.preferences {
        let object = preferences
            .as_object()
            .ok_or("Invalid portable preferences")?;
        for (key, value) in object {
            let valid = match key.as_str() {
                "quiet_start" | "quiet_end" => value.as_u64().is_some_and(|n| n <= 23),
                "ui_scale" => value
                    .as_u64()
                    .is_some_and(|n| [100, 115, 130, 150, 175, 200].contains(&n)),
                "theme" => value
                    .as_str()
                    .is_some_and(|v| ["mono", "lumen", "slate", "midnight"].contains(&v)),
                "high_contrast" | "reduce_motion" => value.is_boolean(),
                "backup_reminder_days" => value.as_u64().is_some_and(|n| n <= 365),
                "enabled_modules" => value.as_array().is_some_and(|v| {
                    v.len() <= MODULES.len()
                        && v.iter()
                            .all(|s| s.as_str().is_some_and(|s| MODULES.contains(&s)))
                }),
                "dashboard_widgets" => value.as_array().is_some_and(|v| {
                    v.len() <= 7
                        && v.iter().all(|s| {
                            s.as_str().is_some_and(|s| {
                                [
                                    "capture", "summary", "pins", "tasks", "dates", "briefs",
                                    "feeds",
                                ]
                                .contains(&s)
                            })
                        })
                }),
                "interests" => value.as_array().is_some_and(|v| {
                    v.len() <= 20 && v.iter().all(|s| s.as_str().is_some_and(|s| s.len() <= 80))
                }),
                _ => false,
            };
            if !valid {
                return Err("Portable preferences contain an unsupported key or value".into());
            }
        }
    }
    if let Some(sources) = &workspace.discovery_sources {
        validate_discovery_sources(sources)?;
    }
    if workspace.feeds.len() > 73 {
        return Err("Too many feed caches".into());
    }
    let mut sources = std::collections::HashSet::new();
    for feed in &workspace.feeds {
        let hub = feed.source.strip_prefix("hub:").is_some_and(|s| {
            !s.is_empty()
                && s.len() <= 80
                && s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
        });
        if !(hub
            || [
                "github", "models", "ollama", "news", "mcp", "apps", "youtube", "arxiv", "rss",
            ]
            .contains(&feed.source.as_str()))
            || !sources.insert(&feed.source)
        {
            return Err("Invalid cached feed source".into());
        }
        chrono::DateTime::parse_from_rfc3339(&feed.fetched_at).map_err(err)?;
        let items = feed.items.as_array().ok_or("Invalid cached feed items")?;
        if items.len()
            > if feed.source == "hub:imported-links" {
                1000
            } else {
                100
            }
        {
            return Err("Cached feed exceeds item limit".into());
        }
        for item in items {
            for key in [
                "id",
                "title",
                "description",
                "url",
                "source",
                "metric",
                "fetched_at",
                "verification",
            ] {
                if !item[key].as_str().is_some_and(|s| s.len() <= 20000) {
                    return Err("Invalid cached discovery".into());
                }
            }
            let url = url::Url::parse(item["url"].as_str().unwrap_or("")).map_err(err)?;
            if !["http", "https"].contains(&url.scheme())
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
            {
                return Err("Unsafe discovery URL".into());
            }
            if !item["published_at"].is_null() && !item["published_at"].is_string() {
                return Err("Invalid discovery date".into());
            }
            if let Some(trend) = item.get("trend") {
                for key in ["delta", "hours", "per_day"] {
                    if !trend[key].is_number() {
                        return Err("Invalid trend observation".into());
                    }
                }
                for key in ["observed_from", "observed_to"] {
                    chrono::DateTime::parse_from_rfc3339(
                        trend[key].as_str().ok_or("Invalid trend date")?,
                    )
                    .map_err(err)?;
                }
            }
        }
    }
    Ok(())
}

impl Repository {
    pub fn portable_discovery_sources(&self) -> Result<Option<serde_json::Value>> {
        let Some(raw) = self.setting("discovery_hub_streams")? else {
            return Ok(None);
        };
        let mut sources: serde_json::Value = serde_json::from_str(&raw).map_err(err)?;
        for source in sources.as_array_mut().ok_or("Invalid discovery sources")? {
            source["enabled"] = serde_json::json!(false);
        }
        validate_discovery_sources(&sources)?;
        Ok(Some(sources))
    }
}
pub fn validate_discovery_sources(value: &serde_json::Value) -> Result<()> {
    let sources = value.as_array().ok_or("Invalid discovery sources")?;
    if sources.len() > 64 {
        return Err("Too many discovery sources".into());
    }
    let mut ids = std::collections::HashSet::new();
    for s in sources {
        let object = s.as_object().ok_or("Invalid discovery source")?;
        if object.keys().any(|k| {
            ![
                "id", "name", "topic", "provider", "query", "kind", "region", "enabled", "options",
            ]
            .contains(&k.as_str())
        }) {
            return Err("Unsupported discovery source field".into());
        }
        let id = s["id"].as_str().ok_or("Invalid discovery source ID")?;
        if id.is_empty()
            || id.len() > 80
            || !id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
            || !ids.insert(id)
            || s["enabled"] != false
        {
            return Err(
                "Portable sources must have unique IDs and remain disabled until reviewed".into(),
            );
        }
        for (k, max) in [
            ("name", 160),
            ("query", 2000),
            ("topic", 80),
            ("provider", 80),
            ("kind", 80),
            ("region", 80),
        ] {
            if !s[k].as_str().is_some_and(|s| s.len() <= max) {
                return Err("Invalid discovery source value".into());
            }
        }
        if ![
            "ai",
            "code",
            "robotics",
            "agriculture",
            "poker",
            "learning",
            "custom",
        ]
        .contains(&s["topic"].as_str().unwrap_or(""))
            || ![
                "github",
                "models",
                "ollama",
                "arxiv",
                "rss",
                "news",
                "youtube",
                "youtube_feed",
                "reddit",
                "bluesky",
                "mastodon",
                "events",
                "calendar",
                "x",
                "linkedin",
                "import",
                "mcp",
                "apps",
            ]
            .contains(&s["provider"].as_str().unwrap_or(""))
            || ![
                "models", "repos", "news", "papers", "videos", "social", "events",
            ]
            .contains(&s["kind"].as_str().unwrap_or(""))
        {
            return Err("Unsupported discovery source type".into());
        }
        if ["rss", "events", "calendar"].contains(&s["provider"].as_str().unwrap_or("")) {
            let u = url::Url::parse(s["query"].as_str().unwrap_or("")).map_err(err)?;
            if u.scheme() != "https"
                || u.host_str().is_none()
                || !u.username().is_empty()
                || u.password().is_some()
                || u.fragment().is_some()
            {
                return Err("Unsafe discovery source URL".into());
            }
        }
        let options = s["options"].as_object().ok_or("Invalid source options")?;
        for (key, value) in options {
            if key == "days" {
                if !value.as_u64().is_some_and(|n| n <= 365) {
                    return Err("Invalid discovery date window".into());
                }
            } else if !["sort", "language", "task", "license", "region", "duration"]
                .contains(&key.as_str())
                || !value.as_str().is_some_and(|s| s.len() <= 100)
            {
                return Err("Unsupported source option".into());
            }
        }
    }
    Ok(())
}
