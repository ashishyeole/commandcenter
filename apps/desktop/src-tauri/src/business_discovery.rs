use crate::{AppState, Result};
use command_center_core::workspace::Record;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{sync::atomic::Ordering, time::Duration};
use tauri::State;
use tokio_util::sync::CancellationToken;

const JOB: &str = "business:public";
const NORTH_AMERICA: &str = "https://ovp.dresel.systems/api-na/interpreter";
const GLOBAL: &str = "https://photon.komoot.io/reverse";
const ALTERNATE: &str = "https://maps.mail.ru/osm/tools/overpass/api/interpreter";

fn safe_url(raw: &str) -> Option<String> {
    if raw.len() > 2000 {
        return None;
    }
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
fn endpoints(endpoint: &str, latitude: f64, longitude: f64) -> Result<Vec<String>> {
    if endpoint == "auto" {
        // Conservative North American bounds; other coordinates use global coverage.
        let first = if (10.0..=84.0).contains(&latitude) && (-170.0..=-52.0).contains(&longitude) {
            NORTH_AMERICA
        } else {
            GLOBAL
        };
        return Ok(vec![
            first.into(),
            if first == NORTH_AMERICA {
                GLOBAL.into()
            } else {
                ALTERNATE.into()
            },
        ]);
    }
    let u = reqwest::Url::parse(endpoint).map_err(|_| "Enter a public HTTPS Overpass endpoint")?;
    if endpoint.len() > 2000
        || u.scheme() != "https"
        || u.host_str().is_none()
        || !u.username().is_empty()
        || u.password().is_some()
        || u.query().is_some()
        || u.fragment().is_some()
    {
        return Err("Use a public HTTPS endpoint without credentials, query or fragment".into());
    }
    Ok(vec![u.to_string()])
}
fn category_filter(category: &str) -> Result<&'static str> {
    match category {
        "restaurants" => Ok("[amenity=restaurant]"),
        "cafes" => Ok("[amenity=cafe]"),
        "shops" => Ok("[shop]"),
        "offices" => Ok("[office]"),
        "salons" => Ok("[shop=hairdresser]"),
        _ => Err("Choose a supported business category".into()),
    }
}
fn business_query(latitude: f64, longitude: f64, radius: u32, filter: &str) -> String {
    // Bbox-only queries avoid the expensive global around scan; exact circle filtering is local.
    let delta_lat = f64::from(radius) / 110_000.0;
    let delta_lon = delta_lat / latitude.to_radians().cos();
    let (west, east) = (longitude - delta_lon, longitude + delta_lon);
    let area = if west >= -180.0 && east <= 180.0 {
        format!(
            "({:.6},{:.6},{:.6},{:.6})",
            latitude - delta_lat,
            west,
            latitude + delta_lat,
            east
        )
    } else {
        format!("(around:{radius},{latitude:.6},{longitude:.6})")
    };
    format!("[out:json][timeout:15][maxsize:33554432];nwr{area}{filter}[name];out tags center 100;")
}
fn distance_metres(a: f64, b: f64, lat: f64, lon: f64) -> f64 {
    let h = ((lat - a).to_radians() / 2.).sin().powi(2)
        + a.to_radians().cos()
            * lat.to_radians().cos()
            * ((lon - b).to_radians() / 2.).sin().powi(2);
    6_371_000. * 2. * h.clamp(0., 1.).sqrt().asin()
}
#[cfg(test)]
fn parse_rows(value: &Value, category: &str) -> Result<Vec<Value>> {
    parse_scoped_rows(value, category, None)
}
fn parse_scoped_rows(
    value: &Value,
    category: &str,
    scope: Option<(f64, f64, u32)>,
) -> Result<Vec<Value>> {
    if value
        .get("remark")
        .is_some_and(|r| !r.is_null() && r.as_str() != Some(""))
    {
        return Err(
            "Provider returned an incomplete query or warning; partial listings were not used"
                .into(),
        );
    }
    let elements = value["elements"]
        .as_array()
        .ok_or("Unexpected Overpass result: missing elements")?;
    let mut seen = std::collections::HashSet::new();
    let mut rows = vec![];
    for r in elements.iter().take(500) {
        let kind = r["type"].as_str().unwrap_or("");
        let Some(id) = r["id"].as_u64() else { continue };
        if !["node", "way", "relation"].contains(&kind) || !seen.insert(format!("{kind}/{id}")) {
            continue;
        }
        let tags = &r["tags"];
        let matches = match category {
            "restaurants" => tags["amenity"] == "restaurant",
            "cafes" => tags["amenity"] == "cafe",
            "shops" => tags["shop"].as_str().is_some_and(|v| !v.is_empty()),
            "offices" => tags["office"].as_str().is_some_and(|v| !v.is_empty()),
            "salons" => tags["shop"] == "hairdresser",
            _ => false,
        };
        let name = tags["name"]
            .as_str()
            .unwrap_or("")
            .trim()
            .chars()
            .take(100)
            .collect::<String>();
        if !matches || name.is_empty() || tags["disused"] == "yes" || tags["abandoned"] == "yes" {
            continue;
        }
        let distance = if let Some((latitude, longitude, radius)) = scope {
            let Some(lat) = r["lat"].as_f64().or_else(|| r["center"]["lat"].as_f64()) else {
                continue;
            };
            let Some(lon) = r["lon"].as_f64().or_else(|| r["center"]["lon"].as_f64()) else {
                continue;
            };
            if !lat.is_finite()
                || !lon.is_finite()
                || !(-90.0..=90.0).contains(&lat)
                || !(-180.0..=180.0).contains(&lon)
            {
                continue;
            }
            let distance = distance_metres(latitude, longitude, lat, lon);
            if distance > f64::from(radius) {
                continue;
            }
            Some(distance.round() as u32)
        } else {
            None
        };
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
        rows.push(json!({"id":format!("{kind}/{id}"),"name":name,"website":website,"address":address,"category":category,"distance_metres":distance,"source_url":format!("https://www.openstreetmap.org/{kind}/{id}"),"verification":"Community map listing; verify operation, ownership and digital presence. Missing website tag does not establish that no website exists."}));
        if rows.len() == 50 {
            break;
        }
    }
    Ok(rows)
}
fn photon_rows(value: &Value, category: &str, scope: (f64, f64, u32)) -> Result<Vec<Value>> {
    let features = value["features"]
        .as_array()
        .ok_or("Unexpected Photon result: missing features")?;
    let elements=features.iter().take(500).filter_map(|feature| {
        let p=&feature["properties"];
        let kind=match p["osm_type"].as_str()? {"N"=>"node","W"=>"way","R"=>"relation",_=>return None};
        if feature["geometry"]["type"]!="Point" {return None;}
        let coords=feature["geometry"]["coordinates"].as_array()?;
        let key=p["osm_key"].as_str()?;let value=p["osm_value"].as_str()?;
        if !["amenity","shop","office"].contains(&key) {return None;}
        let mut tags=serde_json::Map::new();tags.insert(key.into(),json!(value));
        for (target,source) in [("name","name"),("addr:housenumber","housenumber"),("addr:street","street"),("addr:city","city"),("addr:postcode","postcode"),("website","website")] {
            if let Some(value)=p[source].as_str(){tags.insert(target.into(),json!(value));}
        }
        Some(json!({"type":kind,"id":p["osm_id"],"lon":coords.first(),"lat":coords.get(1),"tags":tags}))
    }).collect::<Vec<_>>();
    let mut rows = parse_scoped_rows(&json!({"elements":elements}), category, Some(scope))?;
    for row in &mut rows {
        row["verification"]=json!("Public Photon index of OpenStreetMap; verify operation, ownership and website. This index may omit website tags; an absent value does not establish that no website exists.");
    }
    Ok(rows)
}
fn photon_tag(category: &str) -> Result<&'static str> {
    match category {
        "restaurants" => Ok("amenity:restaurant"),
        "cafes" => Ok("amenity:cafe"),
        "shops" => Ok("shop"),
        "offices" => Ok("office"),
        "salons" => Ok("shop:hairdresser"),
        _ => Err("Choose a supported business category".into()),
    }
}
#[derive(Debug)]
struct ProviderFailure {
    message: String,
    stop: bool,
    pause_seconds: i64,
    retryable: bool,
}
impl ProviderFailure {
    fn unavailable(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            stop: false,
            pause_seconds: 0,
            retryable: false,
        }
    }
    fn transient(message: impl Into<String>) -> Self {
        Self {
            retryable: true,
            ..Self::unavailable(message)
        }
    }
}
fn status_failure(status: reqwest::StatusCode, retry_after: Option<&str>) -> ProviderFailure {
    let pause = retry_after.and_then(|s| {
        s.parse::<i64>().ok().or_else(|| {
            chrono::DateTime::parse_from_rfc2822(s)
                .ok()
                .map(|t| (t.with_timezone(&chrono::Utc) - chrono::Utc::now()).num_seconds())
        })
    });
    let stop = (status.is_client_error() && status.as_u16() != 408) || pause.is_some();
    ProviderFailure {
        message: format!(
            "Public provider returned HTTP {}{}",
            status.as_u16(),
            if stop {
                "; automatic fallback stopped to respect access/rate limits"
            } else {
                ""
            }
        ),
        stop,
        retryable: !stop && (status.is_server_error() || status.as_u16() == 408),
        pause_seconds: if status.as_u16() == 429 {
            pause.unwrap_or(60).clamp(60, 86400)
        } else {
            pause.unwrap_or(0).clamp(0, 86400)
        },
    }
}
async fn read_provider(
    endpoint: String,
    query: &str,
    category: &str,
    scope: (f64, f64, u32),
) -> std::result::Result<Vec<Value>, ProviderFailure> {
    let url =
        reqwest::Url::parse(&endpoint).map_err(|e| ProviderFailure::unavailable(e.to_string()))?;
    let c = crate::research::public_client(&url)
        .await
        .map_err(ProviderFailure::unavailable)?;
    // Overpass POST is a read-only query. Prefer it for global/custom services;
    // their gateways can reject/time out encoded GET queries. The regional replica supports GET.
    let request = if endpoint == GLOBAL {
        c.get(url).query(&[
            ("lat", scope.0.to_string()),
            ("lon", scope.1.to_string()),
            ("radius", (f64::from(scope.2) / 1000.).to_string()),
            ("limit", "50".into()),
            (
                "osm_tag",
                photon_tag(category)
                    .map_err(ProviderFailure::unavailable)?
                    .into(),
            ),
        ])
    } else if endpoint == NORTH_AMERICA {
        c.get(url).query(&[("data", query)])
    } else {
        c.post(url).form(&[("data", query)])
    };
    let response = request
        .header(
            "User-Agent",
            "PersonalCommandCenter/0.5 (+https://github.com/ashishyeole/commandcenter)",
        )
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| {
            ProviderFailure::transient(format!(
                "Public provider connection failed: {}",
                e.without_url()
            ))
        })?;
    if !response.status().is_success() {
        return Err(status_failure(
            response.status(),
            response
                .headers()
                .get("Retry-After")
                .and_then(|v| v.to_str().ok()),
        ));
    }
    let value = crate::connectors::json_response(response)
        .await
        .map_err(ProviderFailure::unavailable)?;
    if endpoint == GLOBAL {
        photon_rows(&value, category, scope)
    } else {
        parse_scoped_rows(&value, category, Some(scope))
    }
    .map_err(ProviderFailure::unavailable)
}
#[derive(Debug)]
struct Lookup {
    rows: Vec<Value>,
    endpoint: String,
    attempts: Vec<Value>,
}
#[derive(Debug)]
struct LookupFailure {
    attempts: Vec<Value>,
    pause_seconds: i64,
}
impl LookupFailure {
    fn message(&self) -> String {
        format!("Public business lookup unavailable. {}. Previous successful results and saved leads are unchanged.",self.attempts.iter().map(|r|format!("{}: {}",r["endpoint"].as_str().unwrap_or("source"),r["error"].as_str().unwrap_or("unavailable"))).collect::<Vec<_>>().join("; "))
    }
}
async fn run_plan<F, Fut>(
    urls: Vec<String>,
    mut read: F,
) -> std::result::Result<Lookup, LookupFailure>
where
    F: FnMut(String) -> Fut,
    Fut: std::future::Future<Output = std::result::Result<Vec<Value>, ProviderFailure>>,
{
    let mut attempts = vec![];
    for endpoint in urls {
        for attempt in 0..2 {
            match read(endpoint.clone()).await {
                Ok(rows) => {
                    attempts.push(json!({"endpoint":endpoint,"attempt":attempt+1,"status":"ok","count":rows.len()}));
                    return Ok(Lookup {
                        rows,
                        endpoint,
                        attempts,
                    });
                }
                Err(e) => {
                    attempts.push(json!({"endpoint":endpoint,"attempt":attempt+1,"status":"error","error":e.message}));
                    if e.stop {
                        return Err(LookupFailure {
                            attempts,
                            pause_seconds: e.pause_seconds,
                        });
                    }
                    if e.retryable && attempt == 0 {
                        tokio::time::sleep(Duration::from_secs(2)).await;
                    } else {
                        break;
                    }
                }
            }
        }
    }
    Err(LookupFailure {
        attempts,
        pause_seconds: 0,
    })
}
struct JobGuard<'a>(&'a AppState);
impl Drop for JobGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut jobs) = self.0.jobs.lock() {
            jobs.remove(JOB);
        }
    }
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
pub fn cancel_public_businesses(state: State<AppState>) -> Result<()> {
    if let Some(cancel) = state.jobs.lock().map_err(|_| "Jobs unavailable")?.get(JOB) {
        cancel.cancel();
    }
    Ok(())
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
    lookup(
        &state, latitude, longitude, radius, category, endpoint, consent,
    )
    .await
}
async fn lookup(
    state: &AppState,
    latitude: f64,
    longitude: f64,
    radius: u32,
    category: String,
    endpoint: String,
    consent: bool,
) -> Result<Value> {
    lookup_with(
        state,
        latitude,
        longitude,
        radius,
        category,
        endpoint,
        consent,
        |url, query, category| async move {
            read_provider(url, &query, &category, (latitude, longitude, radius)).await
        },
    )
    .await
}
async fn lookup_with<F, Fut>(
    state: &AppState,
    latitude: f64,
    longitude: f64,
    radius: u32,
    category: String,
    endpoint: String,
    consent: bool,
    mut read: F,
) -> Result<Value>
where
    F: FnMut(String, String, String) -> Fut,
    Fut: std::future::Future<Output = std::result::Result<Vec<Value>, ProviderFailure>>,
{
    if !consent
        || !latitude.is_finite()
        || !longitude.is_finite()
        || !(-85.0..=85.0).contains(&latitude)
        || !(-180.0..=180.0).contains(&longitude)
        || !(100..=3000).contains(&radius)
    {
        return Err(
            "Approve a public coordinate search with a radius from 100 to 3,000 metres".into(),
        );
    }
    let filter = category_filter(&category)?;
    let urls = endpoints(&endpoint, latitude, longitude)?;
    let query = business_query(latitude, longitude, radius, filter);
    let query_hash = format!(
        "{:x}",
        Sha256::digest(format!("business-v2\n{endpoint}\n{query}"))
    );
    let epoch = state.data_epoch.load(Ordering::SeqCst);
    let cancel = CancellationToken::new();
    {
        let mut jobs = state.jobs.lock().map_err(|_| "Jobs unavailable")?;
        if jobs.contains_key(JOB) {
            return Err("A public business lookup is already running".into());
        }
        jobs.insert(JOB.into(), cancel.clone());
    }
    let _guard = JobGuard(state);
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
            return Err("Public business searches are limited to one per minute; matching cached results are reused for 24 hours. Respect any longer provider pause.".into());
        }
        repo.set_setting(
            "business_search_next",
            &(chrono::Utc::now() + chrono::Duration::minutes(1)).to_rfc3339(),
        )?;
    }
    let work = run_plan(urls, |url| read(url, query.clone(), category.clone()));
    let result = tokio::select! {
        _=cancel.cancelled()=>return Err("Public business lookup stopped; previous results retained".into()),
        r=tokio::time::timeout(Duration::from_secs(75),work)=>r.map_err(|_|"Public business lookup timed out after 75 seconds; previous results retained")?
    };
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    if state.data_epoch.load(Ordering::SeqCst) != epoch || cancel.is_cancelled() {
        return Err("Workspace reset or lookup cancelled; results discarded".into());
    }
    let found = match result {
        Ok(found) => found,
        Err(failure) => {
            if failure.pause_seconds > 0 {
                repo.set_setting(
                    "business_search_next",
                    &(chrono::Utc::now()
                        + chrono::Duration::seconds(failure.pause_seconds.max(60)))
                    .to_rfc3339(),
                )?;
            }
            return Err(failure.message());
        }
    };
    let result = json!({"query_hash":query_hash,"latitude":latitude,"longitude":longitude,"radius_metres":radius,"category":category,"provider_mode":endpoint,"endpoint":found.endpoint,"attempts":found.attempts,"fetched_at":command_center_core::now(),"rows":found.rows,"attribution":"© OpenStreetMap contributors · Open Database License (ODbL)","license_url":"https://www.openstreetmap.org/copyright","limits":"At most 50 community listings in source order; not exhaustive or independently verified. Coordinates or way centres are filtered to the requested radius; unlocated listings are skipped. Public map indexes/replicas may lag or omit website tags. No device location, account, crawling or outreach."});
    repo.set_setting("business_search_last", &result.to_string())?;
    Ok(result)
}

