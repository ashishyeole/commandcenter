use crate::{AppState, Result};
use command_center_core::{
    archive::{self, ExportData},
    now,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use tauri::{Manager, State};
use zeroize::Zeroizing;
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BackupSchedule {
    pub enabled: bool,
    pub approved: bool,
    pub interval_hours: u32,
    pub include_vault: bool,
    pub retain: u32,
    pub backend: String,
}
pub(crate) fn key(device: &str) -> Result<keyring::Entry> {
    keyring::Entry::new("com.personal.commandcenter.backup", device).map_err(|e| e.to_string())
}
fn schedule(state: &AppState) -> Result<BackupSchedule> {
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    serde_json::from_str(&repo.setting("backup_schedule")?.unwrap_or("{}".into()))
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub fn backup_schedule_snapshot(state: State<AppState>) -> Result<Value> {
    let config = schedule(&state)?;
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    Ok(
        json!({"config":config,"destination":repo.setting(if config.backend=="webdav" {"backup_webdav"}else{"backup_destination"})?,"health":repo.setting("backup_job_health")?.and_then(|s|serde_json::from_str::<Value>(&s).ok()),"archives":repo.setting("scheduled_archives")?.and_then(|s|serde_json::from_str::<Value>(&s).ok()).unwrap_or(json!([]))}),
    )
}
#[tauri::command]
pub async fn choose_backup_destination(state: State<'_, AppState>) -> Result<Option<String>> {
    let _guard = state
        .backup_lock
        .try_lock()
        .map_err(|_| "Wait for the active backup to finish")?;
    if let Some(folder) = rfd::AsyncFileDialog::new()
        .set_title("Choose your local or mounted NAS/cloud backup folder")
        .pick_folder()
        .await
    {
        let path = folder.path().canonicalize().map_err(|e| e.to_string())?;
        let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
        let mut config: BackupSchedule =
            serde_json::from_str(&repo.setting("backup_schedule")?.unwrap_or("{}".into()))
                .map_err(|e| e.to_string())?;
        config.backend = "local".into();
        config.enabled = false;
        config.approved = false;
        repo.set_setting(
            "backup_schedule",
            &serde_json::to_string(&config).map_err(|e| e.to_string())?,
        )?;
        repo.set_setting("backup_destination", &path.display().to_string())?;
        Ok(Some(path.display().to_string()))
    } else {
        Ok(None)
    }
}
#[tauri::command]
pub fn save_backup_schedule(
    state: State<AppState>,
    config: BackupSchedule,
    password: String,
) -> Result<()> {
    let _guard = state
        .backup_lock
        .try_lock()
        .map_err(|_| "Wait for the active backup to finish")?;
    let password = Zeroizing::new(password);
    if !(1..=720).contains(&config.interval_hours) || config.retain > 100 {
        return Err("Use 1–720 hours and retention of 0–100 archives".into());
    }
    if !["", "local", "webdav"].contains(&config.backend.as_str()) {
        return Err("Unknown backup backend".into());
    }
    if config.enabled && !config.approved {
        return Err("Approve scheduled backup access first".into());
    }
    if config.include_vault && config.approved {
        let _ = crate::vault::session_key(&state)?;
    }
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    if config.enabled
        && repo
            .setting(if config.backend == "webdav" {
                "backup_webdav"
            } else {
                "backup_destination"
            })?
            .is_none()
    {
        return Err("Choose a destination first".into());
    }
    let entry = key(&repo.device_id)?;
    if !password.is_empty() {
        if password.chars().count() < 12 || password.len() > 1024 {
            return Err("Use a recovery password of 12–1024 characters".into());
        }
        entry
            .set_password(&password)
            .map_err(|e| format!("Secure store refused backup password: {e}"))?;
    }
    if config.enabled {
        let _password = Zeroizing::new(
            entry
                .get_password()
                .map_err(|_| "Set a recovery password before enabling backups")?,
        );
    }
    repo.set_setting(
        "backup_schedule",
        &serde_json::to_string(&config).map_err(|e| e.to_string())?,
    )
}
#[tauri::command]
pub fn forget_backup_schedule(state: State<AppState>) -> Result<()> {
    let _guard = state
        .backup_lock
        .try_lock()
        .map_err(|_| "Wait for the active backup to finish")?;
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    repo.set_setting("backup_schedule", "{}")?;
    crate::webdav_backup::forget(&repo.device_id)?;
    match key(&repo.device_id)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!(
            "Schedule stopped, but secure-store password removal failed: {e}"
        )),
    }
}
async fn execute(app: &tauri::AppHandle) -> Result<String> {
    let state = app.state::<AppState>();
    let _guard = state
        .backup_lock
        .try_lock()
        .map_err(|_| "A backup is already running")?;
    let config = schedule(&state)?;
    if !config.approved {
        return Err("Approve backup permissions first".into());
    }
    let (mut data, device, destination, epoch, vault_rows) = {
        let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
        (
            ExportData {
                items: repo.list("", true)?,
                workspace: repo.portable_workspace()?,
                vault: vec![],
            },
            repo.device_id.clone(),
            repo.setting(if config.backend == "webdav" {
                "backup_webdav"
            } else {
                "backup_destination"
            })?
            .ok_or("Choose a backup destination first")?,
            state.data_epoch.load(std::sync::atomic::Ordering::SeqCst),
            if config.include_vault {
                repo.vault_rows()?
            } else {
                vec![]
            },
        )
    };
    let started = now();
    state.repo.lock().map_err(|_|"Database unavailable")?.set_setting("backup_job_health",&json!({"status":"running","started_at":started,"next_at":(chrono::Utc::now()+chrono::Duration::hours(config.interval_hours as i64)).to_rfc3339()}).to_string())?;
    let result = if config.backend == "webdav" {
        crate::webdav_backup::execute(
            &state,
            &config,
            data,
            &device,
            &destination,
            epoch,
            vault_rows,
        )
        .await
    } else {
        (|| -> Result<Value> {
            let root = PathBuf::from(&destination);
            if !root.is_dir() || root.canonicalize().map_err(|e| e.to_string())? != root {
                return Err("Backup destination is missing or changed. Mount it again or re-authorize the folder.".into());
            }
            let password = Zeroizing::new(
                key(&device)?
                    .get_password()
                    .map_err(|_| "Backup recovery password is unavailable in OS secure storage")?,
            );
            if config.include_vault {
                // Explicit saved per-category permission allows unattended decryption solely for encrypted backup.
                let vault_key = Zeroizing::new(
                    keyring::Entry::new("com.personal.commandcenter.vault", &device)
                        .map_err(|e| e.to_string())?
                        .get_secret()
                        .map_err(|_| "Vault key unavailable; backup was not created")?,
                );
                for (id, bytes) in vault_rows {
                    data.vault
                        .push(crate::vault::reveal(&vault_key, &id, &bytes)?);
                }
            }
            let count = data.items.len()
                + data.workspace.records.len()
                + data.workspace.attachments.len()
                + data.vault.len();
            let zip = Zeroizing::new(archive::export_all(&data)?);
            archive::inspect_all(&zip)?;
            let bytes = archive::encrypt(&zip, &password)?;
            let name = format!(
                "command-center-{device}-{}-{}.pacc",
                chrono::Utc::now().format("%Y%m%dT%H%M%SZ"),
                uuid::Uuid::new_v4()
            );
            let target = root.join(name);
            // Hold the database lock while committing the snapshot file, so reset cannot race a write.
            let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
            if state.data_epoch.load(std::sync::atomic::Ordering::SeqCst) != epoch {
                return Err("Data was reset; backup cancelled".into());
            }
            crate::atomic_write(&target, &bytes)?;
            let readback = std::fs::read(&target).map_err(|e| e.to_string())?;
            let decrypted = Zeroizing::new(archive::decrypt(&readback, &password)?);
            archive::inspect_all(&decrypted)?;
            let info = json!({"sha256":format!("{:x}",Sha256::digest(&bytes)),"verified_at":now(),"path":target.display().to_string(),"encrypted":true,"records":count,"includes_vault":config.include_vault,"modules":[],"bytes":bytes.len()});
            repo.set_setting("last_backup", &info.to_string())?;
            let mut owned: Vec<Value> =
                serde_json::from_str(&repo.setting("scheduled_archives")?.unwrap_or("[]".into()))
                    .map_err(|e| e.to_string())?;
            owned.push(info.clone());
            // Rotation only applies to prior verified files recorded by this app in this exact folder.
            let mut warnings = vec![];
            if config.retain > 0 {
                let current = owned
                    .iter()
                    .filter(|v| {
                        v["path"]
                            .as_str()
                            .is_some_and(|p| PathBuf::from(p).parent() == Some(root.as_path()))
                    })
                    .count();
                let mut remove = current.saturating_sub(config.retain as usize);
                owned.retain(|v| {
                    if remove == 0 {
                        return true;
                    }
                    let Some(path) = v["path"].as_str().map(PathBuf::from) else {
                        return true;
                    };
                    if path.parent() != Some(root.as_path())
                        || !path.file_name().and_then(|s| s.to_str()).is_some_and(|s| {
                            s.starts_with(&format!("command-center-{device}-"))
                                && s.ends_with(".pacc")
                        })
                    {
                        return true;
                    }
                    match std::fs::symlink_metadata(&path) {
                        Ok(meta)
                            if meta.is_file()
                                && !meta.file_type().is_symlink()
                                && meta.len() <= 64 * 1024 * 1024 + 50
                                && std::fs::read(&path).is_ok_and(|b| {
                                    v["sha256"].as_str()
                                        == Some(format!("{:x}", Sha256::digest(&b)).as_str())
                                }) =>
                        {
                            match std::fs::remove_file(&path) {
                                Ok(()) => {
                                    remove -= 1;
                                    false
                                }
                                Err(e) => {
                                    warnings.push(e.to_string());
                                    true
                                }
                            }
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                            remove -= 1;
                            false
                        }
                        _ => true,
                    }
                });
            }
            repo.set_setting(
                "scheduled_archives",
                &serde_json::to_string(&owned).map_err(|e| e.to_string())?,
            )?;
            Ok(json!({"path":info["path"],"rotation_warnings":warnings}))
        })()
    };
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    if state.data_epoch.load(std::sync::atomic::Ordering::SeqCst) != epoch {
        return Err("Data reset during backup".into());
    }
    repo.set_setting("backup_job_health",&json!({"status":if result.is_ok(){"completed"}else{"failed"},"started_at":started,"finished_at":now(),"next_at":(chrono::Utc::now()+chrono::Duration::hours(config.interval_hours.max(1) as i64)).to_rfc3339(),"error":result.as_ref().err(),"result":result.as_ref().ok()}).to_string())?;
    result.map(|v| v["path"].as_str().unwrap_or("").to_owned())
}
#[tauri::command]
pub async fn run_scheduled_backup(app: tauri::AppHandle) -> Result<String> {
    execute(&app).await
}
pub async fn poll(app: &tauri::AppHandle) -> Result<()> {
    let state = app.state::<AppState>();
    let config = schedule(&state)?;
    if !config.enabled || !config.approved {
        return Ok(());
    }
    let due = {
        let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
        repo.setting("backup_job_health")?
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .and_then(|v| {
                v["next_at"]
                    .as_str()
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            })
            .is_none_or(|d| d <= chrono::Utc::now())
    };
    if due {
        execute(app).await?;
    }
    Ok(())
}
