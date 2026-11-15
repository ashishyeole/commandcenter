//! Public model-card metadata only: no weight downloads or remote code execution.
use crate::{AppState, Result};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use tauri::State;
#[tauri::command]
pub fn model_evidence(state: State<AppState>) -> Result<Value> {
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    serde_json::from_str(&repo.setting("model_evidence")?.unwrap_or("[]".into()))
        .map_err(|_| "Invalid model evidence cache".into())
}
#[tauri::command]
pub async fn inspect_public_model(
    state: State<'_, AppState>,
    model_id: String,
    consent: bool,
) -> Result<Value> {
    if !consent {
        return Err("Approve fetching public model metadata from Hugging Face".into());
    }
    let segments: Vec<_> = model_id.split('/').collect();
    if segments.len() != 2
        || segments.iter().any(|s| {
            s.is_empty()
                || *s == "."
                || *s == ".."
                || s.len() > 100
                || !s
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        })
    {
        return Err(
            "Use publisher/model with letters, numbers, dots, underscores or hyphens".into(),
        );
    }
    let epoch = state.data_epoch.load(Ordering::SeqCst);
    {
        let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
        let last = repo
            .setting("model_evidence_attempt")?
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(0);
        if chrono::Utc::now().timestamp() - last < 10 {
            return Err("Wait ten seconds between model metadata requests".into());
        }
        repo.set_setting(
            "model_evidence_attempt",
            &chrono::Utc::now().timestamp().to_string(),
        )?;
    }
    let client = reqwest::Client::builder()
        .https_only(true)
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(25))
        .user_agent("PersonalCommandCenter/0.5")
        .build()
        .map_err(|e| e.to_string())?;
    let raw = crate::connectors::json_response(
        client
            .get(format!("https://huggingface.co/api/models/{model_id}"))
            .send()
            .await
            .map_err(|_| "Model metadata request failed")?,
    )
    .await?;
    let card = &raw["cardData"];
    let evidence = json!({"id":model_id,"source":format!("https://huggingface.co/{model_id}"),"fetched_at":command_center_core::now(),"revision":raw["sha"],"last_modified":raw["lastModified"],"task":raw["pipeline_tag"],"library":raw["library_name"],"license":card["license"],"license_name":card["license_name"],"license_link":card["license_link"],"gated":raw["gated"],"parameters":raw["safetensors"]["total"],"parameter_types":raw["safetensors"]["parameters"],"languages":card["language"],"datasets":card["datasets"],"benchmarks":card["model-index"],"tags":raw["tags"],"verification":"Publisher/API metadata; model-card benchmark claims are not independently verified. Missing metadata is unknown, not permission or compatibility."});
    if serde_json::to_vec(&evidence)
        .map_err(|e| e.to_string())?
        .len()
        > 100000
    {
        return Err("Model metadata exceeds 100 KB".into());
    }
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    if epoch != state.data_epoch.load(Ordering::SeqCst) {
        return Err("Workspace reset during request".into());
    }
    let mut rows: Vec<Value> =
        serde_json::from_str(&repo.setting("model_evidence")?.unwrap_or("[]".into()))
            .map_err(|_| "Invalid model evidence cache")?;
    rows.retain(|row| row["id"] != evidence["id"]);
    rows.insert(0, evidence.clone());
    rows.truncate(12);
    repo.set_setting(
        "model_evidence",
        &serde_json::to_string(&rows).map_err(|e| e.to_string())?,
    )?;
    Ok(evidence)
}
