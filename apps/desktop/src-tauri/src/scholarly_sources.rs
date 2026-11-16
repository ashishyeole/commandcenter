//! Public scholarly/patent metadata and official scheme announcements; no account or full text.
use crate::{AppState, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
fn plain(s: &str, max: usize) -> String {
    let doc = dom_query::Document::from(s);
    doc.select("script,style").remove();
    doc.select("body")
        .text()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(max)
        .collect()
}
fn date(raw: &str) -> Option<String> {
    chrono::NaiveDate::parse_from_str(raw, "%Y-%m-%d")
        .ok()
        .map(|d| format!("{d}T00:00:00Z"))
}
pub(crate) async fn fetch(
    state: &AppState,
    provider: &str,
    query: &str,
    days: u32,
) -> Result<Vec<Value>> {
    if query.len() > 300 {
        return Err("Search query exceeds limit".into());
    }
    if provider == "schemes" {
        let url = "https://www.pib.gov.in/RssMain.aspx?ModId=6&Lang=1&Regid=1";
        let rows = crate::research::fetch(state, "rss", url).await?;
        let term = query.to_lowercase();
        return rows.into_iter().filter(|r|{let text=format!("{} {}",r.title,r.description).to_lowercase();["agricult","farmer","irrigation","crop","kusum","horticult","kisan"].iter().any(|k|text.contains(k))&&(term.is_empty()||text.contains(&term))}).map(|r|{let mut v=serde_json::to_value(r).map_err(|e|e.to_string())?;v["source"]=json!("schemes");v["metric"]=json!("Government of India announcement; verify current scheme rules and deadlines at the official source");Ok(v)}).collect();
    }
    let mut url = reqwest::Url::parse(if provider == "crossref" {
        "https://api.crossref.org/works"
    } else {
        "https://www.ebi.ac.uk/europepmc/webservices/rest/search"
    })
    .map_err(|e| e.to_string())?;
    if provider == "crossref" {
        let mut p = url.query_pairs_mut();
        p.append_pair("query", query)
            .append_pair("rows", "30")
            .append_pair("sort", "published")
            .append_pair("order", "desc");
        if days > 0 {
            p.append_pair(
                "filter",
                &format!(
                    "from-pub-date:{},until-pub-date:{}",
                    (chrono::Utc::now() - chrono::Duration::days(days as i64)).format("%Y-%m-%d"),
                    chrono::Utc::now().format("%Y-%m-%d")
                ),
            );
        }
    } else {
        let mut q = if query.trim().is_empty() {
            "robotics".to_owned()
        } else {
            query.to_owned()
        };
        if provider == "patents" {
            q = format!("({q}) AND SRC:PAT");
        }
        if days > 0 {
            q = format!(
                "({q}) AND FIRST_PDATE:[{} TO {}]",
                (chrono::Utc::now() - chrono::Duration::days(days as i64)).format("%Y-%m-%d"),
                chrono::Utc::now().format("%Y-%m-%d")
            );
        }
        q.push_str(" sort_date:y");
        url.query_pairs_mut()
            .append_pair("query", &q)
            .append_pair("format", "json")
            .append_pair("resultType", "core")
            .append_pair("pageSize", "30");
    }
    let key = format!("scholarly_next:{provider}");
    {
        let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
        let now = chrono::Utc::now().timestamp();
        if repo
            .setting(&key)?
            .and_then(|s| s.parse::<i64>().ok())
            .is_some_and(|until| until > now)
        {
            return Err("Wait 30 seconds between requests to this scholarly provider".into());
        }
        repo.set_setting(&key, &(now + 30).to_string())?;
    }
    let response = crate::research::public_client(&url)
        .await?
        .get(url)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|_| "Scholarly provider request failed")?;
    let value = crate::connectors::json_response(response).await?;
    parse(provider, &value)
}
pub(crate) fn parse(provider: &str, value: &Value) -> Result<Vec<Value>> {
    let rows = if provider == "crossref" {
        value["message"]["items"].as_array()
    } else {
        value["resultList"]["result"].as_array()
    }
    .ok_or("Provider metadata layout changed")?;
    let mut result = vec![];
    for row in rows.iter().take(30) {
        let (title, url, description, published, metric, metadata) = if provider == "crossref" {
            let title = row["title"][0].as_str().unwrap_or("");
            let doi = row["DOI"].as_str().unwrap_or("");
            if !doi.starts_with("10.") || !doi.contains('/') {
                continue;
            }
            let mut u = reqwest::Url::parse("https://doi.org/").unwrap();
            u.path_segments_mut()
                .map_err(|_| "Invalid DOI")?
                .extend(doi.split('/'));
            let parts = row["published"]["date-parts"][0].as_array();
            let published = parts
                .filter(|p| p.len() >= 3)
                .and_then(|p| {
                    Some(format!(
                        "{:04}-{:02}-{:02}",
                        p[0].as_i64()?,
                        p[1].as_i64()?,
                        p[2].as_i64()?
                    ))
                })
                .and_then(|p| date(&p));
            (
                title.to_owned(),
                u.to_string(),
                row["abstract"].as_str().unwrap_or("").to_owned(),
                published,
                format!(
                    "{} provider-reported citations · {}",
                    row["is-referenced-by-count"].as_u64().unwrap_or(0),
                    row["type"].as_str().unwrap_or("Research metadata")
                ),
                json!({"doi":doi,"publisher":row["publisher"],"type":row["type"],"license":row["license"],"source":"Crossref member-deposited metadata"}),
            )
        } else {
            let id = row["id"].as_str().unwrap_or("");
            let source = row["source"].as_str().unwrap_or("");
            if id.is_empty() || source.is_empty() {
                continue;
            }
            let mut u = reqwest::Url::parse("https://europepmc.org/article/").unwrap();
            u.path_segments_mut()
                .map_err(|_| "Invalid publication ID")?
                .extend([source, id]);
            (
                row["title"].as_str().unwrap_or("").to_owned(),
                u.to_string(),
                row["abstractText"].as_str().unwrap_or("").to_owned(),
                row["firstPublicationDate"].as_str().and_then(date),
                format!(
                    "{} provider citations · {}",
                    row["citedByCount"].as_u64().unwrap_or(0),
                    if provider == "patents" {
                        "Indexed patent record; index coverage may be historical"
                    } else {
                        "Europe PMC indexed literature"
                    }
                ),
                json!({"id":id,"index":source,"doi":row["doi"],"publication_year":row["pubYear"],"patent_notice":if provider=="patents"{"Indexed patent metadata does not establish current legal status or complete patent-office coverage"}else{"Not applicable"}}),
            )
        };
        let title = plain(&title, 300);
        if title.is_empty() {
            continue;
        }
        result.push(json!({"id":format!("{provider}:{:x}",Sha256::digest(url.as_bytes())),"title":title,"url":url,"description":plain(&description,1800),"source":provider,"published_at":published,"fetched_at":command_center_core::now(),"metric":metric,"metadata":metadata,"verification":"Public indexed metadata. Publication, patent status and publisher claims need source review; no full text or model weights downloaded."}));
    }
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn partial_dates_and_patent_provenance_remain_explicit() {
        let rows=parse("crossref",&json!({"message":{"items":[{"title":["<b>Robot</b>"],"DOI":"10.123/a?b","published":{"date-parts":[[2026]]},"abstract":"<script>bad()</script>Research"}]}})).unwrap();
        assert!(rows[0]["published_at"].is_null());
        assert_eq!(rows[0]["description"], "Research");
        assert!(rows[0]["url"].as_str().unwrap().contains("%3F"));
        let rows=parse("patents",&json!({"resultList":{"result":[{"id":"EP1","source":"PAT","title":"Patent fixture","firstPublicationDate":"2016-01-02"}]}})).unwrap();
        assert_eq!(rows[0]["source"], "patents");
        assert!(rows[0]["metadata"]["patent_notice"]
            .as_str()
            .unwrap()
            .contains("legal status"));
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;
    #[tokio::test]
    #[ignore = "Explicit public internet adapter verification"]
    async fn live_remaining_sources() {
        let state = crate::discovery_hub::failure_tests::state();
        let mut report = vec![];
        for (provider, query, days) in [
            ("crossref", "robotics", 90),
            ("europepmc", "precision agriculture", 90),
            ("patents", "robot", 0),
            ("schemes", "", 0),
        ] {
            let result = fetch(&state, provider, query, days).await;
            report.push(match result{Ok(rows)=>json!({"provider":provider,"ok":true,"count":rows.len(),"sample_dates":rows.iter().take(2).map(|v|v["published_at"].clone()).collect::<Vec<_>>()}),Err(e)=>json!({"provider":provider,"ok":false,"error":e})});
        }
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../target/remaining-code/live-sources.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path,serde_json::to_string_pretty(&json!({"checked_at":command_center_core::now(),"credentials_used":false,"sources":report})).unwrap()).unwrap();
        assert!(report
            .iter()
            .filter(|v| v["provider"] != "schemes")
            .all(|v| v["ok"] == true && v["count"].as_u64().unwrap() > 0));
    }
}
