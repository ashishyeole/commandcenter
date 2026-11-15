//! Ordinary, user-selected documents. Sensitive documents belong in the encrypted vault.
use crate::{err, Repository, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Attachment {
    pub id: String,
    pub owner_id: String,
    pub filename: String,
    pub mime: String,
    pub content_base64: String,
    pub sha256: String,
    pub created_at: String,
    pub previous_id: Option<String>,
    pub deleted_at: Option<String>,
    pub extracted_text: String,
}
impl Attachment {
    pub fn validate(&self) -> Result<Vec<u8>> {
        uuid::Uuid::parse_str(&self.id).map_err(err)?;
        uuid::Uuid::parse_str(&self.owner_id).map_err(err)?;
        if let Some(id) = &self.previous_id {
            uuid::Uuid::parse_str(id).map_err(err)?;
            if id == &self.id {
                return Err("A document cannot replace itself".into());
            }
        }
        chrono::DateTime::parse_from_rfc3339(&self.created_at).map_err(err)?;
        if let Some(t) = &self.deleted_at {
            chrono::DateTime::parse_from_rfc3339(t).map_err(err)?;
        }
        if self.filename.is_empty()
            || self.filename.len() > 240
            || self.filename.contains(['/', '\\', '\0'])
            || self.filename == "."
            || self.filename == ".."
            || self.mime.len() > 100
            || self.extracted_text.len() > 100000
            || self.content_base64.len() > 14 * 1024 * 1024
        {
            return Err("Invalid document metadata or size".into());
        }
        let bytes = STANDARD.decode(&self.content_base64).map_err(err)?;
        if bytes.len() > 10 * 1024 * 1024 || format!("{:x}", Sha256::digest(&bytes)) != self.sha256
        {
            return Err("Document size or checksum mismatch".into());
        }
        Ok(bytes)
    }
}
pub(crate) fn insert(conn: &Connection, a: &Attachment) -> Result<usize> {
    a.validate()?;
    crate::workspace::check_owner(conn, &a.id, "attachment")?;
    let exists:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM items WHERE id=?1 UNION ALL SELECT 1 FROM records WHERE id=?1)",[&a.owner_id],|r|r.get(0)).map_err(err)?;
    if !exists {
        return Err("Document owner is absent from the workspace".into());
    }
    let n = conn
        .execute(
            "INSERT OR IGNORE INTO attachments VALUES(?1,?2,?3)",
            params![a.id, a.owner_id, serde_json::to_string(a).map_err(err)?],
        )
        .map_err(err)?;
    if n > 0 {
        conn.execute(
            "INSERT INTO attachments_fts(id,title,extracted_text) VALUES(?1,?2,?3)",
            params![a.id, a.filename, a.extracted_text],
        )
        .map_err(err)?;
    }
    Ok(n)
}
impl Repository {
    pub fn attachment_summaries(
        &self,
        owner: Option<&str>,
        query: &str,
        deleted: bool,
    ) -> Result<Vec<serde_json::Value>> {
        if query.len() > 300 {
            return Err("Search is too long".into());
        }
        let terms = query
            .split_whitespace()
            .take(32)
            .map(|t| format!("\"{}\"", t.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(" AND ");
        let fts = if terms.is_empty() {
            "\"__empty__\""
        } else {
            terms.as_str()
        };
        let mut stmt=self.conn.prepare("SELECT json_remove(payload,'$.content_base64','$.extracted_text') FROM attachments WHERE (?1 IS NULL OR owner_id=?1) AND (?2 OR json_extract(payload,'$.deleted_at') IS NULL) AND (?3='' OR id IN (SELECT id FROM attachments_fts WHERE attachments_fts MATCH ?4)) ORDER BY rowid DESC LIMIT 1000").map_err(err)?;
        let rows = stmt
            .query_map(params![owner, deleted, query, fts], |r| {
                r.get::<_, String>(0)
            })
            .map_err(err)?;
        rows.map(|r| serde_json::from_str(&r.map_err(err)?).map_err(err))
            .collect()
    }

    pub fn attachments(
        &self,
        owner: Option<&str>,
        query: &str,
        deleted: bool,
    ) -> Result<Vec<Attachment>> {
        let mut stmt=self.conn.prepare("SELECT payload FROM attachments WHERE (?1 IS NULL OR owner_id=?1) ORDER BY rowid DESC").map_err(err)?;
        let rows = stmt
            .query_map([owner], |r| r.get::<_, String>(0))
            .map_err(err)?;
        let query = query.trim().to_lowercase();
        let mut out = vec![];
        for row in rows {
            let a: Attachment = serde_json::from_str(&row.map_err(err)?).map_err(err)?;
            if (deleted || a.deleted_at.is_none())
                && (query.is_empty()
                    || a.filename.to_lowercase().contains(&query)
                    || a.extracted_text.to_lowercase().contains(&query))
            {
                out.push(a);
            }
        }
        Ok(out)
    }
    pub fn attachment(&self, id: &str) -> Result<Attachment> {
        let raw: Option<String> = self
            .conn
            .query_row("SELECT payload FROM attachments WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .optional()
            .map_err(err)?;
        serde_json::from_str(&raw.ok_or("Document not found")?).map_err(err)
    }
    pub fn add_attachment(&mut self, a: &Attachment) -> Result<()> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM attachments", [], |r| r.get(0))
            .map_err(err)?;
        if count >= 1000 {
            return Err("The workspace attachment limit is 1,000 versions".into());
        }

        if let Some(previous) = &a.previous_id {
            if self.attachment(previous)?.owner_id != a.owner_id {
                return Err("Previous version belongs to another record".into());
            }
        }
        let tx = self.conn.transaction().map_err(err)?;
        if insert(&tx, a)? == 0 {
            return Err("Document identity already exists".into());
        }
        tx.commit().map_err(err)
    }
    pub fn index_attachment_text(
        &mut self,
        id: &str,
        expected_sha256: &str,
        text: &str,
    ) -> Result<()> {
        if text.len() > 100000 {
            return Err("Extracted text exceeds 100 KB".into());
        }
        let mut a = self.attachment(id)?;
        if a.sha256 != expected_sha256 || a.deleted_at.is_some() {
            return Err("Attachment changed or was trashed during extraction".into());
        }
        a.extracted_text = text.to_owned();
        let tx = self.conn.transaction().map_err(err)?;
        tx.execute(
            "UPDATE attachments SET payload=?2 WHERE id=?1",
            params![id, serde_json::to_string(&a).map_err(err)?],
        )
        .map_err(err)?;
        tx.execute("DELETE FROM attachments_fts WHERE id=?1", [id])
            .map_err(err)?;
        tx.execute(
            "INSERT INTO attachments_fts(id,title,extracted_text) VALUES(?1,?2,?3)",
            params![id, a.filename, text],
        )
        .map_err(err)?;
        tx.commit().map_err(err)
    }
    pub fn trash_attachment(&self, id: &str, deleted: bool) -> Result<()> {
        let mut a = self.attachment(id)?;
        a.deleted_at = if deleted { Some(crate::now()) } else { None };
        self.conn
            .execute(
                "UPDATE attachments SET payload=?2 WHERE id=?1",
                params![id, serde_json::to_string(&a).map_err(err)?],
            )
            .map_err(err)?;
        Ok(())
    }
}
