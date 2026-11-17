//! Device-local permission lifecycle metadata. Never stores credential values.
use crate::{err, Repository, Result};
use serde_json::{json, Value};
pub const FLAGS: &[(&str, &str)] = &[
    ("app_market_enabled", "Licensed app-market feed"),
    ("cloud_openai_enabled", "Online assistant"),
    ("google_enabled", "Google read-only"),
    ("github_enabled", "Private GitHub"),
    ("plaid_enabled", "Plaid read-only"),
    ("youtube_enabled", "YouTube API search"),
    ("discovery_x_configured", "X search"),
    ("discovery_reddit_configured", "Reddit read-only"),
    ("agriculture_enabled:usda", "USDA statistics"),
    ("agriculture_enabled:india", "India OGD statistics"),
    ("hosted_enabled", "Optional hosted connection"),
];
impl Repository {
    pub fn set_integration_flag(&self, flag: &str, enabled: bool) -> Result<()> {
        if !FLAGS.iter().any(|(k, _)| *k == flag) {
            return Err("Unknown integration permission".into());
        }
        let mut events: Vec<Value> = self
            .setting("integration_lifecycle")?
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        events.push(json!({"integration":flag,"at":crate::now(),"permission":if enabled{"enabled"}else{"disabled"}}));
        if events.len() > 200 {
            events.drain(..events.len() - 200);
        }
        let tx = self.conn.unchecked_transaction().map_err(err)?;
        tx.execute("INSERT INTO settings VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",rusqlite::params![flag,if enabled{"true"}else{"false"}]).map_err(err)?;
        tx.execute("INSERT INTO settings VALUES('integration_lifecycle',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[serde_json::to_string(&events).map_err(err)?]).map_err(err)?;
        tx.commit().map_err(err)
    }
    pub fn integration_inventory(&self) -> Result<Value> {
        let mut rows = vec![];
        for (flag, name) in FLAGS {
            rows.push(json!({"flag":flag,"name":name,"enabled":self.setting(flag)?.as_deref()==Some("true")}));
        }
        Ok(
            json!({"integrations":rows,"events":self.setting("integration_lifecycle")?.and_then(|s|serde_json::from_str::<Value>(&s).ok()).unwrap_or(json!([])),"boundary":"Device-local permission flags only. No OS credentials are read. Disabling local access does not revoke a provider grant or remove stored credentials; use that provider's disconnect control."}),
        )
    }
}
impl Repository {
    pub fn ai_exclusions(&self) -> Result<Vec<String>> {
        let ids: Vec<String> = serde_json::from_str(
            &self
                .setting("ai_context_exclusions")?
                .unwrap_or("[]".into()),
        )
        .map_err(err)?;
        if ids.len() > 10000 || ids.iter().any(|id| uuid::Uuid::parse_str(id).is_err()) {
            return Err("Invalid device-local AI exclusion policy".into());
        }
        Ok(ids)
    }
    pub fn set_ai_exclusion(&self, id: &str, excluded: bool) -> Result<()> {
        uuid::Uuid::parse_str(id).map_err(err)?;
        let exists:bool=self.conn.query_row("SELECT EXISTS(SELECT 1 FROM items WHERE id=?1 UNION ALL SELECT 1 FROM records WHERE id=?1)",[id],|r|r.get(0)).map_err(err)?;
        if !exists {
            return Err("Ordinary record not found".into());
        }
        let mut ids = self.ai_exclusions()?;
        ids.retain(|v| v != id);
        if excluded {
            ids.push(id.to_owned());
        }
        if ids.len() > 10000 {
            return Err("AI exclusion limit reached".into());
        }
        self.set_setting(
            "ai_context_exclusions",
            &serde_json::to_string(&ids).map_err(err)?,
        )
    }
    pub fn ensure_ai_context_allowed(&self, ids: &[String]) -> Result<()> {
        let excluded = self.ai_exclusions()?;
        if ids.iter().any(|id| excluded.contains(id)) {
            return Err("A selected record is excluded from AI on this device; change the selection or review Data & privacy → AI exclusions".into());
        }
        Ok(())
    }
}

impl Repository {
    /// Atomic first-launch setup. Existing source/refresh policy (including an
    /// explicitly disabled or restored configuration) always wins.
    pub fn initialize_public_feeds(&self, streams: &Value) -> Result<bool> {
        let mut portable = streams.clone();
        for row in portable.as_array_mut().ok_or("Invalid starter sources")? {
            row["enabled"] = json!(false);
        }
        crate::workspace::validate_discovery_sources(&portable)?;
        let tx = self.conn.unchecked_transaction().map_err(err)?;
        use rusqlite::OptionalExtension;
        let present = |key: &str| -> Result<bool> {
            tx.query_row("SELECT 1 FROM settings WHERE key=?1", [key], |_| Ok(()))
                .optional()
                .map(|r| r.is_some())
                .map_err(err)
        };
        if present("public_discovery_initialized")? {
            return Ok(false);
        }
        let fresh = !present("discovery_hub_streams")? && !present("discovery_hub_auto_refresh")?;
        if fresh {
            tx.execute(
                "INSERT INTO settings VALUES('discovery_hub_streams',?1)",
                [serde_json::to_string(streams).map_err(err)?],
            )
            .map_err(err)?;
            tx.execute(
                "INSERT INTO settings VALUES('discovery_hub_auto_refresh','30')",
                [],
            )
            .map_err(err)?;
        }
        tx.execute(
            "INSERT INTO settings VALUES('public_discovery_initialized','1')",
            [],
        )
        .map_err(err)?;
        tx.commit().map_err(err)?;
        Ok(fresh)
    }
}
