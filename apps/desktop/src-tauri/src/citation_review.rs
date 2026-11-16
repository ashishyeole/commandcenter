//! Deterministic source inspection and explicit human factual review; no second model call.
use crate::{AppState, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tauri::State;
fn evidence(repo: &command_center_core::Repository, id: &str) -> Result<Value> {
    let record = repo
        .records("", false)?
        .into_iter()
        .find(|r| {
            r.id == id
                && r.module == "missions"
                && r.fields
                    .get("provider")
                    .is_some_and(|v| v.starts_with("OpenAI Responses"))
        })
        .ok_or("Cloud answer not found")?;
    repo.ensure_ai_context_allowed(&record.links)?;
    let mut sources = vec![];
    for item in repo
        .list("", false)?
        .into_iter()
        .filter(|i| record.links.contains(&i.id))
    {
        sources.push(
            json!({"id":item.id,"title":item.title,"text":item.body,"revision":item.revision}),
        );
    }
    for r in repo.records("", false)?.into_iter().filter(|r| {
        record.links.contains(&r.id)
            && !["missions", "notifications", "sessions"].contains(&r.module.as_str())
    }) {
        sources.push(json!({"id":r.id,"title":r.title,"text":r.summary,"fields":r.fields,"revision":r.revision}));
    }
    let mut value = json!({"record":record,"sources":sources,"missing":record.links.len().saturating_sub(sources.len()),"notice":"Check each factual claim against the saved source. Citation membership does not prove factual entailment. Sources below reflect current local versions; publisher claims need independent verification."});
    let raw = serde_json::to_vec(&value).map_err(|e| e.to_string())?;
    value["hash"] = json!(format!("{:x}", Sha256::digest(raw)));
    Ok(value)
}
#[tauri::command]
pub fn cloud_evidence_review(state: State<AppState>, record_id: String) -> Result<Value> {
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    evidence(&repo, &record_id)
}
#[tauri::command]
pub fn confirm_cloud_evidence(
    state: State<AppState>,
    record_id: String,
    reviewed_hash: String,
    consent: bool,
) -> Result<()> {
    if !consent {
        return Err("Confirm that you checked the factual claims and marked unsupported claims in the answer".into());
    }
    let mut repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    let value = evidence(&repo, &record_id)?;
    if value["hash"] != reviewed_hash || value["missing"] != 0 {
        return Err("Evidence changed or is missing; review it again".into());
    }
    let mut record: command_center_core::workspace::Record =
        serde_json::from_value(value["record"].clone()).map_err(|e| e.to_string())?;
    record.fields.insert("human_evidence_review".into(),json!({"at":command_center_core::now(),"basis_hash":reviewed_hash,"statement":"Owner reviewed claims against these saved source versions; not an automatic factual guarantee"}).to_string());
    record.status = "reviewed".into();
    repo.ensure_ai_context_allowed(&record.links)?;
    repo.save_record(record)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn review_hash_changes_with_source_and_exclusions_block_review() {
        let mut repo = command_center_core::Repository::open(":memory:").unwrap();
        let mut source = command_center_core::workspace::Record::new(
            "projects",
            "Synthetic evidence",
            &repo.device_id,
        );
        source.summary = "Original evidence".into();
        let mut source = repo.save_record(source).unwrap();
        let mut mission = command_center_core::workspace::Record::new(
            "missions",
            "Synthetic answer",
            &repo.device_id,
        );
        mission.summary = "Generated claim".into();
        mission.links.push(source.id.clone());
        mission.fields.insert(
            "provider".into(),
            "OpenAI Responses API fixture; no network".into(),
        );
        let mission = repo.save_record(mission).unwrap();
        let first = evidence(&repo, &mission.id).unwrap();
        assert_eq!(first["missing"], 0);
        source.summary = "Changed evidence".into();
        repo.save_record(source.clone()).unwrap();
        assert_ne!(first["hash"], evidence(&repo, &mission.id).unwrap()["hash"]);
        repo.set_ai_exclusion(&source.id, true).unwrap();
        assert!(evidence(&repo, &mission.id).is_err());
    }
}
