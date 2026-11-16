use crate::{AppState, Result};
use command_center_core::retention::Policy;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tauri::State;
fn hash(v: &Value) -> Result<String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(v).map_err(|e| e.to_string())?)
    ))
}
#[tauri::command]
pub fn preview_retention(state: State<AppState>, policy: Policy) -> Result<Value> {
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    let preview = repo.retention_preview(&policy)?;
    Ok(
        json!({"preview":preview,"sha256":hash(&preview)?,"last_backup":repo.setting("last_backup")?.and_then(|v|serde_json::from_str::<Value>(&v).ok())}),
    )
}
#[tauri::command]
pub fn apply_retention(
    state: State<AppState>,
    policy: Policy,
    reviewed_sha256: String,
    confirmation: String,
) -> Result<Value> {
    if confirmation != "CLEAN REVIEWED LOCAL DATA" {
        return Err(
            "Type CLEAN REVIEWED LOCAL DATA after reviewing the counts and backup status".into(),
        );
    }
    let mut repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    let preview = repo.retention_preview(&policy)?;
    if hash(&preview)? != reviewed_sha256 {
        return Err("Cleanup counts changed; review again".into());
    }
    repo.apply_retention(&policy, &preview)
}
#[tauri::command]
pub async fn export_diagnostics(state: State<'_, AppState>, consent: bool) -> Result<Value> {
    if !consent {
        return Err(
            "Approve saving a local diagnostic file with platform and aggregate counts".into(),
        );
    }
    let data = {
        let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
        json!({"format":"pcc-diagnostics-1","generated_at":command_center_core::now(),"app_version":env!("CARGO_PKG_VERSION"),"os":std::env::consts::OS,"architecture":std::env::consts::ARCH,"counts":repo.diagnostic_counts()?,"contents_excluded":["record contents","titles","record and device IDs","paths","tokens","provider destinations","raw error logs","model prompts","environment variables"],"runtime_tested":false})
    };
    let Some(file) = rfd::AsyncFileDialog::new()
        .set_title("Save aggregate diagnostics — no automatic upload")
        .set_file_name("command-center-diagnostics.json")
        .save_file()
        .await
    else {
        return Ok(json!({"cancelled":true}));
    };
    crate::atomic_write(
        file.path(),
        &serde_json::to_vec_pretty(&data).map_err(|e| e.to_string())?,
    )?;
    Ok(json!({"saved":true,"uploaded":false}))
}

#[tauri::command]
pub fn preview_sync_compaction(state: State<AppState>) -> Result<Value> {
    let mut repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    repo.prepare_sync()?;
    let preview = repo.sync_compaction_preview()?;
    Ok(json!({"preview":preview,"sha256":hash(&preview)?}))
}
#[tauri::command]
pub fn apply_sync_compaction(
    state: State<AppState>,
    reviewed_sha256: String,
    confirmation: String,
) -> Result<usize> {
    if confirmation != "COMPACT ACKNOWLEDGED SYNC HISTORY" {
        return Err("Type COMPACT ACKNOWLEDGED SYNC HISTORY after reviewing the boundary".into());
    }
    let mut repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    repo.prepare_sync()?;
    let preview = repo.sync_compaction_preview()?;
    if hash(&preview)? != reviewed_sha256 {
        return Err("Compaction review changed".into());
    }
    repo.compact_acknowledged_sync(&preview)
}
