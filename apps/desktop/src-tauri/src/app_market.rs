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
    cache_snapshot(&state, snapshot, epoch, None)
}

fn cache_snapshot(
    state: &AppState,
    snapshot: Snapshot,
    epoch: u64,
    expected_feed: Option<&str>,
) -> Result<Value> {
    snapshot.validate()?;
    let value = serde_json::to_value(&snapshot).map_err(|e| e.to_string())?;
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    if epoch != state.data_epoch.load(std::sync::atomic::Ordering::SeqCst) {
        return Err("Workspace reset during file selection".into());
    }
    if let Some(url) = expected_feed {
        if repo.setting("app_market_enabled")?.as_deref() != Some("true")
            || repo.setting("app_market_feed")?.as_deref() != Some(url)
        {
            return Err("Market feed permission or destination changed".into());
        }
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
fn market_entry(state: &AppState) -> Result<keyring::Entry> {
    let device = state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .device_id
        .clone();
    keyring::Entry::new("com.personal.commandcenter.market-feed", &device)
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub fn configure_market_feed(
    state: State<AppState>,
    url: String,
    token: String,
    consent: bool,
) -> Result<()> {
    let token = zeroize::Zeroizing::new(token);
    let u =
        reqwest::Url::parse(&url).map_err(|_| "Enter a fixed public HTTPS snapshot endpoint")?;
    if !consent
        || !public_url(&url)
        || u.query().is_some()
        || u.fragment().is_some()
        || url.len() > 2000
        || token.len() > 4096
        || token.chars().any(char::is_whitespace)
    {
        return Err("Approve a fixed HTTPS schema-1 market snapshot endpoint without URL credentials/query; optional bearer token stays in the OS secure store".into());
    }
    if state
        .jobs
        .lock()
        .map_err(|_| "Jobs unavailable")?
        .contains_key("market:refresh")
    {
        return Err("Cancel the active refresh before replacing its destination".into());
    }
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    repo.set_integration_flag("app_market_enabled", false)?;
    if token.is_empty() {
        match market_entry_unlocked(&repo.device_id)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => (),
            Err(_) => return Err("Could not clear previous market credential".into()),
        }
    } else {
        market_entry_unlocked(&repo.device_id)?
            .set_password(&token)
            .map_err(|_| "OS secure store refused market credential")?;
    }
    repo.set_setting("app_market_feed", u.as_str())?;
    repo.set_setting(
        "app_market_feed_auth",
        if token.is_empty() { "false" } else { "true" },
    )?;
    repo.set_integration_flag("app_market_enabled", true)
}
fn market_entry_unlocked(device: &str) -> Result<keyring::Entry> {
    keyring::Entry::new("com.personal.commandcenter.market-feed", device).map_err(|e| e.to_string())
}
#[tauri::command]
pub fn remove_market_feed(state: State<AppState>) -> Result<()> {
    state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .set_integration_flag("app_market_enabled", false)?;
    if let Some(c) = state
        .jobs
        .lock()
        .map_err(|_| "Jobs unavailable")?
        .get("market:refresh")
    {
        c.cancel();
    }
    match market_entry(&state)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(_) => Err("Feed disabled; OS credential removal failed".into()),
    }
}
struct MarketJob<'a>(&'a AppState);
impl Drop for MarketJob<'_> {
    fn drop(&mut self) {
        if let Ok(mut jobs) = self.0.jobs.lock() {
            jobs.remove("market:refresh");
        }
    }
}
#[tauri::command]
pub async fn refresh_market_feed(state: State<'_, AppState>, consent: bool) -> Result<Value> {
    if !consent {
        return Err("Approve this licensed snapshot request and local caching".into());
    }
    let (url, auth, epoch) = {
        let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
        if repo.setting("app_market_enabled")?.as_deref() != Some("true") {
            return Err("Configure a market feed on this device first".into());
        }
        (
            repo.setting("app_market_feed")?
                .ok_or("No market feed configured")?,
            repo.setting("app_market_feed_auth")?.as_deref() == Some("true"),
            state.data_epoch.load(std::sync::atomic::Ordering::SeqCst),
        )
    };
    let cancel = tokio_util::sync::CancellationToken::new();
    {
        let mut jobs = state.jobs.lock().map_err(|_| "Jobs unavailable")?;
        if jobs.contains_key("market:refresh") {
            return Err("A market refresh is already running".into());
        }
        jobs.insert("market:refresh".into(), cancel.clone());
    }
    let _guard = MarketJob(&state);
    let work = async {
        let endpoint = reqwest::Url::parse(&url).map_err(|_| "Invalid market endpoint")?;
        let client = crate::research::public_client(&endpoint).await?;
        let mut request = client.get(endpoint).header("Accept", "application/json");
        let token = if auth {
            Some(zeroize::Zeroizing::new(
                market_entry(&state)?
                    .get_password()
                    .map_err(|_| "Market credential unavailable")?,
            ))
        } else {
            None
        };
        if let Some(token) = &token {
            request = request.bearer_auth(token.as_str());
        }
        let mut response = request
            .send()
            .await
            .map_err(|_| "Market feed request failed")?;
        if !response.status().is_success() {
            return Err(format!("Market feed returned HTTP {}", response.status()));
        }
        let mut bytes = vec![];
        while let Some(c) = response
            .chunk()
            .await
            .map_err(|_| "Market response interrupted")?
        {
            if bytes.len() + c.len() > 1048576 {
                return Err("Market snapshot exceeds 1 MiB".into());
            }
            bytes.extend_from_slice(&c);
        }
        let snapshot: Snapshot = serde_json::from_slice(&bytes)
            .map_err(|_| "Expected the documented schema-1 licensed market snapshot")?;
        cache_snapshot(&state, snapshot, epoch, Some(&url))
    };
    tokio::select! {_=cancel.cancelled()=>Err("Market refresh cancelled".into()),r=work=>r}
}
