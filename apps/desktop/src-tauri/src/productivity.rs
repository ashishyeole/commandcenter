use crate::{AppState, Result};
use command_center_core::{now, workspace::Record};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{Manager, State};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub enabled: bool,
    pub approved: bool,
    pub interval_hours: u32,
    pub source: String,
    pub query: String,
    pub model: String,
    pub prompt: String,
    pub context_ids: Vec<String>,
}
fn recipes(state: &AppState) -> Result<Vec<Recipe>> {
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    serde_json::from_str(&repo.setting("automation_recipes")?.unwrap_or("[]".into()))
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub fn automation_snapshot(state: State<AppState>) -> Result<Value> {
    let recipes = recipes(&state)?;
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    let mut health = serde_json::Map::new();
    for r in &recipes {
        if let Some(raw) = repo.setting(&format!("recipe_run:{}", r.id))? {
            health.insert(
                r.id.clone(),
                serde_json::from_str::<Value>(&raw).map_err(|e| e.to_string())?,
            );
        }
    }
    Ok(json!({"recipes":recipes,"health":health}))
}
#[tauri::command]
pub fn save_automation(state: State<AppState>, value: Vec<Recipe>) -> Result<()> {
    if value.len() > 20 {
        return Err("At most 20 automation recipes".into());
    }
    let mut ids = std::collections::HashSet::new();
    for r in &value {
        uuid::Uuid::parse_str(&r.id).map_err(|_| "Invalid recipe ID")?;
        if !ids.insert(&r.id)
            || r.name.trim().is_empty()
            || r.name.len() > 200
            || !(1..=720).contains(&r.interval_hours)
            || r.context_ids.len() > 30
            || r.query.len() > 300
            || r.prompt.len() > 8000
            || r.model.len() > 160
        {
            return Err("Invalid automation settings".into());
        }
        if r.enabled && !r.approved {
            return Err("Approve recipe data access before enabling it".into());
        }
        match r.kind.as_str() {
            "source"
                if [
                    "github", "models", "news", "mcp", "apps", "youtube", "arxiv", "rss",
                ]
                .contains(&r.source.as_str()) =>
            {
                ()
            }
            "brief" if !r.model.trim().is_empty() && !r.prompt.trim().is_empty() => (),
            _ => return Err("Choose a supported source or a local model and prompt".into()),
        }
        for id in &r.context_ids {
            uuid::Uuid::parse_str(id).map_err(|_| "Invalid context ID")?;
        }
    }
    // Revoking or removing a recipe cancels any in-flight local AI request.
    for prior in recipes(&state)? {
        if !value
            .iter()
            .any(|r| r.id == prior.id && r.enabled && r.approved)
        {
            if let Some(token) = state
                .jobs
                .lock()
                .map_err(|_| "Job state unavailable")?
                .get(&prior.id)
            {
                token.cancel();
            }
        }
    }
    state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .set_setting(
            "automation_recipes",
            &serde_json::to_string(&value).map_err(|e| e.to_string())?,
        )
}
async fn execute(app: &tauri::AppHandle, recipe: Recipe) -> Result<Value> {
    if !recipe.approved {
        return Err("Approve this recipe first".into());
    }
    let state = app.state::<AppState>();
    let _guard = state
        .automation_lock
        .try_lock()
        .map_err(|_| "An automation is already running")?;
    let key = format!("recipe_run:{}", recipe.id);
    let started = now();
    let previous_failures = {
        let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
        let failures = repo
            .setting(&key)?
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .and_then(|v| v["failures"].as_u64())
            .unwrap_or(0);
        repo.set_setting(&key,&json!({"started_at":started,"status":"running","next_at":(chrono::Utc::now()+chrono::Duration::hours(recipe.interval_hours as i64)).to_rfc3339()}).to_string())?;
        failures
    };
    let result = match recipe.kind.as_str() {
        "source" => crate::connectors::refresh_source(
            app.state(),
            recipe.source.clone(),
            recipe.query.clone(),
            true,
        )
        .await
        .map(|feed| json!({"source":feed.source,"fetched_at":feed.fetched_at})),
        "brief" => {
            crate::connectors::ask_local(
                app.state(),
                recipe.id.clone(),
                recipe.model.clone(),
                recipe.prompt.clone(),
                recipe.context_ids.clone(),
                true,
                None,
            )
            .await
        }
        _ => Err("Unsupported recipe".into()),
    };
    let mut repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    // A reset or removed recipe must not resurrect its content or status.
    let retained: Vec<Recipe> =
        serde_json::from_str(&repo.setting("automation_recipes")?.unwrap_or("[]".into()))
            .map_err(|e| e.to_string())?;
    if !retained.iter().any(|r| r.id == recipe.id) {
        return Err("Recipe was removed".into());
    }
    let failures = if result.is_err() {
        previous_failures + 1
    } else {
        0
    };
    let delay_hours = (recipe.interval_hours as i64).max((1i64 << failures.min(5)).min(24));
    let health = json!({"started_at":started,"finished_at":now(),"status":if result.is_ok(){"completed"}else{"failed"},"message":result.as_ref().err(),"failures":failures,"next_at":(chrono::Utc::now()+chrono::Duration::hours(delay_hours)).to_rfc3339()});
    repo.set_setting(&key, &health.to_string())?;
    let mut log = Record::new(
        "missions",
        &format!("Automation: {}", recipe.name),
        &repo.device_id,
    );
    log.status = if result.is_ok() {
        "completed"
    } else {
        "failed"
    }
    .into();
    log.summary = if let Ok(v) = &result {
        v.get("answer")
            .and_then(Value::as_str)
            .unwrap_or("Public source refreshed and cached.")
            .to_string()
    } else {
        result.as_ref().err().cloned().unwrap_or_default()
    };
    log.fields.insert("recipe_id".into(), recipe.id);
    log.fields.insert(
        "evidence".into(),
        "Automatic local run under saved permission; no tools or messages executed.".into(),
    );
    repo.save_record(log)?;
    result
}
#[tauri::command]
pub async fn run_automation(app: tauri::AppHandle, id: String) -> Result<Value> {
    let recipe = recipes(&app.state::<AppState>())?
        .into_iter()
        .find(|r| r.id == id)
        .ok_or("Save the recipe first")?;
    execute(&app, recipe).await
}
pub async fn poll(app: &tauri::AppHandle) -> Result<()> {
    let entries = recipes(&app.state::<AppState>())?;
    for recipe in entries.into_iter().filter(|r| r.enabled && r.approved) {
        let due = {
            let state = app.state::<AppState>();
            let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
            repo.setting(&format!("recipe_run:{}", recipe.id))?
                .and_then(|s| serde_json::from_str::<Value>(&s).ok())
                .and_then(|v| {
                    v["next_at"]
                        .as_str()
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                })
                .is_none_or(|d| d <= chrono::Utc::now())
        };
        if due {
            let _ = execute(app, recipe).await;
        }
    }
    Ok(())
}
#[tauri::command]
pub fn plan_tasks(
    state: State<AppState>,
    id: String,
    revision: i64,
    titles: Vec<String>,
    due: Option<String>,
) -> Result<usize> {
    state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .plan_tasks(&id, revision, &titles, due)
}
#[tauri::command]
pub async fn export_document(title: String, body: String) -> Result<Option<String>> {
    if title.len() > 500 || body.len() > 200_000 {
        return Err("Document is too large".into());
    }
    if let Some(file) = rfd::AsyncFileDialog::new()
        .set_title("Export reviewed draft as Markdown")
        .set_file_name("workspace-draft.md")
        .add_filter("Markdown", &["md"])
        .save_file()
        .await
    {
        crate::atomic_write(file.path(), format!("# {title}\n\n{body}\n").as_bytes())?;
        Ok(Some(file.path().display().to_string()))
    } else {
        Ok(None)
    }
}
#[tauri::command]
pub fn snooze_task(state: State<AppState>, id: String, minutes: u32) -> Result<()> {
    if ![0, 15, 60, 240, 1440].contains(&minutes) {
        return Err("Unsupported snooze interval".into());
    }
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    if !repo
        .list("", false)?
        .iter()
        .any(|i| i.id == id && i.kind == "task")
    {
        return Err("Task not found".into());
    }
    repo.set_setting(
        &format!("snooze:{id}"),
        &(chrono::Utc::now() + chrono::Duration::minutes(minutes as i64)).to_rfc3339(),
    )
}
#[tauri::command]
pub fn save_workspace_document(
    state: State<AppState>,
    id: String,
    revision: i64,
    title: String,
    body: String,
) -> Result<Record> {
    state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .save_workspace_document(&id, revision, title, body)
}
#[tauri::command]
pub fn mark_discovery(state: State<AppState>, id: String, status: String) -> Result<()> {
    if id.is_empty()
        || id.len() > 500
        || !["read", "unread", "dismissed"].contains(&status.as_str())
    {
        return Err("Invalid reading status".into());
    }
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    let mut values: std::collections::BTreeMap<String, String> =
        serde_json::from_str(&repo.setting("discovery_state")?.unwrap_or("{}".into()))
            .map_err(|e| e.to_string())?;
    if status == "unread" {
        values.remove(&id);
    } else {
        if values.len() >= 5000 && !values.contains_key(&id) {
            return Err(
                "Reading history reached 5,000 records; mark older items unread to free space"
                    .into(),
            );
        }
        values.insert(id, status);
    }
    repo.set_setting(
        "discovery_state",
        &serde_json::to_string(&values).map_err(|e| e.to_string())?,
    )
}

#[tauri::command]
pub fn task_reminder_status(state: State<AppState>, id: String) -> Result<Value> {
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    if !repo
        .list("", false)?
        .iter()
        .any(|i| i.id == id && i.kind == "task")
    {
        return Err("Task not found".into());
    }
    Ok(
        json!({"policy": repo.task_reminder_policy(&id)?, "snooze_until": repo.setting(&format!("snooze:{id}"))?}),
    )
}
#[tauri::command]
pub fn update_task_reminder(
    state: State<AppState>,
    id: String,
    revision: i64,
    cadence: command_center_core::reminders::Cadence,
    acknowledge: bool,
) -> Result<command_center_core::reminders::ReminderPolicy> {
    state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .update_task_reminder(&id, revision, cadence, acknowledge)
}
