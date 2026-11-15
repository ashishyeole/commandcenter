//! Device-local research collections contain queries, never connection permissions.
use crate::{Repository, Result};
use serde::{Deserialize, Serialize};
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Collection {
    pub id: String,
    pub name: String,
    pub searches: Vec<Search>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Search {
    pub source: String,
    pub query: String,
}
impl Repository {
    pub fn research_collections(&self) -> Result<Vec<Collection>> {
        serde_json::from_str(
            &self
                .setting("research_collections")?
                .unwrap_or_else(|| "[]".into()),
        )
        .map_err(|e| e.to_string())
    }
    pub fn save_research_collections(&self, collections: &[Collection]) -> Result<()> {
        validate_collections(collections)?;
        self.set_setting(
            "research_collections",
            &serde_json::to_string(collections).map_err(|e| e.to_string())?,
        )
    }
    pub fn cache_research_query(
        &self,
        source: &str,
        query: &str,
        items: &serde_json::Value,
    ) -> Result<()> {
        use sha2::{Digest, Sha256};
        let key = format!(
            "research_query:{:x}",
            Sha256::digest(format!("{source}\n{query}"))
        );
        self.set_setting(&key,&serde_json::json!({"source":source,"query":query,"fetched_at":crate::now(),"items":items}).to_string())?;
        // Bound disposable snapshots independently of permanent saved discoveries.
        self.conn.execute("DELETE FROM settings WHERE key IN (SELECT key FROM settings WHERE key LIKE 'research_query:%' ORDER BY json_extract(value,'$.fetched_at') DESC, key LIMIT -1 OFFSET 100)",[]).map_err(|e|e.to_string())?;
        Ok(())
    }
    pub fn research_query_snapshots(&self) -> Result<Vec<serde_json::Value>> {
        let mut stmt=self.conn.prepare("SELECT value FROM settings WHERE key LIKE 'research_query:%' ORDER BY json_extract(value,'$.fetched_at') DESC").map_err(|e|e.to_string())?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?;
        rows.map(|r| {
            serde_json::from_str(&r.map_err(|e| e.to_string())?).map_err(|e| e.to_string())
        })
        .collect()
    }
}

pub fn validate_collections(collections: &[Collection]) -> Result<()> {
    if collections.len() > 30 {
        return Err("At most 30 research collections".into());
    }
    let mut ids = std::collections::HashSet::new();
    for c in collections {
        uuid::Uuid::parse_str(&c.id).map_err(|_| "Invalid collection ID")?;
        if !ids.insert(&c.id)
            || c.name.trim().is_empty()
            || c.name.len() > 120
            || c.searches.is_empty()
            || c.searches.len() > 12
        {
            return Err("Use unique IDs, a name and 1–12 source queries".into());
        }
        for s in &c.searches {
            if s.query.len() > 300
                || ![
                    "arxiv", "rss", "github", "models", "ollama", "news", "mcp", "youtube", "apps",
                ]
                .contains(&s.source.as_str())
            {
                return Err("Invalid collection source or query".into());
            }
        }
    }
    Ok(())
}
