//! Evaluate explicit watch thresholds against metadata already approved and cached.
use crate::{workspace::Record, Repository, Result};
impl Repository {
    pub fn evaluate_watchlists(&mut self) -> Result<usize> {
        let snapshots = self.research_query_snapshots()?;
        let mut changed = 0;
        for mut r in self
            .records("", false)?
            .into_iter()
            .filter(|r| r.module == "watchlists" && r.status == "watching")
        {
            let get = |key: &str| r.fields.get(key).map(String::as_str).unwrap_or("");
            if get("monitor") != "enabled" {
                continue;
            }
            let Ok(threshold) = get("target").parse::<f64>() else {
                continue;
            };
            if !threshold.is_finite() {
                continue;
            }
            let operator = get("operator").to_owned();
            if !["at_least", "at_most"].contains(&operator.as_str()) {
                continue;
            }
            let source = get("source");
            let metric = get("metric").to_owned();
            // Only compare known numeric provider fields; never parse arbitrary prices or rank strings.
            let found = snapshots
                .iter()
                .filter_map(|s| s["items"].as_array().map(|items| (s, items)))
                .flat_map(|(s, items)| items.iter().map(move |i| (s, i)))
                .find(|(_, i)| i["url"].as_str() == Some(source));
            let Some((snapshot, item)) = found else {
                continue;
            };
            let expected = match (item["source"].as_str(), metric.as_str()) {
                (Some("github"), "stars") | (Some("models"), "downloads") => true,
                _ => false,
            };
            if !expected {
                continue;
            }
            let value = item["metric"]
                .as_str()
                .and_then(|s| s.split_whitespace().next())
                .and_then(|s| s.parse::<f64>().ok());
            let Some(value) = value.filter(|v| v.is_finite() && *v >= 0.0) else {
                continue;
            };
            let triggered = if operator == "at_least" {
                value >= threshold
            } else {
                value <= threshold
            };
            let observed_at = snapshot["fetched_at"].as_str().unwrap_or("");
            if r.fields.get("observation_timestamp").map(String::as_str) == Some(observed_at) {
                continue;
            }
            r.fields.insert("observed".into(), value.to_string());
            r.fields
                .insert("observed_at".into(), observed_at.chars().take(10).collect());
            r.fields
                .insert("observation_timestamp".into(), observed_at.into());
            if triggered {
                r.status = "triggered".into();
            }
            let saved = self.save_record(r)?;
            changed += 1;
            if triggered {
                let mut n = Record::new(
                    "notifications",
                    &format!("Watchlist: {}", saved.title),
                    &self.device_id,
                );
                n.status = "needs_review".into();
                n.links.push(saved.id);
                n.summary=format!("Cached {metric}: {value}; threshold {operator} {threshold}. Source observation: {observed_at}. Provider-reported metadata; verify freshness before acting.");
                self.save_record(n)?;
            }
        }
        Ok(changed)
    }
}
