use crate::{AppState, Result};
use serde_json::{json, Value};
use tauri::State;
fn client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(std::time::Duration::from_secs(5))
        .timeout(std::time::Duration::from_secs(3600))
        .build()
        .map_err(|e| e.to_string())
}
fn model_name(model: &str) -> Result<()> {
    if model.is_empty()
        || model.len() > 160
        || model.to_lowercase().contains("cloud")
        || !model
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._:-/".contains(c))
    {
        return Err("Use a local model identifier of at most 160 characters".into());
    }
    Ok(())
}
#[tauri::command]
pub async fn inspect_local_model(model: String, consent: bool) -> Result<Value> {
    if !consent {
        return Err("Approve local runtime access first".into());
    }
    model_name(&model)?;
    let c = client()?;
    let details = crate::connectors::json_response(
        c.post("http://127.0.0.1:11434/api/show")
            .json(&json!({"model":model}))
            .send()
            .await
            .map_err(|_| "Local Ollama unavailable")?,
    )
    .await?;
    let running = crate::connectors::json_response(
        c.get("http://127.0.0.1:11434/api/ps")
            .send()
            .await
            .map_err(|_| "Local runtime unavailable")?,
    )
    .await?;
    Ok(json!({"details":details,"running":running}))
}
#[tauri::command]
pub fn local_model_job(state: State<AppState>, request_id: String) -> Result<Value> {
    uuid::Uuid::parse_str(&request_id).map_err(|_| "Invalid job ID")?;
    Ok(state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .setting(&format!("model_job:{request_id}"))?
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(json!({"status":"idle"})))
}
#[tauri::command]
pub async fn manage_local_model(
    state: State<'_, AppState>,
    request_id: String,
    model: String,
    action: String,
    confirmation: String,
    consent: bool,
) -> Result<()> {
    if !consent {
        return Err("Approve the model operation first".into());
    }
    model_name(&model)?;
    uuid::Uuid::parse_str(&request_id).map_err(|_| "Invalid job ID")?;
    if !["pull", "delete"].contains(&action.as_str())
        || (action == "delete" && confirmation != model)
    {
        return Err("Choose download or type the exact model name to confirm removal".into());
    }
    let epoch = state.data_epoch.load(std::sync::atomic::Ordering::SeqCst);
    let key = format!("model:{request_id}");
    let token = tokio_util::sync::CancellationToken::new();
    {
        let mut jobs = state.jobs.lock().map_err(|_| "Job state unavailable")?;
        if !jobs.is_empty() {
            return Err("Finish active model/AI work before changing models".into());
        }
        jobs.insert(key.clone(), token.clone());
    }
    let record = |value: Value| -> Result<()> {
        let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
        if state.data_epoch.load(std::sync::atomic::Ordering::SeqCst) != epoch {
            return Err("Data reset during model operation".into());
        }
        repo.set_setting(&format!("model_job:{request_id}"), &value.to_string())
    };
    let work = async {
        record(json!({"status":"starting","model":model,"action":action}))?;
        let c = client()?;
        if action == "delete" {
            let response = c
                .delete("http://127.0.0.1:11434/api/delete")
                .json(&json!({"model":model}))
                .send()
                .await
                .map_err(|_| "Local model removal failed")?;
            if !response.status().is_success() {
                return Err(format!("Runtime returned HTTP {}", response.status()));
            }
            return Ok(());
        }
        let mut response = c
            .post("http://127.0.0.1:11434/api/pull")
            .json(&json!({"model":model,"stream":true}))
            .send()
            .await
            .map_err(|_| "Local download request failed")?;
        if !response.status().is_success() {
            return Err(format!("Runtime returned HTTP {}", response.status()));
        }
        let mut pending = vec![];
        let mut success = false;
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| "Download status stream interrupted")?
        {
            pending.extend_from_slice(&chunk);
            if pending.len() > 1024 * 1024 {
                return Err("Runtime status exceeds bounds".into());
            }
            while let Some(end) = pending.iter().position(|b| *b == b'\n') {
                let line = pending.drain(..=end).collect::<Vec<_>>();
                if line.len() > 65536 {
                    return Err("Runtime status line exceeds bounds".into());
                }
                if line.iter().all(u8::is_ascii_whitespace) {
                    continue;
                }
                let value: Value =
                    serde_json::from_slice(&line).map_err(|_| "Invalid download status")?;
                if let Some(error) = value["error"].as_str() {
                    return Err(error.chars().take(1000).collect());
                }
                if value["status"] == "success" {
                    success = true;
                }
                record(
                    json!({"status":value["status"],"completed":value["completed"],"total":value["total"],"model":model}),
                )?;
            }
        }
        if !pending.is_empty() {
            let value: Value =
                serde_json::from_slice(&pending).map_err(|_| "Incomplete download status")?;
            success |= value["status"] == "success";
        }
        if !success {
            return Err("Runtime did not confirm a completed download".into());
        }
        Ok(())
    };
    let result = tokio::select! {_=token.cancelled()=>Err("Stopped waiting. Ollama may retain partial files or continue a server-side download; review the runtime.".into()),result=work=>result};
    let _ = record(
        json!({"status":if result.is_ok(){"completed"}else{"failed"},"model":model,"action":action,"error":result.as_ref().err()}),
    );
    state
        .jobs
        .lock()
        .map_err(|_| "Job state unavailable")?
        .remove(&key);
    result
}
