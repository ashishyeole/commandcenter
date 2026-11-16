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
pub(crate) fn date(p: &ical::property::Property) -> Result<Value> {
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
pub(crate) fn parse(bytes: Vec<u8>, source: &Source) -> Result<Value> {
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
            let title = {
                let v = get("SUMMARY");
                if v.is_empty()
                    && get("STATUS") == "CANCELLED"
                    && event
                        .properties
                        .iter()
                        .any(|p| p.name.eq_ignore_ascii_case("RECURRENCE-ID"))
                {
                    "Cancelled occurrence".into()
                } else {
                    v
                }
            };
            if uid.is_empty() || uid.len() > 1024 || title.is_empty() {
                skipped.push("Event missing UID or title".to_string());
                continue;
            }
            let recurrence_id = event
                .properties
                .iter()
                .find(|p| p.name.eq_ignore_ascii_case("RECURRENCE-ID"))
                .and_then(|p| date(p).ok());
            let range_override = event.properties.iter().any(|p| {
                p.name.eq_ignore_ascii_case("RECURRENCE-ID")
                    && p.params.as_ref().is_some_and(|pairs| {
                        pairs.iter().any(|(k, _)| k.eq_ignore_ascii_case("RANGE"))
                    })
            });
            let start = match event
                .properties
                .iter()
                .find(|p| p.name.eq_ignore_ascii_case("DTSTART"))
                .map(date)
                .transpose()
            {
                Ok(Some(v)) => v,
                Ok(None) if get("STATUS") == "CANCELLED" && recurrence_id.is_some() => {
                    recurrence_id.clone().unwrap()
                }
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
            let mut end = event
                .properties
                .iter()
                .find(|p| p.name.eq_ignore_ascii_case("DTEND"))
                .and_then(|p| date(p).ok());
            let duration = get("DURATION");
            let duration_invalid = !duration.is_empty()
                && (end.is_some()
                    || crate::event_recurrence::duration_end(&start, &duration).is_none());
            if end.is_none() && !duration.is_empty() {
                end = crate::event_recurrence::duration_end(&start, &duration);
            }
            let exrules = event
                .properties
                .iter()
                .filter(|p| p.name.eq_ignore_ascii_case("EXRULE"))
                .filter_map(|p| p.value.clone())
                .take(9)
                .collect::<Vec<_>>();
            let mut exdates = vec![];
            let mut rdates = vec![];
            let mut exceptions_invalid = event
                .properties
                .iter()
                .filter(|p| p.name.eq_ignore_ascii_case("RRULE"))
                .count()
                > 1
                || duration_invalid
                || exrules.len() > 8;
            exceptions_invalid |= end.is_none()
                && event
                    .properties
                    .iter()
                    .any(|p| p.name.eq_ignore_ascii_case("DTEND"));
            for property in &event.properties {
                if !["EXDATE", "RDATE"]
                    .iter()
                    .any(|name| property.name.eq_ignore_ascii_case(name))
                {
                    continue;
                }
                for value in property.value.as_deref().unwrap_or("").split(',') {
                    let mut part = property.clone();
                    part.value = Some(value.into());
                    match date(&part) {
                        Ok(v) => {
                            if property.name.eq_ignore_ascii_case("EXDATE") {
                                exdates.push(v)
                            } else {
                                rdates.push(v)
                            }
                        }
                        Err(_) => exceptions_invalid = true,
                    }
                }
            }
            rows.push(json!({"id":identity,"uid":uid,"title":title.chars().take(500).collect::<String>(),"description":get("DESCRIPTION"),"location":get("LOCATION"),"registration":get("X-REGISTRATION-STATUS"),"registration_url":get("X-REGISTRATION-URL"),"categories":get("CATEGORIES"),"related_event":get("RELATED-TO"),"start":start,"end":end,"status":get("STATUS"),"recurrence":recurrence,"recurrence_instance":instance,"recurrence_id":recurrence_id,"range_override":range_override,"duration":duration,"recurrence_exrules":exrules,"recurrence_expanded":false,"recurrence_exdates":exdates,"recurrence_rdates":rdates,"recurrence_exceptions_invalid":exceptions_invalid,"has_recurrence_exceptions":event.properties.iter().any(|p|["EXDATE","RDATE"].contains(&p.name.as_str())),"sequence":get("SEQUENCE"),"source_updated":get("LAST-MODIFIED"),"source_url":url,"calendar_url":source.url,"verification":if source.verified_authority{"Source authority explicitly confirmed by user; schedule is publisher-supplied and can change"}else{"Unconfirmed source authority; independently verify before travel or registration"}}));
        }
    }
    let mut expanded = Vec::new();
    let overrides = rows
        .iter()
        .filter(|r| r["recurrence_id"].is_object())
        .cloned()
        .collect::<Vec<_>>();
    for row in &rows {
        if row["recurrence_id"].is_object() {
            continue;
        }
        let series = overrides
            .iter()
            .filter(|v| v["uid"] == row["uid"])
            .collect::<Vec<_>>();
        let supported = !series.iter().any(|v| v["range_override"] == true);
        if supported {
            if let Some(instances) = crate::event_recurrence::expand(row) {
                for mut instance in instances {
                    let key = crate::event_recurrence::occurrence_key(&instance["start"]);
                    if let Some(replacement) = series.iter().find(|r| {
                        crate::event_recurrence::occurrence_key(&r["recurrence_id"]) == key
                    }) {
                        let id = instance["id"].clone();
                        instance = (*replacement).clone();
                        instance["id"] = id;
                        instance["recurrence_expanded"] = json!(true);
                    }
                    expanded.push(instance);
                }
                continue;
            }
        }
        expanded.push(row.clone());
        expanded.extend(series.into_iter().cloned());
    }
    for row in overrides {
        if !rows
            .iter()
            .any(|r| r["uid"] == row["uid"] && r["recurrence_id"].is_null())
        {
            expanded.push(row);
        }
    }
    if expanded.len() > 1000 {
        return Err("Expanded calendar exceeds 1000 occurrences".into());
    }
    let mut rows = expanded;
    rows.sort_by(|a, b| {
        a["start"]["date"]
            .as_str()
            .cmp(&b["start"]["date"].as_str())
    });
    Ok(
        json!({"source":source,"fetched_at":command_center_core::now(),"events":rows,"skipped":skipped.into_iter().take(30).collect::<Vec<_>>(),"notice":"Bounded DAILY/WEEKLY/MONTHLY/YEARLY all-day, UTC, IANA-zone or floating recurrences support INTERVAL, COUNT/UNTIL, selected BYDAY/BYMONTHDAY/BYMONTH/BYSETPOS/WKST filters, DURATION, EXDATE/RDATE/EXRULE and individual RECURRENCE-ID overrides, up to 366 instances within 100 years. Invalid or ambiguous local times are omitted. Unbounded rules show a rolling 366-day window. Unsupported rules and RANGE overrides remain unexpanded; verify them at the publisher. All-day end dates are exclusive. Floating times require source verification. Monetary guarantees are never inferred from prose."}),
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
            if event_changed(a, b) {
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
// Alert on publisher-supplied details, including registration text and satellite relationships.
// Empty or removed values are changes, never inferred availability.
fn event_changed(a: &Value, b: &Value) -> bool {
    [
        "title",
        "start",
        "end",
        "status",
        "location",
        "description",
        "source_url",
        "registration",
        "registration_url",
        "categories",
        "related_event",
    ]
    .iter()
    .any(|k| a[*k] != b[*k])
}
#[cfg(test)]
mod detail_tests {
    use super::*;
    #[test]
    fn registration_and_satellite_changes_are_reviewed() {
        let a = json!({"start":{"date":"2026-12-01"},"description":"Registration opens soon","related_event":"main-1"});
        let mut b = a.clone();
        b["description"] = json!("Registration open. Satellite seats available.");
        assert!(event_changed(&a, &b));
        b = a.clone();
        b["related_event"] = json!("main-2");
        assert!(event_changed(&a, &b));
        assert!(!event_changed(&a, &a));
    }
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
        ("end", event["end"]["date"].as_str().unwrap_or("")),
        (
            "end_exclusive",
            if event["end"]["all_day"] == true {
                "true"
            } else {
                "false"
            },
        ),
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

#[cfg(test)]
mod recurrence_tests {
    use super::*;
    fn row(rule: &str) -> Value {
        json!({"id":"fixture","uid":"fixture","start":{"date":"2027-01-31","all_day":true},"end":{"date":"2027-02-01","all_day":true},"recurrence":rule,"has_recurrence_exceptions":false})
    }
    #[test]
    fn weekly_all_day_instances_preserve_exclusive_duration() {
        let r = crate::event_recurrence::expand(&row("FREQ=WEEKLY;COUNT=3")).unwrap();
        assert_eq!(r.len(), 3);
        assert_eq!(r[1]["start"]["date"], "2027-02-07");
        assert_eq!(r[1]["end"]["date"], "2027-02-08");
        assert_ne!(r[0]["id"], r[1]["id"]);
    }
    #[test]
    fn until_between_weekly_dates_does_not_drop_supported_rule() {
        let r = crate::event_recurrence::expand(&row("FREQ=WEEKLY;UNTIL=20270215")).unwrap();
        assert_eq!(r.len(), 3);
        assert!(crate::event_recurrence::expand(&row("FREQ=WEEKLY;UNTIL=20260101")).is_none());
        assert!(
            crate::event_recurrence::expand(&row("FREQ=DAILY;COUNT=3;UNTIL=20270215")).is_none()
        );
    }
    #[test]
    fn unsupported_rules_remain_unexpanded_and_unbounded_rules_are_windowed() {
        assert!(crate::event_recurrence::expand(&row("FREQ=HOURLY;COUNT=3")).is_none());
        let instances = crate::event_recurrence::expand(&row("FREQ=DAILY")).unwrap();
        assert!(instances.len() <= 366);
        assert!(instances
            .iter()
            .all(|v| v["recurrence_window_end"].is_string()));
        let mut r = row("FREQ=DAILY;COUNT=3");
        r["has_recurrence_exceptions"] = json!(true);
        assert!(crate::event_recurrence::expand(&r).is_none());
        assert!(crate::event_recurrence::expand(&row("FREQ=DAILY;UNTIL=invalid")).is_none());
    }
}

#[cfg(test)]
mod expanded_calendar_tests {
    use super::*;
    fn calendar(properties: &str) -> Value {
        let source = Source {
            id: "synthetic-source".into(),
            name: "Fixture".into(),
            url: "https://example.com/fixtures.ics".into(),
            category: "poker".into(),
            verified_authority: false,
            enabled: false,
            interval_minutes: 60,
        };
        parse(format!("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:fixture\r\nSUMMARY:Fixture event\r\n{properties}\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n").into_bytes(),&source).unwrap()
    }
    fn dates(c: &Value) -> Vec<&str> {
        c["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["start"]["date"].as_str().unwrap())
            .collect()
    }
    #[test]
    fn monthly_invalid_month_days_are_skipped_instead_of_clamped() {
        let c=calendar("DTSTART;VALUE=DATE:20270131\r\nDTEND;VALUE=DATE:20270201\r\nRRULE:FREQ=MONTHLY;COUNT=3");
        assert_eq!(dates(&c), vec!["2027-01-31", "2027-03-31", "2027-05-31"]);
        assert_eq!(c["events"][1]["end"]["date"], "2027-04-01");
        // Preserve prior all-day instance identity so existing watches still match.
        assert!(c["events"][0]["id"]
            .as_str()
            .unwrap()
            .ends_with(":2027-01-31"));
    }
    #[test]
    fn ordinal_weekdays_negative_month_days_and_leap_years() {
        let c = calendar("DTSTART;VALUE=DATE:20270129\r\nRRULE:FREQ=MONTHLY;BYDAY=-1FR;COUNT=3");
        assert_eq!(dates(&c), vec!["2027-01-29", "2027-02-26", "2027-03-26"]);
        let c = calendar("DTSTART;VALUE=DATE:20270131\r\nRRULE:FREQ=MONTHLY;BYMONTHDAY=-1;COUNT=3");
        assert_eq!(dates(&c), vec!["2027-01-31", "2027-02-28", "2027-03-31"]);
        let c = calendar("DTSTART;VALUE=DATE:20240229\r\nRRULE:FREQ=YEARLY;COUNT=3");
        assert_eq!(dates(&c), vec!["2024-02-29", "2028-02-29", "2032-02-29"]);
    }
    #[test]
    fn weekly_days_respect_interval_and_week_start() {
        let c=calendar("DTSTART;VALUE=DATE:20270301\r\nRRULE:FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,WE;WKST=SU;COUNT=4");
        assert_eq!(
            dates(&c),
            vec!["2027-03-01", "2027-03-03", "2027-03-15", "2027-03-17"]
        );
    }
    #[test]
    fn timezone_dst_preserves_wall_clock_and_duration_with_exceptions() {
        let c=calendar("DTSTART;TZID=America/New_York:20270313T090000\r\nDTEND;TZID=America/New_York:20270313T103000\r\nRRULE:FREQ=DAILY;COUNT=3\r\nEXDATE;TZID=America/New_York:20270314T090000\r\nRDATE;TZID=America/New_York:20270320T090000,20270320T090000");
        let e = c["events"].as_array().unwrap();
        assert_eq!(e.len(), 3);
        assert_eq!(e[0]["start"]["at"], "2027-03-13T09:00:00-05:00");
        assert_eq!(e[1]["start"]["at"], "2027-03-15T09:00:00-04:00");
        assert_eq!(e[1]["end"]["at"], "2027-03-15T10:30:00-04:00");
        assert_eq!(e[2]["start"]["at"], "2027-03-20T09:00:00-04:00");
    }
    #[test]
    fn nonexistent_local_time_does_not_consume_count() {
        let c =
            calendar("DTSTART;TZID=America/New_York:20270313T023000\r\nRRULE:FREQ=DAILY;COUNT=3");
        let e = c["events"].as_array().unwrap();
        assert_eq!(e.len(), 3);
        assert_eq!(e[1]["start"]["raw"], "20270315T023000");
        assert_eq!(e[2]["start"]["raw"], "20270316T023000");
    }
    #[test]
    fn utc_until_is_inclusive_and_unsupported_rules_stay_explicit() {
        let c=calendar("DTSTART:20270101T230000Z\r\nDTEND:20270102T010000Z\r\nRRULE:FREQ=DAILY;UNTIL=20270103T230000Z");
        let e = c["events"].as_array().unwrap();
        assert_eq!(e.len(), 3);
        assert_eq!(e[2]["end"]["at"], "2027-01-04T01:00:00+00:00");
        for rule in [
            "FREQ=MONTHLY;BYDAY=MO,TU;BYSETPOS=-1;COUNT=3",
            "FREQ=DAILY;UNTIL=20270103",
            "FREQ=YEARLY;INTERVAL=366;COUNT=2",
        ] {
            let c = calendar(&format!("DTSTART:20270101T230000Z\r\nRRULE:{rule}"));
            assert_eq!(c["events"].as_array().unwrap().len(), 1);
            assert_eq!(c["events"][0]["recurrence_expanded"], false);
        }
    }
    #[test]
    fn exclusions_win_and_unknown_exception_syntax_prevents_expansion() {
        let c=calendar("DTSTART;VALUE=DATE:20270101\r\nRRULE:FREQ=DAILY;COUNT=2\r\nEXDATE;VALUE=DATE:20270101\r\nRDATE;VALUE=DATE:20270101,20270103");
        assert_eq!(dates(&c), vec!["2027-01-02", "2027-01-03"]);
        for extra in [
            "EXDATE:invalid",
            "RDATE;VALUE=PERIOD:20270101T120000Z/PT1H",
            "DURATION:PT2H",
            "DTEND:invalid",
            "RRULE:FREQ=WEEKLY;COUNT=2",
        ] {
            let c = calendar(&format!(
                "DTSTART;VALUE=DATE:20270101\r\nRRULE:FREQ=DAILY;COUNT=2\r\n{extra}"
            ));
            assert_eq!(c["events"][0]["recurrence_expanded"], false);
        }
    }
    #[test]
    fn positional_weekdays_duration_and_exrules_expand() {
        let c=calendar("DTSTART;VALUE=DATE:20270129\r\nRRULE:FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1;COUNT=3\r\nDURATION:P2D");
        assert_eq!(dates(&c), vec!["2027-01-29", "2027-02-26", "2027-03-31"]);
        assert_eq!(c["events"][1]["end"]["date"], "2027-02-28");
        let c=calendar("DTSTART;VALUE=DATE:20270101\r\nRRULE:FREQ=DAILY;COUNT=5\r\nEXRULE:FREQ=DAILY;INTERVAL=2;COUNT=3");
        assert_eq!(dates(&c), vec!["2027-01-02", "2027-01-04"]);
    }
    #[test]
    fn unbounded_series_has_explicit_finite_window() {
        let today = chrono::Local::now().date_naive();
        let c = calendar(&format!(
            "DTSTART;VALUE=DATE:{}\r\nRRULE:FREQ=DAILY",
            today.format("%Y%m%d")
        ));
        assert_eq!(c["events"].as_array().unwrap().len(), 366);
        assert!(c["events"][0]["recurrence_window_end"].is_string());
    }
    #[test]
    fn individual_override_replaces_original_occurrence() {
        let source = Source {
            id: uuid::Uuid::new_v4().to_string(),
            name: "QA".into(),
            url: "https://example.com/calendar.ics".into(),
            category: "poker".into(),
            verified_authority: false,
            enabled: false,
            interval_minutes: 60,
        };
        let text="BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:series\r\nSUMMARY:Base\r\nDTSTART;VALUE=DATE:20270101\r\nRRULE:FREQ=DAILY;COUNT=3\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:series\r\nSUMMARY:Moved\r\nRECURRENCE-ID;VALUE=DATE:20270102\r\nDTSTART;VALUE=DATE:20270105\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let c = parse(text.as_bytes().to_vec(), &source).unwrap();
        assert_eq!(dates(&c), vec!["2027-01-01", "2027-01-03", "2027-01-05"]);
        assert_eq!(c["events"][2]["title"], "Moved");
    }
}
