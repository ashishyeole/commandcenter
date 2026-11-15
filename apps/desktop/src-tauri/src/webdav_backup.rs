//! Explicitly authorized HTTPS WebDAV destination. Only ciphertext leaves the device.
use crate::{AppState, Result};
use command_center_core::{
    archive::{self, ExportData},
    now,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tauri::State;
use zeroize::Zeroizing;

#[derive(Serialize, Deserialize)]
struct Credentials {
    username: String,
    password: String,
}
fn entry(device: &str) -> Result<keyring::Entry> {
    keyring::Entry::new("com.personal.commandcenter.webdav", device).map_err(|e| e.to_string())
}
fn endpoint(value: &str) -> Result<reqwest::Url> {
    let url = reqwest::Url::parse(value).map_err(|_| "Enter a valid HTTPS folder URL")?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !url.path().ends_with('/')
    {
        return Err(
            "Use an HTTPS folder URL ending in /, without credentials, query or fragment".into(),
        );
    }
    Ok(url)
}
#[tauri::command]
pub fn save_webdav_destination(
    state: State<AppState>,
    url: String,
    username: String,
    password: String,
) -> Result<()> {
    let _guard = state
        .backup_lock
        .try_lock()
        .map_err(|_| "Wait for the active backup")?;
    let url = endpoint(&url)?;
    let password = Zeroizing::new(password);
    if username.len() > 1024 || password.is_empty() || password.len() > 8192 {
        return Err("Supply a username and an app password of at most 8192 bytes".into());
    }
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    let secret = Zeroizing::new(
        serde_json::to_string(&Credentials {
            username,
            password: password.to_string(),
        })
        .map_err(|e| e.to_string())?,
    );
    entry(&repo.device_id)?
        .set_password(&secret)
        .map_err(|e| format!("Secure store refused WebDAV credentials: {e}"))?;
    repo.set_setting("backup_webdav", url.as_str())?;
    // A new destination always requires a fresh scope approval.
    repo.set_setting("backup_schedule", &json!({"backend":"webdav","enabled":false,"approved":false,"interval_hours":24,"include_vault":false,"retain":0}).to_string())
}
pub fn forget(device: &str) -> Result<()> {
    match entry(device)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}
async fn read_bounded(mut response: reqwest::Response) -> Result<(Vec<u8>, Option<String>)> {
    if !response.status().is_success() {
        return Err(format!(
            "WebDAV read failed with HTTP {}",
            response.status()
        ));
    }
    let limit = 64 * 1024 * 1024 + 50;
    if response.content_length().is_some_and(|n| n > limit as u64) {
        return Err("Remote archive exceeds the supported limit".into());
    }
    let etag = response
        .headers()
        .get(reqwest::header::ETAG)
        .and_then(|s| s.to_str().ok())
        .filter(|s| s.starts_with('"') && s.ends_with('"'))
        .map(str::to_owned);
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "WebDAV response interrupted")?
    {
        if bytes.len() + chunk.len() > limit {
            return Err("Remote archive exceeds the supported limit".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok((bytes, etag))
}
pub async fn execute(
    state: &AppState,
    config: &crate::backups::BackupSchedule,
    mut data: ExportData,
    device: &str,
    destination: &str,
    epoch: u64,
    vault_rows: Vec<(String, Vec<u8>)>,
) -> Result<Value> {
    let root = endpoint(destination)?;
    let password = Zeroizing::new(
        crate::backups::key(device)?
            .get_password()
            .map_err(|_| "Backup recovery password unavailable")?,
    );
    if config.include_vault {
        let key = Zeroizing::new(
            keyring::Entry::new("com.personal.commandcenter.vault", device)
                .map_err(|e| e.to_string())?
                .get_secret()
                .map_err(|_| "Vault key unavailable")?,
        );
        for (id, bytes) in vault_rows {
            data.vault.push(crate::vault::reveal(&key, &id, &bytes)?);
        }
    }
    let count = data.items.len()
        + data.workspace.records.len()
        + data.workspace.attachments.len()
        + data.vault.len();
    let zip = Zeroizing::new(archive::export_all(&data)?);
    archive::inspect_all(&zip)?;
    let bytes = archive::encrypt(&zip, &password)?;
    let digest = format!("{:x}", Sha256::digest(&bytes));
    let name = format!(
        "command-center-{device}-{}-{}.pacc",
        chrono::Utc::now().format("%Y%m%dT%H%M%SZ"),
        uuid::Uuid::new_v4()
    );
    let target = root.join(&name).map_err(|e| e.to_string())?;
    let secret = Zeroizing::new(
        entry(device)?
            .get_password()
            .map_err(|_| "WebDAV credentials unavailable; configure destination again")?,
    );
    let credentials: Credentials =
        serde_json::from_str(&secret).map_err(|_| "Invalid stored credentials")?;
    let remote_password = Zeroizing::new(credentials.password);
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(180))
        .build()
        .map_err(|e| e.to_string())?;
    let request = |method, url: reqwest::Url| {
        client
            .request(method, url)
            .basic_auth(&credentials.username, Some(remote_password.as_str()))
    };
    if state.data_epoch.load(std::sync::atomic::Ordering::SeqCst) != epoch {
        return Err("Data reset; upload cancelled".into());
    }
    let size = bytes.len();
    let uploaded = request(reqwest::Method::PUT, target.clone())
        .header(reqwest::header::IF_NONE_MATCH, "*")
        .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
        .body(bytes)
        .send()
        .await
        .map_err(|_| "WebDAV upload failed; check connectivity and destination")?;
    if !uploaded.status().is_success() {
        return Err(format!(
            "WebDAV upload failed with HTTP {}",
            uploaded.status()
        ));
    }
    let (readback, _) = read_bounded(
        request(reqwest::Method::GET, target.clone())
            .send()
            .await
            .map_err(|_| "WebDAV verification download failed")?,
    )
    .await?;
    if format!("{:x}", Sha256::digest(&readback)) != digest {
        return Err("Uploaded archive failed checksum verification".into());
    }
    archive::inspect_all(&Zeroizing::new(archive::decrypt(&readback, &password)?))?;
    let info = json!({"sha256":digest,"verified_at":now(),"path":target.as_str(),"backend":"webdav","encrypted":true,"records":count,"includes_vault":config.include_vault,"modules":[],"bytes":size});
    let mut owned: Vec<Value> = {
        let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
        if state.data_epoch.load(std::sync::atomic::Ordering::SeqCst) != epoch {
            return Err("Data reset during upload; encrypted remote file may remain".into());
        }
        repo.set_setting("last_backup", &info.to_string())?;
        let mut owned: Vec<Value> =
            serde_json::from_str(&repo.setting("scheduled_archives")?.unwrap_or("[]".into()))
                .map_err(|e| e.to_string())?;
        owned.push(info.clone());
        repo.set_setting(
            "scheduled_archives",
            &serde_json::to_string(&owned).map_err(|e| e.to_string())?,
        )?;
        owned
    };
    let belongs = |v: &Value| {
        v["backend"] == "webdav"
            && v["path"]
                .as_str()
                .and_then(|p| reqwest::Url::parse(p).ok())
                .is_some_and(|u| {
                    u.join(".").ok() == Some(root.clone())
                        && u.path_segments()
                            .and_then(|mut s| s.next_back())
                            .is_some_and(|n| {
                                n.starts_with(&format!("command-center-{device}-"))
                                    && n.ends_with(".pacc")
                            })
                })
    };
    let mut remaining = if config.retain == 0 {
        0
    } else {
        owned
            .iter()
            .filter(|v| belongs(v))
            .count()
            .saturating_sub(config.retain as usize)
    };
    let mut warnings = vec![];
    let mut removed = vec![];
    for (index, old) in owned.iter().enumerate() {
        if remaining == 0 {
            break;
        }
        if !belongs(old) {
            continue;
        }
        if state.data_epoch.load(std::sync::atomic::Ordering::SeqCst) != epoch {
            return Err("Data reset; remote retention stopped".into());
        }
        let url = reqwest::Url::parse(old["path"].as_str().unwrap()).map_err(|e| e.to_string())?;
        let result = async {
            let response = request(reqwest::Method::GET, url.clone())
                .send()
                .await
                .map_err(|_| "Retention read failed")?;
            if response.status() == reqwest::StatusCode::NOT_FOUND {
                return Ok(());
            }
            let (bytes, etag) = read_bounded(response).await?;
            if old["sha256"].as_str() != Some(format!("{:x}", Sha256::digest(&bytes)).as_str()) {
                return Err("Older remote archive changed; kept it".to_string());
            }
            let etag = etag.ok_or("Server has no strong ETag; kept older archive")?;
            let deleted = request(reqwest::Method::DELETE, url)
                .header(reqwest::header::IF_MATCH, etag)
                .send()
                .await
                .map_err(|_| "Retention delete failed")?;
            if !deleted.status().is_success() {
                return Err(format!("Retention kept archive: HTTP {}", deleted.status()));
            }
            Ok::<(), String>(())
        }
        .await;
        match result {
            Ok(()) => {
                removed.push(index);
                remaining -= 1;
            }
            Err(e) => warnings.push(e),
        }
    }
    let mut i = 0;
    owned.retain(|_| {
        let keep = !removed.contains(&i);
        i += 1;
        keep
    });
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    if state.data_epoch.load(std::sync::atomic::Ordering::SeqCst) != epoch {
        return Err("Data reset during backup".into());
    }
    repo.set_setting(
        "scheduled_archives",
        &serde_json::to_string(&owned).map_err(|e| e.to_string())?,
    )?;
    Ok(json!({"path":target.as_str(),"rotation_warnings":warnings}))
}
