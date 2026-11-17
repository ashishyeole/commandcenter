//! Public venue metadata for sparse tournament listings; no geolocation guesses.
use crate::AppState;
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::{collections::BTreeMap, sync::atomic::Ordering, time::Duration};

pub(crate) fn country(raw: &Value) -> Option<String> {
    let text = raw
        .as_str()
        .or_else(|| raw["identifier"].as_str())
        .or_else(|| raw["name"].as_str())?
        .trim();
    match text.to_ascii_lowercase().as_str() {
        "united states" | "united states of america" | "usa" | "u.s." | "us" => Some("US".into()),
        "india" | "in" => Some("IN".into()),
        "canada" => Some("CA".into()),
        "taiwan" => Some("TW".into()),
        "cambodia" => Some("KH".into()),
        "bahamas" => Some("BS".into()),
        "dominican republic" => Some("DO".into()),
        "czech republic" | "czechia" => Some("CZ".into()),
        "france" => Some("FR".into()),
        "ireland" => Some("IE".into()),
        "austria" => Some("AT".into()),
        "estonia" => Some("EE".into()),
        "netherlands" => Some("NL".into()),
        "belgium" => Some("BE".into()),
        "spain" => Some("ES".into()),
        "brazil" => Some("BR".into()),
        "australia" => Some("AU".into()),
        "south korea" | "korea" => Some("KR".into()),
        "cyprus" => Some("CY".into()),
        "portugal" => Some("PT".into()),
        "greece" => Some("GR".into()),
        "bulgaria" => Some("BG".into()),
        "united kingdom" => Some("GB".into()),
        "italy" => Some("IT".into()),
        _ if text.len() == 2 && text.bytes().all(|b| b.is_ascii_alphabetic()) => {
            Some(text.to_ascii_uppercase())
        }
        _ => None,
    }
}

pub(crate) fn location_country(location: &str) -> Option<String> {
    let tail = location.rsplit(',').next()?.trim();
    if tail.len() == 2 && !tail.eq_ignore_ascii_case("US") {
        return None;
    }
    country(&json!(tail))
}

fn allowed(url: &str) -> bool {
    reqwest::Url::parse(url).is_ok_and(|u| {
        u.scheme() == "https"
            && u.host_str()
                .is_some_and(|h| ["www.cardplayer.com", "cardplayer.com"].contains(&h))
            && u.username().is_empty()
            && u.password().is_none()
            && u.port().is_none()
            && u.query().is_none()
            && u.fragment().is_none()
            && u.path()
                .strip_prefix("/poker-tournaments/")
                .is_some_and(|p| p.starts_with(|c: char| c.is_ascii_digit()))
    })
}
fn venue(html: &str) -> Option<Value> {
    let doc = dom_query::Document::from(html);
    let location = doc
        .select(".series-hero__venue-label")
        .text()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if location.is_empty() || location.len() > 1000 {
        return None;
    }
    let address = location
        .rsplit_once('(')
        .map(|(_, s)| s.trim_end_matches(')'))
        .unwrap_or("");
    let tail = address.rsplit(',').next().unwrap_or("").trim();
    // CardPlayer publishes City, US-state in this dedicated venue label. Do not
    // search page footers, tournament titles or marketing copy for country words.
    let states="AL AK AZ AR CA CO CT DE FL GA HI ID IL IA KS KY LA ME MD MA MI MN MS MO MT NE NV NH NJ NM NY NC ND OH OK OR PA RI SC SD TN TX UT VT VA WA WV WI WY DC";
    let code = if states.split_whitespace().any(|s| s == tail) {
        Some("US".to_owned())
    } else {
        if tail.len() > 2 {
            country(&json!(tail))
        } else {
            None
        }
    };
    Some(
        json!({"location":location,"country_code":code,"country_basis":if code.is_some(){"Publisher venue address"}else{"Country not supplied"}}),
    )
}

