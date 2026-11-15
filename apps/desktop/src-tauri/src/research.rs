//! Bounded public RSS/Atom ingestion. No scripts, external entities, redirects or local-network fetches.
use crate::{connectors::Discovery, AppState, Result};
use quick_xml::events::Event;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, net::IpAddr, time::Duration};
fn public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => {
            !v.is_private()
                && !v.is_loopback()
                && !v.is_link_local()
                && !v.is_unspecified()
                && !v.is_broadcast()
                && !v.is_multicast()
                && !v.is_documentation()
                && v.octets()[0] != 0
                && v.octets()[0] < 224
                && !(v.octets()[0] == 198 && (v.octets()[1] == 18 || v.octets()[1] == 19))
                && !(v.octets()[0] == 192 && v.octets()[1] == 0 && v.octets()[2] == 0)
                && !(v.octets()[0] == 192 && v.octets()[1] == 88 && v.octets()[2] == 99)
                && (u32::from(v) & 0xffc00000) != 0x64400000
        }
        IpAddr::V6(v) => v.to_ipv4_mapped().map(|v| public(IpAddr::V4(v))).unwrap_or(
            (v.segments()[0] & 0xe000) == 0x2000
                && !(v.segments()[0] == 0x2001 && [0, 0xdb8].contains(&v.segments()[1]))
                && v.segments()[0] != 0x2002
                && !v.is_loopback()
                && !v.is_unspecified()
                && !v.is_multicast()
                && !v.is_unique_local()
                && !v.is_unicast_link_local(),
        ),
    }
}
fn clean(value: &str, max: usize) -> String {
    let mut tag = false;
    value
        .chars()
        .filter(|c| {
            if *c == '<' {
                tag = true;
                false
            } else if *c == '>' {
                tag = false;
                false
            } else {
                !tag
            }
        })
        .take(max)
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
pub async fn fetch(state: &AppState, source: &str, query: &str) -> Result<Vec<Discovery>> {
    let endpoint = if source == "arxiv" {
        let mut u =
            reqwest::Url::parse("https://export.arxiv.org/api/query").map_err(|e| e.to_string())?;
        u.query_pairs_mut()
            .append_pair(
                "search_query",
                if query.is_empty() { "cat:cs.AI" } else { query },
            )
            .append_pair("start", "0")
            .append_pair("max_results", "30")
            .append_pair("sortBy", "submittedDate")
            .append_pair("sortOrder", "descending");
        u
    } else {
        reqwest::Url::parse(query).map_err(|_| "Enter the full HTTPS RSS or Atom feed URL")?
    };
    let key = format!(
        "feed_limit:{:x}",
        Sha256::digest(if source == "arxiv" {
            b"arxiv-provider".as_slice()
        } else {
            endpoint.as_str().as_bytes()
        })
    );
    {
        let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
        if let Some(until) = repo.setting(&key)? {
            if chrono::DateTime::parse_from_rfc3339(&until).is_ok_and(|t| t > chrono::Utc::now()) {
                return Err(format!("Source cooldown: retry after {until}"));
            }
        }
        repo.set_setting(
            &key,
            &(chrono::Utc::now()
                + chrono::Duration::seconds(if source == "arxiv" { 3 } else { 30 }))
            .to_rfc3339(),
        )?;
    }
    let client = public_client(&endpoint).await?;
    let mut response = client
        .get(endpoint.clone())
        .send()
        .await
        .map_err(|_| "Feed request failed")?;
    if response.status().as_u16() == 429 || response.status().as_u16() == 503 {
        let retry = response
            .headers()
            .get("Retry-After")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| {
                v.parse::<i64>()
                    .ok()
                    .map(|seconds| {
                        chrono::Utc::now() + chrono::Duration::seconds(seconds.clamp(3, 86400))
                    })
                    .or_else(|| {
                        chrono::DateTime::parse_from_rfc2822(v)
                            .ok()
                            .map(|d| d.with_timezone(&chrono::Utc))
                    })
            })
            .unwrap_or(chrono::Utc::now() + chrono::Duration::minutes(15));
        state
            .repo
            .lock()
            .map_err(|_| "Database unavailable")?
            .set_setting(&key, &retry.to_rfc3339())?;
        return Err(format!("Provider requested a pause until {retry}"));
    }
    if !response.status().is_success() {
        return Err(format!(
            "Feed returned HTTP {}; redirects are not followed",
            response.status()
        ));
    }
    let mut bytes = vec![];
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "Feed download interrupted")?
    {
        if bytes.len() + chunk.len() > 4 * 1024 * 1024 {
            return Err("Feed exceeds 4 MiB".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    let xml = std::str::from_utf8(&bytes).map_err(|_| "Feed must use UTF-8")?;
    let mut reader = quick_xml::Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut stack: Vec<String> = vec![];
    let mut fields: BTreeMap<String, String> = BTreeMap::new();
    let mut entry_depth = 0;
    let mut rows = vec![];
    let fetched = command_center_core::now();
    loop {
        match reader.read_event().map_err(|_| "Malformed XML feed")? {
            Event::DocType(_) => {
                return Err("Feed document types and external entities are not allowed".into())
            }
            Event::Start(e) | Event::Empty(e) => {
                let empty = matches!(
                    xml.as_bytes()
                        .get(reader.buffer_position().saturating_sub(2) as usize),
                    Some(b'/')
                );
                let name = e.local_name().as_ref().to_string();
                if !empty {
                    stack.push(name.clone());
                    if stack.len() > 32 {
                        return Err("Feed nesting exceeds limit".into());
                    }
                }
                if name == "item" || name == "entry" {
                    fields.clear();
                    entry_depth = stack.len();
                }
                if entry_depth > 0 && name == "link" {
                    let mut href = None;
                    let mut rel = None;
                    for attribute in e.attributes() {
                        let a = attribute.map_err(|_| "Invalid feed attribute")?;
                        if a.key.as_ref() == "href" {
                            href = Some(
                                a.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                                    .map_err(|_| "Invalid link")?
                                    .to_string(),
                            );
                        }
                        if a.key.as_ref() == "rel" {
                            rel = Some(
                                a.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                                    .map_err(|_| "Invalid link")?
                                    .to_string(),
                            );
                        }
                    }
                    if rel.as_deref().is_none_or(|r| r == "alternate") {
                        if let Some(href) = href {
                            fields.insert("link".into(), href);
                        }
                    }
                }
            }
            Event::Text(e) => {
                if entry_depth > 0 {
                    let text = quick_xml::escape::unescape(e.as_ref())
                        .map_err(|_| "Unsupported XML entity")?;
                    if let Some(name) = stack.last() {
                        let field = fields.entry(name.clone()).or_default();
                        if field.len() < 16000 {
                            field.push_str(&text.chars().take(8000).collect::<String>());
                        }
                    }
                }
            }
            Event::CData(e) => {
                if entry_depth > 0 {
                    if let Some(name) = stack.last() {
                        let field = fields.entry(name.clone()).or_default();
                        if field.len() < 16000 {
                            field.push_str(&e.as_ref().chars().take(8000).collect::<String>());
                        }
                    }
                }
            }
            Event::GeneralRef(e) => {
                if entry_depth > 0 {
                    let raw = format!("&{};", e.as_ref());
                    let value =
                        quick_xml::escape::unescape(&raw).map_err(|_| "Unsupported XML entity")?;
                    if let Some(name) = stack.last() {
                        let field = fields.entry(name.clone()).or_default();
                        if field.len() < 16000 {
                            field.push_str(&value);
                        }
                    }
                }
            }
            Event::End(e) => {
                let name = e.local_name().as_ref().to_string();
                if entry_depth > 0
                    && (name == "item" || name == "entry")
                    && stack.len() == entry_depth
                {
                    let title = clean(fields.get("title").map(String::as_str).unwrap_or(""), 300);
                    let link = fields
                        .get("link")
                        .or(fields.get("id"))
                        .or(fields.get("guid"))
                        .and_then(|s| endpoint.join(s.trim()).ok())
                        .filter(|u| {
                            ["https", "http"].contains(&u.scheme())
                                && u.username().is_empty()
                                && u.password().is_none()
                        });
                    if !title.is_empty() {
                        if let Some(link) = link {
                            let description = clean(
                                fields
                                    .get("summary")
                                    .or(fields.get("description"))
                                    .or(fields.get("content"))
                                    .map(String::as_str)
                                    .unwrap_or(""),
                                1800,
                            );
                            let published = fields
                                .get("published")
                                .or(fields.get("pubDate"))
                                .or(fields.get("updated"))
                                .and_then(|s| {
                                    chrono::DateTime::parse_from_rfc3339(s)
                                        .or_else(|_| chrono::DateTime::parse_from_rfc2822(s))
                                        .ok()
                                })
                                .map(|d| d.to_rfc3339());
                            rows.push(Discovery{id:format!("{source}:{:x}",Sha256::digest(link.as_str())),title,description,url:link.into(),source:source.into(),metric:if source=="arxiv"{"Research preprint · review methods and evidence"}else{"Publisher feed · no independent verification"}.into(),published_at:published,fetched_at:fetched.clone(),verification:format!("Fetched from {} · publisher statements, not independently verified",endpoint.host_str().unwrap_or("source"))});
                        }
                    }
                    entry_depth = 0;
                    if rows.len() >= 30 {
                        break;
                    }
                }
                stack.pop();
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if rows.is_empty() {
        return Err("No supported RSS/Atom entries with public links were found".into());
    }
    Ok(rows)
}

/// DNS-pinned, public HTTPS only; never forwards cookies, proxy credentials or redirects.
pub(crate) async fn public_addresses(endpoint: &reqwest::Url) -> Result<Vec<std::net::SocketAddr>> {
    if endpoint.scheme() != "https"
        || !endpoint.username().is_empty()
        || endpoint.password().is_some()
        || endpoint.fragment().is_some()
    {
        return Err("Use an HTTPS feed URL without credentials or fragment".into());
    }
    let host = endpoint.host_str().ok_or("Feed URL has no host")?;
    let addresses = tokio::time::timeout(
        Duration::from_secs(10),
        tokio::net::lookup_host((host, endpoint.port_or_known_default().unwrap_or(443))),
    )
    .await
    .map_err(|_| "Feed DNS lookup timed out")?
    .map_err(|_| "Feed DNS lookup failed")?
    .collect::<Vec<_>>();
    if addresses.is_empty() || addresses.iter().any(|a| !public(a.ip())) {
        return Err("Feed destination must resolve only to public internet addresses".into());
    }
    Ok(addresses)
}
pub(crate) async fn public_client(endpoint: &reqwest::Url) -> Result<reqwest::Client> {
    let addresses = public_addresses(endpoint).await?;
    let host = endpoint.host_str().ok_or("URL has no host")?;
    reqwest::Client::builder()
        .https_only(true)
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .resolve_to_addrs(host, &addresses)
        .timeout(Duration::from_secs(25))
        .user_agent("PersonalCommandCenter/0.5 (user-authorized feed reader)")
        .build()
        .map_err(|e| e.to_string())
}
