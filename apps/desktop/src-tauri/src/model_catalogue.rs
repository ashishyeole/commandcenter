//! Public catalogue metadata only. No runtime, registry blobs or model weights.
use crate::{connectors::Discovery, Result};
use futures_util::StreamExt;
pub async fn fetch(query: &str, timestamp: &str) -> Result<Vec<Discovery>> {
    let mut url = reqwest::Url::parse("https://ollama.com/search").map_err(|e| e.to_string())?;
    if !query.trim().is_empty() {
        url.query_pairs_mut().append_pair("q", query);
    }
    let mut redirects = 0;
    let response = loop {
        let response = crate::research::public_client(&url)
            .await?
            .get(url.clone())
            .send()
            .await
            .map_err(|_| "Ollama catalogue unavailable")?;
        if response.status().is_redirection() {
            if redirects >= 3 {
                return Err("Too many Ollama catalogue redirects".into());
            }
            let location = response
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
                .ok_or("Catalogue redirect missing location")?;
            let next = url
                .join(location)
                .map_err(|_| "Invalid catalogue redirect")?;
            if next.scheme() != "https"
                || next.host_str() != Some("ollama.com")
                || next.path() != "/search"
            {
                return Err("Unexpected catalogue redirect; metadata search only".into());
            }
            url = next;
            redirects += 1;
            continue;
        }
        if !response.status().is_success() {
            return Err(format!(
                "Ollama catalogue returned {}; try later",
                response.status()
            ));
        }
        break response;
    };
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| "Catalogue response interrupted")?;
        if bytes.len() + chunk.len() > 2 * 1024 * 1024 {
            return Err("Catalogue response exceeds 2 MiB".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    let html = String::from_utf8(bytes).map_err(|_| "Invalid catalogue encoding")?;
    let timestamp = timestamp.to_owned();
    tokio::task::spawn_blocking(move || parse(&html, &timestamp))
        .await
        .map_err(|_| "Catalogue parsing interrupted")?
}
fn parse(html: &str, timestamp: &str) -> Result<Vec<Discovery>> {
    if html.bytes().filter(|b| *b == b'<').count() > 20_000 {
        return Err("Catalogue markup exceeds limit".into());
    }
    let doc = dom_query::Document::from(html);
    let mut rows = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for link in doc.select("a[href^='/library/']").iter() {
        let path = link.attr("href").unwrap_or_default().to_string();
        let slug = path.trim_start_matches("/library/");
        if slug.is_empty()
            || slug.len() > 150
            || !slug
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
            || !seen.insert(path.clone())
        {
            continue;
        }
        let title = link
            .select("h2")
            .text()
            .trim()
            .chars()
            .take(200)
            .collect::<String>();
        if title.is_empty() {
            continue;
        }
        rows.push(Discovery{id:format!("ollama:{slug}"),title,description:link.select("p").text().split_whitespace().collect::<Vec<_>>().join(" ").chars().take(1500).collect(),url:format!("https://ollama.com{path}"),source:"ollama".into(),metric:format!("Catalogue position {} in this response",rows.len()+1),published_at:None,fetched_at:timestamp.into(),verification:"Ollama public catalogue snapshot; provider order, not independently measured quality or growth. Metadata only; no model downloaded.".into()});
        if rows.len() == 30 {
            break;
        }
    }
    if rows.is_empty() && !html.contains("data-result-count=\"0\"") {
        return Err("No supported catalogue entries found; the provider layout may have changed. Existing cache is retained.".into());
    }
    Ok(rows)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalogue_only_accepts_model_links_and_plain_metadata() {
        let rows=parse(r#"<a href='/library/demo'><h2>Demo</h2><p>Research &amp; tools</p></a><a href='/library/demo'><h2>Duplicate</h2></a><a href='https://evil.example'><h2>External</h2></a><a href='/library/../escape'><h2>Escape</h2></a>"#,"fixture").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].url, "https://ollama.com/library/demo");
        assert_eq!(rows[0].description, "Research & tools");
    }
    #[test]
    fn layout_failure_is_not_a_successful_empty_feed() {
        assert!(parse("<html>Sign in</html>", "fixture").is_err());
        assert!(parse(r#"<div data-result-count="0"></div>"#, "fixture")
            .unwrap()
            .is_empty());
    }
}
