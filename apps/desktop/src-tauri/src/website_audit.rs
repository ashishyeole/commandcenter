//! Explicit single-page public HTML inspection; never a browser, crawler, or vulnerability scanner.
use crate::{AppState, Result};
use serde_json::{json, Value};
use tauri::State;
#[tauri::command]
pub async fn audit_public_website(
    state: State<'_, AppState>,
    record_id: String,
    revision: i64,
    url: String,
    consent: bool,
) -> Result<Value> {
    if !consent {
        return Err(
            "Approve a public single-page request and confirm permission to inspect the site"
                .into(),
        );
    }
    if url.len() > 2000 {
        return Err("URL exceeds limit".into());
    }
    let endpoint = reqwest::Url::parse(&url).map_err(|_| "Enter a full public HTTPS URL")?;
    if endpoint.query().is_some() {
        return Err("Use a public URL without query parameters or access tokens".into());
    }
    let epoch = state.data_epoch.load(std::sync::atomic::Ordering::SeqCst);
    let mut record = {
        let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
        let r = repo
            .records("", false)?
            .into_iter()
            .find(|r| r.id == record_id && r.module == "crm")
            .ok_or("Save a CRM lead before auditing")?;
        if r.revision != revision {
            return Err("Lead changed; reload before auditing".into());
        }
        let key = format!("website_audit_limit:{}", endpoint.host_str().unwrap_or(""));
        if repo
            .setting(&key)?
            .and_then(|v| chrono::DateTime::parse_from_rfc3339(&v).ok())
            .is_some_and(|t| t > chrono::Utc::now())
        {
            return Err("Wait one minute before another request to this host".into());
        }
        repo.set_setting(
            &key,
            &(chrono::Utc::now() + chrono::Duration::minutes(1)).to_rfc3339(),
        )?;
        r
    };
    let client = crate::research::public_client(&endpoint).await?;
    let mut response = client
        .get(endpoint.clone())
        .header("Accept", "text/html")
        .send()
        .await
        .map_err(|_| "Public site request failed")?;
    if !response.status().is_success() {
        return Err(format!(
            "Site returned HTTP {}. Redirects are not followed; use its final HTTPS address.",
            response.status()
        ));
    }
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_lowercase();
    if !content_type.starts_with("text/html") {
        return Err("Page must be served as text/html".into());
    }
    let mut bytes = vec![];
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "Site download interrupted")?
    {
        if bytes.len() + chunk.len() > 2 * 1024 * 1024 {
            return Err("Page exceeds the 2 MiB limit".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    let html = String::from_utf8(bytes).map_err(|_| "Only UTF-8 pages are supported")?;
    if html.bytes().filter(|b| *b == b'<').count() > 20000 {
        return Err("Page has too many markup elements".into());
    }
    let audit=tokio::task::spawn_blocking(move || {
        let doc=dom_query::Document::from(html);
        let title=doc.select("title").text().trim().chars().take(300).collect::<String>();
        let description=doc.select("meta[name='description']").attr("content").map(|v|v.trim().chars().take(500).collect::<String>()).unwrap_or_default();
        let viewport=doc.select("meta[name='viewport']").attr("content").is_some_and(|v|v.to_ascii_lowercase().contains("width=device-width"));
        let language=doc.select("html").attr("lang").is_some_and(|v|!v.trim().is_empty());
        let h1=doc.select("h1").length();
        let images=doc.select("img").length();
        let missing_alt=doc.select("img:not([alt])").length();
        let checks=vec![
            json!({"label":"Page title present","passed":!title.is_empty(),"weight":20,"evidence":title}),
            json!({"label":"Meta description present","passed":!description.is_empty(),"weight":15,"evidence":description}),
            json!({"label":"Mobile viewport declaration","passed":viewport,"weight":20,"evidence":"Static meta viewport width=device-width check"}),
            json!({"label":"Document language declared","passed":language,"weight":10,"evidence":"Nonempty html lang attribute"}),
            json!({"label":"One primary heading","passed":h1==1,"weight":15,"evidence":format!("{h1} h1 elements")}),
            json!({"label":"Images declare alternate text","passed":missing_alt==0,"weight":20,"evidence":format!("{missing_alt} of {images} images lack an alt attribute; empty alt may be appropriate for decoration")}),
        ];
        let score:u64=checks.iter().filter(|c|c["passed"]==false).map(|c|c["weight"].as_u64().unwrap_or(0)).sum();
        json!({"url":endpoint.as_str(),"observed_at":command_center_core::now(),"opportunity_signal":score,"checks":checks,"scope":"Six static HTML checks, weighted missing-signal score 0–100. Higher means more review opportunities, not proven defects or a revenue prediction. No JavaScript rendering, performance, visual accessibility, security, traffic or business validation. No crawl, login, form submission or outreach."})
    }).await.map_err(|_|"Page analysis interrupted")?;
    record.fields.insert(
        "audit".into(),
        serde_json::to_string_pretty(&audit).map_err(|e| e.to_string())?,
    );
    record.fields.insert("website".into(), url);
    record.fields.insert(
        "opportunity_score".into(),
        audit["opportunity_signal"].to_string(),
    );
    let mut repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    if epoch != state.data_epoch.load(std::sync::atomic::Ordering::SeqCst) {
        return Err("Workspace reset during request".into());
    }
    let saved = repo.save_record(record)?;
    Ok(json!({"record":saved,"audit":audit}))
}
