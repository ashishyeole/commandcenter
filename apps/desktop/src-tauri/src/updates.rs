use crate::{AppState, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tauri::State;
use tauri_plugin_updater::UpdaterExt;
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub manifest_url: String,
    pub public_key: String,
    pub enabled: bool,
}
#[derive(Clone)]
pub struct Candidate {
    update: tauri_plugin_updater::Update,
    hash: String,
    config_hash: String,
    epoch: u64,
}
fn config(state: &AppState) -> Result<Option<Config>> {
    state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .setting("update_publisher")?
        .map(|s| serde_json::from_str(&s).map_err(|_| "Invalid updater configuration".into()))
        .transpose()
}
fn hash(v: &impl Serialize) -> Result<String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(v).map_err(|e| e.to_string())?)
    ))
}
fn validate(c: &Config) -> Result<reqwest::Url> {
    let url =
        reqwest::Url::parse(&c.manifest_url).map_err(|_| "Enter an HTTPS update manifest URL")?;
    if c.manifest_url.len() > 2000
        || url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || c.public_key.len() > 4096
    {
        return Err(
            "Use a direct public HTTPS manifest without credentials, query or fragment".into(),
        );
    }
    let bytes = STANDARD
        .decode(c.public_key.trim())
        .map_err(|_| "Use the base64-encoded Tauri updater public key")?;
    let key = std::str::from_utf8(&bytes).map_err(|_| "Invalid publisher public key")?;
    minisign_verify::PublicKey::decode(key).map_err(|_| "Invalid Minisign publisher public key")?;
    Ok(url)
}
#[tauri::command]
pub fn updater_status(state: State<AppState>) -> Result<Value> {
    let c = config(&state)?;
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    Ok(
        json!({"configuration":c,"current_version":env!("CARGO_PKG_VERSION"),"progress":repo.setting("update_progress")?.and_then(|v|serde_json::from_str::<Value>(&v).ok())}),
    )
}
#[tauri::command]
pub fn save_update_publisher(
    state: State<AppState>,
    configuration: Config,
    confirmation: String,
) -> Result<()> {
    validate(&configuration)?;
    if confirmation != "TRUST UPDATE PUBLISHER" {
        return Err(
            "Type TRUST UPDATE PUBLISHER after independently verifying the signing key".into(),
        );
    }
    if state
        .jobs
        .lock()
        .map_err(|_| "Job state unavailable")?
        .contains_key("update:download")
    {
        return Err("Finish or cancel the active update before changing trust".into());
    }
    state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .set_setting(
            "update_publisher",
            &serde_json::to_string(&configuration).map_err(|e| e.to_string())?,
        )?;
    *state
        .pending_update
        .lock()
        .map_err(|_| "Update state unavailable")? = None;
    Ok(())
}
#[tauri::command]
pub fn disable_updates(state: State<AppState>) -> Result<()> {
    let active = state
        .jobs
        .lock()
        .map_err(|_| "Job state unavailable")?
        .contains_key("update:download");
    if active
        && state
            .repo
            .lock()
            .map_err(|_| "Database unavailable")?
            .setting("update_progress")?
            .is_some_and(|v| v.contains("\"installing\""))
    {
        return Err("Installation has started and cannot be cancelled".into());
    }
    if let Some(mut c) = config(&state)? {
        c.enabled = false;
        state
            .repo
            .lock()
            .map_err(|_| "Database unavailable")?
            .set_setting(
                "update_publisher",
                &serde_json::to_string(&c).map_err(|e| e.to_string())?,
            )?;
    }
    if let Some(token) = state
        .jobs
        .lock()
        .map_err(|_| "Job state unavailable")?
        .get("update:download")
    {
        token.cancel();
    }
    *state
        .pending_update
        .lock()
        .map_err(|_| "Update state unavailable")? = None;
    Ok(())
}
#[tauri::command]
pub async fn check_signed_update(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    consent: bool,
) -> Result<Value> {
    if !consent {
        return Err("Approve contacting your configured update publisher".into());
    }
    let c = config(&state)?
        .filter(|c| c.enabled)
        .ok_or("Configure and explicitly trust an update publisher first")?;
    let url = validate(&c)?;
    let addresses = crate::research::public_addresses(&url).await?;
    let host = url.host_str().ok_or("Missing update host")?.to_owned();
    let epoch = state.data_epoch.load(std::sync::atomic::Ordering::SeqCst);
    let config_hash = hash(&c)?;
    let update = app
        .updater_builder()
        .endpoints(vec![url])
        .map_err(|e| e.to_string())?
        .pubkey(c.public_key.clone())
        .timeout(std::time::Duration::from_secs(30))
        .no_proxy()
        .configure_client(move |builder| {
            builder
                .https_only(true)
                .no_proxy()
                .redirect(reqwest_updater::redirect::Policy::none())
                .resolve_to_addrs(&host, &addresses)
        })
        .build()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| format!("Signed update check failed: {e}"))?;
    if epoch != state.data_epoch.load(std::sync::atomic::Ordering::SeqCst)
        || config(&state)?.as_ref().map(hash).transpose()?.as_deref() != Some(config_hash.as_str())
    {
        return Err("Update trust changed or workspace reset during check".into());
    }
    let Some(update) = update else {
        *state
            .pending_update
            .lock()
            .map_err(|_| "Update state unavailable")? = None;
        return Ok(json!({"available":false,"current_version":env!("CARGO_PKG_VERSION")}));
    };
    if serde_json::to_vec(&update.raw_json)
        .map_err(|e| e.to_string())?
        .len()
        > 1024 * 1024
    {
        return Err("Update metadata exceeds the 1 MiB limit".into());
    }
    crate::research::public_addresses(&update.download_url).await?;
    if epoch != state.data_epoch.load(std::sync::atomic::Ordering::SeqCst)
        || config(&state)?.as_ref().map(hash).transpose()?.as_deref() != Some(config_hash.as_str())
    {
        return Err("Update authorization changed during check".into());
    }
    let payload = json!({"version":update.version,"current_version":update.current_version,"target":update.target,"notes":update.body,"download_url":update.download_url.as_str(),"signature":update.signature,"publisher_fingerprint":format!("{:x}",Sha256::digest(STANDARD.decode(c.public_key.trim()).map_err(|_|"Invalid public key")?))});
    let review = hash(&payload)?;
    *state
        .pending_update
        .lock()
        .map_err(|_| "Update state unavailable")? = Some(Candidate {
        update,
        hash: review.clone(),
        config_hash,
        epoch,
    });
    Ok(
        json!({"available":true,"payload":payload,"sha256":review,"notice":"Metadata is a publisher claim. Package signature is verified before installation. Save an encrypted backup first. Installation replaces the app and may close it."}),
    )
}
struct Guard<'a>(&'a AppState);
impl Drop for Guard<'_> {
    fn drop(&mut self) {
        if let Ok(mut jobs) = self.0.jobs.lock() {
            jobs.remove("update:download");
        }
    }
}
#[tauri::command]
pub async fn install_signed_update(
    state: State<'_, AppState>,
    reviewed_sha256: String,
    confirmation: String,
    consent: bool,
) -> Result<()> {
    let candidate = state
        .pending_update
        .lock()
        .map_err(|_| "Update state unavailable")?
        .clone()
        .ok_or("Check for an update first")?;
    if !consent || candidate.hash != reviewed_sha256 || candidate.update.version != confirmation {
        return Err("Review the update and type its exact version to approve installation".into());
    }
    let current = config(&state)?
        .filter(|c| c.enabled)
        .ok_or("Update publisher disabled")?;
    if hash(&current)? != candidate.config_hash
        || candidate.epoch != state.data_epoch.load(std::sync::atomic::Ordering::SeqCst)
    {
        return Err("Update approval expired; check again".into());
    }
    let cancel = tokio_util::sync::CancellationToken::new();
    {
        let mut jobs = state.jobs.lock().map_err(|_| "Job state unavailable")?;
        if !jobs.is_empty() {
            return Err("Finish or cancel current AI, tool and model work before updating".into());
        }
        jobs.insert("update:download".into(), cancel.clone());
    }
    let _guard = Guard(&state);
    let work = async {
        let url = candidate.update.download_url.clone();
        let addresses = crate::research::public_addresses(&url).await?;
        let client = reqwest::Client::builder()
            .https_only(true)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .resolve_to_addrs(url.host_str().ok_or("Missing download host")?, &addresses)
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .map_err(|e| e.to_string())?;
        let mut response = client
            .get(url)
            .send()
            .await
            .map_err(|_| "Update download failed")?;
        if !response.status().is_success() {
            return Err(format!("Update download returned HTTP {}. Use a direct HTTPS artifact URL without redirects.",response.status()));
        }
        let total = response.content_length();
        if total.is_some_and(|n| n > 512 * 1024 * 1024) {
            return Err("Update exceeds 512 MiB".into());
        }
        let mut bytes = vec![];
        let mut last = std::time::Instant::now() - std::time::Duration::from_secs(1);
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| "Update download interrupted")?
        {
            if bytes.len() + chunk.len() > 512 * 1024 * 1024 {
                return Err("Update exceeds 512 MiB".into());
            }
            bytes.extend_from_slice(&chunk);
            if last.elapsed() >= std::time::Duration::from_millis(500) {
                let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
                if state.data_epoch.load(std::sync::atomic::Ordering::SeqCst) != candidate.epoch {
                    return Err("Workspace reset during download".into());
                }
                repo.set_setting(
                    "update_progress",
                    &json!({"status":"downloading","received":bytes.len(),"total":total})
                        .to_string(),
                )?;
                last = std::time::Instant::now();
            }
        }
        Ok(bytes)
    };
    let downloaded: Result<Vec<u8>> = tokio::select! {_=cancel.cancelled()=>Err("Update download cancelled; no installation started".into()),r=work=>r};
    let bytes = match downloaded {
        Ok(bytes) => bytes,
        Err(e) => {
            if candidate.epoch == state.data_epoch.load(std::sync::atomic::Ordering::SeqCst) {
                state
                    .repo
                    .lock()
                    .map_err(|_| "Database unavailable")?
                    .set_setting(
                        "update_progress",
                        &json!({"status":"failed","error":e}).to_string(),
                    )?;
            }
            return Err(e);
        }
    };
    if cancel.is_cancelled()
        || candidate.epoch != state.data_epoch.load(std::sync::atomic::Ordering::SeqCst)
        || config(&state)?.as_ref().map(hash).transpose()?.as_deref()
            != Some(candidate.config_hash.as_str())
    {
        return Err("Update authorization changed before installation".into());
    }
    {
        let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
        let latest: Option<Config> = repo
            .setting("update_publisher")?
            .map(|v| serde_json::from_str(&v).map_err(|_| "Invalid publisher configuration"))
            .transpose()?;
        if cancel.is_cancelled()
            || candidate.epoch != state.data_epoch.load(std::sync::atomic::Ordering::SeqCst)
            || latest.as_ref().map(hash).transpose()?.as_deref()
                != Some(candidate.config_hash.as_str())
        {
            return Err("Update authorization changed before installation".into());
        }
        repo.set_setting("update_progress", &json!({"status":"installing","version":candidate.update.version,"sha256":format!("{:x}",Sha256::digest(&bytes))}).to_string())?;
    }
    // The official updater verifies Minisign against the approved public key inside install().
    let installed = tauri::async_runtime::spawn_blocking(move || {
        candidate
            .update
            .install(&bytes)
            .map_err(|e| format!("Signature verification or installation failed: {e}"))
    })
    .await
    .map_err(|_| "Installer worker interrupted")?;
    state.repo.lock().map_err(|_|"Database unavailable")?.set_setting("update_progress",&json!({"status":if installed.is_ok(){"installed_restart_required"}else{"failed"},"error":installed.as_ref().err()}).to_string())?;
    *state
        .pending_update
        .lock()
        .map_err(|_| "Update state unavailable")? = None;
    installed
}
