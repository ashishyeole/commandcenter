use crate::{AppState, Result};
use command_center_core::{now, workspace::CachedFeed};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;
use tauri::State;

fn client(local: bool) -> Result<reqwest::Client> {
    let builder = reqwest::Client::builder()
        .user_agent("PersonalCommandCenter/0.2 (user-triggered local desktop)")
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(if local { 180 } else { 25 }));
    (if local {
        builder.no_proxy()
    } else {
        builder.https_only(true)
    })
    .build()
    .map_err(|e| e.to_string())
}
pub(crate) async fn json_response(response: reqwest::Response) -> Result<Value> {
    let response = response
        .error_for_status()
        .map_err(|e| format!("Source request failed: {}", e.without_url()))?;
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| e.without_url().to_string())?;
        if bytes.len() + chunk.len() > 8 * 1024 * 1024 {
            return Err("Source response exceeds 8 MiB".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|e| format!("Unexpected source response: {e}"))
}
#[derive(Serialize, Deserialize, Clone)]
pub struct Discovery {
    pub id: String,
    pub title: String,
    pub description: String,
    pub url: String,
    pub source: String,
    pub metric: String,
    pub published_at: Option<String>,
    pub fetched_at: String,
    pub verification: String,
}
fn text(v: &Value, k: &str) -> String {
    v[k].as_str().unwrap_or("").to_owned()
}
async fn source_json(
    state: &AppState,
    source: &str,
    epoch: u64,
    response: reqwest::Response,
) -> Result<Value> {
    let status = response.status().as_u16();
    let now = chrono::Utc::now().timestamp();
    let retry = response
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| {
            s.parse::<i64>()
                .ok()
                .map(|seconds| now.saturating_add(seconds.max(0)))
                .or_else(|| {
                    chrono::DateTime::parse_from_rfc2822(s)
                        .ok()
                        .map(|date| date.timestamp())
                })
        });
    let quota = response
        .headers()
        .get("x-ratelimit-remaining")
        .and_then(|s| s.to_str().ok())
        == Some("0");
    let reset = response
        .headers()
        .get("x-ratelimit-reset")
        .and_then(|s| s.to_str().ok())
        .and_then(|s| s.parse::<i64>().ok());
    if retry.is_some() || status == 429 || status == 503 || quota {
        let until = retry
            .or(if quota { reset } else { None })
            .unwrap_or(now + 300)
            .max(now + 5);
        let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
        if epoch != state.data_epoch.load(std::sync::atomic::Ordering::SeqCst) {
            return Err("Workspace reset during source request".into());
        }
        repo.set_setting(&format!("source_next:{source}"), &until.to_string())?;
    }
    json_response(response).await
}
#[tauri::command]
pub async fn refresh_source(
    state: State<'_, AppState>,
    source: String,
    query: String,
    consent: bool,
) -> Result<CachedFeed> {
    if !consent {
        return Err("Approve the public-source request first".into());
    }
    refresh_cached(&state, source, query, SourceOptions::default(), None).await
}
#[tauri::command]
pub async fn refresh_public_channel(
    state: State<'_, AppState>,
    channel: String,
    consent: bool,
) -> Result<CachedFeed> {
    if !consent {
        return Err("Approve reading this public YouTube channel".into());
    }
    let epoch = state.data_epoch.load(std::sync::atomic::Ordering::SeqCst);
    let rows = crate::discovery_hub::public_channel(&state, &channel, &channel).await?;
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    if state.data_epoch.load(std::sync::atomic::Ordering::SeqCst) != epoch {
        return Err("Workspace reset during public channel read".into());
    }
    repo.cache_feed("youtube", &serde_json::json!(rows))
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SourceOptions {
    pub sort: String,
    pub days: u32,
    pub language: String,
    pub task: String,
    pub license: String,
    pub region: String,
    pub duration: String,
}
impl SourceOptions {
    pub fn validate(&self) -> Result<()> {
        if ![0, 1, 7, 30, 90, 365].contains(&self.days)
            || self.language.len() > 40
            || self.task.len() > 80
            || self.license.len() > 80
            || !self
                .language
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-+#._".contains(&c))
            || !self
                .task
                .bytes()
                .chain(self.license.bytes())
                .all(|c| c.is_ascii_alphanumeric() || b"-._".contains(&c))
            || ![
                "",
                "stars",
                "forks",
                "updated",
                "downloads",
                "trendingScore",
                "likes",
                "lastModified",
                "date",
                "relevance",
                "viewCount",
                "hot",
                "new",
                "top",
            ]
            .contains(&self.sort.as_str())
            || !["", "US", "IN", "GB", "CA", "AU", "DE"].contains(&self.region.as_str())
            || !["", "any", "short", "medium", "long"].contains(&self.duration.as_str())
        {
            return Err("Invalid provider filters".into());
        }
        Ok(())
    }
}
pub(crate) async fn refresh_cached(
    state: &AppState,
    source: String,
    query: String,
    options: SourceOptions,
    cache_key: Option<String>,
) -> Result<CachedFeed> {
    options.validate()?;

    if query.len() > 300 {
        return Err("Search is too long".into());
    }
    let epoch = state.data_epoch.load(std::sync::atomic::Ordering::SeqCst);
    if ![
        "arxiv", "rss", "github", "models", "ollama", "news", "mcp", "youtube", "apps",
    ]
    .contains(&source.as_str())
    {
        return Err("Unsupported public source".into());
    }
    if !(source == "rss" && cache_key.is_some()) {
        let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
        let next = repo
            .setting(&format!("source_next:{source}"))?
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(0);
        let timestamp = chrono::Utc::now().timestamp();
        if timestamp < next {
            return Err(format!(
                "Source cooldown active: retry after {} seconds",
                next - timestamp
            ));
        }
        repo.set_setting(
            &format!("source_next:{source}"),
            &(timestamp + 5).to_string(),
        )?;
    }
    let c = client(false)?;
    let timestamp = now();
    let mut out = vec![];
    let mut attributes = std::collections::HashMap::<String, Value>::new();
    match source.as_str() {
        "arxiv" | "rss" => {
            out = crate::research::fetch(&state, &source, &query).await?;
        }
        "github" => {
            let mut q = if query.trim().is_empty() {
                format!(
                    "created:>{}",
                    (chrono::Utc::now() - chrono::Duration::days(30)).format("%Y-%m-%d")
                )
            } else {
                query.clone()
            };
            if options.days > 0 {
                q.push_str(&format!(
                    " created:>{}",
                    (chrono::Utc::now() - chrono::Duration::days(options.days.into()))
                        .format("%Y-%m-%d")
                ));
            }
            if !options.language.is_empty() {
                q.push_str(&format!(" language:{}", options.language));
            }
            if !options.license.is_empty() {
                q.push_str(&format!(" license:{}", options.license));
            }
            let gh_sort = if ["stars", "forks", "updated"].contains(&options.sort.as_str()) {
                options.sort.as_str()
            } else {
                "stars"
            };
            let v = source_json(
                &state,
                &source,
                epoch,
                c.get("https://api.github.com/search/repositories")
                    .header("Accept", "application/vnd.github+json")
                    .query(&[
                        ("q", q.as_str()),
                        ("sort", gh_sort),
                        ("order", "desc"),
                        ("per_page", "30"),
                    ])
                    .send()
                    .await
                    .map_err(|e| e.to_string())?,
            )
            .await?;
            for r in v["items"]
                .as_array()
                .ok_or("GitHub returned no results array")?
            {
                attributes.insert(format!("github:{}",r["id"]),json!({"language":r["language"],"license":r["license"]["spdx_id"],"stars":r["stargazers_count"],"forks":r["forks_count"],"topics":r["topics"],"updated_at":r["updated_at"],"provider_order":gh_sort}));
                out.push(Discovery {
                    id: format!("github:{}", r["id"]),
                    title: text(r, "full_name"),
                    description: text(r, "description"),
                    url: text(r, "html_url"),
                    source: source.clone(),
                    metric: format!("{} stars · {}", r["stargazers_count"], text(r, "language")),
                    published_at: r["created_at"].as_str().map(str::to_owned),
                    fetched_at: timestamp.clone(),
                    verification: "GitHub API · star count, not growth velocity".into(),
                });
            }
        }
        "ollama" => {
            out = crate::model_catalogue::fetch(&query, &timestamp).await?;
        }
        "models" => {
            let hf_sort = if ["downloads", "trendingScore", "likes", "lastModified"]
                .contains(&options.sort.as_str())
            {
                options.sort.as_str()
            } else {
                "downloads"
            };
            let mut request = c.get("https://huggingface.co/api/models").query(&[
                ("search", query.as_str()),
                ("sort", hf_sort),
                ("direction", "-1"),
                ("limit", "30"),
                ("full", "true"),
            ]);
            let mut filters = Vec::new();
            if !options.task.is_empty() {
                request = request.query(&[("pipeline_tag", options.task.as_str())]);
            }
            if !options.language.is_empty() {
                filters.push(options.language.clone());
            }
            if !options.license.is_empty() {
                filters.push(format!("license:{}", options.license));
            }
            if !filters.is_empty() {
                request = request.query(
                    &filters
                        .iter()
                        .map(|f| ("filter", f.as_str()))
                        .collect::<Vec<_>>(),
                );
            }
            let v = source_json(
                &state,
                &source,
                epoch,
                request.send().await.map_err(|e| e.to_string())?,
            )
            .await?;
            for r in v.as_array().ok_or("Model API returned no results array")? {
                let id = text(r, "id");
                attributes.insert(format!("hf:{id}"),json!({"task":r["pipeline_tag"],"library":r["library_name"],"downloads":r["downloads"],"likes":r["likes"],"tags":r["tags"],"license":r["tags"].as_array().and_then(|tags|tags.iter().filter_map(Value::as_str).find_map(|t|t.strip_prefix("license:"))),"provider_order":hf_sort}));
                out.push(Discovery {
                    id: format!("hf:{id}"),
                    title: id.clone(),
                    description: text(r, "pipeline_tag"),
                    url: format!("https://huggingface.co/{id}"),
                    source: source.clone(),
                    metric: format!("{} downloads · {} likes", r["downloads"], r["likes"]),
                    published_at: r["lastModified"].as_str().or(r["last_modified"].as_str()).map(str::to_owned),
                    fetched_at: timestamp.clone(),
                    verification: format!("Hugging Face API · provider rank: {hf_sort}; metadata only, no model download or inference"),
                });
            }
        }
        "news" => {
            let endpoint = if options.sort == "date" {
                "https://hn.algolia.com/api/v1/search_by_date"
            } else {
                "https://hn.algolia.com/api/v1/search"
            };
            let mut request = c.get(endpoint).query(&[
                ("query", query.as_str()),
                ("tags", "story"),
                ("hitsPerPage", "30"),
            ]);
            if options.days > 0 {
                request = request.query(&[(
                    "numericFilters",
                    format!(
                        "created_at_i>{}",
                        (chrono::Utc::now() - chrono::Duration::days(options.days.into()))
                            .timestamp()
                    ),
                )]);
            }
            let v = source_json(
                &state,
                &source,
                epoch,
                request.send().await.map_err(|e| e.to_string())?,
            )
            .await?;
            for r in v["hits"]
                .as_array()
                .ok_or("News API returned no results array")?
            {
                let id = text(r, "objectID");
                out.push(Discovery {
                    id: format!("hn:{id}"),
                    title: text(r, "title"),
                    description: format!("Discussion by {}", text(r, "author")),
                    url: format!("https://news.ycombinator.com/item?id={id}"),
                    source: source.clone(),
                    metric: format!("{} points · {} comments", r["points"], r["num_comments"]),
                    published_at: r["created_at"].as_str().map(str::to_owned),
                    fetched_at: timestamp.clone(),
                    verification: "HN search API · community report, not independently verified"
                        .into(),
                });
            }
        }
        "mcp" => {
            let v = source_json(
                &state,
                &source,
                epoch,
                c.get("https://registry.modelcontextprotocol.io/v0.1/servers")
                    .query(&[("limit", "30"), ("search", query.as_str())])
                    .send()
                    .await
                    .map_err(|e| e.to_string())?,
            )
            .await?;
            for r in v["servers"]
                .as_array()
                .ok_or("MCP registry returned no servers array")?
            {
                let s = &r["server"];
                let name = text(s, "name");
                let mut registry_url =
                    reqwest::Url::parse("https://registry.modelcontextprotocol.io/v0.1/servers")
                        .map_err(|e| e.to_string())?;
                registry_url
                    .query_pairs_mut()
                    .append_pair("search", &name)
                    .append_pair("limit", "30");
                let url = registry_url.to_string();
                attributes.insert(format!("mcp:{name}"),json!({"registry_name":name,"version":s["version"],"description":s["description"],"repository":s["repository"],"website":s["websiteUrl"],"remotes":s["remotes"],"packages":s["packages"],"notice":"Public registry declarations. This lists available integrations; connecting or running tools remains separately approved."}));
                out.push(Discovery {
                    id: format!("mcp:{name}"),
                    title: name,
                    description: text(s, "description"),
                    url,
                    source: source.clone(),
                    metric: format!("Version {}", text(s, "version")),
                    published_at: None,
                    fetched_at: timestamp.clone(),
                    verification: "Registry listing only · publisher and tool safety not audited"
                        .into(),
                });
            }
        }
        "youtube" => {
            if state
                .repo
                .lock()
                .map_err(|_| "Database unavailable")?
                .setting("youtube_enabled")?
                .as_deref()
                != Some("true")
            {
                return Err("Configure YouTube keyword-search access on this device first; public channel feeds need no key".into());
            }
            let key = zeroize::Zeroizing::new(
                keyring::Entry::new("com.personal.commandcenter.api", "youtube")
                    .map_err(|e| e.to_string())?
                    .get_password()
                    .map_err(|_| "Add a YouTube Data API key in Settings first")?,
            );
            if let Some(handle) = query.trim().strip_prefix("channel:") {
                let handle = handle.trim();
                if handle.is_empty()
                    || handle.len() > 100
                    || handle.chars().any(char::is_whitespace)
                {
                    return Err("Enter channel:@handle to inspect one public creator".into());
                }
                let v = source_json(
                    &state,
                    &source,
                    epoch,
                    c.get("https://www.googleapis.com/youtube/v3/channels")
                        .query(&[
                            ("key", key.as_str()),
                            ("part", "snippet,statistics"),
                            ("forHandle", handle),
                        ])
                        .send()
                        .await
                        .map_err(|_| "YouTube channel request failed")?,
                )
                .await?;
                for row in v["items"]
                    .as_array()
                    .ok_or("Channel API returned no items")?
                {
                    let snippet = &row["snippet"];
                    let stats = &row["statistics"];
                    let subscribers = if stats["hiddenSubscriberCount"] == false {
                        stats["subscriberCount"]
                            .as_str()
                            .filter(|s| s.parse::<u64>().is_ok())
                            .unwrap_or("unknown")
                    } else {
                        "hidden"
                    };
                    let id = text(row, "id");
                    out.push(Discovery {id:format!("youtube-channel:{id}"),title:text(snippet,"title"),description:text(snippet,"description"),url:format!("https://www.youtube.com/channel/{id}"),source:source.clone(),metric:format!("{subscribers} subscribers (provider rounded) · {} views · {} videos",stats["viewCount"].as_str().unwrap_or("unknown"),stats["videoCount"].as_str().unwrap_or("unknown")),published_at:snippet["publishedAt"].as_str().map(str::to_owned),fetched_at:timestamp.clone(),verification:"YouTube API public channel snapshot. Subscribers may be rounded/hidden; observed changes are not exact audience growth or a forecast.".into()});
                }
            } else {
                let order = if ["date", "relevance", "viewCount"].contains(&options.sort.as_str()) {
                    options.sort.as_str()
                } else {
                    "date"
                };
                let mut request = c
                    .get("https://www.googleapis.com/youtube/v3/search")
                    .query(&[
                        ("key", key.as_str()),
                        ("part", "snippet"),
                        ("type", "video"),
                        ("q", query.as_str()),
                        ("order", order),
                        ("maxResults", "25"),
                    ]);
                if options.days > 0 {
                    request = request.query(&[(
                        "publishedAfter",
                        (chrono::Utc::now() - chrono::Duration::days(options.days.into()))
                            .to_rfc3339(),
                    )]);
                }
                if !options.region.is_empty() {
                    request = request.query(&[("regionCode", &options.region)]);
                }
                if !options.duration.is_empty() {
                    request = request.query(&[("videoDuration", &options.duration)]);
                }
                let v = source_json(
                    &state,
                    &source,
                    epoch,
                    request.send().await.map_err(|_| "YouTube request failed")?,
                )
                .await?;
                for r in v["items"]
                    .as_array()
                    .ok_or("YouTube returned no results array")?
                {
                    let id = r["id"]["videoId"].as_str().unwrap_or("");
                    let snippet = &r["snippet"];
                    out.push(Discovery {
                        id: format!("youtube:{id}"),
                        title: text(snippet, "title"),
                        description: text(snippet, "description"),
                        url: format!("https://www.youtube.com/watch?v={id}"),
                        source: source.clone(),
                        metric: text(snippet, "channelTitle"),
                        published_at: snippet["publishedAt"].as_str().map(str::to_owned),
                        fetched_at: timestamp.clone(),
                        verification:
                            "YouTube API · newest matching videos, not verified growth trends"
                                .into(),
                    });
                }
            }
        }
        "apps" => {
            let country = if query.trim().is_empty() {
                "us".to_owned()
            } else {
                query.trim().to_lowercase()
            };
            if country.len() != 2 || !country.bytes().all(|b| b.is_ascii_lowercase()) {
                return Err("Enter a two-letter storefront code, such as us or in".into());
            }
            let v=source_json(&state, &source, epoch, c.get(format!("https://rss.marketingtools.apple.com/api/v2/{country}/apps/top-free/50/apps.json")).send().await.map_err(|e|e.to_string())?).await?;
            for (rank, r) in v["feed"]["results"]
                .as_array()
                .ok_or("Apple returned no chart results")?
                .iter()
                .enumerate()
            {
                out.push(Discovery {
                    id: format!("apple:{}", text(r, "id")),
                    title: text(r, "name"),
                    description: text(r, "artistName"),
                    url: text(r, "url"),
                    source: source.clone(),
                    metric: format!("#{} · top free · {}", rank + 1, country.to_uppercase()),
                    published_at: r["releaseDate"].as_str().map(str::to_owned),
                    fetched_at: timestamp.clone(),
                    verification:
                        "Apple chart snapshot · rank, not download estimate or growth velocity"
                            .into(),
                });
            }
        }
        _ => return Err("Unsupported public source".into()),
    }
    let mut repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    if state.data_epoch.load(std::sync::atomic::Ordering::SeqCst) != epoch {
        return Err("Data was reset during request".into());
    }
    let mut payload = serde_json::to_value(out).map_err(|e| e.to_string())?;
    for item in payload.as_array_mut().into_iter().flatten() {
        if let Some(meta) = item["id"].as_str().and_then(|id| attributes.get(id)) {
            item["metadata"] = meta.clone();
        }
    }
    let cache_source = cache_key.as_deref().unwrap_or(&source);
    let fingerprint = serde_json::to_string(&json!([query, options])).map_err(|e| e.to_string())?;
    let same_query = repo
        .setting(&format!("feed_query:{cache_source}"))?
        .as_deref()
        == Some(fingerprint.as_str());
    if (source == "github"
        || source == "models"
        || (source == "youtube" && query.trim().starts_with("channel:")))
        && same_query
    {
        if let Some(prior) = repo.feeds()?.into_iter().find(|f| f.source == cache_source) {
            if let (Ok(a), Ok(b)) = (
                chrono::DateTime::parse_from_rfc3339(&prior.fetched_at),
                chrono::DateTime::parse_from_rfc3339(&timestamp),
            ) {
                let hours = (b - a).num_seconds() as f64 / 3600.0;
                if hours >= 1.0 / 60.0 {
                    for item in payload.as_array_mut().into_iter().flatten() {
                        if let Some(old) = prior
                            .items
                            .as_array()
                            .and_then(|items| items.iter().find(|o| o["id"] == item["id"]))
                        {
                            let count = |v: &Value| {
                                v["metric"]
                                    .as_str()
                                    .and_then(|s| s.split_whitespace().next())
                                    .and_then(|s| s.parse::<i64>().ok())
                            };
                            if let (Some(before), Some(after)) = (count(old), count(item)) {
                                item["trend"] = json!({"unit":if source=="github" {"stars"} else if source=="models" {"reported download-count change (provider window)"} else {"reported subscribers"},"delta":after-before,"hours":hours,"per_day":(after-before) as f64/hours*24.0,"observed_from":prior.fetched_at,"observed_to":timestamp});
                            }
                        }
                    }
                }
            }
        }
    }
    repo.set_setting(&format!("feed_query:{cache_source}"), &fingerprint)?;
    if cache_key.is_some() {
        return Ok(CachedFeed {
            source: cache_source.into(),
            fetched_at: timestamp,
            items: payload,
        });
    }
    repo.cache_research_query(&source, &query, &payload)?;
    let feed = repo.cache_feed(cache_source, &payload)?;
    repo.evaluate_watchlists()?;
    Ok(feed)
}
#[tauri::command]
pub fn cancel_ai(state: State<AppState>, request_id: String) -> Result<()> {
    if let Some(token) = state
        .jobs
        .lock()
        .map_err(|_| "AI unavailable")?
        .get(&request_id)
    {
        token.cancel();
    }
    Ok(())
}

#[tauri::command]
pub fn save_youtube_key(state: State<AppState>, value: String) -> Result<()> {
    let value = zeroize::Zeroizing::new(value);
    if value.len() < 20 || value.len() > 256 || value.chars().any(char::is_whitespace) {
        return Err("Enter a valid YouTube API key".into());
    }
    keyring::Entry::new("com.personal.commandcenter.api", "youtube")
        .map_err(|e| e.to_string())?
        .set_password(&value)
        .map_err(|_| "OS secure store could not save the API key".to_string())?;
    state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .set_integration_flag("youtube_enabled", true)
}
#[tauri::command]
pub fn remove_youtube_key(state: State<AppState>) -> Result<()> {
    state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .set_integration_flag("youtube_enabled", false)?;
    match keyring::Entry::new("com.personal.commandcenter.api", "youtube")
        .map_err(|e| e.to_string())?
        .delete_credential()
    {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(_) => Err("OS secure store could not remove the API key".into()),
    }
}