#[tauri::command]
pub fn save_discovered_lead(state: State<AppState>, source_id: String) -> Result<String> {
    save_lead(&state, &source_id)
}
fn save_lead(state: &AppState, source_id: &str) -> Result<String> {
    let mut repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    if let Some(existing) = repo.records("", false)?.iter().find(|r| {
        r.module == "crm" && r.fields.get("osm_id").map(String::as_str) == Some(source_id)
    }) {
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
        ("osm_id", source_id),
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    };
    fn state(dir: &std::path::Path) -> AppState {
        AppState {
            repo: Mutex::new(command_center_core::Repository::open(dir.join("qa.db")).unwrap()),
            peer_sync: Mutex::new(crate::peer_sync::Runtime::default()),
            project_watchers: Mutex::new(Default::default()),
            pending_update: Mutex::new(None),
            automation_lock: tokio::sync::Mutex::new(()),
            backup_lock: tokio::sync::Mutex::new(()),
            data_epoch: std::sync::atomic::AtomicU64::new(0),
            pending: Mutex::new(None),
            large_import: Mutex::new(None),
            google_cache: Mutex::new(None),
            vault: Mutex::new(None),
            jobs: Mutex::new(Default::default()),
        }
    }
    fn fixture() -> Vec<Value> {
        parse_rows(&json!({"elements":[{"type":"node","id":123,"tags":{"name":"QA public restaurant","amenity":"restaurant","website":"https://public.example/"}}]}),"restaurants").unwrap()
    }
    #[test]
    fn provider_scope_and_custom_urls_are_explicit() {
        assert_eq!(
            endpoints("auto", 30.2672, -97.7431).unwrap(),
            vec![NORTH_AMERICA, GLOBAL]
        );
        assert_eq!(
            endpoints("auto", 12.9716, 77.5946).unwrap(),
            vec![GLOBAL, ALTERNATE]
        );
        for u in [
            "http://example.com/api",
            "https://user:secret@example.com/api",
            "https://example.com/api?token=x",
            "https://example.com/api#fragment",
        ] {
            assert!(endpoints(u, 0., 0.).is_err());
        }
        assert_eq!(
            endpoints("https://public.example/api", 0., 0.)
                .unwrap()
                .len(),
            1
        );
    }
    #[test]
    fn bounded_queries_preserve_circle_and_do_not_clip_the_dateline() {
        let q = business_query(12.9716, 77.5946, 1500, "[amenity=restaurant]");
        assert!(q.contains("nwr(12."));
        assert!(!q.contains("around:"));
        let edge = business_query(85., 179.99, 3000, "[shop]");
        assert!(!edge.contains("[bbox:"));
        assert!(!edge.contains("NaN"));
        assert!(edge.contains("(around:3000,85.000000,179.990000)"));
    }
    #[test]
    fn listings_reject_partial_and_malformed_data_and_keep_attribution_inputs() {
        let value = json!({"elements":[{"type":"node","id":1,"tags":{"name":"Cafe","amenity":"cafe"}}, {"type":"way","id":2,"tags":{"name":"Diner","amenity":"restaurant","website":"javascript:bad","contact:website":"https://unused.example"}},{"type":"way","id":2,"tags":{"name":"Duplicate","amenity":"restaurant"}}, {"type":"x","id":3,"tags":{"name":"Invalid","amenity":"restaurant"}}]});
        let rows = parse_rows(&value, "restaurants").unwrap();
        assert_eq!(rows.len(), 1);
        assert!(rows[0]["website"].is_null());
        assert_eq!(rows[0]["source_url"], "https://www.openstreetmap.org/way/2");
        assert!(parse_rows(
            &json!({"remark":"runtime timeout","elements":value["elements"]}),
            "restaurants"
        )
        .is_err());
        assert!(parse_rows(&json!({}), "restaurants").is_err());
        assert!(parse_rows(&json!({"elements":[]}), "restaurants")
            .unwrap()
            .is_empty());
    }
    #[test]
    fn bbox_candidates_are_filtered_by_actual_distance_and_way_centres() {
        let v = json!({"elements":[{"type":"node","id":1,"lat":12.9716,"lon":77.5946,"tags":{"name":"Inside","amenity":"restaurant"}}, {"type":"way","id":2,"center":{"lat":12.9717,"lon":77.5946},"tags":{"name":"Way inside","amenity":"restaurant"}},{"type":"node","id":3,"lat":12.9816,"lon":77.6046,"tags":{"name":"Outside circle","amenity":"restaurant"}},{"type":"node","id":4,"tags":{"name":"Unknown location","amenity":"restaurant"}}]});
        let rows = parse_scoped_rows(&v, "restaurants", Some((12.9716, 77.5946, 1000))).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["distance_metres"], 0);
        assert!(rows[1]["distance_metres"].as_u64().unwrap() < 20);
        assert!(distance_metres(0., 179.999, 0., -179.999) < 250.);
    }
    #[test]
    fn photon_businesses_keep_osm_identity_and_reject_wrong_category_or_location() {
        let feature = |id, tag, lon| json!({"type":"Feature","properties":{"osm_type":"N","osm_id":id,"osm_key":"amenity","osm_value":tag,"name":"Public business","city":"Bengaluru"},"geometry":{"type":"Point","coordinates":[lon,12.9716]}});
        let v = json!({"features":[feature(1,"restaurant",77.5946),feature(2,"cafe",77.5946),feature(3,"restaurant",80.)]});
        let rows = photon_rows(&v, "restaurants", (12.9716, 77.5946, 1500)).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["id"], "node/1");
        assert_eq!(
            rows[0]["source_url"],
            "https://www.openstreetmap.org/node/1"
        );
        assert!(rows[0]["website"].is_null());
        assert!(rows[0]["verification"]
            .as_str()
            .unwrap()
            .contains("may omit website"));
        assert!(photon_rows(
            &json!({"error":"unavailable"}),
            "restaurants",
            (12.9716, 77.5946, 1500)
        )
        .is_err());
    }
    #[tokio::test]
    async fn unavailable_provider_falls_back_without_fabricating_rows() {
        let calls = AtomicUsize::new(0);
        let found = run_plan(vec!["first".into(), "second".into()], |_| {
            let first = calls.fetch_add(1, Ordering::SeqCst) == 0;
            std::future::ready(if first {
                Err(ProviderFailure::unavailable("HTTP 502"))
            } else {
                Ok(fixture())
            })
        })
        .await
        .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(found.endpoint, "second");
        assert_eq!(found.rows[0]["id"], "node/123");
        assert_eq!(found.attempts[0]["status"], "error");
        let empty = run_plan(vec!["first".into(), "second".into()], |_| {
            std::future::ready(Ok(vec![]))
        })
        .await
        .unwrap();
        assert!(empty.rows.is_empty());
        assert_eq!(empty.attempts.len(), 1);
    }
    #[tokio::test]
    async fn transient_failure_retries_once_on_the_same_provider() {
        let calls = AtomicUsize::new(0);
        let found = run_plan(vec!["first".into(), "second".into()], |_| {
            std::future::ready(if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                Err(status_failure(reqwest::StatusCode::GATEWAY_TIMEOUT, None))
            } else {
                Ok(fixture())
            })
        })
        .await
        .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(found.endpoint, "first");
        assert_eq!(found.attempts[1]["attempt"], 2);
        let failed = run_plan(vec!["first".into()], |_| {
            std::future::ready(Err(ProviderFailure::transient("offline")))
        })
        .await
        .unwrap_err();
        assert_eq!(failed.attempts.len(), 2);
    }
    #[tokio::test]
    async fn permission_and_quota_refusals_do_not_rotate_providers() {
        for code in [400, 401, 403, 406, 429] {
            let calls = AtomicUsize::new(0);
            let failed = run_plan(vec!["first".into(), "second".into()], |_| {
                calls.fetch_add(1, Ordering::SeqCst);
                std::future::ready(Err(status_failure(
                    reqwest::StatusCode::from_u16(code).unwrap(),
                    Some("120"),
                )))
            })
            .await
            .unwrap_err();
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            assert_eq!(failed.pause_seconds, 120);
        }
    }
    #[tokio::test]
    async fn cache_scope_throttle_deduplication_and_failure_preserve_records() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(dir.path());
        let calls = AtomicUsize::new(0);
        let first = lookup_with(
            &state,
            30.2672,
            -97.7431,
            1500,
            "restaurants".into(),
            "auto".into(),
            true,
            |_, _, _| {
                calls.fetch_add(1, Ordering::SeqCst);
                std::future::ready(Ok(fixture()))
            },
        )
        .await
        .unwrap();
        let cached = lookup_with(
            &state,
            30.2672,
            -97.7431,
            1500,
            "restaurants".into(),
            "auto".into(),
            true,
            |_, _, _| {
                std::future::ready(Err(ProviderFailure::unavailable(
                    "matching cache unexpectedly contacted provider",
                )))
            },
        )
        .await
        .unwrap();
        assert_eq!(first, cached);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(lookup_with(
            &state,
            31.,
            -97.,
            1500,
            "cafes".into(),
            "auto".into(),
            true,
            |_, _, _| std::future::ready(Ok(vec![]))
        )
        .await
        .unwrap_err()
        .contains("one per minute"));
        let id = save_lead(&state, "node/123").unwrap();
        assert_eq!(save_lead(&state, "node/123").unwrap(), id);
        assert_eq!(
            state.repo.lock().unwrap().records("", false).unwrap().len(),
            1
        );
        state
            .repo
            .lock()
            .unwrap()
            .set_setting("business_search_next", "2000-01-01T00:00:00Z")
            .unwrap();
        let failed = lookup_with(
            &state,
            30.,
            -97.,
            1500,
            "restaurants".into(),
            "auto".into(),
            true,
            |_, _, _| std::future::ready(Err(ProviderFailure::unavailable("offline"))),
        )
        .await
        .unwrap_err();
        assert!(failed.contains("Previous successful results"));
        assert_eq!(
            serde_json::from_str::<Value>(
                &state
                    .repo
                    .lock()
                    .unwrap()
                    .setting("business_search_last")
                    .unwrap()
                    .unwrap()
            )
            .unwrap(),
            first
        );
        assert!(state.jobs.lock().unwrap().is_empty());
    }
    #[tokio::test]
    async fn cancelled_or_reset_lookup_cannot_publish_and_invalid_consent_does_not_send() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(dir.path());
        assert!(lookup_with(
            &state,
            30.,
            -97.,
            1500,
            "restaurants".into(),
            "auto".into(),
            false,
            |_, _, _| async { panic!("lookup contacted provider without consent") }
        )
        .await
        .is_err());
        assert!(state
            .repo
            .lock()
            .unwrap()
            .setting("business_search_next")
            .unwrap()
            .is_none());
        let failed = lookup_with(
            &state,
            30.,
            -97.,
            1500,
            "restaurants".into(),
            "auto".into(),
            true,
            |_, _, _| {
                state.data_epoch.fetch_add(1, Ordering::SeqCst);
                std::future::ready(Ok(fixture()))
            },
        )
        .await
        .unwrap_err();
        assert!(failed.contains("discarded"));
        assert!(state
            .repo
            .lock()
            .unwrap()
            .setting("business_search_last")
            .unwrap()
            .is_none());
        assert!(state.jobs.lock().unwrap().is_empty());
        state
            .repo
            .lock()
            .unwrap()
            .set_setting("business_search_next", "2000-01-01T00:00:00Z")
            .unwrap();
        let failed = lookup_with(
            &state,
            30.,
            -97.,
            1500,
            "restaurants".into(),
            "auto".into(),
            true,
            |_, _, _| {
                state
                    .jobs
                    .lock()
                    .unwrap()
                    .get("business:public")
                    .unwrap()
                    .cancel();
                std::future::pending::<std::result::Result<Vec<Value>, ProviderFailure>>()
            },
        )
        .await
        .unwrap_err();
        assert!(failed.contains("stopped"));
        assert!(state
            .repo
            .lock()
            .unwrap()
            .setting("business_search_last")
            .unwrap()
            .is_none());
        assert!(state.jobs.lock().unwrap().is_empty());
    }
    #[tokio::test]
    #[ignore = "Live public lookup; isolated DB, no account or key"]
    async fn live_public_business_lookup() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(dir.path());
        let austin = lookup(
            &state,
            30.2672,
            -97.7431,
            1500,
            "restaurants".into(),
            "auto".into(),
            true,
        )
        .await
        .unwrap();
        assert!(!austin["rows"].as_array().unwrap().is_empty());
        let id = austin["rows"][0]["id"].as_str().unwrap();
        let saved = save_lead(&state, id).unwrap();
        assert_eq!(save_lead(&state, id).unwrap(), saved);
        let cached = lookup(
            &state,
            30.2672,
            -97.7431,
            1500,
            "restaurants".into(),
            "auto".into(),
            true,
        )
        .await
        .unwrap();
        assert_eq!(austin, cached);
        state
            .repo
            .lock()
            .unwrap()
            .set_setting("business_search_next", "2000-01-01T00:00:00Z")
            .unwrap();
        let india = lookup(
            &state,
            12.9716,
            77.5946,
            1500,
            "restaurants".into(),
            "auto".into(),
            true,
        )
        .await;
        let report = json!({"checked_at":command_center_core::now(),"isolated":true,"accounts_used":false,"austin":austin,"india":india,"duplicate_lead_kept_same_id":true});
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../target/live-business/results.json");
        std::fs::create_dir_all(out.parent().unwrap()).unwrap();
        std::fs::write(out, serde_json::to_string_pretty(&report).unwrap()).unwrap();
        assert!(india.is_ok(), "Global India lookup: {india:?}");
        assert!(!india.unwrap()["rows"].as_array().unwrap().is_empty());
    }
}
