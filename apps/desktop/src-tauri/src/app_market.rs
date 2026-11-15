//! Explicit import adapter for licensed iOS/Android market snapshots; never scrapes stores.
use crate::{AppState, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::State;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct App {
    id: String,
    platform: String,
    name: String,
    category: String,
    store_url: String,
    features: Vec<String>,
    monetization: String,
    ux_notes: String,
    downloads: Option<u64>,
    revenue_minor: Option<u64>,
    currency: String,
    measurement_basis: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    schema: u32,
    provider: String,
    source_url: String,
    license_note: String,
    market: String,
    period_start: String,
    period_end: String,
    observed_at: String,
    apps: Vec<App>,
}
fn public_url(text: &str) -> bool {
    reqwest::Url::parse(text).is_ok_and(|u| {
        u.scheme() == "https"
            && u.host_str().is_some()
            && u.username().is_empty()
            && u.password().is_none()
            && u.fragment().is_none()
    })
}
impl Snapshot {
    fn validate(&self) -> Result<()> {
        if self.schema != 1
            || self.provider.trim().is_empty()
            || self.provider.len() > 200
            || !public_url(&self.source_url)
            || self.source_url.len() > 2000
            || self.license_note.trim().is_empty()
            || self.license_note.len() > 2000
            || self.market.len() != 2
            || !self.market.bytes().all(|b| b.is_ascii_uppercase())
            || self.apps.is_empty()
            || self.apps.len() > 250
        {
            return Err("Invalid snapshot metadata; supply provider, HTTPS source, license/use-rights note, two-letter market and 1–250 apps".into());
        }
        let start = chrono::NaiveDate::parse_from_str(&self.period_start, "%Y-%m-%d")
            .map_err(|_| "Invalid period start")?;
        let end = chrono::NaiveDate::parse_from_str(&self.period_end, "%Y-%m-%d")
            .map_err(|_| "Invalid period end")?;
        let observed = chrono::DateTime::parse_from_rfc3339(&self.observed_at)
            .map_err(|_| "Invalid observation timestamp")?;
        if start > end
            || end > chrono::Utc::now().date_naive()
            || end > observed.date_naive()
            || observed > chrono::Utc::now()
        {
            return Err("Snapshot periods/observations cannot be future-dated or inverted".into());
        }
        let mut ids = std::collections::HashSet::new();
        for a in &self.apps {
            if !["ios", "android"].contains(&a.platform.as_str())
                || a.id.is_empty()
                || a.id.len() > 200
                || !ids.insert((&a.platform, &a.id))
                || a.name.trim().is_empty()
                || a.name.len() > 500
                || a.category.len() > 200
                || a.store_url.len() > 2000
                || !public_url(&a.store_url)
                || a.features.len() > 50
                || a.features.iter().any(|f| f.is_empty() || f.len() > 500)
                || a.monetization.len() > 2000
                || a.ux_notes.len() > 4000
                || !["provider_reported", "estimated", "manual"]
                    .contains(&a.measurement_basis.as_str())
                || a.currency.len() != 3
                || !a.currency.bytes().all(|b| b.is_ascii_uppercase())
                || a.downloads.is_some_and(|v| v > 9_007_199_254_740_991)
                || a.revenue_minor.is_some_and(|v| v > 9_007_199_254_740_991)
            {
                return Err(
                    "Invalid app observation, duplicate identity, metric bound or source URL"
                        .into(),
                );
            }
        }
        Ok(())
    }
}
#[tauri::command]
pub fn app_market_snapshots(state: State<AppState>) -> Result<Value> {
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    serde_json::from_str(&repo.setting("app_market_snapshots")?.unwrap_or("[]".into()))
        .map_err(|_| "Invalid snapshot cache".into())
}
#[tauri::command]
pub async fn import_app_market_snapshot(
    state: State<'_, AppState>,
    confirmation: String,
    consent: bool,
) -> Result<Value> {
    if !consent || confirmation != "IMPORT LICENSED SNAPSHOT" {
        return Err("Confirm your right to use the dataset and approve this import".into());
    }
    let epoch = state.data_epoch.load(std::sync::atomic::Ordering::SeqCst);
    let Some(file) = rfd::AsyncFileDialog::new()
        .set_title("Select licensed app-market snapshot JSON — no store scraping")
        .add_filter("JSON", &["json"])
        .pick_file()
        .await
    else {
        return Ok(json!({"imported":false}));
    };
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(file.path())
        .map_err(|_| "Cannot open selected snapshot")?
        .take(1048577)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot read selected snapshot")?;
    if bytes.len() > 1048576 {
        return Err("Snapshot exceeds 1 MiB".into());
    }
    let snapshot: Snapshot = serde_json::from_slice(&bytes)
        .map_err(|_| "Invalid schema-1 market snapshot JSON; see docs/APP-MARKET-FORMAT.md")?;
    snapshot.validate()?;
    let value = serde_json::to_value(&snapshot).map_err(|e| e.to_string())?;
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    if epoch != state.data_epoch.load(std::sync::atomic::Ordering::SeqCst) {
        return Err("Workspace reset during file selection".into());
    }
    let mut rows: Vec<Value> =
        serde_json::from_str(&repo.setting("app_market_snapshots")?.unwrap_or("[]".into()))
            .map_err(|_| "Invalid snapshot cache")?;
    rows.retain(|r| {
        !(r["provider"] == value["provider"]
            && r["market"] == value["market"]
            && r["observed_at"] == value["observed_at"])
    });
    rows.insert(0, value);
    rows.truncate(8);
    repo.set_setting(
        "app_market_snapshots",
        &serde_json::to_string(&rows).map_err(|e| e.to_string())?,
    )?;
    Ok(
        json!({"imported":true,"apps":snapshot.apps.len(),"notice":"Imported publisher/user declarations, not independently verified metrics. Up to eight recent snapshots are cached; retain the original licensed files separately."}),
    )
}
