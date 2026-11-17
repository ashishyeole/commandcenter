//! Keyless Apple-owned Top Free chart feeds. Never substitute a different storefront.
use crate::{connectors::Discovery, AppState, Result};
use serde_json::{json, Value};
use std::{collections::HashSet, sync::atomic::Ordering, time::Duration};

pub(crate) struct Chart {
    pub items: Vec<Discovery>,
    pub metadata: Value,
}

fn endpoints(country: &str) -> Result<[String; 2]> {
    if country.len() != 2 || !country.bytes().all(|b| b.is_ascii_lowercase()) {
        return Err("Enter a two-letter storefront code, such as us or in".into());
    }
    Ok([
        format!("https://itunes.apple.com/{country}/rss/topfreeapplications/limit=50/json"),
        format!("https://rss.marketingtools.apple.com/api/v2/{country}/apps/top-free/50/apps.json"),
    ])
}

pub(crate) async fn fetch(state: &AppState, country: &str, fetched_at: &str) -> Result<Chart> {
    let urls = endpoints(country)?;
    let client = reqwest::Client::builder()
        .user_agent("PersonalCommandCenter/0.5 (public Apple chart reader)")
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(12))
        .build()
        .map_err(|e| e.to_string())?;
    fetch_from(&client, country, fetched_at, &urls, Some(state)).await
}

// Only transport/timeouts and transient gateway/server failures allow fallback.
// Authorization/quota responses (including Retry-After) stop, never rotate hosts.
fn may_fallback(status: reqwest::StatusCode, retry_after: bool) -> bool {
    !retry_after && [500, 502, 503, 504].contains(&status.as_u16())
}

async fn fetch_from(
    client: &reqwest::Client,
    country: &str,
    fetched_at: &str,
    urls: &[String; 2],
    state: Option<&AppState>,
) -> Result<Chart> {
    let epoch = state.map(|s| s.data_epoch.load(Ordering::SeqCst));
    let mut failures = vec![];
    for (index, url) in urls.iter().enumerate() {
        let response = match client.get(url).send().await {
            Ok(response) => response,
            Err(error) => {
                failures.push(format!("Apple feed {}: {}", index + 1, error.without_url()));
                continue;
            }
        };
        let status = response.status();
        let retry_after = response
            .headers()
            .contains_key(reqwest::header::RETRY_AFTER);
        if retry_after || status.as_u16() == 429 {
            if let Some(state) = state {
                let now = chrono::Utc::now().timestamp();
                let until = response
                    .headers()
                    .get(reqwest::header::RETRY_AFTER)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| {
                        v.parse::<i64>()
                            .ok()
                            .map(|seconds| now.saturating_add(seconds.max(0)))
                            .or_else(|| {
                                chrono::DateTime::parse_from_rfc2822(v)
                                    .ok()
                                    .map(|d| d.timestamp())
                            })
                    })
                    .unwrap_or(now + 300)
                    .max(now + 5);
                let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
                if epoch != Some(state.data_epoch.load(Ordering::SeqCst)) {
                    return Err("Workspace reset during Apple request".into());
                }
                repo.set_setting("source_next:apps", &until.to_string())?;
            }
        }
        if !status.is_success() {
            let error = format!(
                "Apple chart request failed: {status}{}",
                if retry_after {
                    " (Retry-After; wait before refreshing)"
                } else {
                    ""
                }
            );
            if may_fallback(status, retry_after) {
                failures.push(error);
                continue;
            }
            return Err(error);
        }
        // Body/JSON/schema errors are explicit; no silent partial chart or rank changes.
        let value = crate::connectors::json_response(response).await?;
        let mut chart = parse(&value, country, fetched_at, url)?;
        chart.metadata["fallback_used"] = json!(index > 0);
        if !failures.is_empty() {
            chart.metadata["earlier_feed_errors"] = json!(failures);
        }
        return Ok(chart);
    }
    Err(format!(
        "Apple chart feeds unavailable; previous cache retained. {}",
        failures.join("; ")
    ))
}

