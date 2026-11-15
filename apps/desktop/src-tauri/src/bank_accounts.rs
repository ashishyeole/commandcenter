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
    let saved = read(&state).ok();
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
        .set_setting("plaid_enabled", "true")
}
#[tauri::command]
pub fn disconnect_bank_connection(state: State<AppState>) -> Result<()> {
    state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .set_setting("plaid_enabled", "false")?;
    if let Some(c) = state
        .jobs
        .lock()
        .map_err(|_| "Job state unavailable")?
        .get("plaid:read")
    {
        c.cancel();
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
