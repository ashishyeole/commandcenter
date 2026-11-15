//! Device-local desktop reminder policy; never exported as portable permissions.
use crate::{Item, Repository, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReminderPolicy {
    #[serde(default)]
    pub cadence: Cadence,
    pub acknowledged_due: Option<String>,
}
#[derive(Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cadence {
    #[default]
    Default,
    Daily,
    Hourly,
    Once,
    Muted,
}

impl Repository {
    pub fn task_reminder_policy(&self, id: &str) -> Result<ReminderPolicy> {
        self.setting(&format!("task_reminder:{id}"))?
            .map(|v| serde_json::from_str(&v).map_err(|e| e.to_string()))
            .unwrap_or_else(|| Ok(ReminderPolicy::default()))
    }
    pub fn update_task_reminder(
        &self,
        id: &str,
        revision: i64,
        cadence: Cadence,
        acknowledge: bool,
    ) -> Result<ReminderPolicy> {
        let task = self
            .list("", false)?
            .into_iter()
            .find(|i| i.id == id && i.kind == "task")
            .ok_or("Task not found")?;
        if task.revision != revision {
            return Err("Task changed. Reopen it before changing reminder controls.".into());
        }
        if acknowledge && (task.due.is_none() || task.status != "active") {
            return Err("Only active dated tasks can be acknowledged".into());
        }
        let policy = ReminderPolicy {
            cadence,
            acknowledged_due: if acknowledge { task.due } else { None },
        };
        self.set_setting(
            &format!("task_reminder:{id}"),
            &serde_json::to_string(&policy).map_err(|e| e.to_string())?,
        )?;
        Ok(policy)
    }
}
/// Returns the existing-compatible delivery identity, or suppresses this occurrence.
/// Callers still enforce global consent, quiet hours and peer receipt coordination.
pub fn delivery_key(
    item: &Item,
    policy: &ReminderPolicy,
    today: &str,
    now: DateTime<Utc>,
    snooze: &str,
    escalate: bool,
) -> Option<String> {
    let due = item.due.as_deref()?;
    if item.kind != "task"
        || item.status != "active"
        || item.deleted_at.is_some()
        || due > today
        || matches!(policy.cadence, Cadence::Muted)
        || policy.acknowledged_due.as_deref() == Some(due)
        || DateTime::parse_from_rfc3339(snooze).is_ok_and(|d| d > now)
    {
        return None;
    }
    let cadence = match policy.cadence {
        Cadence::Hourly => format!("hour:{}", now.timestamp() / 3600),
        Cadence::Default if escalate && item.priority == "critical" => {
            format!("hour:{}", now.timestamp() / 3600)
        }
        Cadence::Once => "once".into(),
        _ => today.into(),
    };
    Some(format!("{}:{due}:{cadence}:{snooze}", item.id))
}
