//! Explicitly authorized personal reminder channels. Never used for CRM outreach.
use crate::{AppState, Result};
use chrono::Timelike;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{Manager, State};
use zeroize::{Zeroize, Zeroizing};
const MESSAGE: &str =
    "A task needs your attention. Open Personal Command Center to review your due tasks.";
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub kind: String,
    pub account: String,
    pub secret: String,
    pub from: String,
    pub to: String,
    pub origin: String,
    pub topic: String,
    pub hour: u32,
    pub daily_enabled: bool,
    pub generation: String,
}
impl Drop for Config {
    fn drop(&mut self) {
        self.secret.zeroize();
        self.from.zeroize();
        self.to.zeroize();
        self.topic.zeroize();
    }
}
fn valid_kind(s: &str) -> Result<()> {
    if !["email", "sms", "voice", "push", "native_push"].contains(&s) {
        return Err("Unsupported notification channel".into());
    }
    Ok(())
}
fn entry(state: &AppState, kind: &str) -> Result<keyring::Entry> {
    valid_kind(kind)?;
    let device = state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .device_id
        .clone();
    keyring::Entry::new(
        &format!("com.personal.commandcenter.notify.{kind}"),
        &device,
    )
    .map_err(|e| e.to_string())
}
fn read(state: &AppState, kind: &str) -> Result<Config> {
    let raw = Zeroizing::new(
        entry(state, kind)?
            .get_password()
            .map_err(|_| "Configure this reminder provider first")?,
    );
    serde_json::from_str(&raw).map_err(|_| "Invalid channel configuration".into())
}
fn validate(c: &Config) -> Result<()> {
    valid_kind(&c.kind)?;
    if c.hour > 23
        || c.secret.len() < 16
        || c.secret.len() > 4096
        || c.secret.contains(['\r', '\n'])
    {
        return Err("Invalid reminder time or provider credential".into());
    }
    let phone = |s: &str| {
        s.starts_with('+')
            && (8..=16).contains(&s.len())
            && s[1..].bytes().all(|b| b.is_ascii_digit())
    };
    let email = |s: &str| {
        s.len() <= 254
            && s.split('@').count() == 2
            && !s.chars().any(char::is_whitespace)
            && !s.contains(['<', '>', '\r', '\n'])
            && s.split('@').all(|x| !x.is_empty())
    };
    match c.kind.as_str() {
        "sms" | "voice" => {
            if c.account.len() != 34
                || !c.account.starts_with("AC")
                || !c.account[2..].bytes().all(|b| b.is_ascii_hexdigit())
                || !phone(&c.from)
                || !phone(&c.to)
            {
                return Err(
                    "Use a Twilio AC account SID and sender/own recipient E.164 numbers".into(),
                );
            }
        }
        "email" => {
            if !email(&c.from) || !email(&c.to) {
                return Err("Use a verified sender and your personal destination email".into());
            }
        }
        "push" | "native_push" => {
            let u = reqwest::Url::parse(&c.origin).map_err(|_| "Invalid ntfy origin")?;
            if u.scheme() != "https"
                || u.host_str().is_none()
                || u.path() != "/"
                || !u.username().is_empty()
                || u.password().is_some()
                || u.query().is_some()
                || u.fragment().is_some()
                || (c.kind == "push" && c.topic.is_empty())
                || c.topic.len() > 64
                || !c
                    .topic
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            {
                return Err("Use an HTTPS ntfy origin and a private topic identifier".into());
            }
        }
        _ => (),
    };
    if c.kind == "native_push"
        && (c.to.is_empty()
            || c.to.len() > 100
            || !c
                .to
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b)))
    {
        return Err(
            "Use the device ID shown by your companion after native push registration".into(),
        );
    }
    Ok(())
}
#[tauri::command]
pub fn reminder_channels(state: State<AppState>) -> Result<Value> {
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    let mut rows = vec![];
    for kind in ["email", "sms", "voice", "push", "native_push"] {
        rows.push(json!({"kind":kind,"policy":repo.setting(&format!("reminder_policy:{kind}"))?.and_then(|s|serde_json::from_str::<Value>(&s).ok()),"last_attempt":repo.setting(&format!("reminder_receipt:{kind}"))?.and_then(|s|serde_json::from_str::<Value>(&s).ok())}));
    }
    Ok(json!(rows))
}
#[tauri::command]
pub fn configure_reminder_channel(
    state: State<AppState>,
    mut configuration: Config,
    confirmation: String,
    consent: bool,
) -> Result<()> {
    validate(&configuration)?;
    if !consent || confirmation != "AUTHORIZE MY REMINDERS" {
        return Err("Confirm you control the destination and type AUTHORIZE MY REMINDERS to approve the exact provider, destination and daily policy".into());
    }
    let kind = configuration.kind.clone();
    if state
        .jobs
        .lock()
        .map_err(|_| "Job state unavailable")?
        .contains_key(&format!("notify:{kind}"))
    {
        return Err("Finish the active reminder before changing configuration".into());
    }
    configuration.generation = uuid::Uuid::new_v4().to_string();
    let key = entry(&state, &kind)?;
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    key.set_password(&Zeroizing::new(
        serde_json::to_string(&configuration).map_err(|e| e.to_string())?,
    ))
    .map_err(|_| "OS secure store refused reminder credentials")?;
    repo.set_setting(&format!("reminder_policy:{kind}"),&json!({"enabled":true,"daily_enabled":configuration.daily_enabled,"hour":configuration.hour,"generation":configuration.generation,"destination":"Stored privately in OS secure storage","content":MESSAGE}).to_string())
}
#[tauri::command]
pub fn disable_reminder_channel(state: State<AppState>, kind: String) -> Result<()> {
    valid_kind(&kind)?;
    state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .set_setting(
            &format!("reminder_policy:{kind}"),
            &json!({"enabled":false}).to_string(),
        )?;
    if let Some(t) = state
        .jobs
        .lock()
        .map_err(|_| "Job state unavailable")?
        .get(&format!("notify:{kind}"))
    {
        t.cancel();
    }
    match entry(&state, &kind)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(_) => Err("Channel disabled; secure credential removal failed".into()),
    }
}
fn authorized(state: &AppState, c: &Config, epoch: u64) -> Result<()> {
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    let p: Value = serde_json::from_str(
        &repo
            .setting(&format!("reminder_policy:{}", c.kind))?
            .unwrap_or("{}".into()),
    )
    .map_err(|_| "Invalid reminder policy")?;
    if epoch != state.data_epoch.load(std::sync::atomic::Ordering::SeqCst)
        || p["enabled"] != true
        || p["generation"].as_str() != Some(c.generation.as_str())
    {
        return Err("Reminder permission changed".into());
    }
    Ok(())
}
struct Guard<'a> {
    state: &'a AppState,
    key: String,
}
impl Drop for Guard<'_> {
    fn drop(&mut self) {
        if let Ok(mut jobs) = self.state.jobs.lock() {
            jobs.remove(&self.key);
        }
    }
}
async fn deliver(state: &AppState, kind: &str, automatic: bool) -> Result<Value> {
    valid_kind(kind)?;
    let c = read(state, kind)?;
    validate(&c)?;
    if automatic {
        let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
        let prefs = crate::preferences(&repo)?;
        let hour = chrono::Local::now().hour();
        let quiet = if prefs.quiet_start < prefs.quiet_end {
            hour >= prefs.quiet_start && hour < prefs.quiet_end
        } else if prefs.quiet_start > prefs.quiet_end {
            hour >= prefs.quiet_start || hour < prefs.quiet_end
        } else {
            false
        };
        if !c.daily_enabled || hour < c.hour || quiet {
            return Ok(
                json!({"skipped":"Current reminder policy does not permit an automatic send"}),
            );
        }
        let today = chrono::Local::now().date_naive().to_string();
        let mut due = false;
        for item in repo.list("", false)? {
            let snoozed = repo
                .setting(&format!("snooze:{}", item.id))?
                .is_some_and(|v| {
                    chrono::DateTime::parse_from_rfc3339(&v)
                        .is_ok_and(|until| until > chrono::Utc::now())
                });
            if item.kind == "task"
                && item.status == "active"
                && item.due.as_deref().is_some_and(|d| d <= today.as_str())
                && !snoozed
            {
                due = true;
                break;
            }
        }
        if !due {
            return Ok(json!({"skipped":"No unsnoozed due tasks"}));
        }
    }
    let epoch = state.data_epoch.load(std::sync::atomic::Ordering::SeqCst);
    authorized(state, &c, epoch)?;
    let cancel = tokio_util::sync::CancellationToken::new();
    let key = format!("notify:{kind}");
    {
        let mut jobs = state.jobs.lock().map_err(|_| "Job state unavailable")?;
        if jobs.contains_key(&key) {
            return Err("A reminder is already in progress".into());
        }
        jobs.insert(key.clone(), cancel.clone());
    }
    let _guard = Guard { state, key };
    {
        let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
        let now = chrono::Utc::now().timestamp();
        let previous = repo
            .setting(&format!("reminder_attempt_time:{kind}"))?
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(0);
        if now - previous < 60 {
            return Err("Wait at least one minute between personal reminder requests".into());
        }
        let daily = format!("reminder_attempt_day:{kind}");
        let today = chrono::Local::now().date_naive().to_string();
        if automatic && repo.setting(&daily)?.as_deref() == Some(today.as_str()) {
            return Ok(json!({"already_attempted_today":true}));
        }
        repo.set_setting(&format!("reminder_attempt_time:{kind}"), &now.to_string())?;
        if automatic {
            repo.set_setting(&daily, &today)?;
        }
        // Record before sending: ambiguous network outcomes never trigger automatic duplicate sends.
        repo.set_setting(&format!("reminder_receipt:{kind}"),&json!({"at":command_center_core::now(),"status":"attempting","automatic":automatic,"notice":"Outcome unknown until provider response"}).to_string())?;
    }
    let work = async {
        authorized(state, &c, epoch)?;
        let url = match kind {
            "email" => reqwest::Url::parse("https://api.sendgrid.com/v3/mail/send"),
            "sms" | "voice" => reqwest::Url::parse(&format!(
                "https://api.twilio.com/2010-04-01/Accounts/{}/{}.json",
                c.account,
                if kind == "sms" { "Messages" } else { "Calls" }
            )),
            "native_push" => {
                reqwest::Url::parse(&format!("{}/v1/push/send", c.origin.trim_end_matches('/')))
            }
            _ => reqwest::Url::parse(&c.origin),
        }
        .map_err(|_| "Invalid provider endpoint")?;
        let client = crate::research::public_client(&url).await?;
        authorized(state, &c, epoch)?;
        if cancel.is_cancelled() {
            return Err("Reminder cancelled before submission".into());
        }
        let request=match kind{
 "email"=>client.post(url).bearer_auth(&c.secret).json(&json!({"personalizations":[{"to":[{"email":c.to}]}],"from":{"email":c.from},"subject":"Personal Command Center reminder","content":[{"type":"text/plain","value":MESSAGE}]})),
 "sms"=>client.post(url).basic_auth(&c.account,Some(&c.secret)).form(&[("From",c.from.as_str()),("To",c.to.as_str()),("Body",MESSAGE)]),
 "voice"=>client.post(url).basic_auth(&c.account,Some(&c.secret)).form(&[("From",c.from.as_str()),("To",c.to.as_str()),("Twiml","<Response><Say>A task needs your attention. Open Personal Command Center to review your due tasks.</Say></Response>"),("Timeout","20"),("Record","false")]),
 "native_push"=>client.post(url).bearer_auth(&c.secret).json(&json!({"device":c.to,"event":uuid::Uuid::new_v4().to_string(),"confirmation":"SEND NATIVE PUSH","consent":true})),
 _=>client.post(url).bearer_auth(&c.secret).json(&json!({"topic":c.topic,"title":"Personal Command Center reminder","message":MESSAGE}))};
        let mut response = request
            .send()
            .await
            .map_err(|_| "Provider outcome unknown; check your provider before retrying")?;
        let status = response.status();
        let message_id = response
            .headers()
            .get("X-Message-Id")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let mut bytes = vec![];
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| "Provider response interrupted; delivery outcome unknown")?
        {
            if bytes.len() + chunk.len() > 65536 {
                return Err("Provider response exceeds limit; outcome needs review".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        if !status.is_success() {
            return Err(format!("Provider rejected reminder with HTTP {status}; no automatic retry will occur today"));
        }
        let v = serde_json::from_slice::<Value>(&bytes).unwrap_or(json!({}));
        if kind == "native_push" && v["status"] != "provider_accepted" {
            return Err("Native push host did not confirm provider acceptance; inspect its receipt before retrying".into());
        }
        Ok(
            json!({"at":command_center_core::now(),"status":"provider_accepted","provider_status":v["status"],"provider_id":v["sid"].as_str().or(v["id"].as_str()).map(str::to_owned).or(message_id),"automatic":automatic,"notice":"Provider acceptance is not confirmed delivery. Quiet hours affect automatic sends only."}),
        )
    };
    let result: Result<Value> = tokio::select! {biased;_=cancel.cancelled()=>Err("Reminder stopped locally; provider delivery may already be in progress".into()),r=work=>r};
    let receipt = match &result {
        Ok(v) => v.clone(),
        Err(e) => {
            json!({"at":command_center_core::now(),"status":"needs_review","error":e,"automatic":automatic})
        }
    };
    {
        let mut repo = state.repo.lock().map_err(|_| "Database unavailable")?;
        if epoch == state.data_epoch.load(std::sync::atomic::Ordering::SeqCst) {
            repo.set_setting(&format!("reminder_receipt:{kind}"), &receipt.to_string())?;
            let mut r = command_center_core::workspace::Record::new(
                "notifications",
                &format!("Personal {kind} reminder"),
                &repo.device_id,
            );
            r.status = if result.is_ok() {
                "submitted"
            } else {
                "needs_review"
            }
            .into();
            r.summary = receipt.to_string();
            repo.save_record(r)?;
        }
    }
    result
}
#[tauri::command]
pub async fn send_personal_reminder(
    state: State<'_, AppState>,
    kind: String,
    confirmation: String,
    consent: bool,
) -> Result<Value> {
    let required = if kind == "voice" {
        "CALL MY NUMBER"
    } else {
        "SEND MY REMINDER"
    };
    if !consent || confirmation != required {
        return Err(format!(
            "Type {required} to authorize this real provider request; charges may apply"
        ));
    }
    deliver(&state, &kind, false).await
}
pub async fn poll(app: &tauri::AppHandle) -> Result<()> {
    let state = app.state::<AppState>();
    let hour = chrono::Local::now().hour();
    let today = chrono::Local::now().date_naive().to_string();
    let kinds = {
        let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
        let prefs = crate::preferences(&repo)?;
        let quiet = if prefs.quiet_start < prefs.quiet_end {
            hour >= prefs.quiet_start && hour < prefs.quiet_end
        } else if prefs.quiet_start > prefs.quiet_end {
            hour >= prefs.quiet_start || hour < prefs.quiet_end
        } else {
            false
        };
        if quiet {
            return Ok(());
        }
        if !repo.list("", false)?.iter().any(|i| {
            i.kind == "task"
                && i.status == "active"
                && i.due.as_deref().is_some_and(|due| due <= today.as_str())
        }) {
            return Ok(());
        }
        let mut kinds = vec![];
        for kind in ["email", "sms", "voice", "push", "native_push"] {
            let p: Value = serde_json::from_str(
                &repo
                    .setting(&format!("reminder_policy:{kind}"))?
                    .unwrap_or("{}".into()),
            )
            .unwrap_or(json!({}));
            if p["enabled"] == true
                && p["daily_enabled"] == true
                && p["hour"].as_u64().is_some_and(|h| u64::from(hour) >= h)
                && repo
                    .setting(&format!("reminder_attempt_day:{kind}"))?
                    .as_deref()
                    != Some(today.as_str())
            {
                kinds.push(kind);
            }
        }
        kinds
    };
    for kind in kinds {
        let _ = deliver(&state, kind, true).await;
    }
    Ok(())
}
