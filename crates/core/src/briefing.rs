//! Deterministic record briefing: no model, network or generated claims.
use crate::{Repository, Result};
impl Repository {
    pub fn record_briefing(&self, ids: &[String], today: &str) -> Result<String> {
        if ids.len() > 30 {
            return Err("Choose at most 30 records".into());
        }
        chrono::NaiveDate::parse_from_str(today, "%Y-%m-%d")
            .map_err(|_| "Invalid briefing date")?;
        let mut due = Vec::new();
        let mut waiting = Vec::new();
        let mut other = Vec::new();
        let mut included = std::collections::HashSet::new();
        for i in self
            .list("", false)?
            .into_iter()
            .filter(|i| ids.contains(&i.id))
        {
            included.insert(i.id.clone());
            let row = format!(
                "- {} [{}] · {} · due {} · priority {}",
                i.title,
                i.id,
                i.status,
                i.due.as_deref().unwrap_or("not set"),
                i.priority
            );
            if i.kind == "task"
                && i.status == "active"
                && i.due.as_deref().is_some_and(|d| d <= today)
            {
                due.push(row);
            } else if i.status == "waiting" {
                waiting.push(row);
            } else {
                other.push(row);
            }
        }
        for r in self.records("", false)?.into_iter().filter(|r| {
            ids.contains(&r.id)
                && !["missions", "notifications", "sessions"].contains(&r.module.as_str())
        }) {
            included.insert(r.id.clone());
            let row = format!(
                "- {} [{}] · {} · {} · next: {}",
                r.title,
                r.id,
                r.module,
                r.status,
                r.fields
                    .get("next_action")
                    .map(String::as_str)
                    .unwrap_or("not recorded")
            );
            if r.status == "blocked" {
                waiting.push(row);
            } else {
                other.push(row);
            }
        }
        due.sort();
        waiting.sort();
        other.sort();
        let section = |rows: Vec<String>| {
            if rows.is_empty() {
                "No matching selected records.".into()
            } else {
                rows.join("\n")
            }
        };
        let missing = ids.iter().collect::<std::collections::HashSet<_>>().len() - included.len();
        Ok(format!("# Record briefing · {today}\n\nCompiled from saved fields, not AI-generated analysis. Only selected ordinary records are included. Dates and claims are user/source supplied.\n\n## Due or overdue\n{}\n\n## Waiting or blocked\n{}\n\n## Other selected records\n{}\n\nUnavailable or excluded selections: {missing}. No external requests or actions were performed.",section(due),section(waiting),section(other)))
    }
}