async fn detail(url: &str) -> Value {
    let result = async {
        let endpoint = reqwest::Url::parse(url).map_err(|e| e.to_string())?;
        let response = crate::research::public_client(&endpoint)
            .await?
            .get(endpoint)
            .send()
            .await
            .map_err(|_| "Schedule detail request failed".to_owned())?;
        let status = response.status();
        let pause = if [401, 403, 429, 503].contains(&status.as_u16())
            || response.headers().contains_key("retry-after")
        {
            let now = chrono::Utc::now().timestamp();
            Some(
                response
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| {
                        v.parse::<i64>()
                            .ok()
                            .map(|n| now.saturating_add(n.max(0)))
                            .or_else(|| {
                                chrono::DateTime::parse_from_rfc2822(v)
                                    .ok()
                                    .map(|t| t.timestamp())
                            })
                    })
                    .unwrap_or(now + 900)
                    .max(now + 5),
            )
        } else {
            None
        };
        if !status.is_success() {
            return Ok(
                json!({"detail_error":format!("Publisher detail returned HTTP {status}"),"http_status":status.as_u16(),"pause_until":pause}),
            );
        }
        let mut bytes = vec![];
        let mut chunks = response.bytes_stream();
        while let Some(chunk) = chunks.next().await {
            let b = chunk.map_err(|_| "Schedule detail body interrupted".to_owned())?;
            if bytes.len() + b.len() > 2 * 1024 * 1024 {
                return Err("Schedule detail exceeds 2 MiB".to_owned());
            }
            bytes.extend_from_slice(&b);
        }
        let html =
            std::str::from_utf8(&bytes).map_err(|_| "Schedule detail is not UTF-8".to_owned())?;
        let mut value = venue(html)
            .unwrap_or_else(|| json!({"country_basis":"Country not supplied by publisher"}));
        value["http_status"] = json!(status.as_u16());
        Ok::<Value, String>(value)
    };
    match tokio::time::timeout(Duration::from_secs(4), result).await {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => json!({"detail_error":e}),
        Err(_) => json!({"detail_error":"Schedule detail timed out"}),
    }
}

