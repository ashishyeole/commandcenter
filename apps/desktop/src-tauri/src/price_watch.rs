//! Explicit public JSON price/availability observations. No checkout, account, scripts or tokens.
use crate::{AppState, Result};
use serde_json::{json, Value};
use tauri::State;
fn observation(
    value: &Value,
    fields: &std::collections::BTreeMap<String, String>,
) -> Result<Value> {
    let get = |k: &str| fields.get(k).map(String::as_str).unwrap_or("");
    let pointer = get("price_path");
    let availability = get("availability_path");
    if [pointer, availability]
        .iter()
        .any(|s| s.len() > 300 || (!s.is_empty() && !s.starts_with('/')))
    {
        return Err("Use JSON pointers such as /offers/price or /available".into());
    }
    let price = if pointer.is_empty() {
        None
    } else {
        value
            .pointer(pointer)
            .and_then(|v| {
                v.as_f64()
                    .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
            })
            .filter(|n| n.is_finite() && *n >= 0.)
    };
    let available = if availability.is_empty() {
        None
    } else {
        value.pointer(availability).and_then(Value::as_bool)
    };
    if get("metric") == "price" && price.is_none()
        || get("metric") == "availability" && available.is_none()
    {
        return Err("Configured JSON pointer does not contain a valid numeric price or boolean availability".into());
    }
    let currency_path = get("currency_path");
    if get("metric") == "price"
        && (currency_path.is_empty()
            || value.pointer(currency_path).and_then(Value::as_str) != Some(get("currency")))
    {
        return Err("Price source currency must explicitly match the configured currency".into());
    }
    Ok(json!({"price":price,"available":available,"currency":get("currency")}))
}
#[tauri::command]
pub async fn refresh_price_watch(
    state: State<'_, AppState>,
    id: String,
    revision: i64,
    consent: bool,
) -> Result<Value> {
    if !consent {
        return Err("Approve this public product JSON request".into());
    }
    let epoch = state.data_epoch.load(std::sync::atomic::Ordering::SeqCst);
    let record = {
        let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
        repo.records("", false)?
            .into_iter()
            .find(|r| r.id == id && r.module == "watchlists" && r.revision == revision)
            .ok_or("Watchlist changed; save/reload before refreshing")?
    };
    if !["price", "availability"].contains(
        &record
            .fields
            .get("metric")
            .map(String::as_str)
            .unwrap_or(""),
    ) || record.fields.get("monitor").map(String::as_str) != Some("enabled")
    {
        return Err("Save an enabled price/availability watch first".into());
    }
    let source = record
        .fields
        .get("source")
        .ok_or("Add a public JSON source URL")?;
    let url = reqwest::Url::parse(source).map_err(|_| "Invalid source URL")?;
    if url.query().is_some() {
        return Err("Use a public JSON URL without query tokens; account APIs need a separately authorized integration".into());
    }
    let cooldown = format!("price_watch_next:{id}");
    {
        let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
        let now = chrono::Utc::now().timestamp();
        if repo
            .setting(&cooldown)?
            .and_then(|s| s.parse::<i64>().ok())
            .is_some_and(|v| v > now)
        {
            return Err("Wait one minute before refreshing this price source".into());
        }
        repo.set_setting(&cooldown, &(now + 60).to_string())?;
    }
    let response = crate::research::public_client(&url)
        .await?
        .get(url)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|_| "Public product request failed")?;
    let value = crate::connectors::json_response(response).await?;
    let mut item = observation(&value, &record.fields)?;
    item["id"] = json!(format!("price:{id}"));
    item["source"] = json!("price");
    item["url"] = json!(source);
    item["title"] = json!(record.title);
    item["metric"] = json!(format!("{} {}", item["price"], item["currency"]));
    let mut repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    if state.data_epoch.load(std::sync::atomic::Ordering::SeqCst) != epoch
        || !repo
            .records("", false)?
            .iter()
            .any(|r| r.id == id && r.revision == revision)
    {
        return Err("Watchlist or workspace changed during request; result discarded".into());
    }
    repo.cache_research_query("price", source, &json!([item]))?;
    let updated = repo.evaluate_watchlists()?;
    Ok(
        json!({"observation":item,"updated":updated,"notice":"Source-reported observation, not a reserved price or purchase guarantee."}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn currency_and_boolean_types_are_required() {
        let mut fields = std::collections::BTreeMap::from([
            ("metric".into(), "price".into()),
            ("price_path".into(), "/offer/price".into()),
            ("currency".into(), "USD".into()),
            ("currency_path".into(), "/offer/currency".into()),
            ("availability_path".into(), "/available".into()),
        ]);
        assert!(observation(
            &json!({"offer":{"price":"12.50","currency":"USD"},"available":true}),
            &fields
        )
        .is_ok());
        assert!(observation(&json!({"offer":{"price":12,"currency":"INR"}}), &fields).is_err());
        fields.insert("metric".into(), "availability".into());
        assert!(observation(&json!({"available":"false"}), &fields).is_err());
    }
}
