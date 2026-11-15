//! Device-local standing grants for bounded public-feed/local-model plans only.
use crate::{AppState, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tauri::{Manager, State};
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Schedule {
    id: String,
    generation: String,
    plan_sha256: String,
    interval_minutes: u32,
    enabled: bool,
}
fn rows(state: &AppState) -> Result<Vec<Schedule>> {
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    serde_json::from_str(&repo.setting("agent_schedules")?.unwrap_or("[]".into()))
        .map_err(|_| "Invalid plan schedule settings".into())
}
fn digest(plan: &crate::agent_plans::Plan) -> Result<String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(plan).map_err(|e| e.to_string())?)
    ))
}
pub(crate) fn permission(
    state: &AppState,
    id: &str,
    generation: &str,
    plan: &crate::agent_plans::Plan,
) -> Result<()> {
    let approved = digest(plan)?;
    if !rows(state)?
        .iter()
        .any(|s| s.id == id && s.generation == generation && s.enabled && s.plan_sha256 == approved)
    {
        return Err("Scheduled plan permission changed or was revoked".into());
    }
    Ok(())
}
#[tauri::command]
pub fn agent_schedule_status(state: State<AppState>, id: String) -> Result<Value> {
    let schedule = rows(&state)?.into_iter().find(|s| s.id == id);
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    let health = repo
        .setting(&format!("agent_schedule_health:{id}"))?
        .and_then(|s| serde_json::from_str::<Value>(&s).ok());
    Ok(json!({"schedule":schedule,"health":health}))
}
#[tauri::command]
pub fn configure_agent_schedule(
    state: State<AppState>,
    id: String,
    interval_minutes: u32,
    enabled: bool,
    reviewed_sha256: String,
    confirmation: String,
    consent: bool,
) -> Result<()> {
    let epoch = state.data_epoch.load(std::sync::atomic::Ordering::SeqCst);
    uuid::Uuid::parse_str(&id).map_err(|_| "Invalid plan")?;
    if enabled
        && (!consent
            || confirmation != "AUTHORIZE RECURRING PLAN"
            || !(15..=10080).contains(&interval_minutes))
    {
        return Err("Review the plan and authorize a 15–10080 minute schedule".into());
    }
    let plan_sha = if enabled {
        let preview = crate::agent_plans::preview_agent_plan(state.clone(), id.clone())?;
        if preview["sha256"].as_str() != Some(reviewed_sha256.as_str()) {
            return Err("Plan/context changed after review; review again".into());
        }
        let plan: crate::agent_plans::Plan =
            serde_json::from_value(preview["payload"]["plan"].clone())
                .map_err(|_| "Invalid plan")?;
        digest(&plan)?
    } else {
        String::new()
    };
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    if epoch != state.data_epoch.load(std::sync::atomic::Ordering::SeqCst) {
        return Err("Workspace reset during schedule review".into());
    }
    let mut schedules: Vec<Schedule> =
        serde_json::from_str(&repo.setting("agent_schedules")?.unwrap_or("[]".into()))
            .map_err(|_| "Invalid schedules")?;
    schedules.retain(|s| s.id != id);
    if enabled {
        if schedules.len() >= 20 {
            return Err("At most 20 scheduled plans".into());
        }
        schedules.push(Schedule {
            id: id.clone(),
            generation: uuid::Uuid::new_v4().to_string(),
            plan_sha256: plan_sha,
            interval_minutes,
            enabled,
        });
    }
    repo.set_setting(
        "agent_schedules",
        &serde_json::to_string(&schedules).map_err(|e| e.to_string())?,
    )?;
    if enabled {
        repo.set_setting(
            &format!("agent_schedule_next:{id}"),
            &(chrono::Utc::now().timestamp() + i64::from(interval_minutes) * 60).to_string(),
        )?;
    }
    drop(repo);
    if let Some(token) = state
        .jobs
        .lock()
        .map_err(|_| "Job state unavailable")?
        .get(&format!("plan:{id}"))
    {
        token.cancel();
    }
    Ok(())
}
pub async fn poll(app: &tauri::AppHandle) -> Result<()> {
    let state = app.state::<AppState>();
    if !state
        .jobs
        .lock()
        .map_err(|_| "Job state unavailable")?
        .is_empty()
    {
        return Ok(());
    }
    for schedule in rows(&state)?.into_iter().filter(|s| s.enabled) {
        let epoch = state.data_epoch.load(std::sync::atomic::Ordering::SeqCst);
        {
            let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
            let next = repo
                .setting(&format!("agent_schedule_next:{}", schedule.id))?
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or(i64::MAX);
            if chrono::Utc::now().timestamp() < next {
                continue;
            }
            repo.set_setting(
                &format!("agent_schedule_next:{}", schedule.id),
                &(chrono::Utc::now().timestamp() + i64::from(schedule.interval_minutes) * 60)
                    .to_string(),
            )?;
        }
        let result = async {
            let preview = crate::agent_plans::preview_agent_plan(app.state(), schedule.id.clone())?;
            let plan: crate::agent_plans::Plan =
                serde_json::from_value(preview["payload"]["plan"].clone())
                    .map_err(|_| "Invalid plan")?;
            permission(&state, &schedule.id, &schedule.generation, &plan)?;
            crate::agent_plans::run_authorized(
                app.clone(),
                schedule.id.clone(),
                preview["sha256"]
                    .as_str()
                    .ok_or("Missing approval digest")?
                    .into(),
                true,
                Some(schedule.generation.clone()),
            )
            .await
        }
        .await;
        let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
        if epoch != state.data_epoch.load(std::sync::atomic::Ordering::SeqCst) {
            return Ok(());
        }
        repo.set_setting(&format!("agent_schedule_health:{}",schedule.id),&json!({"at":command_center_core::now(),"status":if result.is_ok(){"completed"}else{"failed"},"error":result.err(),"notice":"Only explicitly listed public feeds and local model context were authorized. Results remain suggestions; no external actions were performed."}).to_string())?;
        break; // Bound each poll to one plan. Missed intervals are not replayed.
    }
    Ok(())
}
