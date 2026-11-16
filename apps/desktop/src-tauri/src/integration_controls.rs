use crate::{AppState, Result};
use serde_json::Value;
use tauri::State;
#[tauri::command]
pub fn integration_inventory(state: State<AppState>) -> Result<Value> {
    state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .integration_inventory()
}
#[tauri::command]
pub fn pause_data_connections(state: State<AppState>, consent: bool) -> Result<Value> {
    if !consent {
        return Err("Approve disabling the listed data connections".into());
    }
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    for (flag, _) in command_center_core::integrations::FLAGS {
        repo.set_integration_flag(flag, false)?;
    }
    repo.set_setting("google_body_approved", "false")?;
    for (id, token) in state.jobs.lock().map_err(|_| "Jobs unavailable")?.iter() {
        if [
            "cloud:",
            "google:",
            "github:",
            "plaid:",
            "hosted:",
            "market:",
            "agriculture:",
        ]
        .iter()
        .any(|prefix| id.starts_with(prefix))
        {
            token.cancel();
        }
    }
    *state
        .google_cache
        .lock()
        .map_err(|_| "Account cache unavailable")? = None;
    repo.integration_inventory()
}
#[tauri::command]
pub fn ai_context_policy(state: State<AppState>) -> Result<Value> {
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    let excluded = repo.ai_exclusions()?;
    let mut records = repo
        .list("", false)?
        .iter()
        .map(|i| serde_json::json!({"id":i.id,"title":i.title,"kind":i.kind}))
        .collect::<Vec<_>>();
    records.extend(
        repo.records("", false)?
            .iter()
            .map(|r| serde_json::json!({"id":r.id,"title":r.title,"kind":r.module})),
    );
    Ok(serde_json::json!({"excluded":excluded,"records":records}))
}
#[tauri::command]
pub fn set_ai_context_exclusion(state: State<AppState>, id: String, excluded: bool) -> Result<()> {
    state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .set_ai_exclusion(&id, excluded)?;
    if excluded {
        for (key, token) in state.jobs.lock().map_err(|_| "Jobs unavailable")?.iter() {
            if key.starts_with("cloud:") || key.starts_with("plan:") {
                token.cancel();
            }
        }
    }
    Ok(())
}
