//! Opt-in official agricultural statistics. Preserve units, dates and suppressed values.
use crate::{AppState, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tauri::State;
use zeroize::Zeroizing;
fn entry(state: &AppState, provider: &str) -> Result<keyring::Entry> {
    if !["usda", "india"].contains(&provider) {
        return Err("Unsupported agriculture provider".into());
    }
    let device = state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .device_id
        .clone();
    keyring::Entry::new(
        &format!("com.personal.commandcenter.agriculture.{provider}"),
        &device,
    )
    .map_err(|e| e.to_string())
}
#[tauri::command]
pub fn configure_agriculture_key(
    state: State<AppState>,
    provider: String,
    key: String,
) -> Result<()> {
    let key = Zeroizing::new(key);
    if key.len() < 16 || key.len() > 1000 || key.chars().any(char::is_whitespace) {
        return Err("Enter your official provider API key".into());
    }
    entry(&state, &provider)?
        .set_password(&key)
        .map_err(|_| "OS secure store refused the API key")?;
    state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .set_integration_flag(&format!("agriculture_enabled:{provider}"), true)
}
#[tauri::command]
pub fn remove_agriculture_key(state: State<AppState>, provider: String) -> Result<()> {
    let key = entry(&state, &provider)?;
    state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .set_integration_flag(&format!("agriculture_enabled:{provider}"), false)?;
    match key.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(_) => Err("Provider disabled; secure credential removal failed".into()),
    }
}
#[tauri::command]
pub fn agriculture_status(state: State<AppState>) -> Result<Value> {
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    Ok(
        json!({"usda_enabled":repo.setting("agriculture_enabled:usda")?.as_deref()==Some("true"),"india_enabled":repo.setting("agriculture_enabled:india")?.as_deref()==Some("true"),"cache":repo.setting("agriculture_cache")?.and_then(|v|serde_json::from_str::<Value>(&v).ok())}),
    )
}
async fn bounded(mut response: reqwest::Response) -> Result<Value> {
    if !response.status().is_success() {
        return Err(format!("Agricultural provider returned HTTP {}. Check credentials, provider quotas and filters.",response.status()));
    }
    let mut bytes = vec![];
    while let Some(c) = response
        .chunk()
        .await
        .map_err(|_| "Provider response interrupted")?
    {
        if bytes.len() + c.len() > 4 * 1024 * 1024 {
            return Err("Provider response exceeds 4 MiB; narrow the query".into());
        }
        bytes.extend_from_slice(&c);
    }
    serde_json::from_slice(&bytes).map_err(|_| "Invalid provider JSON".into())
}
#[tauri::command]
pub async fn fetch_agriculture(
    state: State<'_, AppState>,
    provider: String,
    filters: Value,
    resource_id: String,
    consent: bool,
) -> Result<Value> {
    if !consent {
        return Err("Approve this official statistics request".into());
    }
    let key = Zeroizing::new(
        entry(&state, &provider)?
            .get_password()
            .map_err(|_| "Configure the provider API key first")?,
    );
    let object = filters.as_object().ok_or("Filters must be an object")?;
    if object.len() > 8
        || object.values().any(|v| {
            v.as_str()
                .is_none_or(|s| s.len() > 120 || s.chars().any(char::is_control))
        })
    {
        return Err("Use at most eight bounded text filters".into());
    }
    let epoch = state.data_epoch.load(std::sync::atomic::Ordering::SeqCst);
    {
        let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
        if repo
            .setting(&format!("agriculture_enabled:{provider}"))?
            .as_deref()
            != Some("true")
        {
            return Err("Provider is disabled".into());
        }
        let last = repo
            .setting(&format!("agriculture_attempt:{provider}"))?
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(0);
        if chrono::Utc::now().timestamp() - last < 30 {
            return Err("Wait 30 seconds between provider queries".into());
        }
        repo.set_setting(
            &format!("agriculture_attempt:{provider}"),
            &chrono::Utc::now().timestamp().to_string(),
        )?;
    }
    let (data, source_url) = if provider == "usda" {
        let allowed = [
            "commodity_desc",
            "year",
            "state_alpha",
            "statisticcat_desc",
            "agg_level_desc",
            "freq_desc",
            "domain_desc",
            "unit_desc",
        ];
        if object.keys().any(|k| !allowed.contains(&k.as_str()))
            || object
                .get("commodity_desc")
                .and_then(Value::as_str)
                .is_none_or(|s| s.trim().is_empty())
            || object
                .get("year")
                .and_then(Value::as_str)
                .and_then(|s| s.parse::<u32>().ok())
                .is_none_or(|year| !(1900..=2200).contains(&year))
        {
            return Err(
                "USDA requires commodity_desc and a single year; use the documented filter keys"
                    .into(),
            );
        }
        let mut query = object
            .iter()
            .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_owned()))
            .collect::<Vec<_>>();
        query.push(("key".into(), key.to_string()));
        query.push(("format".into(), "JSON".into()));
        let count_url = reqwest::Url::parse("https://quickstats.nass.usda.gov/api/get_counts/")
            .map_err(|e| e.to_string())?;
        let client = crate::research::public_client(&count_url).await?;
        let count = bounded(
            client
                .get(count_url)
                .query(&query)
                .send()
                .await
                .map_err(|_| "USDA count request failed")?,
        )
        .await?;
        let count = count["count"]
            .as_u64()
            .or_else(|| count["count"].as_str().and_then(|v| v.parse().ok()))
            .ok_or("USDA did not return a count; verify filters and account")?;
        if count > 500 {
            return Err(format!("USDA query matches {count} rows. Narrow filters to 500 or fewer before downloading."));
        }
        let v = bounded(
            client
                .get("https://quickstats.nass.usda.gov/api/api_GET/")
                .query(&query)
                .send()
                .await
                .map_err(|_| "USDA data request failed")?,
        )
        .await?;
        (
            v["data"]
                .as_array()
                .cloned()
                .ok_or("USDA returned no data array")?,
            "https://quickstats.nass.usda.gov/".to_owned(),
        )
    } else {
        uuid::Uuid::parse_str(&resource_id)
            .map_err(|_| "Copy the resource UUID from the official data.gov.in dataset page")?;
        if object.keys().any(|k| {
            ![
                "state",
                "district",
                "market",
                "commodity",
                "variety",
                "arrival_date",
            ]
            .contains(&k.as_str())
        }) {
            return Err("Unsupported India commodity filter".into());
        }
        let url = reqwest::Url::parse(&format!("https://api.data.gov.in/resource/{resource_id}"))
            .map_err(|e| e.to_string())?;
        let client = crate::research::public_client(&url).await?;
        let mut query = vec![
            ("api-key".to_owned(), key.to_string()),
            ("format".into(), "json".into()),
            ("limit".into(), "100".into()),
        ];
        query.extend(
            object
                .iter()
                .map(|(k, v)| (format!("filters[{k}]"), v.as_str().unwrap_or("").to_owned())),
        );
        let v = bounded(
            client
                .get(url)
                .query(&query)
                .send()
                .await
                .map_err(|_| "India open-data request failed")?,
        )
        .await?;
        (v["records"].as_array().cloned().ok_or("Dataset returned no records array; check the selected official resource and schema")?,format!("https://api.data.gov.in/resource/{resource_id}"))
    };
    let keys = if provider == "usda" {
        vec![
            "commodity_desc",
            "short_desc",
            "Value",
            "unit_desc",
            "year",
            "reference_period_desc",
            "week_ending",
            "state_name",
            "state_alpha",
            "county_name",
            "load_time",
            "source_desc",
            "statisticcat_desc",
        ]
    } else {
        vec![
            "state",
            "district",
            "market",
            "commodity",
            "variety",
            "grade",
            "arrival_date",
            "min_price",
            "max_price",
            "modal_price",
            "unit",
        ]
    };
    let rows = data
        .iter()
        .take(500)
        .map(|row| {
            let mut clean = serde_json::Map::new();
            for k in &keys {
                if let Some(v) = row.get(*k) {
                    let value = match v {
                        Value::String(s) => json!(s.chars().take(2000).collect::<String>()),
                        Value::Number(_) => v.clone(),
                        _ => Value::Null,
                    };
                    clean.insert((*k).into(), value);
                }
            }
            let digest = format!(
                "{:x}",
                Sha256::digest(
                    format!("{provider}:{source_url}:{}", Value::Object(clean.clone())).as_bytes()
                )
            );
            json!({"id":digest,"data":clean})
        })
        .collect::<Vec<_>>();
    let result = json!({"provider":provider,"source_url":source_url,"filters":filters,"resource_id":resource_id,"fetched_at":command_center_core::now(),"rows":rows,"notice":"Published observations/estimates, not live trading quotes or a forecast. Preserve provider units and reporting dates. Missing, suppressed or nonnumeric values are not zero. India results are capped at 100 rows; narrow filters for coverage. Verify dataset units on the official resource before comparisons."});
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    if epoch != state.data_epoch.load(std::sync::atomic::Ordering::SeqCst)
        || repo
            .setting(&format!("agriculture_enabled:{provider}"))?
            .as_deref()
            != Some("true")
    {
        return Err("Provider access changed or workspace reset".into());
    }
    repo.set_setting("agriculture_cache", &result.to_string())?;
    Ok(result)
}
#[tauri::command]
pub fn save_agriculture_observation(
    state: State<AppState>,
    observation_id: String,
) -> Result<String> {
    let mut repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    if let Some(existing) = repo
        .records("", false)?
        .iter()
        .find(|r| r.fields.get("agriculture_observation_id") == Some(&observation_id))
    {
        return Ok(existing.id.clone());
    }
    let cache: Value = serde_json::from_str(
        &repo
            .setting("agriculture_cache")?
            .ok_or("Fetch observations first")?,
    )
    .map_err(|_| "Invalid agriculture cache")?;
    let row = cache["rows"]
        .as_array()
        .and_then(|v| {
            v.iter()
                .find(|r| r["id"].as_str() == Some(observation_id.as_str()))
        })
        .ok_or("Observation missing from current cache")?;
    let data = &row["data"];
    let title = data["short_desc"]
        .as_str()
        .or(data["commodity"].as_str())
        .unwrap_or("Agricultural observation")
        .chars()
        .take(450)
        .collect::<String>();
    let mut r = command_center_core::workspace::Record::new("agriculture", &title, &repo.device_id);
    r.status = "observed".into();
    r.summary=json!({"data":data,"fetched_at":cache["fetched_at"],"provider":cache["provider"],"notice":cache["notice"]}).to_string();
    r.fields
        .insert("agriculture_observation_id".into(), observation_id);
    r.fields.insert(
        "source".into(),
        cache["source_url"].as_str().unwrap_or("").into(),
    );
    r.fields
        .insert("category".into(), "Official statistical observation".into());
    let id = r.id.clone();
    repo.save_record(r)?;
    Ok(id)
}
