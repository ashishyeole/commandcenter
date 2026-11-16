//! Opt-in foreground submission and authenticated peer receipt coordination.
use command_center_core::{
    reminders::{delivery_key, ReminderPolicy},
    Repository, Result,
};
use serde_json::{json, Value};
pub(crate) fn save_peer(
    repo: &Repository,
    peer: &str,
    scopes: &[String],
    receipts: &[String],
) -> Result<()> {
    if !scopes.iter().any(|s| s == "task") {
        return Ok(());
    }
    uuid::Uuid::parse_str(peer).map_err(|e| e.to_string())?;
    if receipts.len() > 128 || receipts.iter().any(|s| s.len() > 300) {
        return Err("Invalid peer reminder receipts".into());
    }
    repo.set_setting(
        &format!("mobile_receipts:{peer}"),
        &json!({"at":chrono::Utc::now().timestamp(),"receipts":receipts}).to_string(),
    )
}
pub(crate) fn candidates(repo: &Repository, today: &str) -> Result<Vec<String>> {
    chrono::NaiveDate::parse_from_str(today, "%Y-%m-%d").map_err(|_| "Invalid local date")?;
    // A peer receipt suppresses only an exact task occurrence whose revision was exchanged.
    let peers = repo.peer_status()?;
    let mut keys = vec![];
    for item in repo.list("", false)? {
        let Some(key) = delivery_key(
            &item,
            &ReminderPolicy::default(),
            today,
            chrono::Utc::now(),
            "",
            false,
        ) else {
            continue;
        };
        if repo.delivery_exists(&key)? {
            continue;
        }
        let mut seen = false;
        for peer in &peers {
            let Some(id) = peer["device"].as_str() else {
                continue;
            };
            if !repo.peer_has_current_entity(id, &item.id)? {
                continue;
            }
            let data = repo
                .setting(&format!("mobile_receipts:{id}"))?
                .and_then(|v| serde_json::from_str::<Value>(&v).ok())
                .unwrap_or(Value::Null);
            if data["at"].as_i64().is_some_and(|v| {
                chrono::Utc::now().timestamp() - v <= 120 && v <= chrono::Utc::now().timestamp()
            }) && data["receipts"]
                .as_array()
                .is_some_and(|r| r.iter().any(|r| r.as_str() == Some(&key)))
            {
                seen = true;
                break;
            }
        }
        if !seen {
            keys.push(key);
        }
    }
    keys.truncate(128);
    Ok(keys)
}
pub(crate) fn dispatch(repo: &Repository, operation: &str, request: &Value) -> Result<Value> {
    let today = request["today"].as_str().ok_or("Local date required")?;
    let eligible = candidates(repo, today)?;
    if operation == "reminder_submitted" {
        let keys: Vec<String> = serde_json::from_value(request["keys"].clone())
            .map_err(|_| "Invalid submitted reminder keys")?;
        if request["consent"] != true
            || keys.len() > 128
            || keys.iter().any(|k| !eligible.contains(k))
        {
            return Err("Reminder selection changed or notification permission missing".into());
        }
        for key in keys {
            repo.mark_delivered(&key)?;
        }
        return Ok(json!({"recorded":true}));
    }
    Ok(
        json!({"keys":eligible,"notice":"Only recent authenticated exact-occurrence receipts suppress a reminder. Disconnected devices and simultaneous submissions can duplicate alerts."}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn submission_is_bounded_to_current_due_tasks() {
        let dir = tempfile::tempdir().unwrap();
        let mut repo = Repository::open(dir.path().join("db")).unwrap();
        let item = command_center_core::Item {
            id: uuid::Uuid::new_v4().to_string(),
            kind: "task".into(),
            title: "fixture".into(),
            body: String::new(),
            pinned: false,
            priority: "normal".into(),
            status: "active".into(),
            due: Some("2026-11-16".into()),
            recurrence: "none".into(),
            url: None,
            created_at: command_center_core::now(),
            updated_at: command_center_core::now(),
            device_id: repo.device_id.clone(),
            revision: 1,
            deleted_at: None,
        };
        repo.save(item).unwrap();
        let keys = candidates(&repo, "2026-11-16").unwrap();
        assert_eq!(keys.len(), 1);
        assert!(dispatch(
            &repo,
            "reminder_submitted",
            &json!({"today":"2026-11-16","keys":["forged"],"consent":true})
        )
        .is_err());
        dispatch(
            &repo,
            "reminder_submitted",
            &json!({"today":"2026-11-16","keys":keys,"consent":true}),
        )
        .unwrap();
        assert!(candidates(&repo, "2026-11-16").unwrap().is_empty());
        assert_eq!(candidates(&repo, "2026-11-17").unwrap().len(), 1);
    }
}