pub(crate) async fn enrich(state: &AppState, mut rows: Vec<Value>) -> Vec<Value> {
    let epoch = state.data_epoch.load(Ordering::SeqCst);
    let now = chrono::Utc::now().timestamp();
    let mut cache: BTreeMap<String, Value> = state
        .repo
        .lock()
        .ok()
        .and_then(|r| r.setting("poker_venue_cache").ok().flatten())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    cache.retain(|_, v| v["expires"].as_i64().unwrap_or(0) > now);
    let paused = state
        .repo
        .lock()
        .ok()
        .and_then(|r| r.setting("poker_venue_pause").ok().flatten())
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0)
        > now;
    let mut pending = vec![];
    for (i, row) in rows.iter_mut().enumerate() {
        let url = row["url"].as_str().unwrap_or("").to_owned();
        if !allowed(&url) {
            continue;
        }
        if let Some(cached) = cache
            .get(&url)
            .filter(|v| v["base_location"] == row["location"])
        {
            apply(row, cached);
        } else if !paused {
            pending.push((i, url, row["location"].clone()));
        }
    }
    let deadline = tokio::time::Instant::now() + Duration::from_secs(12);
    for batch in pending.chunks(3) {
        let reads = futures_util::future::join_all(batch.iter().map(|(i, url, base)| async move {
            (*i, url.clone(), base.clone(), detail(url).await)
        }));
        let Ok(results) = tokio::time::timeout_at(deadline, reads).await else {
            break;
        };
        let mut stop = false;
        for (i, url, base, mut value) in results {
            value["source_url"] = json!(url);
            value["base_location"] = base;
            value["checked_at"] = json!(command_center_core::now());
            value["expires"] = json!(
                now + if value.get("detail_error").is_some() {
                    3600
                } else {
                    7 * 86400
                }
            );
            if let Some(until) = value["pause_until"].as_i64() {
                stop = true;
                if let Ok(repo) = state.repo.lock() {
                    if state.data_epoch.load(Ordering::SeqCst) == epoch {
                        let _ = repo.set_setting("poker_venue_pause", &until.to_string());
                    }
                }
            }
            apply(&mut rows[i], &value);
            cache.insert(url, value);
        }
        if stop {
            break;
        }
    }
    while cache.len() > 200 {
        if let Some(key) = cache
            .iter()
            .min_by_key(|(_, v)| v["expires"].as_i64().unwrap_or(0))
            .map(|(k, _)| k.clone())
        {
            cache.remove(&key);
        }
    }
    if let Ok(repo) = state.repo.lock() {
        if state.data_epoch.load(Ordering::SeqCst) == epoch {
            if let Ok(value) = serde_json::to_string(&cache) {
                let _ = repo.set_setting("poker_venue_cache", &value);
            }
        }
    }
    rows
}
fn apply(row: &mut Value, value: &Value) {
    if value["location"].is_string() {
        row["location"] = value["location"].clone();
    }
    if value["country_code"].is_string() {
        row["country_code"] = value["country_code"].clone();
    }
    row["location_evidence"] = json!({"source_url":value["source_url"],"checked_at":value["checked_at"],"basis":value["country_basis"],"detail_error":value["detail_error"],"http_status":value["http_status"]});
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn venue_country_comes_from_dedicated_address_not_title_or_footer() {
        let us=venue("<h1>India Poker Cup</h1><span class='series-hero__venue-label'>Borgata (Atlantic City, NJ)</span><footer>India</footer>").unwrap();
        assert_eq!(us["country_code"], "US");
        let india =
            venue("<span class='series-hero__venue-label'>Casino (Goa, India)</span>").unwrap();
        assert_eq!(india["country_code"], "IN");
        assert!(venue("<h1>US Poker</h1><footer>New Jersey</footer>").is_none());
        assert!(
            venue("<span class='series-hero__venue-label'>Casino (Unknown city)</span>").unwrap()
                ["country_code"]
                .is_null()
        );
        assert_eq!(country(&json!({"name":"India"})).as_deref(), Some("IN"));
    }
    #[test]
    fn details_are_limited_to_public_series_pages_and_preserve_origin() {
        assert!(allowed(
            "https://www.cardplayer.com/poker-tournaments/123-example"
        ));
        for u in [
            "http://www.cardplayer.com/poker-tournaments/1-x",
            "https://cardplayer.com.evil.test/poker-tournaments/1-x",
            "https://user:secret@cardplayer.com/poker-tournaments/1-x",
            "https://www.cardplayer.com/poker-tournaments?query=x",
            "https://127.0.0.1/poker-tournaments/1-x",
        ] {
            assert!(!allowed(u));
        }
        let mut row = json!({"url":"https://www.cardplayer.com/poker-tournaments/1-x","location":"Atlantic City"});
        let cached = json!({"location":"Borgata (Atlantic City, NJ)","country_code":"US","source_url":row["url"],"checked_at":"now"});
        apply(&mut row, &cached);
        assert_eq!(row["country_code"], "US");
        assert_eq!(row["url"], cached["source_url"]);
    }
    #[tokio::test]
    async fn cached_venue_and_provider_pause_do_not_make_new_reads() {
        let state = crate::discovery_hub::failure_tests::state();
        let url = "https://www.cardplayer.com/poker-tournaments/123-example";
        let now = chrono::Utc::now().timestamp();
        let record = json!({"location":"Borgata (Atlantic City, NJ)","country_code":"US","base_location":"Atlantic City","source_url":url,"checked_at":"2026-11-17T00:00:00Z","expires":now+3600});
        let cache = json!({(url):record});
        state
            .repo
            .lock()
            .unwrap()
            .set_setting("poker_venue_cache", &cache.to_string())
            .unwrap();
        state
            .repo
            .lock()
            .unwrap()
            .set_setting("poker_venue_pause", &(now + 3600).to_string())
            .unwrap();
        let rows=enrich(&state,vec![json!({"url":url,"location":"Atlantic City"}),json!({"url":"https://www.cardplayer.com/poker-tournaments/124-other","location":"Unknown"})]).await;
        assert_eq!(rows[0]["country_code"], "US");
        assert_eq!(rows[1]["location"], "Unknown");
        assert!(rows[1]["country_code"].is_null());
    }
}
