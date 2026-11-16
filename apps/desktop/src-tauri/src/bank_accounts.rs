//! Advanced user-owned Plaid read adapter. Never accepts bank login credentials or performs payments.
use crate::{AppState, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::State;
use zeroize::{Zeroize, Zeroizing};
#[derive(Serialize, Deserialize)]
struct Credentials {
    client_id: String,
    secret: String,
    access_token: String,
    environment: String,
}
impl Drop for Credentials {
    fn drop(&mut self) {
        self.secret.zeroize();
        self.access_token.zeroize();
    }
}
fn entry(state: &AppState) -> Result<keyring::Entry> {
    let device = state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .device_id
        .clone();
    keyring::Entry::new("com.personal.commandcenter.plaid", &device).map_err(|e| e.to_string())
}
fn read(state: &AppState) -> Result<Credentials> {
    let raw = Zeroizing::new(
        entry(state)?
            .get_password()
            .map_err(|_| "Configure an already authorized Plaid Item first")?,
    );
    serde_json::from_str(&raw).map_err(|_| "Invalid provider credentials".into())
}
#[tauri::command]
pub fn bank_connection_status(state: State<AppState>) -> Result<Value> {
    let enabled = state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .setting("plaid_enabled")?
        .as_deref()
        == Some("true");
    let saved = if enabled { read(&state).ok() } else { None };
    Ok(
        json!({"configured":saved.is_some(),"enabled":enabled&&saved.is_some(),"environment":saved.as_ref().map(|v|v.environment.as_str()).unwrap_or("sandbox")}),
    )
}
#[tauri::command]
pub fn configure_bank_connection(
    state: State<AppState>,
    client_id: String,
    secret: String,
    access_token: String,
    environment: String,
    consent: bool,
) -> Result<()> {
    let c = Credentials {
        client_id,
        secret,
        access_token,
        environment,
    };
    if !consent
        || !["sandbox", "production"].contains(&c.environment.as_str())
        || c.client_id.len() < 8
        || c.client_id.len() > 200
        || c.secret.len() < 8
        || c.secret.len() > 4096
        || !c.access_token.starts_with("access-")
        || c.access_token.len() > 4096
        || [&c.client_id, &c.secret, &c.access_token]
            .iter()
            .any(|v| v.chars().any(char::is_whitespace))
    {
        return Err("Use provider-issued API credentials and an already consented access token; never enter bank passwords".into());
    }
    if state
        .jobs
        .lock()
        .map_err(|_| "Job state unavailable")?
        .contains_key("plaid:read")
    {
        return Err(
            "Cancel or finish the current provider read before replacing credentials".into(),
        );
    }
    let raw = Zeroizing::new(serde_json::to_string(&c).map_err(|e| e.to_string())?);
    entry(&state)?
        .set_password(&raw)
        .map_err(|_| "OS secure store refused provider credentials")?;
    state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .set_integration_flag("plaid_enabled", true)
}
#[tauri::command]
pub fn disconnect_bank_connection(state: State<AppState>) -> Result<()> {
    state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .set_integration_flag("plaid_enabled", false)?;
    if let Some(c) = state
        .jobs
        .lock()
        .map_err(|_| "Job state unavailable")?
        .get("plaid:read")
    {
        c.cancel();
    }
    match link_entry(&state)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => (),
        Err(_) => {
            return Err("Provider disabled; temporary enrollment credential removal failed".into())
        }
    }
    match entry(&state)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(_) => Err("Provider access disabled, but OS credential removal failed".into()),
    }
}
struct Guard<'a>(&'a AppState);
impl Drop for Guard<'_> {
    fn drop(&mut self) {
        if let Ok(mut jobs) = self.0.jobs.lock() {
            jobs.remove("plaid:read");
        }
    }
}
#[tauri::command]
pub async fn read_bank_connection(
    state: State<'_, AppState>,
    kind: String,
    start_date: String,
    end_date: String,
    offset: u32,
    consent: bool,
) -> Result<Value> {
    if !consent || !["accounts", "transactions"].contains(&kind.as_str()) || offset > 5000 {
        return Err(
            "Approve an accounts or transactions read; pagination is bounded to 5,100 transactions"
                .into(),
        );
    }
    let start = chrono::NaiveDate::parse_from_str(&start_date, "%Y-%m-%d")
        .map_err(|_| "Invalid start date")?;
    let end =
        chrono::NaiveDate::parse_from_str(&end_date, "%Y-%m-%d").map_err(|_| "Invalid end date")?;
    if end < start || (end - start).num_days() > 366 {
        return Err("Choose a date range of at most 366 days".into());
    }
    if state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .setting("plaid_enabled")?
        .as_deref()
        != Some("true")
    {
        return Err("Provider connection is disabled".into());
    }
    let cancel = tokio_util::sync::CancellationToken::new();
    {
        let mut jobs = state.jobs.lock().map_err(|_| "Job state unavailable")?;
        if jobs.contains_key("plaid:read") {
            return Err("A provider read is already running".into());
        }
        jobs.insert("plaid:read".into(), cancel.clone());
    }
    let _guard = Guard(&state);
    let epoch = state.data_epoch.load(std::sync::atomic::Ordering::SeqCst);
    let work = async {
        let c = read(&state)?;
        let host = if c.environment == "production" {
            "production.plaid.com"
        } else if c.environment == "sandbox" {
            "sandbox.plaid.com"
        } else {
            return Err("Invalid provider environment".into());
        };
        let mut body =
            json!({"client_id":c.client_id,"secret":c.secret,"access_token":c.access_token});
        if kind == "transactions" {
            body["start_date"] = json!(start_date);
            body["end_date"] = json!(end_date);
            body["options"] = json!({"count":100,"offset":offset});
        }
        let serialized = Zeroizing::new(serde_json::to_string(&body).map_err(|e| e.to_string())?);
        drop(body);
        let client = reqwest::Client::builder()
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(45))
            .build()
            .map_err(|e| e.to_string())?;
        let mut response = client
            .post(format!("https://{host}/{}/get", kind))
            .header("Content-Type", "application/json")
            .header("Plaid-Version", "2020-09-14")
            .body(serialized.to_string())
            .send()
            .await
            .map_err(|_| "Financial provider request failed")?;
        let status = response.status();
        let mut bytes = Zeroizing::new(vec![]);
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| "Financial response interrupted")?
        {
            if bytes.len() + chunk.len() > 4 * 1024 * 1024 {
                return Err("Financial provider response exceeds limit".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        let value: Value =
            serde_json::from_slice(&bytes).map_err(|_| "Invalid financial provider response")?;
        if !status.is_success() {
            let code = value["error_code"]
                .as_str()
                .filter(|s| s.len() < 100 && s.bytes().all(|b| b.is_ascii_uppercase() || b == b'_'))
                .unwrap_or("PROVIDER_ERROR");
            return Err(format!("Provider returned {status}: {code}. Reauthorize through your provider when needed; no automatic retries or financial actions are performed."));
        }
        let mut rows = vec![];
        if kind == "accounts" {
            for a in value["accounts"].as_array().into_iter().flatten().take(200) {
                rows.push(json!({"id":a["account_id"],"name":a["name"],"mask":a["mask"],"type":a["type"],"subtype":a["subtype"],"balances":a["balances"]}));
            }
        } else {
            for t in value["transactions"]
                .as_array()
                .into_iter()
                .flatten()
                .take(100)
            {
                rows.push(json!({"id":t["transaction_id"],"account_id":t["account_id"],"name":t["name"],"merchant":t["merchant_name"],"amount":t["amount"],"currency":t["iso_currency_code"],"unofficial_currency":t["unofficial_currency_code"],"date":t["date"],"pending":t["pending"],"pending_transaction_id":t["pending_transaction_id"]}));
            }
        }
        if state.data_epoch.load(std::sync::atomic::Ordering::SeqCst) != epoch
            || state
                .repo
                .lock()
                .map_err(|_| "Database unavailable")?
                .setting("plaid_enabled")?
                .as_deref()
                != Some("true")
        {
            return Err("Financial access revoked or workspace reset".into());
        }
        let next = offset + rows.len() as u32;
        let more = kind == "transactions"
            && value["total_transactions"]
                .as_u64()
                .is_some_and(|n| u64::from(next) < n)
            && !rows.is_empty()
            && next <= 5000;
        Ok(
            json!({"kind":kind,"environment":c.environment,"rows":rows,"fetched_at":command_center_core::now(),"next_offset":if more{Some(next)}else{None},"total_transactions":value["total_transactions"],"notice":"Sandbox records are synthetic. Account balances are provider-cached, not a live available-to-spend guarantee. Transaction pages can change while paging; pending and posted records require reconciliation. No automatic ledger import, payments or transfers."}),
        )
    };
    tokio::select! {_=cancel.cancelled()=>Err("Financial provider read cancelled".into()),r=work=>r}
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LinkSession {
    credentials: Credentials,
    link_token: String,
    expires: String,
}
impl Drop for LinkSession {
    fn drop(&mut self) {
        self.link_token.zeroize();
    }
}
fn link_entry(state: &AppState) -> Result<keyring::Entry> {
    let device = state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .device_id
        .clone();
    keyring::Entry::new("com.personal.commandcenter.plaid-link", &device).map_err(|e| e.to_string())
}
async fn plaid_post(c: &Credentials, path: &str, mut body: Value) -> Result<Value> {
    if ![
        "link/token/create",
        "link/token/get",
        "item/public_token/exchange",
    ]
    .contains(&path)
    {
        return Err("Unsupported enrollment operation".into());
    }
    let host = match c.environment.as_str() {
        "sandbox" => "sandbox.plaid.com",
        "production" => "production.plaid.com",
        _ => return Err("Invalid Plaid environment".into()),
    };
    body["client_id"] = json!(c.client_id);
    body["secret"] = json!(c.secret);
    let serialized = Zeroizing::new(body.to_string());
    drop(body);
    let client = reqwest::Client::builder()
        .https_only(true)
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(45))
        .build()
        .map_err(|e| e.to_string())?;
    let mut response = client
        .post(format!("https://{host}/{path}"))
        .header("Content-Type", "application/json")
        .header("Plaid-Version", "2020-09-14")
        .body(serialized.to_string())
        .send()
        .await
        .map_err(|_| "Plaid enrollment request failed; no automatic retry")?;
    let status = response.status();
    let mut bytes = Zeroizing::new(vec![]);
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "Enrollment response interrupted")?
    {
        if bytes.len() + chunk.len() > 1024 * 1024 {
            return Err("Enrollment response exceeds limit".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| "Invalid enrollment response")?;
    if !status.is_success() {
        let code = value["error_code"]
            .as_str()
            .filter(|s| s.len() < 100 && s.bytes().all(|b| b.is_ascii_uppercase() || b == b'_'))
            .unwrap_or("PROVIDER_ERROR");
        return Err(format!("Plaid enrollment returned {status}: {code}. Check provider product/country access; no bank password is collected here."));
    }
    Ok(value)
}
fn hosted_link_response(value: &Value) -> Result<(String, String, String)> {
    let token = value["link_token"]
        .as_str()
        .filter(|s| s.starts_with("link-") && s.len() < 1000)
        .ok_or("Provider did not return a Link token")?;
    let url = value["hosted_link_url"]
        .as_str()
        .ok_or("Provider did not return Hosted Link")?;
    let parsed = reqwest::Url::parse(url).map_err(|_| "Invalid Hosted Link URL")?;
    if parsed.scheme() != "https"
        || parsed.host_str() != Some("secure.plaid.com")
        || !parsed.path().starts_with("/hl/")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.port_or_known_default() != Some(443)
        || parsed.fragment().is_some()
    {
        return Err("Provider returned an unexpected enrollment destination".into());
    }
    let expires = value["expiration"]
        .as_str()
        .ok_or("Link expiration missing")?;
    chrono::DateTime::parse_from_rfc3339(expires).map_err(|_| "Invalid Link expiration")?;
    Ok((token.into(), url.into(), expires.into()))
}
fn completed_public_token(value: &Value) -> Result<Option<String>> {
    let mut tokens = value["link_sessions"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|s| {
            s["results"]["item_add_results"]
                .as_array()
                .into_iter()
                .flatten()
        })
        .filter_map(|r| r["public_token"].as_str())
        .filter(|s| s.starts_with("public-") && s.len() < 1000)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    tokens.sort();
    tokens.dedup();
    if tokens.len() > 1 {
        return Err(
            "This app supports one active Plaid Item. Complete a single institution Link session."
                .into(),
        );
    }
    Ok(tokens.pop())
}
#[tauri::command]
pub async fn start_bank_enrollment(
    state: State<'_, AppState>,
    client_id: String,
    secret: String,
    environment: String,
    country: String,
    consent: bool,
) -> Result<Value> {
    if !consent
        || client_id.len() < 8
        || client_id.len() > 200
        || secret.len() < 8
        || secret.len() > 4096
        || [&client_id, &secret]
            .iter()
            .any(|v| v.chars().any(char::is_whitespace))
        || country.len() != 2
        || !country.bytes().all(|b| b.is_ascii_uppercase())
    {
        return Err("Approve provider setup and use your Plaid client ID/API secret and two-letter country code, never bank passwords".into());
    }
    let device = {
        let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
        if repo.setting("plaid_enabled")?.as_deref() == Some("true") {
            return Err(
                "Disconnect the current app Item before connecting another institution".into(),
            );
        }
        repo.device_id.clone()
    };
    let cancel = tokio_util::sync::CancellationToken::new();
    {
        let mut jobs = state.jobs.lock().map_err(|_| "Jobs unavailable")?;
        if jobs.contains_key("plaid:read") {
            return Err("A financial operation is already running".into());
        }
        jobs.insert("plaid:read".into(), cancel.clone());
    }
    let _guard = Guard(&state);
    let epoch = state.data_epoch.load(std::sync::atomic::Ordering::SeqCst);
    let c = Credentials {
        client_id,
        secret,
        environment,
        access_token: String::new(),
    };
    let result = tokio::select! {_=cancel.cancelled()=>return Err("Enrollment preparation cancelled".into()),v=plaid_post(&c,"link/token/create",json!({"client_name":"Personal Command Center","language":"en","country_codes":[country],"user":{"client_user_id":device},"products":["transactions"],"hosted_link":{"url_lifetime_seconds":900}}))=>v?};
    let (token, url, expires) = hosted_link_response(&result)?;
    if state.data_epoch.load(std::sync::atomic::Ordering::SeqCst) != epoch || cancel.is_cancelled()
    {
        return Err("Workspace changed during enrollment setup".into());
    }
    let session = LinkSession {
        credentials: c,
        link_token: token,
        expires: expires.clone(),
    };
    let raw = Zeroizing::new(serde_json::to_string(&session).map_err(|e| e.to_string())?);
    link_entry(&state)?
        .set_password(&raw)
        .map_err(|_| "OS secure store refused temporary Link session")?;
    Ok(
        json!({"hosted_url":url,"expires":expires,"notice":"Open Plaid's secure page in your browser, complete institution consent, then return here. No SMS, email or app-owned callback server is used."}),
    )
}
#[tauri::command]
pub async fn complete_bank_enrollment(state: State<'_, AppState>, consent: bool) -> Result<Value> {
    if !consent {
        return Err("Approve retrieving and storing the Item you authorized at Plaid".into());
    }
    let raw = Zeroizing::new(
        link_entry(&state)?
            .get_password()
            .map_err(|_| "Start a new enrollment session")?,
    );
    let session: LinkSession =
        serde_json::from_str(&raw).map_err(|_| "Invalid local Link session")?;
    if chrono::DateTime::parse_from_rfc3339(&session.expires).map_err(|_| "Invalid expiration")?
        < chrono::Utc::now()
    {
        return Err("Link expired; start a new session".into());
    }
    let cancel = tokio_util::sync::CancellationToken::new();
    {
        let mut jobs = state.jobs.lock().map_err(|_| "Jobs unavailable")?;
        if jobs.contains_key("plaid:read") {
            return Err("A financial operation is already running".into());
        }
        jobs.insert("plaid:read".into(), cancel.clone());
    }
    let _guard = Guard(&state);
    let epoch = state.data_epoch.load(std::sync::atomic::Ordering::SeqCst);
    let data = tokio::select! {_=cancel.cancelled()=>return Err("Enrollment completion cancelled".into()),v=plaid_post(&session.credentials,"link/token/get",json!({"link_token":session.link_token}))=>v?};
    let Some(public_token) = completed_public_token(&data)? else {
        return Ok(
            json!({"completed":false,"notice":"Plaid has not reported a completed single-institution session yet. Finish in your browser, then check again."}),
        );
    };
    let data = tokio::select! {_=cancel.cancelled()=>return Err("Local completion cancelled; provider work may already have occurred".into()),v=plaid_post(&session.credentials,"item/public_token/exchange",json!({"public_token":public_token}))=>v?};
    let access = data["access_token"]
        .as_str()
        .filter(|v| v.starts_with("access-") && v.len() < 4096)
        .ok_or("No supported access token returned")?;
    if state.data_epoch.load(std::sync::atomic::Ordering::SeqCst) != epoch || cancel.is_cancelled()
    {
        return Err("Workspace reset or completion cancelled; reconnect if needed".into());
    }
    let current = Zeroizing::new(
        link_entry(&state)?
            .get_password()
            .map_err(|_| "Enrollment permission/session removed")?,
    );
    if *current != *raw {
        return Err("Enrollment session changed; restart".into());
    }
    let c = Credentials {
        client_id: session.credentials.client_id.clone(),
        secret: session.credentials.secret.clone(),
        environment: session.credentials.environment.clone(),
        access_token: access.into(),
    };
    let value = Zeroizing::new(serde_json::to_string(&c).map_err(|e| e.to_string())?);
    entry(&state)?
        .set_password(&value)
        .map_err(|_| "OS secure store could not save authorized Item")?;
    state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .set_integration_flag("plaid_enabled", true)?;
    let _ = link_entry(&state)?.delete_credential();
    Ok(
        json!({"completed":true,"notice":"Authorized Item saved in the OS secure store. Reads still require separate consent; no account data was imported or payment made."}),
    )
}
#[tauri::command]
pub fn cancel_bank_enrollment(state: State<AppState>) -> Result<()> {
    if let Some(token) = state
        .jobs
        .lock()
        .map_err(|_| "Jobs unavailable")?
        .get("plaid:read")
    {
        token.cancel();
    }
    match link_entry(&state)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(_) => Err("Could not remove temporary Link session from OS store".into()),
    }
}
#[cfg(test)]
mod enrollment_tests {
    use super::*;
    #[test]
    fn hosted_url_and_completed_tokens_are_validated() {
        assert!(hosted_link_response(&json!({"link_token":"link-sandbox-fixture","hosted_link_url":"https://evil.example/hl/test","expiration":"2026-11-16T20:00:00Z"})).is_err());
        assert!(hosted_link_response(&json!({"link_token":"link-sandbox-fixture","hosted_link_url":"https://secure.plaid.com/hl/test","expiration":"2026-11-16T20:00:00Z"})).is_ok());
        assert!(completed_public_token(&json!({"link_sessions":[]}))
            .unwrap()
            .is_none());
        assert!(completed_public_token(&json!({"link_sessions":[{"results":{"item_add_results":[{"public_token":"public-a"},{"public_token":"public-b"}]}}]})).is_err());
    }
}
