//! Public iCalendar schedules: provenance, explicit watch/import, no invented dates or guarantees.
use crate::{AppState, Result};
use chrono::{NaiveDate, NaiveDateTime, TimeZone};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tauri::{Manager, State};
#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub id: String,
    pub name: String,
    pub url: String,
    pub category: String,
    pub verified_authority: bool,
    pub enabled: bool,
    pub interval_minutes: u32,
}
fn list(state: &AppState) -> Result<Vec<Source>> {
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    serde_json::from_str(&repo.setting("event_sources")?.unwrap_or("[]".into()))
        .map_err(|_| "Invalid event source configuration".into())
}
fn validate(s: &Source) -> Result<reqwest::Url> {
    uuid::Uuid::parse_str(&s.id).map_err(|_| "Invalid source ID")?;
    let u = reqwest::Url::parse(&s.url).map_err(|_| "Invalid calendar URL")?;
    if s.name.is_empty()
        || s.name.len() > 160
        || s.url.len() > 2000
        || u.scheme() != "https"
        || !u.username().is_empty()
        || u.password().is_some()
        || u.query().is_some()
        || u.fragment().is_some()
        || u.host_str().is_none()
        || !["poker", "agriculture", "travel"].contains(&s.category.as_str())
        || !(60..=10080).contains(&s.interval_minutes)
    {
        return Err("Use a public HTTPS calendar without query/credentials, a supported category and 60–10080 minute interval".into());
    }
    Ok(u)
}
#[tauri::command]
pub fn event_source_status(state: State<AppState>) -> Result<Value> {
    let sources = list(&state)?;
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    let rows = sources
        .into_iter()
        .map(|s| {
            let cache = repo
                .setting(&format!("event_cache:{}", s.id))
                .ok()
                .flatten()
                .and_then(|v| serde_json::from_str::<Value>(&v).ok());
            let health = repo
                .setting(&format!("event_health:{}", s.id))
                .ok()
                .flatten()
                .and_then(|v| serde_json::from_str::<Value>(&v).ok());
            json!({"source":s,"cache":cache,"health":health})
        })
        .collect::<Vec<_>>();
    Ok(json!(rows))
}
#[tauri::command]
pub fn save_event_source(state: State<AppState>, source: Source, consent: bool) -> Result<()> {
    validate(&source)?;
    if !consent {
        return Err("Approve this public calendar URL and its selected refresh policy".into());
    }
    let mut all = list(&state)?;
    let source_id = source.id.clone();
    let changed = all.iter().find(|s| s.id == source.id).is_some_and(|s| {
        s.url != source.url
            || s.category != source.category
            || s.verified_authority != source.verified_authority
    });
    all.retain(|s| s.id != source.id);
    if all.len() >= 20 {
        return Err("At most 20 public calendar sources".into());
    }
    all.push(source);
    if changed {
        state
            .repo
            .lock()
            .map_err(|_| "Database unavailable")?
            .set_setting(&format!("event_cache:{source_id}"), "null")?;
    }
    state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .set_setting(
            "event_sources",
            &serde_json::to_string(&all).map_err(|e| e.to_string())?,
        )
}
#[tauri::command]
pub fn remove_event_source(state: State<AppState>, id: String) -> Result<()> {
    let mut all = list(&state)?;
    all.retain(|s| s.id != id);
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    repo.set_setting(
        "event_sources",
        &serde_json::to_string(&all).map_err(|e| e.to_string())?,
    )?;
    repo.set_setting(&format!("event_cache:{id}"), "null")?;
    Ok(())
}
fn clean(s: &str) -> String {
    s.replace("\\n", "\n")
        .replace("\\N", "\n")
        .replace("\\,", ",")
        .replace("\\;", ";")
        .replace("\\\\", "\\")
        .chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .take(12000)
        .collect()
}
fn date(p: &ical::property::Property) -> Result<Value> {
    let value = p.value.as_deref().ok_or("Missing date")?;
    if value.len() == 8 {
        let d = NaiveDate::parse_from_str(value, "%Y%m%d").map_err(|_| "Invalid all-day date")?;
        return Ok(
            json!({"date":d.to_string(),"all_day":true,"timezone":"calendar date","raw":value}),
        );
    }
    let raw = value.trim_end_matches('Z');
    let naive = NaiveDateTime::parse_from_str(raw, "%Y%m%dT%H%M%S")
        .map_err(|_| "Unsupported event date format")?;
    let tz = p
        .params
        .as_ref()
        .and_then(|v| v.iter().find(|(k, _)| k.eq_ignore_ascii_case("TZID")))
        .and_then(|(_, v)| v.first());
    if value.ends_with('Z') {
        let utc = chrono::Utc.from_utc_datetime(&naive);
        return Ok(
            json!({"date":utc.with_timezone(&chrono::Local).date_naive().to_string(),"at":utc.to_rfc3339(),"all_day":false,"timezone":"UTC","raw":value}),
        );
    }
    if let Some(zone) = tz {
        let timezone: chrono_tz::Tz = zone
            .parse()
            .map_err(|_| "Unsupported timezone; review original calendar")?;
        let at = timezone
            .from_local_datetime(&naive)
            .single()
            .ok_or("Ambiguous or nonexistent local event time; review original source")?;
        return Ok(
            json!({"date":at.with_timezone(&chrono::Local).date_naive().to_string(),"at":at.to_rfc3339(),"all_day":false,"timezone":zone,"raw":value}),
        );
    }
    Ok(
        json!({"date":naive.date().to_string(),"local_time":naive.to_string(),"all_day":false,"timezone":"floating; source does not specify timezone","raw":value}),
    )
}
fn parse(bytes: Vec<u8>, source: &Source) -> Result<Value> {
    let text = std::str::from_utf8(&bytes).map_err(|_| "Calendar must be UTF-8")?;
    if !text.contains("BEGIN:VCALENDAR") {
        return Err("Source is not an iCalendar document".into());
    }
    let mut rows = vec![];
    let mut skipped = vec![];
    let mut seen = std::collections::HashSet::new();
    let mut total = 0;
    for cal in ical::IcalParser::new(std::io::Cursor::new(bytes)) {
        let cal = cal.map_err(|_| "Invalid calendar syntax")?;
        for event in cal.events {
            total += 1;
            if total > 1000 {
                return Err("Calendar exceeds 1000 events; use a narrower publisher feed".into());
            }
            let get = |name: &str| {
                event
                    .properties
                    .iter()
                    .find(|p| p.name.eq_ignore_ascii_case(name))
                    .and_then(|p| p.value.as_deref())
                    .map(clean)
                    .unwrap_or_default()
            };
            let uid = get("UID");
            let title = get("SUMMARY");
            if uid.is_empty() || uid.len() > 1024 || title.is_empty() {
                skipped.push("Event missing UID or title".to_string());
                continue;
            }
            let start = match event
                .properties
                .iter()
                .find(|p| p.name.eq_ignore_ascii_case("DTSTART"))
                .map(date)
                .transpose()
            {
                Ok(Some(v)) => v,
                Ok(None) => {
                    skipped.push(format!("{title}: missing start date"));
                    continue;
                }
                Err(e) => {
                    skipped.push(format!("{title}: {e}"));
                    continue;
                }
            };
            let recurrence = get("RRULE");
            let instance = get("RECURRENCE-ID");
            let identity = format!(
                "{:x}",
                Sha256::digest(format!("{}\0{uid}\0{instance}", source.id).as_bytes())
            );
            if !seen.insert(identity.clone()) {
                skipped.push(format!("{title}: duplicate UID/instance"));
                continue;
            }
            let provider_url = get("URL");
            let url = reqwest::Url::parse(&provider_url)
                .ok()
                .filter(|u| {
                    u.scheme() == "https" && u.username().is_empty() && u.password().is_none()
                })
                .map(|u| u.to_string())
                .unwrap_or(source.url.clone());
            let end = event
                .properties
                .iter()
                .find(|p| p.name.eq_ignore_ascii_case("DTEND"))
                .and_then(|p| date(p).ok());
            rows.push(json!({"id":identity,"uid":uid,"title":title.chars().take(500).collect::<String>(),"description":get("DESCRIPTION"),"location":get("LOCATION"),"start":start,"end":end,"status":get("STATUS"),"recurrence":recurrence,"recurrence_instance":instance,"recurrence_expanded":false,"has_recurrence_exceptions":event.properties.iter().any(|p|["EXDATE","RDATE"].contains(&p.name.as_str())),"sequence":get("SEQUENCE"),"source_updated":get("LAST-MODIFIED"),"source_url":url,"calendar_url":source.url,"verification":if source.verified_authority{"Source authority explicitly confirmed by user; schedule is publisher-supplied and can change"}else{"Unconfirmed source authority; independently verify before travel or registration"}}));
        }
    }
    // Expand only a precisely supported subset; complex rules retain their original declaration.
    let overridden = rows
        .iter()
        .filter(|r| {
            r["recurrence_instance"]
                .as_str()
                .is_some_and(|s| !s.is_empty())
        })
        .filter_map(|r| r["uid"].as_str().map(str::to_owned))
        .collect::<std::collections::HashSet<_>>();
    let mut expanded = Vec::new();
    for row in rows {
        if !overridden.contains(row["uid"].as_str().unwrap_or("")) {
            if let Some(instances) = expand_all_day(&row) {
                if expanded.len() + instances.len() > 1000 {
                    return Err("Expanded calendar exceeds 1000 occurrences".into());
                }
                expanded.extend(instances);
                continue;
            }
        }
        expanded.push(row);
    }
    let mut rows = expanded;
    rows.sort_by(|a, b| {
        a["start"]["date"]
            .as_str()
            .cmp(&b["start"]["date"].as_str())
    });
    Ok(
        json!({"source":source,"fetched_at":command_center_core::now(),"events":rows,"skipped":skipped.into_iter().take(30).collect::<Vec<_>>(),"notice":"Simple all-day DAILY/WEEKLY rules with INTERVAL and COUNT or date UNTIL are expanded up to 366 occurrences. Complex rules, exceptions and timed recurrences remain unexpanded; verify them at the publisher. All-day end dates are exclusive. Floating times require source verification. Monetary guarantees are never inferred from prose."}),
    )
}
async fn refresh(state: &AppState, id: &str) -> Result<Value> {
    let source = list(state)?
        .into_iter()
        .find(|s| s.id == id)
        .ok_or("Calendar source not found")?;
    let url = validate(&source)?;
    let epoch = state.data_epoch.load(std::sync::atomic::Ordering::SeqCst);
    {
        let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
        let last = repo
            .setting(&format!("event_attempt:{id}"))?
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(0);
        if chrono::Utc::now().timestamp() - last < 60 {
            return Err("Wait one minute between refreshes of this calendar".into());
        }
        repo.set_setting(
            &format!("event_attempt:{id}"),
            &chrono::Utc::now().timestamp().to_string(),
        )?;
    }
    let mut response = crate::research::public_client(&url)
        .await?
        .get(url)
        .header("Accept", "text/calendar")
        .send()
        .await
        .map_err(|_| "Calendar request failed")?;
    if !response.status().is_success() {
        return Err(format!("Calendar returned HTTP {}", response.status()));
    }
    let mut bytes = vec![];
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "Calendar response interrupted")?
    {
        if bytes.len() + chunk.len() > 2 * 1024 * 1024 {
            return Err("Calendar exceeds 2 MiB".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    let copied = source.clone();
    let cache = tauri::async_runtime::spawn_blocking(move || parse(bytes, &copied))
        .await
        .map_err(|_| "Calendar parser interrupted")??;
    if !list(state)?.iter().any(|s| s == &source) {
        return Err("Calendar authorization changed during refresh".into());
    }
    let mut repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    if epoch != state.data_epoch.load(std::sync::atomic::Ordering::SeqCst) {
        return Err("Workspace reset during calendar refresh".into());
    }
    let current: Vec<Source> =
        serde_json::from_str(&repo.setting("event_sources")?.unwrap_or("[]".into()))
            .map_err(|_| "Invalid event sources")?;
    if !current.iter().any(|s| s == &source) {
        return Err("Calendar source changed before saving results".into());
    }
    let old: Value = repo
        .setting(&format!("event_cache:{id}"))?
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(json!({}));
    let watched = repo
        .records("", false)?
        .into_iter()
        .filter(|r| {
            r.fields.get("calendar_source_id").map(String::as_str) == Some(id)
                && r.fields.get("watch_schedule").map(String::as_str) == Some("true")
        })
        .collect::<Vec<_>>();
    for record in watched {
        let event_id = record
            .fields
            .get("calendar_event_id")
            .cloned()
            .unwrap_or_default();
        let previous = old["events"].as_array().and_then(|v| {
            v.iter()
                .find(|e| e["id"].as_str() == Some(event_id.as_str()))
        });
        let next = cache["events"].as_array().and_then(|v| {
            v.iter()
                .find(|e| e["id"].as_str() == Some(event_id.as_str()))
        });
        if let (Some(a), Some(b)) = (previous, next) {
            if a["start"] != b["start"]
                || a["status"] != b["status"]
                || a["location"] != b["location"]
            {
                let mut n = command_center_core::workspace::Record::new(
                    "notifications",
                    &format!("Schedule changed: {}", record.title),
                    &repo.device_id,
                );
                n.links.push(record.id);
                n.status = "needs_review".into();
                n.summary=json!({"source":source.url,"observed_at":command_center_core::now(),"previous":a,"current":b,"notice":"Saved event dates were not overwritten. Review the publisher and update your plans."}).to_string();
                if n.summary.len() > 90000 {
                    n.summary="Publisher schedule changed. Open the public calendar source and review the current event.".into();
                }
                repo.save_record(n)?;
            }
        }
    }
    repo.set_setting(&format!("event_cache:{id}"), &cache.to_string())?;
    repo.set_setting(
        &format!("event_health:{id}"),
        &json!({"at":command_center_core::now(),"status":"refreshed"}).to_string(),
    )?;
    Ok(cache)
}
#[tauri::command]
pub async fn refresh_event_source(
    state: State<'_, AppState>,
    id: String,
    consent: bool,
) -> Result<Value> {
    if !consent {
        return Err("Approve refreshing this public calendar".into());
    }
    refresh(&state, &id).await
}
#[tauri::command]
pub fn save_calendar_event(
    state: State<AppState>,
    source_id: String,
    event_id: String,
    watch: bool,
) -> Result<String> {
    let source = list(&state)?
        .into_iter()
        .find(|s| s.id == source_id)
        .ok_or("Calendar source not found")?;
    let mut repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    if let Some(existing) = repo
        .records("", false)?
        .into_iter()
        .find(|r| r.fields.get("calendar_event_id") == Some(&event_id))
    {
        return Ok(existing.id);
    }
    let cache: Value = serde_json::from_str(
        &repo
            .setting(&format!("event_cache:{source_id}"))?
            .ok_or("Refresh the calendar first")?,
    )
    .map_err(|_| "Invalid calendar cache")?;
    if cache["source"]["url"].as_str() != Some(source.url.as_str())
        || cache["source"]["category"].as_str() != Some(source.category.as_str())
        || cache["source"]["verified_authority"] != json!(source.verified_authority)
    {
        return Err("Calendar source changed; refresh it before importing events".into());
    }
    let event = cache["events"]
        .as_array()
        .and_then(|v| {
            v.iter()
                .find(|e| e["id"].as_str() == Some(event_id.as_str()))
        })
        .ok_or("Event not found in current cache")?;
    let mut r = command_center_core::workspace::Record::new(
        if source.category == "travel" {
            "events"
        } else {
            &source.category
        },
        event["title"].as_str().ok_or("Invalid event")?,
        &repo.device_id,
    );
    r.status = if event["status"] == "CANCELLED" {
        "cancelled"
    } else {
        "unconfirmed"
    }
    .into();
    r.summary = format!(
        "{}\n\n{}\nFetched: {}",
        event["description"].as_str().unwrap_or(""),
        event["verification"].as_str().unwrap_or(""),
        cache["fetched_at"].as_str().unwrap_or("")
    );
    for (key, value) in [
        ("calendar_source_id", source_id.as_str()),
        ("calendar_event_id", event_id.as_str()),
        ("watch_schedule", if watch { "true" } else { "false" }),
        ("due", event["start"]["date"].as_str().unwrap_or("")),
        ("source", event["source_url"].as_str().unwrap_or("")),
        ("venue", event["location"].as_str().unwrap_or("")),
        ("location", event["location"].as_str().unwrap_or("")),
        (
            "timezone",
            event["start"]["timezone"].as_str().unwrap_or(""),
        ),
        ("recurrence", event["recurrence"].as_str().unwrap_or("")),
    ] {
        r.fields.insert(key.into(), value.into());
    }
    let id = r.id.clone();
    repo.save_record(r)?;
    Ok(id)
}
pub async fn poll(app: &tauri::AppHandle) -> Result<()> {
    let state = app.state::<AppState>();
    for s in list(&state)?.into_iter().filter(|s| s.enabled) {
        let due = {
            let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
            let last = repo
                .setting(&format!("event_attempt:{}", s.id))?
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(0);
            chrono::Utc::now().timestamp() - last >= i64::from(s.interval_minutes) * 60
        };
        if due {
            let epoch = state.data_epoch.load(std::sync::atomic::Ordering::SeqCst);
            if let Err(error) = refresh(&state, &s.id).await {
                let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
                if epoch != state.data_epoch.load(std::sync::atomic::Ordering::SeqCst) {
                    return Ok(());
                }
                repo.set_setting(
                    &format!("event_health:{}", s.id),
                    &json!({"at":command_center_core::now(),"status":"failed","error":error})
                        .to_string(),
                )?;
            }
        }
    }
    Ok(())
}

fn expand_all_day(row: &Value) -> Option<Vec<Value>> {
    if row["start"]["all_day"] != true
        || row["has_recurrence_exceptions"] == true
        || row["status"] == "CANCELLED"
    {
        return None;
    }
    let rule = row["recurrence"].as_str()?;
    if rule.is_empty() {
        return None;
    }
    let mut parts = std::collections::BTreeMap::new();
    for part in rule.split(';') {
        let (key, value) = part.split_once('=')?;
        if !["FREQ", "INTERVAL", "COUNT", "UNTIL"].contains(&key)
            || parts.insert(key, value).is_some()
        {
            return None;
        }
    }
    let unit = match *parts.get("FREQ")? {
        "DAILY" => 1u64,
        "WEEKLY" => 7,
        _ => return None,
    };
    let interval = parts
        .get("INTERVAL")
        .map(|s| s.parse::<u64>().ok())
        .unwrap_or(Some(1))?;
    if interval == 0 || interval > 366 {
        return None;
    }
    let count = parts
        .get("COUNT")
        .map(|s| s.parse::<usize>().ok())
        .unwrap_or(Some(366))?;
    if count == 0 || count > 366 {
        return None;
    }
    let until = parts
        .get("UNTIL")
        .map(|s| NaiveDate::parse_from_str(s, "%Y%m%d").ok())
        .transpose_option()?;
    if !parts.contains_key("COUNT") && until.is_none() {
        return None;
    }
    let start = NaiveDate::parse_from_str(row["start"]["date"].as_str()?, "%Y-%m-%d").ok()?;
    if until.is_some_and(|u| u < start)
        || (parts.contains_key("COUNT") && parts.contains_key("UNTIL"))
    {
        return None;
    }
    let end = row["end"]["date"]
        .as_str()
        .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok());
    let duration = end.map(|d| d - start);
    let mut rows = Vec::new();
    for i in 0..count {
        let date = start.checked_add_days(chrono::Days::new(
            unit.checked_mul(interval)?.checked_mul(i as u64)?,
        ))?;
        if until.is_some_and(|u| date > u) {
            break;
        }
        let mut instance = row.clone();
        instance["id"] = json!(format!("{}:{}", row["id"].as_str()?, date));
        instance["start"]["date"] = json!(date.to_string());
        if let Some(duration) = duration {
            instance["end"]["date"] = json!((date + duration).to_string());
        }
        instance["recurrence_instance"] = json!(date.to_string());
        instance["recurrence_expanded"] = json!(true);
        rows.push(instance);
    }
    if !parts.contains_key("COUNT")
        && until.is_some_and(|u| {
            rows.last()
                .and_then(|r| r["start"]["date"].as_str())
                .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
                .and_then(|d| d.checked_add_days(chrono::Days::new(unit * interval)))
                .is_some_and(|next| next <= u)
        })
    {
        return None;
    }
    Some(rows)
}
trait TransposeOption<T> {
    fn transpose_option(self) -> Option<Option<T>>;
}
impl<T> TransposeOption<T> for Option<Option<T>> {
    fn transpose_option(self) -> Option<Option<T>> {
        match self {
            None => Some(None),
            Some(Some(v)) => Some(Some(v)),
            Some(None) => None,
        }
    }
}
#[cfg(test)]
mod recurrence_tests {
    use super::*;
    fn row(rule: &str) -> Value {
        json!({"id":"fixture","uid":"fixture","start":{"date":"2027-01-31","all_day":true},"end":{"date":"2027-02-01"},"recurrence":rule,"has_recurrence_exceptions":false})
    }
    #[test]
    fn weekly_all_day_instances_preserve_exclusive_duration() {
        let r = expand_all_day(&row("FREQ=WEEKLY;COUNT=3")).unwrap();
        assert_eq!(r.len(), 3);
        assert_eq!(r[1]["start"]["date"], "2027-02-07");
        assert_eq!(r[1]["end"]["date"], "2027-02-08");
        assert_ne!(r[0]["id"], r[1]["id"]);
    }
    #[test]
    fn until_between_weekly_dates_does_not_drop_supported_rule() {
        let r = expand_all_day(&row("FREQ=WEEKLY;UNTIL=20270215")).unwrap();
        assert_eq!(r.len(), 3);
        assert!(expand_all_day(&row("FREQ=WEEKLY;UNTIL=20260101")).is_none());
        assert!(expand_all_day(&row("FREQ=DAILY;COUNT=3;UNTIL=20270215")).is_none());
    }
    #[test]
    fn unsupported_or_unbounded_rules_remain_unexpanded() {
        assert!(expand_all_day(&row("FREQ=MONTHLY;COUNT=3")).is_none());
        assert!(expand_all_day(&row("FREQ=DAILY")).is_none());
        let mut r = row("FREQ=DAILY;COUNT=3");
        r["has_recurrence_exceptions"] = json!(true);
        assert!(expand_all_day(&r).is_none());
        assert!(expand_all_day(&row("FREQ=DAILY;UNTIL=invalid")).is_none());
    }
}
