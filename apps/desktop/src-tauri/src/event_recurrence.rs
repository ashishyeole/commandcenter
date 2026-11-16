//! Bounded RFC 5545 subset. Unsupported/incomplete series are left unexpanded.
use chrono::{Datelike, NaiveDate, NaiveDateTime};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};
const LIMIT: usize = 366;
const SCAN_DAYS: u64 = 36_600;
fn local(v: &Value) -> Option<NaiveDateTime> {
    if v["all_day"] == true {
        return NaiveDate::parse_from_str(v["date"].as_str()?, "%Y-%m-%d")
            .ok()?
            .and_hms_opt(0, 0, 0);
    }
    NaiveDateTime::parse_from_str(v["raw"].as_str()?.trim_end_matches('Z'), "%Y%m%dT%H%M%S").ok()
}
fn make(template: &Value, local: NaiveDateTime) -> Option<Value> {
    let all_day = template["all_day"] == true;
    let zone = template["timezone"].as_str().unwrap_or("");
    let utc = template["raw"].as_str().is_some_and(|r| r.ends_with('Z'));
    let raw = if all_day {
        local.format("%Y%m%d").to_string()
    } else {
        format!(
            "{}{}",
            local.format("%Y%m%dT%H%M%S"),
            if utc { "Z" } else { "" }
        )
    };
    let params = if !all_day && !utc && template["at"].is_string() {
        Some(vec![("TZID".into(), vec![zone.to_owned()])])
    } else {
        None
    };
    super::event_sources::date(&ical::property::Property {
        name: "DTSTART".into(),
        params,
        value: Some(raw),
    })
    .ok()
}
fn instant(v: &Value) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(v["at"].as_str()?)
        .ok()
        .map(|d| d.with_timezone(&chrono::Utc))
}
fn key(v: &Value) -> Option<String> {
    if let Some(at) = instant(v) {
        Some(format!("T:{at}"))
    } else {
        Some(format!(
            "{}:{}",
            if v["all_day"] == true { "D" } else { "F" },
            local(v)?
        ))
    }
}
fn compatible(a: &Value, b: &Value) -> bool {
    (a["all_day"] == true) == (b["all_day"] == true) && a["at"].is_string() == b["at"].is_string()
}
fn end_for(start: &Value, old_start: &Value, old_end: &Value) -> Option<Value> {
    if old_end.is_null() {
        return Some(Value::Null);
    }
    if !compatible(old_start, old_end) {
        return None;
    }
    if let Some(original) = instant(old_start) {
        let duration = instant(old_end)? - original;
        if duration < chrono::Duration::zero() {
            return None;
        }
        let at = instant(start)?.checked_add_signed(duration)?;
        let end_local = if old_end["raw"].as_str()?.ends_with('Z') {
            at.naive_utc()
        } else {
            let zone: chrono_tz::Tz = old_end["timezone"].as_str()?.parse().ok()?;
            at.with_timezone(&zone).naive_local()
        };
        make(old_end, end_local)
    } else {
        let duration = local(old_end)? - local(old_start)?;
        if duration < chrono::Duration::zero() {
            return None;
        }
        make(old_end, local(start)?.checked_add_signed(duration)?)
    }
}
fn weekday(s: &str) -> Option<u32> {
    Some(match s {
        "MO" => 0,
        "TU" => 1,
        "WE" => 2,
        "TH" => 3,
        "FR" => 4,
        "SA" => 5,
        "SU" => 6,
        _ => return None,
    })
}
fn month_days(date: NaiveDate) -> Option<i32> {
    let next = if date.month() == 12 {
        NaiveDate::from_ymd_opt(date.year() + 1, 1, 1)?
    } else {
        NaiveDate::from_ymd_opt(date.year(), date.month() + 1, 1)?
    };
    Some(next.pred_opt()?.day() as i32)
}
struct Rule<'a> {
    freq: &'a str,
    interval: i32,
    count: Option<usize>,
    until: Option<Value>,
    byday: Vec<(i32, u32)>,
    bymonthday: Vec<i32>,
    bymonth: Vec<u32>,
    wkst: u32,
}
impl<'a> Rule<'a> {
    fn parse(text: &'a str, start: &Value) -> Option<Self> {
        let mut p = BTreeMap::new();
        for part in text.split(';') {
            let (k, v) = part.split_once('=')?;
            if ![
                "FREQ",
                "INTERVAL",
                "COUNT",
                "UNTIL",
                "BYDAY",
                "BYMONTHDAY",
                "BYMONTH",
                "WKST",
            ]
            .contains(&k)
                || v.is_empty()
                || p.insert(k, v).is_some()
            {
                return None;
            }
        }
        let freq = *p.get("FREQ")?;
        if !["DAILY", "WEEKLY", "MONTHLY", "YEARLY"].contains(&freq) {
            return None;
        }
        let interval = p
            .get("INTERVAL")
            .map(|s| s.parse::<i32>().ok())
            .unwrap_or(Some(1))?;
        if !(1..=366).contains(&interval) {
            return None;
        }
        let count = if let Some(n) = p.get("COUNT") {
            let n = n.parse::<usize>().ok()?;
            if n == 0 || n > LIMIT {
                return None;
            }
            Some(n)
        } else {
            None
        };
        let until = if let Some(raw) = p.get("UNTIL") {
            // Zoned DTSTART requires UTC UNTIL; all-day and floating types must match.
            let all_day = start["all_day"] == true;
            if all_day && raw.len() != 8
                || !all_day && start["at"].is_string() != raw.ends_with('Z')
            {
                return None;
            }
            let v = super::event_sources::date(&ical::property::Property {
                name: "UNTIL".into(),
                params: None,
                value: Some((*raw).into()),
            })
            .ok()?;
            if !compatible(start, &v) {
                return None;
            }
            Some(v)
        } else {
            None
        };
        if count.is_some() == until.is_some() {
            return None;
        }
        let mut byday = vec![];
        if let Some(days) = p.get("BYDAY") {
            for day in days.split(',') {
                if day.len() < 2 {
                    return None;
                }
                let at = day.len() - 2;
                if !day.is_char_boundary(at) {
                    return None;
                }
                let dow = weekday(&day[at..])?;
                let n = if at == 0 {
                    0
                } else {
                    let n = day[..at].parse::<i32>().ok()?;
                    if n == 0 || !(-5..=5).contains(&n) {
                        return None;
                    }
                    n
                };
                if n != 0 && !["MONTHLY", "YEARLY"].contains(&freq) {
                    return None;
                }
                byday.push((n, dow));
            }
            if byday.len() > 35 {
                return None;
            }
        }
        let mut bymonthday = vec![];
        if let Some(days) = p.get("BYMONTHDAY") {
            if freq == "WEEKLY" {
                return None;
            }
            for d in days.split(',') {
                let n = d.parse::<i32>().ok()?;
                if n == 0 || !(-31..=31).contains(&n) {
                    return None;
                }
                bymonthday.push(n);
            }
            if bymonthday.len() > 62 {
                return None;
            }
        }
        let mut bymonth = vec![];
        if let Some(months) = p.get("BYMONTH") {
            for m in months.split(',') {
                let n = m.parse::<u32>().ok()?;
                if !(1..=12).contains(&n) {
                    return None;
                }
                bymonth.push(n);
            }
            if bymonth.len() > 12 {
                return None;
            }
        }
        // Year-relative ordinal BYDAY is deliberately unsupported; ordinals below are month-relative.
        if freq == "YEARLY" && bymonth.is_empty() && (!byday.is_empty() || !bymonthday.is_empty()) {
            return None;
        }
        let wkst = p.get("WKST").map(|s| weekday(s)).unwrap_or(Some(0))?;
        Some(Self {
            freq,
            interval,
            count,
            until,
            byday,
            bymonthday,
            bymonth,
            wkst,
        })
    }
    fn matches(&self, date: NaiveDate, start: NaiveDate) -> Option<bool> {
        let days = (date - start).num_days();
        let months = (date.year() - start.year()) * 12 + date.month() as i32 - start.month() as i32;
        let weekday = date.weekday().num_days_from_monday();
        let interval = match self.freq {
            "DAILY" => days % self.interval as i64 == 0,
            "WEEKLY" => {
                let offset =
                    |d: NaiveDate| (d.weekday().num_days_from_monday() + 7 - self.wkst) % 7;
                let week = date.checked_sub_days(chrono::Days::new(offset(date) as u64))?;
                let first = start.checked_sub_days(chrono::Days::new(offset(start) as u64))?;
                (week - first).num_days() / 7 % self.interval as i64 == 0
            }
            "MONTHLY" => months % self.interval == 0,
            "YEARLY" => (date.year() - start.year()) % self.interval == 0,
            _ => return None,
        };
        if !interval || !self.bymonth.is_empty() && !self.bymonth.contains(&date.month()) {
            return Some(false);
        }
        let last = month_days(date)?;
        if !self.bymonthday.is_empty()
            && !self.bymonthday.iter().any(|n| {
                if *n > 0 {
                    date.day() as i32 == *n
                } else {
                    date.day() as i32 == last + 1 + n
                }
            })
        {
            return Some(false);
        }
        if !self.byday.is_empty()
            && !self.byday.iter().any(|(n, dow)| {
                *dow == weekday
                    && (*n == 0
                        || if *n > 0 {
                            (date.day() as i32 - 1) / 7 + 1 == *n
                        } else {
                            -((last - date.day() as i32) / 7 + 1) == *n
                        })
            })
        {
            return Some(false);
        }
        if self.freq == "WEEKLY" && self.byday.is_empty() && date.weekday() != start.weekday() {
            return Some(false);
        }
        if ["MONTHLY", "YEARLY"].contains(&self.freq)
            && self.byday.is_empty()
            && self.bymonthday.is_empty()
            && date.day() != start.day()
        {
            return Some(false);
        }
        if self.freq == "YEARLY" && self.bymonth.is_empty() && date.month() != start.month() {
            return Some(false);
        }
        Some(true)
    }
    fn after_until(&self, value: &Value) -> Option<bool> {
        let Some(until) = &self.until else {
            return Some(false);
        };
        Some(if let Some(at) = instant(value) {
            at > instant(until)?
        } else {
            local(value)? > local(until)?
        })
    }
}
pub(crate) fn expand(row: &Value) -> Option<Vec<Value>> {
    if row["status"] == "CANCELLED" || row["recurrence_exceptions_invalid"] == true {
        return None;
    }
    let start = &row["start"];
    let base = local(start)?;
    let rule = Rule::parse(row["recurrence"].as_str()?, start)?;
    // RFC defines unsynchronized DTSTART/RRULE behavior as undefined; do not invent dates.
    if !rule.matches(base.date(), base.date())? || rule.after_until(start)? {
        return None;
    }
    if row["has_recurrence_exceptions"] == true && row.get("recurrence_exdates").is_none() {
        return None;
    }
    let mut excluded = HashSet::new();
    if let Some(exdates) = row["recurrence_exdates"].as_array() {
        for v in exdates {
            if !compatible(start, v) {
                return None;
            }
            excluded.insert(key(v)?);
        }
    }
    let mut starts = vec![];
    let mut seen = HashSet::new();
    let mut count = 0usize;
    let mut complete = false;
    for offset in 0..=SCAN_DAYS {
        let day = base.date().checked_add_days(chrono::Days::new(offset))?;
        // A one-day cushion handles UTC UNTIL compared with IANA-local dates.
        if let Some(until) = &rule.until {
            if day
                > local(until)?
                    .date()
                    .checked_add_days(chrono::Days::new(1))?
            {
                complete = true;
                break;
            }
        }
        if !rule.matches(day, base.date())? {
            continue;
        }
        let Some(value) = make(start, day.and_time(base.time())) else {
            continue;
        };
        if rule.after_until(&value)? {
            complete = true;
            break;
        }
        count += 1;
        if count > LIMIT {
            return None;
        }
        let id = key(&value)?;
        if !excluded.contains(&id) && seen.insert(id) {
            starts.push(value);
        }
        if rule.count == Some(count) {
            complete = true;
            break;
        }
    }
    if !complete {
        return None;
    }
    if let Some(rdates) = row["recurrence_rdates"].as_array() {
        for v in rdates {
            if !compatible(start, v) {
                return None;
            }
            let id = key(v)?;
            if !excluded.contains(&id) && seen.insert(id) {
                starts.push(v.clone());
            }
        }
    }
    if starts.len() > LIMIT {
        return None;
    }
    starts.sort_by_key(key);
    let mut out = vec![];
    for start in starts {
        let mut v = row.clone();
        let id = if start["all_day"] == true {
            start["date"].as_str()?.to_owned()
        } else {
            key(&start)?
        };
        v["id"] = json!(format!("{}:{id}", row["id"].as_str()?));
        v["end"] = end_for(&start, &row["start"], &row["end"])?;
        v["start"] = start;
        v["recurrence_instance"] = json!(id);
        v["recurrence_expanded"] = json!(true);
        out.push(v);
    }
    Some(out)
}
