use crate::{AppState, Result};
use command_center_core::workspace::Record;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tauri::{Manager, State};
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub kind: String,
    pub prompt: String,
    pub source: String,
    pub query: String,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub name: String,
    pub model: String,
    pub context_ids: Vec<String>,
    pub steps: Vec<Step>,
}
fn validate(p: &Plan) -> Result<()> {
    if p.name.trim().is_empty()
        || p.name.len() > 200
        || p.model.len() > 160
        || p.context_ids.len() > 30
        || p.steps.is_empty()
        || p.steps.len() > 6
    {
        return Err("Use up to 30 context records and one to six steps".into());
    }
    let mut ids = std::collections::HashSet::new();
    for id in &p.context_ids {
        uuid::Uuid::parse_str(id).map_err(|_| "Invalid context ID")?;
        if !ids.insert(id) {
            return Err("Duplicate context ID".into());
        }
    }
    for s in &p.steps {
        if s.prompt.len() > 2000 || s.query.len() > 300 {
            return Err("Step prompt or query exceeds limit".into());
        }
        match s.kind.as_str() {
            "analyze" if !s.prompt.trim().is_empty() => (),
            "source"
                if [
                    "github", "models", "ollama", "news", "youtube", "apps", "mcp", "arxiv", "rss",
                ]
                .contains(&s.source.as_str()) =>
            {
                ()
            }
            _ => {
                return Err(
                    "Steps may only read approved public feeds or analyze selected context locally"
                        .into(),
                )
            }
        }
    }
    Ok(())
}
fn context(state: &AppState, p: &Plan) -> Result<Value> {
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    repo.ensure_ai_context_allowed(&p.context_ids)?;
    let mut rows = vec![];
    for i in repo.list("", false)? {
        if p.context_ids.contains(&i.id) {
            rows.push(serde_json::to_value(i).map_err(|e| e.to_string())?);
        }
    }
    for r in repo.records("", false)? {
        if p.context_ids.contains(&r.id)
            && !["missions", "sessions", "notifications"].contains(&r.module.as_str())
        {
            rows.push(serde_json::to_value(r).map_err(|e| e.to_string())?);
        }
    }
    if rows.len() != p.context_ids.len() {
        return Err("Context must contain existing ordinary records; vault, missions, sessions and notifications are excluded".into());
    }
    rows.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    let v = json!(rows);
    if serde_json::to_vec(&v).map_err(|e| e.to_string())?.len() > 60000 {
        return Err("Selected context exceeds 60 KB".into());
    }
    Ok(v)
}
fn hash(v: &Value) -> Result<String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(v).map_err(|e| e.to_string())?)
    ))
}
fn get(state: &AppState, id: &str) -> Result<(Record, Plan)> {
    let r = state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .records("", false)?
        .into_iter()
        .find(|r| r.id == id && r.module == "missions")
        .ok_or("Saved plan not found")?;
    let p = serde_json::from_str::<Plan>(
        r.fields
            .get("agent_plan")
            .ok_or("This mission is not an executable plan")?,
    )
    .map_err(|_| "Invalid saved plan")?;
    validate(&p)?;
    Ok((r, p))
}
#[tauri::command]
pub fn save_agent_plan(
    state: State<AppState>,
    id: Option<String>,
    revision: Option<i64>,
    plan: Plan,
) -> Result<String> {
    validate(&plan)?;
    context(&state, &plan)?;
    let mut r = if let Some(id) = id {
        let (r, _) = get(&state, &id)?;
        if Some(r.revision) != revision {
            return Err("Plan changed; reload it before editing".into());
        }
        r
    } else {
        Record::new(
            "missions",
            &plan.name,
            &state
                .repo
                .lock()
                .map_err(|_| "Database unavailable")?
                .device_id,
        )
    };
    r.title = plan.name.clone();
    r.status = "draft".into();
    r.summary =
        "Saved plan; no steps executed. Review its exact context and permissions before running."
            .into();
    r.links = plan.context_ids.clone();
    r.fields.insert(
        "agent_plan".into(),
        serde_json::to_string(&plan).map_err(|e| e.to_string())?,
    );
    r.fields.remove("step_states");
    r.fields.remove("run_id");
    let id = r.id.clone();
    state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .save_record(r)?;
    Ok(id)
}
#[tauri::command]
pub fn preview_agent_plan(state: State<AppState>, id: String) -> Result<Value> {
    let (r, p) = get(&state, &id)?;
    let payload = json!({"plan":p,"revision":r.revision,"context":context(&state,&p)?,"permissions":"Only the listed public feed requests and local record briefing compilation. No cloud models, filesystem reads, account reads, MCP calls, sends, purchases, deletes or commits. Public excerpts are collected as evidence; no model is used."});
    Ok(json!({"payload":payload,"sha256":hash(&payload)?}))
}
struct Guard<'a> {
    state: &'a AppState,
    id: String,
}
impl Drop for Guard<'_> {
    fn drop(&mut self) {
        if let Ok(mut jobs) = self.state.jobs.lock() {
            jobs.remove(&self.id);
        }
    }
}
#[tauri::command]
pub async fn run_agent_plan(
    app: tauri::AppHandle,
    id: String,
    reviewed_sha256: String,
    consent: bool,
) -> Result<Value> {
    run_authorized(app, id, reviewed_sha256, consent, None).await
}
pub(crate) async fn run_authorized(
    app: tauri::AppHandle,
    id: String,
    reviewed_sha256: String,
    consent: bool,
    schedule_generation: Option<String>,
) -> Result<Value> {
    let state = app.state::<AppState>();
    if !consent {
        return Err("Approve the reviewed plan and data access".into());
    }
    let preview = preview_agent_plan(app.state(), id.clone())?;
    if preview["sha256"].as_str() != Some(reviewed_sha256.as_str()) {
        return Err("Plan or context changed; review again".into());
    }
    let (mut record, plan) = get(&state, &id)?;
    let approved_context = context(&state, &plan)?;
    if preview["payload"]["plan"] != serde_json::to_value(&plan).map_err(|e| e.to_string())?
        || preview["payload"]["revision"] != json!(record.revision)
        || preview["payload"]["context"] != approved_context
    {
        return Err("Plan or context changed during approval; review again".into());
    }
    if let Some(generation) = &schedule_generation {
        crate::agent_schedules::permission(&state, &id, generation, &plan)?;
    }
    let initial_context = hash(&approved_context)?;
    let epoch = state.data_epoch.load(std::sync::atomic::Ordering::SeqCst);
    let job_id = format!("plan:{id}");
    let cancel = tokio_util::sync::CancellationToken::new();
    {
        let mut jobs = state.jobs.lock().map_err(|_| "Job state unavailable")?;
        if !jobs.is_empty() {
            return Err(
                "Finish or cancel other AI/tool/model work before running a multi-step plan".into(),
            );
        }
        jobs.insert(job_id.clone(), cancel.clone());
    }
    let _guard = Guard {
        state: &state,
        id: job_id,
    };
    if let Some(generation) = &schedule_generation {
        crate::agent_schedules::permission(&state, &id, generation, &plan)?;
    }
    let conversation = uuid::Uuid::new_v4().to_string();
    let mut states = vec![json!({"status":"pending"}); plan.steps.len()];
    record.status = "running".into();
    record.fields.insert("run_id".into(), conversation.clone());
    record.fields.insert(
        "step_states".into(),
        serde_json::to_string(&states).map_err(|e| e.to_string())?,
    );
    record = state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .save_record(record)?;
    let mut evidence: Vec<Value> = vec![];
    let mut outputs: Vec<Value> = vec![];
    let work = async {
        for (index, step) in plan.steps.iter().enumerate() {
            if let Some(generation) = &schedule_generation {
                crate::agent_schedules::permission(&state, &id, generation, &plan)?;
            }
            if cancel.is_cancelled() {
                return Err("Plan cancelled".to_string());
            }
            if state.data_epoch.load(std::sync::atomic::Ordering::SeqCst) != epoch
                || hash(&context(&state, &plan)?)? != initial_context
            {
                return Err("Workspace or selected context changed; review a new run".into());
            }
            states[index] = json!({"status":"running","started_at":command_center_core::now()});
            record.fields.insert(
                "step_states".into(),
                serde_json::to_string(&states).map_err(|e| e.to_string())?,
            );
            record = state
                .repo
                .lock()
                .map_err(|_| "Database unavailable")?
                .save_record(record.clone())?;
            let step_work = async {
                if step.kind == "source" {
                    let feed = crate::connectors::refresh_source(
                        app.state(),
                        step.source.clone(),
                        step.query.clone(),
                        true,
                    )
                    .await?;
                    let summary = json!({"source":feed.source,"fetched_at":feed.fetched_at,"items":feed.items.as_array().map(|a|a.iter().take(5).cloned().collect::<Vec<_>>()).unwrap_or_default()});
                    evidence.push(summary.clone());
                    Ok(summary)
                } else {
                    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
                    let briefing = repo.record_briefing(
                        &plan.context_ids,
                        &chrono::Local::now().format("%Y-%m-%d").to_string(),
                    )?;
                    Ok(
                        json!({"research_question":step.prompt,"record_briefing":briefing,"public_evidence":evidence,"method":"Deterministic compilation, not generated analysis. Research questions are retained for your review, not answered by a model."}),
                    )
                }
            };
            let value = tokio::select! {_=cancel.cancelled()=>Err("Plan cancelled. Already completed steps remain in history.".into()),r=step_work=>r};
            match value {
                Ok(value) => {
                    states[index] =
                        json!({"status":"completed","finished_at":command_center_core::now()});
                    outputs.push(value);
                }
                Err(error) => {
                    states[index] = json!({"status":if cancel.is_cancelled(){"cancelled"}else{"failed"},"error":error});
                    return Err(error);
                }
            }
            record.fields.insert(
                "step_states".into(),
                serde_json::to_string(&states).map_err(|e| e.to_string())?,
            );
            record.summary = truncate(
                &serde_json::to_string_pretty(&outputs).map_err(|e| e.to_string())?,
                95000,
            );
            record = state
                .repo
                .lock()
                .map_err(|_| "Database unavailable")?
                .save_record(record.clone())?;
        }
        Ok(())
    };
    let result: Result<()> = work.await;
    if state.data_epoch.load(std::sync::atomic::Ordering::SeqCst) == epoch {
        record.status = if result.is_ok() {
            "completed"
        } else if cancel.is_cancelled() {
            "cancelled"
        } else {
            "failed"
        }
        .into();
        record.fields.insert(
            "step_states".into(),
            serde_json::to_string(&states).map_err(|e| e.to_string())?,
        );
        if let Err(e) = &result {
            record.summary = format!("{}\n\nStopped: {e}", truncate(&record.summary, 95000));
        }
        state
            .repo
            .lock()
            .map_err(|_| "Database unavailable")?
            .save_record(record)?;
    }
    result?;
    Ok(json!({"steps":states,"outputs":outputs,"plan_id":id}))
}
fn truncate(s: &str, max: usize) -> String {
    let mut n = s.len().min(max);
    while !s.is_char_boundary(n) {
        n -= 1;
    }
    s[..n].to_owned()
}
