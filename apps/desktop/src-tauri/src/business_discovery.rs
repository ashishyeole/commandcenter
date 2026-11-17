use crate::{AppState, Result};
use command_center_core::workspace::Record;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tauri::State;
fn safe_url(raw: &str) -> Option<String> {
    reqwest::Url::parse(raw)
        .ok()
        .filter(|u| {
            ["https", "http"].contains(&u.scheme())
                && u.username().is_empty()
                && u.password().is_none()
                && u.host_str().is_some()
        })
        .map(|u| u.to_string())
}
#[tauri::command]
pub fn cached_business_search(state: State<AppState>) -> Result<Option<Value>> {
    state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .setting("business_search_last")?
        .map(|raw| serde_json::from_str(&raw).map_err(|_| "Invalid business cache".into()))
        .transpose()
}
#[tauri::command]
pub async fn search_public_businesses(
    state: State<'_, AppState>,
    latitude: f64,
    longitude: f64,
    radius: u32,
    category: String,
    endpoint: String,
    consent: bool,
) -> Result<Value> {
    if !consent
        || !latitude.is_finite()
        || !longitude.is_finite()
        || !(-85.0..=85.0).contains(&latitude)
        || !(-180.0..=180.0).contains(&longitude)
        || !(100..=3000).contains(&radius)
        || endpoint.len() > 2000
    {
        return Err(
            "Approve a public coordinate search with a radius from 100 to 3,000 metres".into(),
        );
    }
    let filter = match category.as_str() {
        "restaurants" => "[amenity=restaurant]",
        "cafes" => "[amenity=cafe]",
        "shops" => "[shop]",
        "offices" => "[office]",
        "salons" => "[shop=hairdresser]",
        _ => return Err("Choose a supported business category".into()),
    };
    let url = reqwest::Url::parse(&endpoint).map_err(|_| "Enter an HTTPS Overpass endpoint")?;
    if url.query().is_some() {
        return Err("Endpoint query parameters are not allowed".into());
    }
    let query=format!("[out:json][timeout:15][maxsize:33554432];nwr{filter}[name](around:{radius},{latitude:.6},{longitude:.6});out tags center 50;");
    let query_hash = format!("{:x}", Sha256::digest(format!("{endpoint}\n{query}")));
    let epoch = state.data_epoch.load(std::sync::atomic::Ordering::SeqCst);
    {
        let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
        if let Some(cached) = repo
            .setting("business_search_last")?
            .and_then(|r| serde_json::from_str::<Value>(&r).ok())
        {
            if cached["query_hash"] == query_hash
                && cached["fetched_at"]
                    .as_str()
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                    .is_some_and(|t| t > chrono::Utc::now() - chrono::Duration::hours(24))
            {
                return Ok(cached);
            }
        }
        if repo
            .setting("business_search_next")?
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
            .is_some_and(|t| t > chrono::Utc::now())
        {
            return Err("Public business searches are limited to one per minute; cached identical results are reused for 24 hours".into());
        }
        repo.set_setting(
            "business_search_next",
            &(chrono::Utc::now() + chrono::Duration::minutes(1)).to_rfc3339(),
        )?;
    }
    let response = crate::research::public_client(&url)
        .await?
        .post(url)
        .header(
            "User-Agent",
            "PersonalCommandCenter/0.5 (single-user, manually approved OSM business lookup)",
        )
        .form(&[("data", query)])
        .send()
        .await
        .map_err(|e| format!("Public business source request failed: {}. Retry later or choose another approved Overpass endpoint; existing saved leads remain available.", e.without_url()))?;
    let value = crate::connectors::json_response(response).await?;
    if value.get("remark").is_some() {
        return Err(
            "Provider returned a partial result or query warning. Try a smaller radius later."
                .into(),
        );
    }
    let mut rows = vec![];
    for r in value["elements"]
        .as_array()
        .ok_or("Unexpected Overpass result")?
        .iter()
        .take(50)
    {
        let kind = r["type"].as_str().unwrap_or("");
        if !["node", "way", "relation"].contains(&kind) {
            continue;
        }
        let Some(id) = r["id"].as_u64() else { continue };
        let tags = &r["tags"];
        let name = tags["name"]
            .as_str()
            .unwrap_or("")
            .chars()
            .take(100)
            .collect::<String>();
        if name.is_empty() {
            continue;
        }
        let website = tags["website"]
            .as_str()
            .or(tags["contact:website"].as_str())
            .and_then(safe_url);
        let address = [
            "addr:housenumber",
            "addr:street",
            "addr:city",
            "addr:postcode",
        ]
        .iter()
        .filter_map(|k| tags[k].as_str())
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(200)
        .collect::<String>();
        rows.push(json!({"id":format!("{kind}/{id}"),"name":name,"website":website,"address":address,"category":category,"source_url":format!("https://www.openstreetmap.org/{kind}/{id}"),"verification":"Community map listing; verify operation, ownership and digital presence. Missing website tag does not establish that no website exists."}));
    }
    let result = json!({"query_hash":query_hash,"latitude":latitude,"longitude":longitude,"radius_metres":radius,"category":category,"endpoint":endpoint,"fetched_at":command_center_core::now(),"rows":rows,"attribution":"© OpenStreetMap contributors · Open Database License (ODbL)","license_url":"https://www.openstreetmap.org/copyright","limits":"At most 50 community listings; not exhaustive, ranked by source order, not commercial value. No geocoding, crawling or outreach."});
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    if state.data_epoch.load(std::sync::atomic::Ordering::SeqCst) != epoch {
        return Err("Workspace reset during business lookup".into());
    }
    repo.set_setting("business_search_last", &result.to_string())?;
    Ok(result)
}
#[tauri::command]
pub fn save_discovered_lead(state: State<AppState>, source_id: String) -> Result<String> {
    let mut repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    if let Some(existing) = repo
        .records("", false)?
        .iter()
        .find(|r| r.module == "crm" && r.fields.get("osm_id") == Some(&source_id))
    {
        return Ok(existing.id.clone());
    }
    let cached: Value = serde_json::from_str(
        &repo
            .setting("business_search_last")?
            .ok_or("Search for a business first")?,
    )
    .map_err(|_| "Invalid cache")?;
    let r = cached["rows"]
        .as_array()
        .and_then(|rows| rows.iter().find(|r| r["id"] == source_id))
        .ok_or("This listing is no longer in the last search; search again")?;
    let mut record = Record::new(
        "crm",
        r["name"].as_str().ok_or("Invalid listing name")?,
        &repo.device_id,
    );
    record.status = "researching".into();
    record.summary=format!("Community business listing. {}\nSource: {}\nFetched: {}\n© OpenStreetMap contributors · ODbL. No contact or website audit has been performed.",r["verification"].as_str().unwrap_or("Unverified"),r["source_url"].as_str().unwrap_or(""),cached["fetched_at"].as_str().unwrap_or(""));
    for (key, value) in [
        ("osm_id", source_id.as_str()),
        ("website", r["website"].as_str().unwrap_or("")),
        ("location", r["address"].as_str().unwrap_or("")),
        ("source", r["source_url"].as_str().unwrap_or("")),
        (
            "next_action",
            "Verify the business and review its public website before qualifying the opportunity",
        ),
    ] {
        record.fields.insert(key.into(), value.into());
    }
    let id = record.id.clone();
    repo.save_record(record)?;
    Ok(id)
}
