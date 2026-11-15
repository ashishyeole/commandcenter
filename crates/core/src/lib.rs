pub mod archive;
pub mod attachments;
pub mod briefing;
pub mod collections;
pub mod reminder_lease;
pub mod reminders;
pub mod retention;
pub mod sync;
pub mod vault_codec;
pub mod watchlists;
pub mod workspace;
use chrono::{Datelike, Days, Months, NaiveDate, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
pub type Result<T> = std::result::Result<T, String>;
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
pub fn now() -> String {
    Utc::now().to_rfc3339()
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Item {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub body: String,
    pub pinned: bool,
    pub priority: String,
    pub status: String,
    pub due: Option<String>,
    pub recurrence: String,
    pub url: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub device_id: String,
    pub revision: i64,
    pub deleted_at: Option<String>,
}
impl Item {
    pub fn validate(&self) -> Result<()> {
        if Uuid::parse_str(&self.id).is_err() || Uuid::parse_str(&self.device_id).is_err() {
            return Err("Invalid record or device ID".into());
        }
        if !["note", "task", "saved"].contains(&self.kind.as_str())
            || self.title.trim().is_empty()
            || self.title.len() > 500
            || self.body.len() > 100_000
        {
            return Err("Invalid type, empty title, or content too large".into());
        }
        if !["normal", "important", "critical"].contains(&self.priority.as_str())
            || !["active", "done", "waiting", "someday"].contains(&self.status.as_str())
            || !valid_recurrence(&self.recurrence)
        {
            return Err("Invalid task attributes".into());
        }
        if self.kind != "task"
            && (self.recurrence != "none" || self.due.is_some() || self.status != "active")
        {
            return Err("Only tasks have schedules or completion state".into());
        }
        if let Some(d) = &self.due {
            NaiveDate::parse_from_str(d, "%Y-%m-%d").map_err(err)?;
        }
        if self.recurrence != "none" && !self.recurrence.starts_with("after:") && self.due.is_none()
        {
            return Err("Recurring tasks need a due date".into());
        }
        if let Some((_, end)) = self.recurrence.split_once(";until:") {
            if self.due.as_deref().is_some_and(|due| due > end) {
                return Err("The first due date must not be after the recurrence end date".into());
            }
        }
        if let Some(u) = &self.url {
            let parsed = url::Url::parse(u).map_err(|_| "Invalid source URL")?;
            if u.len() > 4096
                || !["http", "https"].contains(&parsed.scheme())
                || parsed.host_str().is_none()
                || !parsed.username().is_empty()
                || parsed.password().is_some()
            {
                return Err("Use an HTTP or HTTPS URL without credentials".into());
            }
        }
        if self.revision < 1 {
            return Err("Invalid revision".into());
        }
        chrono::DateTime::parse_from_rfc3339(&self.created_at).map_err(err)?;
        chrono::DateTime::parse_from_rfc3339(&self.updated_at).map_err(err)?;
        if let Some(d) = &self.deleted_at {
            chrono::DateTime::parse_from_rfc3339(d).map_err(err)?;
        }
        Ok(())
    }
}
#[derive(Serialize)]
pub struct Snapshot {
    pub items: Vec<Item>,
    pub device_id: String,
    pub changes: i64,
}
pub struct Repository {
    conn: Connection,
    pub device_id: String,
}
impl Repository {
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<Self> {
        let conn = Connection::open(path).map_err(err)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(err)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .map_err(err)?;
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(err)?;
        if version > 5 {
            return Err("Database is newer than this application".into());
        }
        if version == 0 {
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE; {} COMMIT;",
                include_str!("../migrations/001_initial.sql")
            ))
            .map_err(err)?;
        }
        if version < 2 {
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE; {} COMMIT;",
                include_str!("../migrations/002_workspaces.sql")
            ))
            .map_err(err)?;
        }
        if version < 3 {
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE; {} COMMIT;",
                include_str!("../migrations/003_embeddings.sql")
            ))
            .map_err(err)?;
        }
        if version < 4 {
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE; {} COMMIT;",
                include_str!("../migrations/004_peer_sync.sql")
            ))
            .map_err(err)?;
        }
        if version < 5 {
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE; {} COMMIT;",
                include_str!("../migrations/005_attachments.sql")
            ))
            .map_err(err)?;
        }
        conn.execute(
            "INSERT OR IGNORE INTO settings VALUES('device_id',?1)",
            [Uuid::new_v4().to_string()],
        )
        .map_err(err)?;
        let device_id = conn
            .query_row(
                "SELECT value FROM settings WHERE key='device_id'",
                [],
                |r| r.get(0),
            )
            .map_err(err)?;
        Ok(Self { conn, device_id })
    }
    pub fn list(&self, query: &str, include_deleted: bool) -> Result<Vec<Item>> {
        let mut sql = "SELECT i.* FROM items i".to_owned();
        let phrase = query
            .split_whitespace()
            .map(|s| format!("\"{}\"*", s.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(" AND ");
        if !phrase.is_empty() {
            sql.push_str(" JOIN items_fts ON items_fts.rowid=i.rowid WHERE items_fts MATCH ?1");
        } else {
            sql.push_str(" WHERE ?1='' ");
        }
        if !include_deleted {
            sql.push_str(" AND i.deleted_at IS NULL");
        }
        sql.push_str(" ORDER BY i.pinned DESC, i.updated_at DESC");
        let mut stmt = self.conn.prepare(&sql).map_err(err)?;
        let rows = stmt
            .query_map([phrase], |r| {
                Ok(Item {
                    id: r.get(0)?,
                    kind: r.get(1)?,
                    title: r.get(2)?,
                    body: r.get(3)?,
                    pinned: r.get(4)?,
                    priority: r.get(5)?,
                    status: r.get(6)?,
                    due: r.get(7)?,
                    recurrence: r.get(8)?,
                    url: r.get(9)?,
                    created_at: r.get(10)?,
                    updated_at: r.get(11)?,
                    device_id: r.get(12)?,
                    revision: r.get(13)?,
                    deleted_at: r.get(14)?,
                })
            })
            .map_err(err)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(err)
    }
    pub fn snapshot(&self, query: &str) -> Result<Snapshot> {
        Ok(Snapshot {
            items: self.list(query, false)?,
            device_id: self.device_id.clone(),
            changes: self
                .conn
                .query_row("SELECT COUNT(*) FROM changes", [], |r| r.get(0))
                .map_err(err)?,
        })
    }
    pub fn save(&mut self, mut item: Item) -> Result<Item> {
        if let Some(parent) = item.recurrence.strip_prefix("after:") {
            let items = self.list("", false)?;
            let mut current = parent;
            let mut seen = std::collections::HashSet::new();
            seen.insert(item.id.as_str());
            loop {
                if !seen.insert(current) {
                    return Err("Task prerequisites cannot contain a cycle".into());
                }
                let task = items
                    .iter()
                    .find(|i| i.id == current && i.kind == "task")
                    .ok_or("Prerequisite task is missing")?;
                if let Some(next) = task.recurrence.strip_prefix("after:") {
                    current = next;
                } else {
                    break;
                }
            }
            item.status = "waiting".into();
        }

        item.title = item.title.trim().to_owned();
        item.validate()?;
        let tx = self.conn.transaction().map_err(err)?;
        let previous: Option<(i64, String, String)> = tx
            .query_row(
                "SELECT revision,created_at,kind FROM items WHERE id=?1",
                [&item.id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(err)?;
        if let Some((revision, created, kind)) = previous {
            if kind != item.kind {
                return Err("Record type cannot change".into());
            }
            if revision != item.revision {
                return Err("This item changed. Reload before saving.".into());
            }
            item.revision = revision + 1;
            item.created_at = created;
        } else {
            item.revision = 1;
            item.created_at = now();
        }
        item.updated_at = now();
        item.device_id = self.device_id.clone();
        write(&tx, &item)?;
        tx.commit().map_err(err)?;
        Ok(item)
    }
    pub fn activate_conditions(&mut self) -> Result<usize> {
        let items = self.list("", false)?;
        let mut changed = 0;
        for mut item in items
            .iter()
            .filter(|i| {
                i.kind == "task" && i.status == "waiting" && i.recurrence.starts_with("after:")
            })
            .cloned()
        {
            if items.iter().any(|p| {
                Some(p.id.as_str()) == item.recurrence.strip_prefix("after:")
                    && p.kind == "task"
                    && p.status == "done"
            }) {
                item.status = "active".into();
                item.recurrence = "none".into();
                if item.due.is_none() {
                    item.due = Some(chrono::Local::now().date_naive().to_string());
                }
                self.save(item)?;
                changed += 1;
            }
        }
        Ok(changed)
    }
    pub fn complete(&mut self, id: &str) -> Result<Item> {
        self.activate_conditions()?;
        let mut item = self
            .list("", false)?
            .into_iter()
            .find(|i| i.id == id)
            .ok_or("Task not found")?;
        if item.kind != "task" {
            return Err("Only tasks can be completed".into());
        }
        if item.recurrence.starts_with("after:") {
            return Err("This task is waiting for its prerequisite to reach Done".into());
        }
        if item.status == "done" {
            item.status = "active".into();
            return self.save(item);
        }
        if item.recurrence == "none" {
            item.status = if item.status == "done" {
                "active"
            } else {
                "done"
            }
            .into();
        } else {
            let date =
                NaiveDate::parse_from_str(item.due.as_deref().ok_or("Missing date")?, "%Y-%m-%d")
                    .map_err(err)?;
            let next = next_due(date, &item.recurrence)?;
            let end = item
                .recurrence
                .split_once(";until:")
                .map(|(_, d)| NaiveDate::parse_from_str(d, "%Y-%m-%d").map_err(err))
                .transpose()?;
            if end.is_some_and(|end| next > end) {
                item.status = "done".into();
            } else {
                item.due = Some(next.to_string());
                item.status = "active".into();
            }
        }
        self.save(item)
    }
    pub fn import_missing(&mut self, items: &[Item]) -> Result<usize> {
        for i in items {
            i.validate()?;
        }
        let tx = self.conn.transaction().map_err(err)?;
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
                write(&tx, i)?;
                count += 1;
            }
        }
        tx.commit().map_err(err)?;
        Ok(count)
    }
}
use rusqlite::OptionalExtension;
fn write(tx: &rusqlite::Transaction<'_>, i: &Item) -> Result<()> {
    workspace::check_owner(tx, &i.id, "item")?;
    tx.execute("INSERT INTO items VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15) ON CONFLICT(id) DO UPDATE SET title=excluded.title,body=excluded.body,pinned=excluded.pinned,priority=excluded.priority,status=excluded.status,due=excluded.due,recurrence=excluded.recurrence,url=excluded.url,updated_at=excluded.updated_at,device_id=excluded.device_id,revision=excluded.revision,deleted_at=excluded.deleted_at",params![i.id,i.kind,i.title,i.body,i.pinned,i.priority,i.status,i.due,i.recurrence,i.url,i.created_at,i.updated_at,i.device_id,i.revision,i.deleted_at]).map_err(err)?;
    tx.execute("INSERT INTO changes(event_id,entity_id,device_id,revision,operation,timestamp,payload) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![Uuid::new_v4().to_string(),i.id,i.device_id,i.revision,if i.deleted_at.is_some(){"delete"}else{"upsert"},now(),serde_json::to_string(i).map_err(err)?]).map_err(err)?;
    Ok(())
}
pub fn valid_recurrence(value: &str) -> bool {
    if let Some(id) = value.strip_prefix("after:") {
        return Uuid::parse_str(id).is_ok();
    }
    let value = if let Some((rule, end)) = value.split_once(";until:") {
        if rule == "none"
            || rule.starts_with("after:")
            || NaiveDate::parse_from_str(end, "%Y-%m-%d").is_err()
        {
            return false;
        }
        rule
    } else {
        value
    };
    if let Some(days) = value.strip_prefix("weekdays:") {
        let days = days.split(',').collect::<Vec<_>>();
        return !days.is_empty()
            && days.len() <= 7
            && days
                .iter()
                .all(|d| ["1", "2", "3", "4", "5", "6", "7"].contains(d))
            && days.iter().collect::<std::collections::HashSet<_>>().len() == days.len();
    }

    ["none", "daily", "weekly", "monthly", "yearly"].contains(&value)
        || value
            .strip_prefix("days:")
            .and_then(|n| n.parse::<u64>().ok())
            .is_some_and(|n| (1..=3650).contains(&n))
        || value
            .strip_prefix("weeks:")
            .and_then(|n| n.parse::<u64>().ok())
            .is_some_and(|n| (1..=520).contains(&n))
        || value
            .strip_prefix("months:")
            .and_then(|n| n.parse::<u32>().ok())
            .is_some_and(|n| (1..=120).contains(&n))
}
pub fn next_due(date: NaiveDate, recurrence: &str) -> Result<NaiveDate> {
    if !valid_recurrence(recurrence) {
        return Err("Invalid recurrence".into());
    }
    let recurrence = recurrence.split(";until:").next().unwrap_or(recurrence);
    if let Some(days) = recurrence.strip_prefix("weekdays:") {
        for offset in 1..=7 {
            let next = date
                .checked_add_days(Days::new(offset))
                .ok_or("Date overflow")?;
            if days
                .split(',')
                .any(|d| d.parse::<u32>().ok() == Some(next.weekday().number_from_monday()))
            {
                return Ok(next);
            }
        }
        return Err("Choose at least one weekday".into());
    }
    if let Some(n) = recurrence.strip_prefix("days:") {
        return date
            .checked_add_days(Days::new(n.parse().map_err(err)?))
            .ok_or("Date overflow".into());
    }
    if let Some(n) = recurrence.strip_prefix("weeks:") {
        return date
            .checked_add_days(Days::new(n.parse::<u64>().map_err(err)? * 7))
            .ok_or("Date overflow".into());
    }
    if let Some(n) = recurrence.strip_prefix("months:") {
        return date
            .checked_add_months(Months::new(n.parse().map_err(err)?))
            .ok_or("Date overflow".into());
    }
    match recurrence {
        "daily" => date.checked_add_days(Days::new(1)),
        "weekly" => date.checked_add_days(Days::new(7)),
        "monthly" => date.checked_add_months(Months::new(1)),
        "yearly" => date.checked_add_months(Months::new(12)),
        _ => None,
    }
    .ok_or("Invalid recurrence or date overflow".into())
}
#[cfg(test)]
mod tests {
    use super::*;
    pub fn item() -> Item {
        Item {
            id: Uuid::new_v4().to_string(),
            kind: "task".into(),
            title: "Review underwater robots".into(),
            body: "Research sonar".into(),
            pinned: true,
            priority: "important".into(),
            status: "active".into(),
            due: Some("2026-01-31".into()),
            recurrence: "monthly".into(),
            url: None,
            created_at: now(),
            updated_at: now(),
            device_id: Uuid::new_v4().to_string(),
            revision: 1,
            deleted_at: None,
        }
    }
    #[test]
    fn persists_searches_and_recurs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db");
        let mut r = Repository::open(&path).unwrap();
        let i = r.save(item()).unwrap();
        let device = r.device_id.clone();
        drop(r);
        let mut r = Repository::open(&path).unwrap();
        assert_eq!(device, r.device_id);
        assert_eq!(r.list("underwater", false).unwrap().len(), 1);
        let next = r.complete(&i.id).unwrap();
        assert_eq!(next.due.as_deref(), Some("2026-02-28"));
        assert_eq!(next.status, "active");
        assert!(r.save(i).is_err());
        assert_eq!(r.snapshot("").unwrap().changes, 2);
    }
    #[test]
    fn restore_is_idempotent_and_preserves_local_conflicts() {
        let mut r = Repository::open(":memory:").unwrap();
        let i = item();
        assert_eq!(r.import_missing(&[i.clone()]).unwrap(), 1);
        let mut conflict = i.clone();
        conflict.title = "Do not overwrite".into();
        assert_eq!(r.import_missing(&[conflict]).unwrap(), 0);
        assert_eq!(r.list("", false).unwrap()[0], i);
    }
    #[test]
    fn invalid_import_is_atomic() {
        let mut r = Repository::open(":memory:").unwrap();
        let mut bad = item();
        bad.due = Some("nonsense".into());
        assert!(r.import_missing(&[item(), bad]).is_err());
        assert!(r.list("", false).unwrap().is_empty());
    }
    #[test]
    fn tombstones_and_hostile_search() {
        let mut r = Repository::open(":memory:").unwrap();
        let mut i = r.save(item()).unwrap();
        i.deleted_at = Some(now());
        r.save(i).unwrap();
        assert!(r.list("", false).unwrap().is_empty());
        assert_eq!(r.list("", true).unwrap().len(), 1);
        assert!(r.list("\" OR * --", false).is_ok());
    }
}
