//! Optional hosted services; never required for local repositories or peer sync.
use crate::{AppState, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tauri::State;
use zeroize::Zeroizing;
fn entry(state: &AppState) -> Result<keyring::Entry> {
    let device = state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .device_id
        .clone();
    keyring::Entry::new("com.personal.commandcenter.optional-host", &device)
        .map_err(|e| e.to_string())
}
fn endpoint(value: &str) -> Result<reqwest::Url> {
    let url = reqwest::Url::parse(value).map_err(|_| "Enter the HTTPS service origin")?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
        || value.len() > 2000
    {
        return Err("Use an HTTPS service origin without a path, query or credentials".into());
    }
    Ok(url)
}
#[tauri::command]
pub fn hosted_status(state: State<AppState>) -> Result<Value> {
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    Ok(
        json!({"origin":repo.setting("hosted_origin")?,"enabled":repo.setting("hosted_enabled")?.as_deref()==Some("true")}),
    )
}
#[tauri::command]
pub fn configure_hosted(
    state: State<AppState>,
    origin: String,
    token: String,
    consent: bool,
) -> Result<()> {
    if !consent {
        return Err("Approve connecting to your optional hosted service".into());
    }
    let origin = endpoint(&origin)?.to_string();
    let token = Zeroizing::new(token);
    if token.len() < 32 || token.len() > 512 || token.chars().any(char::is_whitespace) {
        return Err("Use an operator-issued strong access token of at least 32 characters".into());
    }
    entry(&state)?
        .set_password(&json!({"origin":origin,"token":token.as_str()}).to_string())
        .map_err(|e| e.to_string())?;
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    repo.set_setting("hosted_origin", &origin)?;
    repo.set_setting("hosted_enabled", "true")
}
#[tauri::command]
pub fn disconnect_hosted(state: State<AppState>) -> Result<()> {
    state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .set_setting("hosted_enabled", "false")?;
    match entry(&state)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}
async fn request(
    state: &AppState,
    method: reqwest::Method,
    path: &str,
    body: Option<Value>,
) -> Result<Value> {
    let (base, epoch) = {
        let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
        if repo.setting("hosted_enabled")?.as_deref() != Some("true") {
            return Err("Optional hosted service is disabled".into());
        }
        (
            repo.setting("hosted_origin")?
                .ok_or("No hosted service configured")?,
            state.data_epoch.load(std::sync::atomic::Ordering::SeqCst),
        )
    };
    let token = Zeroizing::new(
        entry(state)?
            .get_password()
            .map_err(|_| "Hosted credential unavailable")?,
    );
    let credential: Value =
        serde_json::from_str(&token).map_err(|_| "Reconnect the optional hosted service")?;
    if credential["origin"].as_str() != Some(base.as_str()) {
        return Err("Hosted endpoint changed; reconnect before sending credentials".into());
    }
    let bearer = credential["token"]
        .as_str()
        .ok_or("Invalid hosted credential")?;
    let url = endpoint(&base)?.join(path).map_err(|e| e.to_string())?;
    let client = crate::research::public_client(&url).await?;
    let mut req = client.request(method, url).bearer_auth(bearer);
    if let Some(body) = body {
        req = req.json(&body);
    }
    let mut response = req
        .send()
        .await
        .map_err(|_| "Optional hosted service request failed")?;
    let status = response.status();
    let mut bytes = vec![];
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "Hosted response interrupted")?
    {
        if bytes.len() + chunk.len() > 10 * 1024 * 1024 {
            return Err("Hosted response exceeds limit".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    if epoch != state.data_epoch.load(std::sync::atomic::Ordering::SeqCst)
        || repo.setting("hosted_enabled")?.as_deref() != Some("true")
        || repo.setting("hosted_origin")?.as_deref() != Some(base.as_str())
    {
        return Err("Hosted authorization changed while request was in progress; remote effects may already have occurred".into());
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| "Invalid hosted response")?;
    if !status.is_success() {
        return Err(format!(
            "Hosted HTTP {}: {}",
            status,
            value["error"].as_str().unwrap_or("Request failed")
        ));
    }
    Ok(value)
}
fn id(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 100
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err("Invalid hosted object ID".into());
    }
    Ok(())
}
#[tauri::command]
pub async fn hosted_read(
    state: State<'_, AppState>,
    resource: String,
    consent: bool,
) -> Result<Value> {
    if !consent {
        return Err("Approve contacting the optional host".into());
    }
    let path = match resource.as_str() {
        "identity" => "/v1/me",
        "jobs" => "/v1/jobs",
        "relay" => "/v1/relay",
        "audit" => "/v1/audit",
        "catalog_pending" => "/v1/catalog/pending",
        _ => return Err("Unsupported hosted resource".into()),
    };
    request(&state, reqwest::Method::GET, path, None).await
}
#[tauri::command]
pub async fn hosted_save_job(
    state: State<'_, AppState>,
    job: Value,
    consent: bool,
) -> Result<Value> {
    if !consent {
        return Err("Approve this always-on public source collector".into());
    }
    if serde_json::to_vec(&job).map_err(|e| e.to_string())?.len() > 8192 {
        return Err("Collector configuration too large".into());
    }
    request(&state, reqwest::Method::POST, "/v1/jobs", Some(job)).await
}
#[tauri::command]
pub async fn hosted_delete(
    state: State<'_, AppState>,
    resource: String,
    object_id: String,
    consent: bool,
) -> Result<Value> {
    if !consent {
        return Err("Approve deletion of the selected hosted object".into());
    }
    id(&object_id)?;
    let resource = match resource.as_str() {
        "jobs" => "jobs",
        "relay" => "relay",
        _ => return Err("Unsupported hosted object".into()),
    };
    request(
        &state,
        reqwest::Method::DELETE,
        &format!("/v1/{resource}/{object_id}"),
        None,
    )
    .await
}
#[tauri::command]
pub async fn hosted_send_archive(
    state: State<'_, AppState>,
    recipient: String,
    password: String,
    consent: bool,
) -> Result<Value> {
    if !consent {
        return Err("Approve uploading the selected encrypted archive to this recipient".into());
    }
    id(&recipient)?;
    let password = Zeroizing::new(password);
    let Some(file) = rfd::AsyncFileDialog::new()
        .set_title(
            "Choose an encrypted archive to send through your optional host (128 KiB maximum)",
        )
        .pick_file()
        .await
    else {
        return Ok(json!({"cancelled":true}));
    };
    if std::fs::metadata(file.path())
        .map_err(|e| e.to_string())?
        .len()
        > 131072
    {
        return Err("Relay envelopes are limited to 128 KiB; use peer sync or user-owned backup for larger data".into());
    }
    let bytes = std::fs::read(file.path()).map_err(|e| e.to_string())?;
    if bytes.len() > 131072 {
        return Err("Archive exceeds limit".into());
    }
    let plain = Zeroizing::new(command_center_core::archive::decrypt(&bytes, &password)?);
    command_center_core::archive::inspect_all(&plain)?;
    request(&state,reqwest::Method::POST,"/v1/relay",Some(json!({"id":uuid::Uuid::new_v4().to_string(),"recipient":recipient,"format":"pcc-encrypted-archive-v1","ciphertext_base64":STANDARD.encode(&bytes),"ttl_seconds":86400}))).await
}
#[tauri::command]
pub async fn hosted_download_archive(
    state: State<'_, AppState>,
    object_id: String,
    consent: bool,
) -> Result<Value> {
    if !consent {
        return Err("Approve downloading this encrypted archive".into());
    }
    id(&object_id)?;
    let result = request(&state, reqwest::Method::GET, "/v1/relay", None).await?;
    let messages = result["messages"]
        .as_array()
        .ok_or("Invalid relay response")?;
    let message = messages
        .iter()
        .find(|v| v["id"].as_str() == Some(object_id.as_str()))
        .ok_or("Relay envelope no longer available")?;
    let bytes = STANDARD
        .decode(
            message["ciphertext_base64"]
                .as_str()
                .ok_or("Missing encrypted data")?,
        )
        .map_err(|_| "Invalid encrypted data")?;
    if bytes.len() > 131072
        || !bytes.starts_with(b"PACC01")
        || message["sha256"].as_str() != Some(format!("{:x}", Sha256::digest(&bytes)).as_str())
    {
        return Err("Invalid relay envelope or digest".into());
    }
    let Some(file) = rfd::AsyncFileDialog::new()
        .set_title("Save encrypted archive; restore separately after verifying the sender")
        .set_file_name("received-encrypted-backup.pacc")
        .save_file()
        .await
    else {
        return Ok(json!({"cancelled":true}));
    };
    crate::atomic_write(file.path(), &bytes)?;
    Ok(
        json!({"saved":true,"notice":"The password is required for authenticated restore. This download did not import or acknowledge the message."}),
    )
}

#[tauri::command]
pub async fn browse_extension_catalog(origin: String, consent: bool) -> Result<Value> {
    if !consent {
        return Err("Approve contacting this public extension catalog".into());
    }
    let url = endpoint(&origin)?
        .join("/v1/catalog")
        .map_err(|e| e.to_string())?;
    let mut response = crate::research::public_client(&url)
        .await?
        .get(url)
        .send()
        .await
        .map_err(|_| "Catalog unavailable")?;
    if !response.status().is_success() {
        return Err(format!("Catalog returned HTTP {}", response.status()));
    }
    let mut bytes = vec![];
    while let Some(chunk) = response.chunk().await.map_err(|_| "Catalog interrupted")? {
        if bytes.len() + chunk.len() > 4 * 1024 * 1024 {
            return Err("Catalog exceeds 4 MiB".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| "Invalid catalog JSON".into())
}
#[tauri::command]
pub async fn hosted_catalog_action(
    state: State<'_, AppState>,
    action: String,
    payload: Value,
    confirmation: String,
    consent: bool,
) -> Result<Value> {
    if !consent
        || serde_json::to_vec(&payload)
            .map_err(|e| e.to_string())?
            .len()
            > 40000
    {
        return Err("Approve a bounded catalog request".into());
    }
    let path = match action.as_str() {
        "submit" if confirmation == "SUBMIT PUBLIC PACKAGE" => "/v1/catalog/submit",
        "moderate" if confirmation == "MODERATE EXACT PACKAGE" => "/v1/catalog/moderate",
        "withdraw" if confirmation == "WITHDRAW PUBLIC PACKAGE" => "/v1/catalog/withdraw",
        _ => return Err("Use the required typed confirmation for this catalog action".into()),
    };
    request(&state, reqwest::Method::POST, path, Some(payload)).await
}
#[tauri::command]
pub async fn hosted_export(state: State<'_, AppState>, consent: bool) -> Result<Value> {
    if !consent {
        return Err("Approve exporting your hosted tenant data".into());
    }
    let result = request(&state, reqwest::Method::GET, "/v1/export", None).await?;
    let Some(file) = rfd::AsyncFileDialog::new()
        .set_title("Save hosted tenant data — metadata and public collectors are plaintext")
        .set_file_name("hosted-tenant-export.json")
        .save_file()
        .await
    else {
        return Ok(json!({"cancelled":true}));
    };
    crate::atomic_write(
        file.path(),
        &serde_json::to_vec_pretty(&result).map_err(|e| e.to_string())?,
    )?;
    Ok(json!({"saved":true}))
}
#[tauri::command]
pub async fn hosted_reset(state: State<'_, AppState>, confirmation: String) -> Result<Value> {
    if confirmation != "DELETE HOSTED TENANT DATA" {
        return Err("Type DELETE HOSTED TENANT DATA after exporting any data you need".into());
    }
    request(
        &state,
        reqwest::Method::POST,
        "/v1/reset",
        Some(json!({"confirmation":confirmation})),
    )
    .await
}
