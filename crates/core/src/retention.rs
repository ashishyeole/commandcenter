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
    "key IN ('agriculture_cache','business_search_last','model_evidence','app_market_snapshots') OR key LIKE 'event_cache:%'";
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
