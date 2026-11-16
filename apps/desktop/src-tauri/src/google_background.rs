//! Explicit device-local, read-only Google refresh. Account content is memory-only.
use crate::{AppState, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use tauri::{Manager, State};
use zeroize::Zeroizing;
fn interval() -> u32 {
    15
}
#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub enabled: bool,
    pub mail: bool,
    pub calendar: bool,
    #[serde(default = "interval")]
    pub interval_minutes: u32,
    pub id: String,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: false,
            mail: false,
            calendar: false,
            interval_minutes: 15,
            id: String::new(),
        }
    }
}
impl Config {
    fn validate(&self, scopes: &str, consent: bool) -> Result<()> {
        if !(15..=1440).contains(&self.interval_minutes) {
            return Err("Use a background refresh interval of 15–1440 minutes".into());
        }
        if self.enabled && (!consent || !self.mail && !self.calendar) {
            return Err("Select account data and approve background reads first".into());
        }
        for (selected, scope) in [
            (self.mail, "https://www.googleapis.com/auth/gmail.metadata"),
            (
                self.calendar,
                "https://www.googleapis.com/auth/calendar.events.readonly",
            ),
        ] {
            if self.enabled && selected && !crate::google_accounts::scope_allows(scopes, scope) {
                return Err("Reconnect Google with the selected read-only scope first".into());
            }
        }
        Ok(())
    }
}
pub struct Cache {
    epoch: u64,
    config_id: String,
    expires: i64,
    pages: Zeroizing<String>,
}
impl Cache {
    fn read(&self, epoch: u64, config: &Config, now: i64) -> Option<Value> {
        if !config.enabled
            || epoch != self.epoch
            || config.id != self.config_id
            || now >= self.expires
        {
            return None;
        }
        serde_json::from_str(&self.pages).ok()
    }
}
fn config(state: &AppState) -> Result<Config> {
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    serde_json::from_str(
        &repo
            .setting("google_background")?
            .unwrap_or_else(|| "{}".into()),
    )
    .map_err(|_| "Invalid Google background configuration".into())
}
pub(crate) fn clear(state: &AppState) -> Result<()> {
    state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .set_setting("google_background", "{}")?;
    *state
        .google_cache
        .lock()
        .map_err(|_| "Account cache unavailable")? = None;
    Ok(())
}
#[tauri::command]
pub fn google_background_status(state: State<AppState>) -> Result<Value> {
    let config = config(&state)?;
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    let health = repo
        .setting("google_background_health")?
        .and_then(|s| serde_json::from_str::<Value>(&s).ok());
    Ok(json!({"config":config,"health":health}))
}
#[tauri::command]
pub fn save_google_background(
    state: State<AppState>,
    mut config: Config,
    consent: bool,
) -> Result<()> {
    let scopes = if config.enabled {
        crate::google_accounts::granted_scopes(&state)?
    } else {
        String::new()
    };
    config.validate(&scopes, consent)?;
    config.id = uuid::Uuid::new_v4().to_string();
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    if config.enabled && repo.setting("google_enabled")?.as_deref() != Some("true") {
        return Err("Connect Google before enabling background reads".into());
    }
    repo.set_setting(
        "google_background",
        &serde_json::to_string(&config).map_err(|e| e.to_string())?,
    )?;
    repo.set_setting("google_background_health", "{}")?;
    *state
        .google_cache
        .lock()
        .map_err(|_| "Account cache unavailable")? = None;
    if let Some(token) = state
        .jobs
        .lock()
        .map_err(|_| "Job state unavailable")?
        .get("google:request")
    {
        token.cancel();
    }
    // In-flight background snapshots also check this configuration ID before publishing.
    Ok(())
}
#[tauri::command]
pub fn read_google_background(state: State<AppState>, consent: bool) -> Result<Value> {
    if !consent {
        return Err("Approve showing the temporary account snapshot".into());
    }
    let config = config(&state)?;
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    if repo.setting("google_enabled")?.as_deref() != Some("true") {
        return Err("Google is disconnected".into());
    }
    let epoch = state.data_epoch.load(Ordering::SeqCst);
    state.google_cache.lock().map_err(|_|"Account cache unavailable")?.as_ref()
 .and_then(|c|c.read(epoch,&config,chrono::Utc::now().timestamp())).ok_or_else(||"No fresh background snapshot. Read manually or wait for the approved refresh; expired data is never displayed.".into())
}
pub async fn poll(app: &tauri::AppHandle) -> Result<()> {
    let state = app.state::<AppState>();
    let epoch = state.data_epoch.load(Ordering::SeqCst);
    let current = config(&state)?;
    let now = chrono::Utc::now().timestamp();
    {
        let mut cache = state
            .google_cache
            .lock()
            .map_err(|_| "Account cache unavailable")?;
        if cache
            .as_ref()
            .is_some_and(|c| c.read(epoch, &current, now).is_none())
        {
            *cache = None;
        }
    }
    if !current.enabled {
        return Ok(());
    }
    {
        let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
        if repo.setting("google_enabled")?.as_deref() != Some("true") {
            return Ok(());
        }
        let last = repo
            .setting("google_background_health")?
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .and_then(|v| v["attempt_at"].as_i64());
        if last.is_some_and(|last| now - last < i64::from(current.interval_minutes) * 60) {
            return Ok(());
        }
        // Busy foreground reads take priority and do not consume the background interval.
        if state
            .jobs
            .lock()
            .map_err(|_| "Job state unavailable")?
            .contains_key("google:request")
        {
            return Ok(());
        }
        repo.set_setting(
            "google_background_health",
            &json!({"status":"running","attempt_at":now}).to_string(),
        )?;
    }
    let work=async{
  let mut pages=vec![];
  for (selected,kind) in [(current.mail,"mail"),(current.calendar,"calendar")]{if selected{
   let page=tokio::time::timeout(std::time::Duration::from_secs(90),crate::google_accounts::read_google_account(app.state::<AppState>(),kind.into(),None,true)).await.map_err(|_|"Background Google read timed out")??;
   pages.push(page);
  }}
  Ok::<_,String>(json!({"pages":pages,"fetched_at":command_center_core::now(),"expires_at":now+1800,"notice":"Memory-only background snapshot: first 20 inbox headers and/or first 100 primary-calendar events in the next 30 days. No bodies, attachments, sending, marking read, ordinary storage, search or AI access. More pages require a manual read."}))
 }.await;
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    let latest: Config = serde_json::from_str(
        &repo
            .setting("google_background")?
            .unwrap_or_else(|| "{}".into()),
    )
    .map_err(|_| "Invalid Google configuration")?;
    if state.data_epoch.load(Ordering::SeqCst) != epoch
        || latest != current
        || repo.setting("google_enabled")?.as_deref() != Some("true")
    {
        return Err("Background authorization changed; results discarded".into());
    }
    match work {
        Ok(value) => {
            *state
                .google_cache
                .lock()
                .map_err(|_| "Account cache unavailable")? = Some(Cache {
                epoch,
                config_id: current.id,
                expires: now + 1800,
                pages: Zeroizing::new(value.to_string()),
            });
            repo.set_setting("google_background_health",&json!({"status":"completed","attempt_at":now,"finished_at":command_center_core::now(),"expires_at":now+1800}).to_string())
        }
        Err(error) => {
            *state
                .google_cache
                .lock()
                .map_err(|_| "Account cache unavailable")? = None;
            repo.set_setting("google_background_health",&json!({"status":"failed","attempt_at":now,"error":error.chars().take(500).collect::<String>()}).to_string())?;
            Err(error)
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn background_requires_saved_consent_and_matching_scopes() {
        let mut c = Config::default();
        assert!(!c.enabled);
        c.enabled = true;
        c.mail = true;
        assert!(c.validate("", true).is_err());
        assert!(c
            .validate("https://www.googleapis.com/auth/gmail.readonly", false)
            .is_err());
        assert!(c
            .validate("https://www.googleapis.com/auth/gmail.readonly", true)
            .is_ok());
        c.calendar = true;
        assert!(c
            .validate("https://www.googleapis.com/auth/gmail.readonly", true)
            .is_err());
        c.enabled = false;
        c.interval_minutes = 1;
        assert!(c.validate("", false).is_err());
    }
    #[test]
    fn expired_reset_revoked_and_replaced_snapshots_are_hidden() {
        let mut c = Config {
            enabled: true,
            id: "approved".into(),
            ..Default::default()
        };
        let cache = Cache {
            epoch: 4,
            config_id: c.id.clone(),
            expires: 100,
            pages: Zeroizing::new("{\"pages\":[]}".into()),
        };
        assert!(cache.read(4, &c, 99).is_some());
        assert!(cache.read(4, &c, 100).is_none());
        assert!(cache.read(5, &c, 99).is_none());
        c.id = "changed".into();
        assert!(cache.read(4, &c, 99).is_none());
        c.id = "approved".into();
        c.enabled = false;
        assert!(cache.read(4, &c, 99).is_none());
    }
}
