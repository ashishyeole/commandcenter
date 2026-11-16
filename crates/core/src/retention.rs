//! Explicit local retention. Keep current entities, tombstones and causal sync data.
use crate::{err, Repository, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub clear_feeds: bool,
    pub clear_embeddings: bool,
    pub history_before: Option<String>,
}
impl Policy {
    fn validate(&self) -> Result<()> {
        if let Some(d) = &self.history_before {
            chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").map_err(err)?;
            if d.as_str() > chrono::Local::now().format("%Y-%m-%d").to_string().as_str() {
                return Err("History cutoff cannot be in the future".into());
            }
        }
        Ok(())
    }
}
const CACHE_WHERE: &str =
    "key IN ('agriculture_cache','business_search_last','model_evidence','app_market_snapshots') OR key LIKE 'event_cache:%' OR key LIKE 'research_query:%'";
impl Repository {
    pub fn retention_preview(&self, p: &Policy) -> Result<Value> {
        p.validate()?;
        let feeds: i64 = if p.clear_feeds {
            self.conn
                .query_row("SELECT COUNT(*) FROM feed_cache", [], |r| r.get(0))
                .map_err(err)?
        } else {
            0
        };
        let extra: i64 = if p.clear_feeds {
            self.conn
                .query_row(
                    &format!(
                        "SELECT COUNT(*) FROM settings WHERE ({CACHE_WHERE}) AND value != 'null'"
                    ),
                    [],
                    |r| r.get(0),
                )
                .map_err(err)?
        } else {
            0
        };
        let embeddings: i64 = if p.clear_embeddings {
            self.conn
                .query_row("SELECT COUNT(*) FROM embeddings", [], |r| r.get(0))
                .map_err(err)?
        } else {
            0
        };
        let history: i64 = if let Some(before) = &p.history_before {
            self.conn.query_row("SELECT COUNT(*) FROM changes WHERE timestamp < ?1 AND sequence NOT IN (SELECT MAX(sequence) FROM changes GROUP BY entity_id)",[before],|r|r.get(0)).map_err(err)?
        } else {
            0
        };
        let latest: i64 = self
            .conn
            .query_row("SELECT COALESCE(MAX(sequence),0) FROM changes", [], |r| {
                r.get(0)
            })
            .map_err(err)?;
        Ok(
            json!({"policy":p,"cached_feeds":feeds,"public_provider_caches":extra,"embedding_rows":embeddings,"old_history_entries":history,"latest_change_sequence":latest,"boundary":"Current records, original attachments, vault entries, tombstones, causal sync journal, device permissions and saved discoveries are retained. This is not complete erasure of old content from sync history, backups or filesystem snapshots."}),
        )
    }
    pub fn apply_retention(&mut self, p: &Policy, expected: &Value) -> Result<Value> {
        let preview = self.retention_preview(p)?;
        if &preview != expected {
            return Err("Data changed after preview; review cleanup again".into());
        }
        self.conn
            .execute_batch("PRAGMA secure_delete=ON;")
            .map_err(err)?;
        let tx = self.conn.transaction().map_err(err)?;
        if p.clear_feeds {
            tx.execute("DELETE FROM feed_cache", []).map_err(err)?;
            tx.execute(&format!("DELETE FROM settings WHERE {CACHE_WHERE}"), [])
                .map_err(err)?;
        }
        if p.clear_embeddings {
            tx.execute("DELETE FROM embeddings", []).map_err(err)?;
        }
        if let Some(before) = &p.history_before {
            tx.execute("DELETE FROM changes WHERE timestamp < ?1 AND sequence NOT IN (SELECT MAX(sequence) FROM changes GROUP BY entity_id)",[before]).map_err(err)?;
        }
        tx.commit().map_err(err)?;
        Ok(preview)
    }
    pub fn diagnostic_counts(&self) -> Result<Value> {
        let count = |table: &str| -> Result<i64> {
            self.conn
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
                .map_err(err)
        };
        let schema: i64 = self
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(err)?;
        Ok(
            json!({"schema_version":schema,"items":count("items")?,"workspace_records":count("records")?,"attachments":count("attachments")?,"vault_records":count("vault")?,"history_entries":count("changes")?,"sync_events":count("sync_events")?,"sync_conflicts":count("sync_conflicts")?,"cached_feeds":count("feed_cache")?,"embeddings":count("embeddings")?}),
        )
    }
}

fn sync_compaction_preview(conn: &rusqlite::Connection) -> Result<Value> {
    use rusqlite::OptionalExtension;
    let raw: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key='paired_peers'",
            [],
            |r| r.get(0),
        )
        .optional()
        .map_err(err)?;
    let peers: Vec<String> = raw
        .map(|v| serde_json::from_str(&v).map_err(err))
        .transpose()?
        .unwrap_or_default();
    if peers.len() > 8 {
        return Err("Invalid paired-peer registry".into());
    }
    let latest: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(sequence),0) FROM sync_events",
            [],
            |r| r.get(0),
        )
        .map_err(err)?;
    let mut floor = latest;
    for peer in &peers {
        uuid::Uuid::parse_str(peer).map_err(err)?;
        let sent: Option<i64> = conn
            .query_row(
                "SELECT sent FROM peer_progress WHERE peer_id=?1",
                [peer],
                |r| r.get(0),
            )
            .optional()
            .map_err(err)?;
        floor = floor.min(sent.unwrap_or(0));
    }
    let count:i64=conn.query_row("SELECT COUNT(*) FROM sync_events WHERE sequence<=?1 AND sequence NOT IN (SELECT MAX(sequence) FROM sync_events GROUP BY entity_id)",[floor],|r|r.get(0)).map_err(err)?;
    Ok(
        json!({"through":floor,"latest_sequence":latest,"paired_peers":peers,"removable_events":count,"boundary":"Only superseded events acknowledged by every currently paired peer are removed. Latest event per entity, vector clocks, tombstones, replay hashes and conflicts remain. Old content can still exist in latest-event merge bases, backups and other devices."}),
    )
}
impl Repository {
    pub fn sync_compaction_preview(&self) -> Result<Value> {
        sync_compaction_preview(&self.conn)
    }
    pub fn compact_acknowledged_sync(&mut self, expected: &Value) -> Result<usize> {
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(err)?;
        let actual = sync_compaction_preview(&tx)?;
        if &actual != expected {
            return Err("Peer acknowledgements or journal changed; review again".into());
        }
        let floor = actual["through"]
            .as_i64()
            .ok_or("Invalid compaction boundary")?;
        let deleted=tx.execute("DELETE FROM sync_events WHERE sequence<=?1 AND sequence NOT IN (SELECT MAX(sequence) FROM sync_events GROUP BY entity_id)",[floor]).map_err(err)?;
        tx.commit().map_err(err)?;
        Ok(deleted)
    }
}
