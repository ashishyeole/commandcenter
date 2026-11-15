use crate::{AppState, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use command_center_core::archive::VaultEntry;
use rand::{rngs::OsRng, RngCore};
use std::time::{Duration, Instant};
use tauri::State;
use zeroize::Zeroizing;
pub struct VaultSession {
    pub key: Zeroizing<Vec<u8>>,
    pub opened: Instant,
}
pub fn session_key(state: &AppState) -> Result<Zeroizing<Vec<u8>>> {
    let mut guard = state.vault.lock().map_err(|_| "Vault unavailable")?;
    if guard
        .as_ref()
        .is_some_and(|s| s.opened.elapsed() > Duration::from_secs(600))
    {
        *guard = None;
    }
    guard
        .as_ref()
        .map(|s| Zeroizing::new(s.key.to_vec()))
        .ok_or("Unlock the vault first (sessions expire after 10 minutes)".into())
}
pub use command_center_core::vault_codec::{reveal, seal};
#[tauri::command]
pub fn unlock_vault(state: State<AppState>) -> Result<()> {
    let (device, has_data) = {
        let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
        (repo.device_id.clone(), !repo.vault_rows()?.is_empty())
    };
    let entry = keyring::Entry::new("com.personal.commandcenter.vault", &device)
        .map_err(|e| e.to_string())?;
    let key = match entry.get_secret() {
        Ok(key) => key,
        Err(keyring::Error::NoEntry) if !has_data => {
            let mut key = vec![0u8; 32];
            OsRng.fill_bytes(&mut key);
            entry
                .set_secret(&key)
                .map_err(|e| format!("OS secure store refused the vault key: {e}"))?;
            key
        }
        Err(e) => {
            return Err(format!(
                "Cannot unlock vault. Restore an encrypted backup if the device key was lost. {e}"
            ))
        }
    };
    if key.len() != 32 {
        return Err("Invalid OS vault key".into());
    }
    *state.vault.lock().map_err(|_| "Vault unavailable")? = Some(VaultSession {
        key: Zeroizing::new(key),
        opened: Instant::now(),
    });
    Ok(())
}
#[tauri::command]
pub fn lock_vault(state: State<AppState>) -> Result<()> {
    *state.pending.lock().map_err(|_| "Import unavailable")? = None;
    *state.vault.lock().map_err(|_| "Vault unavailable")? = None;
    Ok(())
}
#[tauri::command]
pub fn vault_entries(state: State<AppState>) -> Result<Vec<VaultEntry>> {
    let key = session_key(&state)?;
    state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .vault_rows()?
        .iter()
        .map(|(id, bytes)| reveal(&key, id, bytes))
        .collect()
}
#[tauri::command]
pub fn save_vault_entry(state: State<AppState>, mut entry: VaultEntry) -> Result<()> {
    let key = session_key(&state)?;
    let mut repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    let previous = repo.vault_ciphertext(&entry.id)?;
    if let Some(bytes) = &previous {
        let original = reveal(&key, &entry.id, bytes)?;
        if original.updated_at != entry.updated_at {
            return Err("Private record changed; reload before saving".into());
        }
    }
    entry.updated_at = command_center_core::now();
    let bytes = seal(&key, &entry)?;
    repo.save_vault_if_unchanged(&entry.id, &bytes, previous.as_deref())
}
#[tauri::command]
pub async fn attach_vault_file(state: State<'_, AppState>) -> Result<Option<String>> {
    let key = session_key(&state)?;
    if let Some(file) = rfd::AsyncFileDialog::new()
        .set_title("Encrypt a document into your local vault")
        .pick_file()
        .await
    {
        let meta = std::fs::metadata(file.path()).map_err(|e| e.to_string())?;
        if meta.len() > 10 * 1024 * 1024 {
            return Err("Document exceeds 10 MiB".into());
        }
        let data = Zeroizing::new(std::fs::read(file.path()).map_err(|e| e.to_string())?);
        if data.len() > 10 * 1024 * 1024 {
            return Err("Document exceeds 10 MiB".into());
        }
        let title = file.file_name();
        let entry = VaultEntry {
            id: uuid::Uuid::new_v4().to_string(),
            title: title.clone(),
            category: "document".into(),
            body: String::new(),
            amount: None,
            due: None,
            filename: Some(title.clone()),
            file_base64: Some(STANDARD.encode(&*data)),
            updated_at: command_center_core::now(),
        };
        let encrypted = seal(&key, &entry)?;
        state
            .repo
            .lock()
            .map_err(|_| "Database unavailable")?
            .save_vault(&entry.id, &encrypted)?;
        Ok(Some(title))
    } else {
        Ok(None)
    }
}
#[tauri::command]
pub async fn export_vault_file(state: State<'_, AppState>, id: String) -> Result<Option<String>> {
    let key = session_key(&state)?;
    let rows = state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .vault_rows()?;
    let (_, bytes) = rows
        .iter()
        .find(|(key, _)| key == &id)
        .ok_or("Document not found")?;
    let entry = reveal(&key, &id, bytes)?;
    let data = Zeroizing::new(
        STANDARD
            .decode(entry.file_base64.ok_or("Record has no attachment")?)
            .map_err(|e| e.to_string())?,
    );
    let name = entry
        .filename
        .as_deref()
        .and_then(|s| std::path::Path::new(s).file_name())
        .and_then(|s| s.to_str())
        .unwrap_or("document");
    if let Some(file) = rfd::AsyncFileDialog::new()
        .set_title("Export decrypted document — plaintext destination")
        .set_file_name(name)
        .save_file()
        .await
    {
        crate::atomic_write(file.path(), &data)?;
        Ok(Some(file.path().display().to_string()))
    } else {
        Ok(None)
    }
}