fn parse(v: &Value, country: &str, fetched_at: &str, endpoint: &str) -> Result<Chart> {
    let urls = endpoints(country)?;
    let legacy = endpoint == urls[0] || v["feed"]["entry"].is_array();
    let feed = &v["feed"];
    let (rows, updated) = if legacy {
        let expected = &urls[0];
        if feed["id"]["label"].as_str() != Some(expected.as_str()) {
            return Err("Apple chart storefront/feed identity mismatch".into());
        }
        (feed["entry"].as_array(), feed["updated"]["label"].as_str())
    } else {
        if feed["country"].as_str() != Some(country)
            || feed["id"].as_str() != Some(urls[1].as_str())
        {
            return Err("Apple chart storefront/feed identity mismatch".into());
        }
        (feed["results"].as_array(), feed["updated"].as_str())
    };
    let rows = rows
        .filter(|rows| !rows.is_empty() && rows.len() <= 50)
        .ok_or("Apple returned an empty or invalid Top Free chart")?;
    let mut ids = HashSet::new();
    let mut items = Vec::with_capacity(rows.len());
    for (rank, row) in rows.iter().enumerate() {
        let (id, title, artist, url, release) = if legacy {
            (
                row["id"]["attributes"]["im:id"].as_str(),
                row["im:name"]["label"].as_str(),
                row["im:artist"]["label"].as_str(),
                row["id"]["label"].as_str(),
                row["im:releaseDate"]["label"].as_str(),
            )
        } else {
            (
                row["id"].as_str(),
                row["name"].as_str(),
                row["artistName"].as_str(),
                row["url"].as_str(),
                row["releaseDate"].as_str(),
            )
        };
        let id = id
            .filter(|s| !s.is_empty() && s.len() <= 24 && s.bytes().all(|b| b.is_ascii_digit()))
            .ok_or("Apple returned an invalid app ID")?;
        let title = title
            .filter(|s| !s.trim().is_empty())
            .ok_or("Apple returned an unnamed app")?;
        let url = reqwest::Url::parse(url.unwrap_or(""))
            .map_err(|_| "Apple returned an invalid app URL")?;
        if url.scheme() != "https"
            || url.host_str() != Some("apps.apple.com")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
            || !url.path().starts_with(&format!("/{country}/app/"))
            || !url.path().ends_with(&format!("/id{id}"))
            || !ids.insert(id.to_owned())
        {
            return Err("Apple returned a mismatched or duplicate app listing".into());
        }
        items.push(Discovery {
            id: format!("apple:{country}:{id}"),
            title: title.into(),
            description: artist.unwrap_or("").into(),
            url: url.to_string(),
            source: "apps".into(),
            metric: format!("#{} · top free · {}", rank + 1, country.to_uppercase()),
            published_at: release.map(str::to_owned),
            fetched_at: fetched_at.into(),
            verification: format!(
                "Apple {} chart snapshot · rank, not download estimate or growth velocity",
                if legacy {
                    "iTunes RSS"
                } else {
                    "Marketing Tools"
                }
            ),
        });
    }
    Ok(Chart {
        items,
        metadata: json!({"feed_url":endpoint,"storefront":country.to_uppercase(),"chart":"top-free","feed_updated":updated,"fallback_used":false}),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn legacy(country: &str) -> Value {
        json!({"feed":{"id":{"label":endpoints(country).unwrap()[0]},"updated":{"label":"2026-11-17T01:00:00-07:00"},"entry":[{"id":{"label":format!("https://apps.apple.com/{country}/app/example/id123?uo=2"),"attributes":{"im:id":"123"}},"im:name":{"label":"Example"},"im:artist":{"label":"Publisher"},"im:releaseDate":{"label":"2026-01-01"}}]}})
    }
    fn modern(country: &str) -> Value {
        json!({"feed":{"id":endpoints(country).unwrap()[1],"country":country,"updated":"Tue, 17 Nov 2026 08:00:00 +0000","results":[{"id":"123","name":"Example","artistName":"Publisher","url":format!("https://apps.apple.com/{country}/app/example/id123"),"releaseDate":"2026-01-01"}]}})
    }
    #[test]
    fn both_official_schemas_preserve_identity_rank_storefront_and_provenance() {
        for country in ["us", "in"] {
            for (v, url) in [
                (legacy(country), endpoints(country).unwrap()[0].clone()),
                (modern(country), endpoints(country).unwrap()[1].clone()),
            ] {
                let chart = parse(&v, country, "fetched", &url).unwrap();
                assert_eq!(chart.items[0].id, format!("apple:{country}:123"));
                assert_eq!(
                    chart.items[0].metric,
                    format!("#1 · top free · {}", country.to_uppercase())
                );
                assert_eq!(chart.items[0].fetched_at, "fetched");
                assert_eq!(chart.metadata["feed_url"], url);
                assert!(chart.metadata["feed_updated"].is_string());
            }
        }
        for invalid in ["US", "../us", "", "us?x=1"] {
            assert!(endpoints(invalid).is_err());
        }
    }
    #[test]
    fn rejects_wrong_storefront_duplicates_invalid_urls_and_partial_rows() {
        let url = endpoints("us").unwrap()[0].clone();
        assert!(parse(&legacy("in"), "us", "now", &url).is_err());
        let mut v = legacy("us");
        v["feed"]["entry"][0]["id"]["label"] = json!("https://evil.example/us/app/example/id123");
        assert!(parse(&v, "us", "now", &url).is_err());
        let mut v = legacy("us");
        let duplicate = v["feed"]["entry"][0].clone();
        v["feed"]["entry"].as_array_mut().unwrap().push(duplicate);
        assert!(parse(&v, "us", "now", &url).is_err());
        for rows in [
            json!([]),
            json!([{}]),
            json!(vec![legacy("us")["feed"]["entry"][0].clone(); 51]),
        ] {
            let mut v = legacy("us");
            v["feed"]["entry"] = rows;
            assert!(parse(&v, "us", "now", &url).is_err());
        }
        let mut v = modern("us");
        v["feed"]["country"] = json!("in");
        assert!(parse(&v, "us", "now", &endpoints("us").unwrap()[1]).is_err());
    }
    #[test]
    fn fallback_never_rotates_on_access_limits_or_retry_after() {
        for status in [200, 301, 400, 401, 403, 404, 429] {
            assert!(!may_fallback(
                reqwest::StatusCode::from_u16(status).unwrap(),
                false
            ));
        }
        for status in [500, 502, 503, 504] {
            assert!(may_fallback(
                reqwest::StatusCode::from_u16(status).unwrap(),
                false
            ));
            assert!(!may_fallback(
                reqwest::StatusCode::from_u16(status).unwrap(),
                true
            ));
        }
    }

    fn state(dir: &std::path::Path) -> AppState {
        use std::sync::{atomic::AtomicU64, Mutex};
        AppState {
            repo: Mutex::new(command_center_core::Repository::open(dir.join("qa.db")).unwrap()),
            peer_sync: Mutex::new(crate::peer_sync::Runtime::default()),
            project_watchers: Mutex::new(Default::default()),
            pending_update: Mutex::new(None),
            automation_lock: tokio::sync::Mutex::new(()),
            backup_lock: tokio::sync::Mutex::new(()),
            data_epoch: AtomicU64::new(0),
            pending: Mutex::new(None),
            large_import: Mutex::new(None),
            google_cache: Mutex::new(None),
            vault: Mutex::new(None),
            jobs: Mutex::new(Default::default()),
        }
    }

    async fn server(
        status: u16,
        headers: &str,
        body: Value,
    ) -> (
        String,
        std::sync::Arc<std::sync::atomic::AtomicUsize>,
        tokio::task::JoinHandle<()>,
    ) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/chart", listener.local_addr().unwrap());
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = calls.clone();
        let body = body.to_string();
        let headers = headers.to_owned();
        let task = tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let mut request = [0; 4096];
                socket.read(&mut request).await.unwrap();
                count.fetch_add(1, Ordering::SeqCst);
                let response=format!("HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n{body}",body.len());
                socket.write_all(response.as_bytes()).await.unwrap();
            }
        });
        (url, calls, task)
    }

    #[tokio::test]
    async fn actual_gateway_failure_uses_one_fallback_and_labels_it() {
        let (a, ac, at) = server(502, "", json!({})).await;
        let (b, bc, bt) = server(200, "", modern("us")).await;
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let chart = fetch_from(&client, "us", "now", &[a, b.clone()], None)
            .await
            .unwrap();
        assert_eq!(chart.items[0].id, "apple:us:123");
        assert_eq!(chart.metadata["feed_url"], b);
        assert_eq!(chart.metadata["fallback_used"], true);
        assert_eq!(
            chart.metadata["earlier_feed_errors"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(ac.load(Ordering::SeqCst), 1);
        assert_eq!(bc.load(Ordering::SeqCst), 1);
        at.abort();
        bt.abort();
    }

    #[tokio::test]
    async fn actual_refusals_stop_and_quota_pause_is_persisted() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(dir.path());
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        for (status, headers) in [
            (403, ""),
            (429, "Retry-After: 120\r\n"),
            (503, "Retry-After: 120\r\n"),
        ] {
            let (a, ac, at) = server(status, headers, json!({})).await;
            let (b, bc, bt) = server(200, "", modern("us")).await;
            let before = chrono::Utc::now().timestamp();
            assert!(fetch_from(&client, "us", "now", &[a, b], Some(&state))
                .await
                .is_err());
            assert_eq!(ac.load(Ordering::SeqCst), 1);
            assert_eq!(bc.load(Ordering::SeqCst), 0);
            if status != 403 {
                assert!(
                    state
                        .repo
                        .lock()
                        .unwrap()
                        .setting("source_next:apps")
                        .unwrap()
                        .unwrap()
                        .parse::<i64>()
                        .unwrap()
                        >= before + 120
                );
            }
            at.abort();
            bt.abort();
        }
    }

    #[tokio::test]
    #[ignore = "Live Apple public feed reads through isolated native hub; no account/key"]
    async fn live_apple_charts() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(dir.path());
        let mut runs = vec![];
        for (id, country) in [("app-charts-us", "US"), ("app-charts-india", "IN")] {
            let result = crate::discovery_hub::refresh(&state, id).await.unwrap();
            let rows = result["feed"]["items"].as_array().unwrap();
            assert_eq!(rows.len(), 50);
            assert_eq!(result["health"]["status"], "ok");
            for (rank, row) in rows.iter().enumerate() {
                assert_eq!(
                    row["metric"],
                    format!("#{} · top free · {country}", rank + 1)
                );
                assert_eq!(row["metadata"]["storefront"], country);
                assert!(row["metadata"]["feed_url"]
                    .as_str()
                    .unwrap()
                    .starts_with("https://"));
            }
            let cached = state
                .repo
                .lock()
                .unwrap()
                .feeds()
                .unwrap()
                .into_iter()
                .find(|f| f.source == format!("hub:{id}"))
                .unwrap();
            assert_eq!(cached.items, result["feed"]["items"]);
            runs.push(json!({"id":id,"health":result["health"],"count":rows.len(),"metadata":rows[0]["metadata"],"samples":rows.iter().take(5).collect::<Vec<_>>(),"cache_matches_live_response":true}));
        }
        let out =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../target/live-apple");
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(out.join("results.json"),serde_json::to_string_pretty(&json!({"checked_at":command_center_core::now(),"isolated_temp_database":true,"credentials_used":false,"app_login_used":false,"runs":runs})).unwrap()).unwrap();
    }
}
