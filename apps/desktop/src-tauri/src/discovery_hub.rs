//! Topic-first public aggregation. No model weights, cookies, private timelines or tool execution.
use crate::{
    connectors::{Discovery, SourceOptions},
    AppState, Result,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::atomic::Ordering;
use tauri::State;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stream {
    pub id: String,
    pub name: String,
    pub topic: String,
    pub provider: String,
    pub query: String,
    pub kind: String,
    pub region: String,
    pub enabled: bool,
    #[serde(default)]
    pub options: SourceOptions,
}
fn stream(
    id: &str,
    name: &str,
    topic: &str,
    provider: &str,
    query: &str,
    kind: &str,
    region: &str,
) -> Stream {
    Stream {
        id: id.into(),
        name: name.into(),
        topic: topic.into(),
        provider: provider.into(),
        query: query.into(),
        kind: kind.into(),
        region: region.into(),
        enabled: true,
        options: SourceOptions::default(),
    }
}
pub(crate) fn defaults() -> Vec<Stream> {
    let mut rows=vec![
 stream("hf-trending","Trending models","ai","models","","models","world"),
 stream("hf-downloads","Most downloaded models","ai","models","","models","world"),
 stream("ollama-catalogue","Ollama model catalogue","ai","ollama","","models","world"),
 stream("github-top","Top public repositories","code","github","stars:>1000 archived:false","repos","world"),
 stream("github-new","New repositories this month","code","github","stars:>20 archived:false","repos","world"),
 stream("robotics-repos","Robotics libraries","robotics","github","topic:robotics archived:false","repos","world"),
 stream("robotics-papers","Robotics papers","robotics","arxiv","cat:cs.RO","papers","world"),
 stream("robotics-news","IEEE robotics news","robotics","rss","https://spectrum.ieee.org/feeds/topic/robotics.rss","news","world"),
 stream("robotics-models","Robotics models","robotics","models","robotics","models","world"),
 stream("agri-news","USDA research archive","agriculture","rss","https://www.ars.usda.gov/rss/?productName=Research%20News","news","US"),
 stream("agri-discussion","Agri-tech discussions","agriculture","news","agriculture technology","news","world"),
 stream("agritech-india","AgriTech India events","agriculture","events","https://agritechindia.com/","events","IN"),
 stream("agritechnica","AGRITECHNICA calendar","agriculture","calendar","https://www.agritechnica.com/fileadmin/redaktion/downloads/2027/AGRITECHNICA_2027_The_world_s_leading_trade_fair_for_agricultural_machinery.ics","events","world"),
 stream("world-ag-expo","World Ag Expo","agriculture","events","https://www.worldagexpo.com/","events","US"),
 stream("poker-news","PokerNews headlines","poker","rss","https://www.pokernews.com/rss.php","news","world"),
 stream("poker-wpt","WPT tournament schedule","poker","events","https://www.worldpokertour.com/event/schedule","events","world"),
 stream("poker-wsop","WSOP tournament schedule","poker","events","https://www.wsop.com/schedule/","events","world"),
 stream("poker-stars","PokerStars live festivals","poker","events","https://www.pokerstarslive.com/","events","world"),
 stream("poker-all","Multi-organizer tournament calendar","poker","events","https://www.cardplayer.com/poker-tournaments","events","world"),
 stream("stanford-lectures","Stanford Online lectures","learning","youtube_feed","UCBa5G_ESCn8Yd4vw5U-gIcg","videos","world"),
 stream("mit-robotics-videos","MIT CSAIL research videos","robotics","youtube_feed","UCBpxspUNl1Th33XbugiHJzw","videos","world"),
 stream("agri-videos","John Deere technology videos","agriculture","youtube_feed","@JohnDeere","videos","world"),
 stream("poker-videos","PokerStars videos","poker","youtube_feed","@PokerStars","videos","world"),
 stream("ai-videos","Hugging Face videos","ai","youtube_feed","@huggingface","videos","world"),
 stream("mit-lectures","MIT OpenCourseWare lectures","learning","youtube_feed","UCEBb1b_L6zDS3xTUrIALZOw","videos","world"),
 stream("robotics-reddit","Reddit robotics","robotics","reddit","robotics","social","world"),
 stream("ai-reddit","Reddit LocalLLaMA","ai","reddit","LocalLLaMA","social","world"),
 stream("poker-reddit","Reddit poker","poker","reddit","poker","social","world"),
 stream("robotics-bluesky","Bluesky robotics","robotics","bluesky","robotics","social","world"),
 stream("agri-bluesky","Bluesky agri-tech","agriculture","bluesky","agritech","social","world"),
 stream("robotics-mastodon","Mastodon robotics","robotics","mastodon","robotics","social","world"),
 stream("ai-news","AI community news","ai","news","artificial intelligence","news","world"),
 stream("x-robotics","X robotics search","robotics","x","robotics -is:retweet","social","world"),
 stream("linkedin","LinkedIn source import","code","linkedin","","social","world"),
 ];
    rows.push(stream(
        "agfunder-news",
        "AgFunderNews agri-food technology",
        "agriculture",
        "rss",
        "https://agfundernews.com/feed",
        "news",
        "world",
    ));
    rows.push(stream(
        "agdaily-news",
        "AGDAILY agriculture news",
        "agriculture",
        "rss",
        "https://www.agdaily.com/feed/",
        "news",
        "US",
    ));
    rows.extend(research_starters());
    rows.extend([
        stream(
            "app-charts-us",
            "Top free iPhone apps · US",
            "apps",
            "apps",
            "us",
            "apps",
            "US",
        ),
        stream(
            "app-charts-india",
            "Top free iPhone apps · India",
            "apps",
            "apps",
            "in",
            "apps",
            "IN",
        ),
        stream(
            "mcp-public-registry",
            "Public MCP tools",
            "apps",
            "mcp",
            "",
            "tools",
            "world",
        ),
    ]);
    let archive = rows.iter_mut().find(|s| s.id == "agri-news").unwrap();
    archive.enabled = false;
    for s in rows.iter_mut().filter(|s| s.provider == "news") {
        s.options.sort = "date".into();
        s.options.days = 30;
    }
    rows[0].options.sort = "trendingScore".into();
    rows[1].options.sort = "downloads".into();
    rows[3].options.sort = "stars".into();
    rows[4].options.days = 30;
    rows.iter_mut()
        .filter(|s| ["x", "linkedin", "reddit"].contains(&s.provider.as_str()))
        .for_each(|s| s.enabled = false);
    rows
}
fn research_starters() -> Vec<Stream> {
    let mut rows = Vec::from([
        stream(
            "crossref-robotics",
            "Crossref robotics research",
            "robotics",
            "crossref",
            "robotics",
            "papers",
            "world",
        ),
        stream(
            "epmc-agriculture",
            "Europe PMC agricultural research",
            "agriculture",
            "europepmc",
            "precision agriculture",
            "papers",
            "world",
        ),
        stream(
            "patent-index",
            "Patent index — historical coverage",
            "robotics",
            "patents",
            "robot",
            "papers",
            "world",
        ),
        stream(
            "india-schemes",
            "Official India agriculture announcements",
            "agriculture",
            "schemes",
            "",
            "news",
            "IN",
        ),
    ]);
    for s in &mut rows {
        s.enabled = !["patents", "schemes"].contains(&s.provider.as_str());
        if s.provider != "patents" {
            s.options.days = 90;
        }
    }
    rows
}
/// Enable only built-in public defaults for an unconfigured profile. Never override an
/// explicit off setting, restored disabled subscriptions or owner-defined source policy.
pub(crate) fn initialize_public_defaults(repo: &command_center_core::Repository) -> Result<bool> {
    repo.initialize_public_feeds(&serde_json::to_value(defaults()).map_err(|e| e.to_string())?)
}
#[tauri::command]
pub fn add_research_starters(state: State<AppState>) -> Result<usize> {
    let mut rows = list(&state)?;
    let mut count = 0;
    for source in research_starters() {
        if rows.iter().any(|r| r.id == source.id) {
            continue;
        }
        if rows.len() >= 64 {
            return Err("Remove a source before adding research starters".into());
        }
        rows.push(source);
        count += 1;
    }
    state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .set_setting(
            "discovery_hub_streams",
            &serde_json::to_string(&rows).map_err(|e| e.to_string())?,
        )?;
    Ok(count)
}

fn validate(s: &Stream) -> Result<()> {
    if s.id.is_empty()
        || s.id.len() > 80
        || !s.id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
        || s.name.trim().is_empty()
        || s.name.len() > 160
        || s.query.len() > 2000
        || ![
            "ai",
            "code",
            "robotics",
            "agriculture",
            "poker",
            "learning",
            "custom",
            "apps",
        ]
        .contains(&s.topic.as_str())
        || ![
            "crossref",
            "europepmc",
            "patents",
            "schemes",
            "github",
            "models",
            "ollama",
            "arxiv",
            "rss",
            "news",
            "youtube",
            "youtube_feed",
            "reddit",
            "bluesky",
            "mastodon",
            "events",
            "calendar",
            "x",
            "linkedin",
            "import",
            "mcp",
            "apps",
        ]
        .contains(&s.provider.as_str())
        || ![
            "models", "repos", "news", "papers", "videos", "social", "events", "apps", "tools",
        ]
        .contains(&s.kind.as_str())
        || !["world", "US", "IN", "GB", "CA", "AU", "DE"].contains(&s.region.as_str())
    {
        return Err("Invalid discovery source configuration".into());
    }
    s.options.validate()?;
    if ["rss", "events", "calendar"].contains(&s.provider.as_str()) {
        let u = reqwest::Url::parse(&s.query).map_err(|_| "Use a full public HTTPS feed URL")?;
        if u.scheme() != "https"
            || !u.username().is_empty()
            || u.password().is_some()
            || u.fragment().is_some()
        {
            return Err("Use a public HTTPS URL without credentials or fragment".into());
        }
    }
    if s.provider == "youtube_feed" {
        youtube_input(&s.query)?;
    }
    if ["reddit", "mastodon"].contains(&s.provider.as_str())
        && (s.query.is_empty()
            || s.query.len() > 80
            || !s
                .query
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_'))
    {
        return Err("Enter one subreddit name or hashtag without spaces or URL".into());
    }
    if !["rss", "events", "calendar"].contains(&s.provider.as_str()) && s.query.len() > 300 {
        return Err("Search query exceeds 300 bytes".into());
    }
    Ok(())
}
fn list(state: &AppState) -> Result<Vec<Stream>> {
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    match repo.setting("discovery_hub_streams")? {
        Some(s) => serde_json::from_str(&s).map_err(|_| "Invalid discovery configuration".into()),
        None => {
            let rows = defaults();
            repo.set_setting(
                "discovery_hub_streams",
                &serde_json::to_string(&rows).map_err(|e| e.to_string())?,
            )?;
            Ok(rows)
        }
    }
}
#[tauri::command]
pub fn discovery_hub(state: State<AppState>) -> Result<Value> {
    status(&state)
}
pub(crate) fn status(state: &AppState) -> Result<Value> {
    let streams = list(state)?;
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    let feeds = repo.feeds()?;
    let rows = streams
        .iter()
        .map(|s| {
            let cache = feeds.iter().find(|f| f.source == format!("hub:{}", s.id));
            let health = repo
                .setting(&format!("hub_health:{}", s.id))
                .ok()
                .flatten()
                .and_then(|s| serde_json::from_str::<Value>(&s).ok());
            json!({"stream":s,"cache":cache,"health":health})
        })
        .collect::<Vec<_>>();
    let auto = repo
        .setting("discovery_hub_auto_refresh")?
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(0);
    let jobs = state.jobs.lock().map_err(|_| "Jobs unavailable")?;
    let active = jobs
        .keys()
        .filter(|s| s.starts_with("hub:"))
        .cloned()
        .collect::<Vec<_>>();
    Ok(
        json!({"streams":rows,"auto_refresh_minutes":auto,"active":active,"last_refresh":repo.setting("discovery_hub_last_refresh")?.and_then(|v|serde_json::from_str::<Value>(&v).ok())}),
    )
}
#[tauri::command]
pub fn save_discovery_stream(state: State<AppState>, stream: Stream) -> Result<()> {
    validate(&stream)?;
    let mut rows = list(&state)?;
    let old = rows.iter().find(|s| s.id == stream.id);
    let changed = old.is_some_and(|s| {
        json!([s.provider, s.query, s.options, s.topic, s.kind, s.region])
            != json!([
                stream.provider,
                stream.query,
                stream.options,
                stream.topic,
                stream.kind,
                stream.region
            ])
    });
    if old.is_none() && rows.len() >= 64 {
        return Err("At most 64 discovery sources".into());
    }
    let id = stream.id.clone();
    rows.retain(|s| s.id != id);
    rows.push(stream);
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    if changed {
        repo.cache_feed(&format!("hub:{id}"), &json!([]))?;
        repo.set_setting(&format!("hub_health:{id}"), "null")?;
    }
    repo.set_setting(
        "discovery_hub_streams",
        &serde_json::to_string(&rows).map_err(|e| e.to_string())?,
    )
}
#[tauri::command]
pub fn remove_discovery_stream(state: State<AppState>, id: String) -> Result<()> {
    let mut rows = list(&state)?;
    rows.retain(|s| s.id != id);
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    repo.remove_cached_feed(&format!("hub:{id}"))?;
    repo.set_setting(
        "discovery_hub_streams",
        &serde_json::to_string(&rows).map_err(|e| e.to_string())?,
    )
}

fn event_observation(item: &Value) -> Value {
    let mut v = serde_json::Map::new();
    for key in [
        "title",
        "description",
        "start_date",
        "end_date",
        "location",
        "event_status",
        "url",
    ] {
        v.insert(key.into(), item[key].clone());
    }
    Value::Object(v)
}
fn review_event_changes(
    repo: &mut command_center_core::Repository,
    stream: &str,
    items: &[Value],
) -> Result<()> {
    for mut record in repo
        .records("", false)?
        .into_iter()
        .filter(|r| r.fields.get("hub_watch_stream").map(String::as_str) == Some(stream))
    {
        let Some(item) = items
            .iter()
            .find(|i| i["id"].as_str() == record.fields.get("hub_watch_id").map(String::as_str))
        else {
            continue;
        };
        let next = event_observation(item);
        let previous = record
            .fields
            .get("hub_watch_snapshot")
            .and_then(|v| serde_json::from_str::<Value>(v).ok());
        if previous.as_ref() == Some(&next) {
            continue;
        }
        if previous.is_some() {
            let title = format!("Publisher event details changed: {}", record.title);
            let mut alert = command_center_core::workspace::Record::new(
                "notifications",
                &title.chars().take(500).collect::<String>(),
                &repo.device_id,
            );
            alert.status = "needs_review".into();
            alert.links.push(record.id.clone());
            alert.summary=format!("Observed {}. Review dates, venue, registration and satellite details at {}. Saved plans were retained. A missing event in a bounded feed does not establish cancellation.",command_center_core::now(),item["url"].as_str().unwrap_or("the publisher"));
            repo.save_record(alert)?;
        }
        record
            .fields
            .insert("hub_watch_snapshot".into(), next.to_string());
        repo.save_record(record)?;
    }
    Ok(())
}
fn transient_read_error(error: &str) -> bool {
    [
        "error sending request",
        "request failed",
        "request timed out",
        "DNS lookup failed",
        "502 Bad Gateway",
        "503 Service Unavailable",
        "504 Gateway Timeout",
    ]
    .iter()
    .any(|needle| error.contains(needle))
        && !["401", "403", "429", "cooldown", "permission"]
            .iter()
            .any(|needle| error.contains(needle))
}
async fn retry_public_read<T, F, Fut>(allow: bool, mut read: F) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T>>,
{
    let first = read().await;
    if allow
        && first
            .as_ref()
            .err()
            .is_some_and(|e| transient_read_error(e))
    {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        read().await
    } else {
        first
    }
}
pub(crate) async fn refresh(state: &AppState, id: &str) -> Result<Value> {
    let s = list(state)?
        .into_iter()
        .find(|s| s.id == id)
        .ok_or("Source not found")?;
    validate(&s)?;
    if !s.enabled {
        return Err("Source disabled; enable it in Sources first".into());
    }
    let epoch = state.data_epoch.load(Ordering::SeqCst);
    let request = format!("hub:{id}");
    let cancel = tokio_util::sync::CancellationToken::new();
    {
        let mut jobs = state.jobs.lock().map_err(|_| "Jobs unavailable")?;
        if jobs.contains_key(&request) {
            return Err("Source already refreshing".into());
        }
        jobs.insert(request.clone(), cancel.clone());
    }
    // Apple charts have their own bounded official-feed fallback; do not multiply requests.
    let result = tokio::select! {_=cancel.cancelled()=>Err("Refresh cancelled; previous cache retained".into()),r=tokio::time::timeout(std::time::Duration::from_secs(65),retry_public_read(!["x","reddit","youtube","linkedin","apps"].contains(&s.provider.as_str()),||fetch(state,&s)))=>r.unwrap_or_else(|_|Err("Source timed out; previous cache retained".into()))};
    state
        .jobs
        .lock()
        .map_err(|_| "Jobs unavailable")?
        .remove(&request);
    let mut repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    if state.data_epoch.load(Ordering::SeqCst) != epoch {
        return Err("Workspace reset during refresh".into());
    }
    // A source edited/removed while in flight must not republish old results.
    let current: Vec<Stream> = repo
        .setting("discovery_hub_streams")?
        .map(|v| serde_json::from_str(&v))
        .transpose()
        .map_err(|_| "Invalid discovery configuration")?
        .unwrap_or_else(defaults);
    if !current
        .iter()
        .any(|v| serde_json::to_value(v).ok() == serde_json::to_value(&s).ok())
    {
        return Err("Source changed during refresh; results discarded".into());
    }
    match result {
        Ok(mut items) => {
            for (rank, item) in items.iter_mut().enumerate() {
                item["topic"] = json!(s.topic);
                item["stream_id"] = json!(s.id);
                item["stream_name"] = json!(s.name);
                item["kind"] = json!(s.kind);
                item["region"] = json!(s.region);
                item["provider_rank"] = json!(rank + 1);
            }
            review_event_changes(&mut repo, &s.id, &items)?;
            let feed = repo.cache_feed(&format!("hub:{}", s.id), &json!(items))?;
            repo.evaluate_watchlists()?;
            let health =
                json!({"status":"ok","checked_at":command_center_core::now(),"count":items.len()});
            repo.set_setting(&format!("hub_health:{}", s.id), &health.to_string())?;
            Ok(json!({"feed":feed,"health":health}))
        }
        Err(error) => {
            repo.set_setting(
                &format!("hub_health:{}", s.id),
                &json!({"status":"error","checked_at":command_center_core::now(),"error":error})
                    .to_string(),
            )?;
            Err(error)
        }
    }
}
#[tauri::command]
pub async fn refresh_discovery_stream(state: State<'_, AppState>, id: String) -> Result<Value> {
    refresh(&state, &id).await
}

async fn fetch(state: &AppState, s: &Stream) -> Result<Vec<Value>> {
    match s.provider.as_str(){
 "crossref"|"europepmc"|"patents"|"schemes"=>crate::scholarly_sources::fetch(state,&s.provider,&s.query,s.options.days).await,
 "rss"=>values(crate::research::fetch(state,"rss",&s.query).await?),
 "youtube_feed"=>public_channel(state,&s.query,&s.name).await,
 "reddit"=>{if s.provider=="reddit" {if let Some(rows)=reddit_authorized(state,s).await?{return Ok(rows);}let until=state.repo.lock().map_err(|_|"Database unavailable")?.setting("hub_reddit_next")?.and_then(|s|s.parse::<i64>().ok()).unwrap_or(0);let delay=until-chrono::Utc::now().timestamp();if delay>10{return Err(format!("Reddit public RSS is paused/rate-limited. Retry in {delay} seconds; existing results retained."));}if delay>0{tokio::time::sleep(std::time::Duration::from_secs(delay as u64+1)).await;}state.repo.lock().map_err(|_|"Database unavailable")?.set_setting("hub_reddit_next",&(chrono::Utc::now().timestamp()+6).to_string())?;}let url=if s.provider=="youtube_feed"{format!("https://www.youtube.com/feeds/videos.xml?channel_id={}",resolve_youtube(&s.query).await?)}else{let order=if ["hot","new","top"].contains(&s.options.sort.as_str()){s.options.sort.as_str()}else{"hot"};format!("https://www.reddit.com/r/{}/{order}/.rss?limit=10",s.query)};let fetched=crate::research::fetch(state,"rss",&url).await;if s.provider=="reddit"&&fetched.is_err(){state.repo.lock().map_err(|_|"Database unavailable")?.set_setting("hub_reddit_next",&(chrono::Utc::now().timestamp()+900).to_string())?;}let mut rows=values(fetched?)?;for r in &mut rows{r["source"]=json!(if s.provider=="youtube_feed"{"youtube"}else{"reddit"});r["metric"]=json!(if s.provider=="youtube_feed"{format!("{} · public channel feed",s.name)}else{format!("r/{} · {}",s.query,if s.options.sort.is_empty(){"hot"}else{&s.options.sort})});}Ok(rows)},
 "bluesky"|"mastodon"|"x"=>social(state,s).await,
 "linkedin"=>Err("LinkedIn does not provide an open personal home-feed API. Use an authorized RSS source or save individual post URLs; approved LinkedIn API access is a separate dependency.".into()),
 "events"|"calendar"=>events(state,s).await,
 _=>{
 // Respect shared provider reservations across topic streams, rather than failing the second topic.
 let until=state.repo.lock().map_err(|_|"Database unavailable")?.setting(&format!("source_next:{}",s.provider))?.and_then(|v|v.parse::<i64>().ok()).unwrap_or(0);
 let delay=until-chrono::Utc::now().timestamp();if delay>30{return Err(format!("Provider cooldown; retry in {delay} seconds"));}if delay>0{tokio::time::sleep(std::time::Duration::from_secs(delay as u64+1)).await;}
 let feed=crate::connectors::refresh_cached(state,s.provider.clone(),s.query.clone(),s.options.clone(),Some(format!("hub:{}",s.id))).await?;
 Ok(feed.items.as_array().cloned().unwrap_or_default())
 }
 }
}
pub(crate) async fn public_channel(
    state: &AppState,
    query: &str,
    name: &str,
) -> Result<Vec<Value>> {
    let id = resolve_youtube(query).await?;
    let rows = crate::research::fetch(
        state,
        "rss",
        &format!("https://www.youtube.com/feeds/videos.xml?channel_id={id}"),
    )
    .await?;
    let mut rows = values(rows)?;
    for row in &mut rows {
        row["source"] = json!("youtube");
        row["metric"] = json!(format!("{name} · public channel feed"));
    }
    Ok(rows)
}

fn values(rows: Vec<Discovery>) -> Result<Vec<Value>> {
    rows.into_iter()
        .map(|r| serde_json::to_value(r).map_err(|e| e.to_string()))
        .collect()
}
fn clean(s: &str, max: usize) -> String {
    let doc = dom_query::Document::from(format!("<div>{s}</div>"));
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
fn row(
    provider: &str,
    title: &str,
    url: &str,
    description: &str,
    metric: &str,
    published: Option<&str>,
) -> Value {
    json!({"id":format!("{provider}:{:x}",Sha256::digest(url)),"source":provider,"title":title.chars().take(500).collect::<String>(),"url":url,"description":description.chars().take(3000).collect::<String>(),"metric":metric,"published_at":published,"fetched_at":command_center_core::now(),"verification":"Live public source metadata; publisher/user claims require verification."})
}
async fn social(state: &AppState, s: &Stream) -> Result<Vec<Value>> {
    let endpoint = match s.provider.as_str() {
        "bluesky" => reqwest::Url::parse_with_params(
            "https://api.bsky.app/xrpc/app.bsky.feed.searchPosts",
            &[
                ("q", s.query.as_str()),
                (
                    "sort",
                    if s.options.sort == "top" {
                        "top"
                    } else {
                        "latest"
                    },
                ),
                ("limit", "30"),
            ],
        ),
        "mastodon" => reqwest::Url::parse(&format!(
            "https://mastodon.social/api/v1/timelines/tag/{}?limit=30",
            s.query
        )),
        _ => reqwest::Url::parse_with_params(
            "https://api.x.com/2/tweets/search/recent",
            &[
                ("query", s.query.as_str()),
                ("max_results", "30"),
                ("tweet.fields", "created_at,public_metrics,author_id"),
            ],
        ),
    }
    .map_err(|_| "Invalid social query")?;
    let client = crate::research::public_client(&endpoint).await?;
    let mut request = client.get(endpoint);
    if s.provider == "x" {
        if state
            .repo
            .lock()
            .map_err(|_| "Database unavailable")?
            .setting("discovery_x_configured")?
            .as_deref()
            != Some("true")
        {
            return Err("X requires developer API access. Add a token in Sources → Connections or import links instead.".into());
        }
        let key=zeroize::Zeroizing::new(keyring::Entry::new("com.personal.commandcenter.api","discovery-x").map_err(|e|e.to_string())?.get_password().map_err(|_|"X recent search requires an X developer bearer token with search access; add it in Sources → Connections")?);
        request = request.bearer_auth(key.as_str());
    }
    let response = request
        .send()
        .await
        .map_err(|_| "Social provider request failed")?;
    let v = crate::connectors::json_response(response).await?;
    let mut rows = vec![];
    if s.provider == "bluesky" {
        for p in v["posts"]
            .as_array()
            .ok_or("Bluesky response has no posts array")?
        {
            let uri = p["uri"].as_str().unwrap_or("");
            let key = uri.rsplit('/').next().unwrap_or("");
            let handle = p["author"]["handle"].as_str().unwrap_or("");
            if handle.is_empty() || key.is_empty() {
                continue;
            }
            let (title, description, metadata) = social_content(p, "bluesky", handle);
            let mut item = row(
                "bluesky",
                &title,
                &format!("https://bsky.app/profile/{handle}/post/{key}"),
                &description,
                &format!(
                    "@{handle} · {} likes · {} reposts",
                    p["likeCount"].as_u64().unwrap_or(0),
                    p["repostCount"].as_u64().unwrap_or(0)
                ),
                p["record"]["createdAt"].as_str(),
            );
            item["metadata"] = metadata;
            rows.push(item);
        }
    } else if s.provider == "mastodon" {
        for p in v.as_array().ok_or("Mastodon response has no posts array")? {
            let (title, description, metadata) = social_content(p, "mastodon", p["account"]["acct"].as_str().unwrap_or("unknown"));
            let url = p["url"].as_str().unwrap_or("");
            if !valid_public_link(url) {
                continue;
            }
            let mut item = row(
                "mastodon",
                &title,
                url,
                &description,
                &format!(
                    "@{} · {} favourites",
                    p["account"]["acct"].as_str().unwrap_or(""),
                    p["favourites_count"].as_u64().unwrap_or(0)
                ),
                p["created_at"].as_str(),
            );
            item["metadata"] = metadata;
            rows.push(item);
        }
    } else {
        for p in v["data"].as_array().unwrap_or(&vec![]) {
            let text = p["text"].as_str().unwrap_or("");
            let id = p["id"].as_str().unwrap_or("");
            if !id.bytes().all(|c| c.is_ascii_digit()) || id.is_empty() {
                continue;
            }
            rows.push(row(
                "x",
                &text.chars().take(140).collect::<String>(),
                &format!("https://x.com/i/status/{id}"),
                text,
                &format!(
                    "{} likes",
                    p["public_metrics"]["like_count"].as_u64().unwrap_or(0)
                ),
                p["created_at"].as_str(),
            ));
        }
    }
    Ok(rows)
}
// Public social posts may legitimately contain only attached articles, images,
// videos or quotes. Preserve those details and give every card a saveable title.
fn social_content(post: &Value, provider: &str, handle: &str) -> (String, String, Value) {
    let bluesky = provider == "bluesky";
    let text = clean(if bluesky { post["record"]["text"].as_str() } else { post["content"].as_str() }.unwrap_or(""), 3000);
    let embed = if post["embed"].is_object() { &post["embed"] } else { &post["record"]["embed"] };
    let external = if bluesky {
        if embed["external"].is_object() { &embed["external"] } else { &embed["media"]["external"] }
    } else { &post["card"] };
    let article_title = clean(external["title"].as_str().unwrap_or(""), 500);
    let article_description = clean(external["description"].as_str().unwrap_or(""), 1800);
    let external_url = external[if bluesky { "uri" } else { "url" }].as_str().filter(|u| valid_public_link(u));
    let images = if bluesky {
        embed["images"].as_array().or_else(|| embed["media"]["images"].as_array())
    } else { post["media_attachments"].as_array() };
    let alts = images.into_iter().flatten().take(10).filter_map(|image| image[if bluesky { "alt" } else { "description" }].as_str()).map(|s| clean(s, 1000)).filter(|s| !s.is_empty()).collect::<Vec<_>>();
    let quote = if embed["record"]["value"]["text"].is_string() { &embed["record"]["value"]["text"] } else { &embed["record"]["record"]["value"]["text"] };
    let quoted_text = clean(quote.as_str().unwrap_or(""), 1800);
    let title = if !text.is_empty() { text.chars().take(140).collect() }
        else if !article_title.is_empty() { article_title.clone() }
        else { format!("Post by @{handle} (no text supplied)") };
    let mut descriptions = vec![];
    if !text.is_empty() { descriptions.push(text.clone()); }
    if !article_title.is_empty() { descriptions.push(format!("Attached article: {article_title}")); }
    if !article_description.is_empty() { descriptions.push(article_description.clone()); }
    if !alts.is_empty() { descriptions.push(format!("Source-provided media descriptions: {}", alts.join(" · "))); }
    if !quoted_text.is_empty() { descriptions.push(format!("Quoted post: {quoted_text}")); }
    if descriptions.is_empty() { descriptions.push("This public post has no text. Open the original to view its content.".into()); }
    let metadata = json!({"original_text_supplied":!text.is_empty(),"attached_title":article_title,"attached_description":article_description,"attached_url":external_url,"media_descriptions":alts,"quoted_text":quoted_text,"embed_type":embed["$type"].as_str()});
    (title, descriptions.join("\n\n").chars().take(6000).collect(), metadata)
}
fn valid_public_link(raw: &str) -> bool {
    reqwest::Url::parse(raw).is_ok_and(|u| {
        ["http", "https"].contains(&u.scheme())
            && u.username().is_empty()
            && u.password().is_none()
            && u.host_str().is_some()
    })
}
async fn public_bytes(raw: &str) -> Result<Vec<u8>> {
    let mut u = reqwest::Url::parse(raw).map_err(|_| "Invalid public source URL")?;
    for _ in 0..4 {
        let mut response = crate::research::public_client(&u)
            .await?
            .get(u.clone())
            .send()
            .await
            .map_err(|_| "Publisher request failed")?;
        if response.status().is_redirection() {
            let location = response
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
                .ok_or("Publisher redirect has no location")?;
            u = u.join(location).map_err(|_| "Invalid publisher redirect")?;
            continue;
        }
        if !response.status().is_success() {
            return Err(format!("Publisher returned HTTP {}", response.status()));
        }
        let mut bytes = vec![];
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| "Publisher download interrupted")?
        {
            if bytes.len() + chunk.len() > 2 * 1024 * 1024 {
                return Err("Publisher response exceeds 2 MiB".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        return Ok(bytes);
    }
    Err("Too many publisher redirects".into())
}
async fn events(state: &AppState, s: &Stream) -> Result<Vec<Value>> {
    let bytes = public_bytes(&s.query).await?;
    if s.provider == "calendar" {
        let source = crate::event_sources::Source {
            id: uuid::Uuid::new_v4().to_string(),
            name: s.name.clone(),
            url: s.query.clone(),
            category: if s.topic == "poker" {
                "poker"
            } else {
                "agriculture"
            }
            .into(),
            verified_authority: false,
            enabled: false,
            interval_minutes: 1440,
        };
        let parsed = crate::event_sources::parse(bytes, &source)?;
        let mut rows = vec![];
        for e in parsed["events"]
            .as_array()
            .ok_or("Calendar has no event array")?
        {
            let start = e["start"]["date"].as_str().unwrap_or("");
            let mut end = e["end"]["date"].as_str().unwrap_or(start).to_owned();
            if e["end"]["all_day"] == true {
                if let Ok(d) = chrono::NaiveDate::parse_from_str(&end, "%Y-%m-%d") {
                    end = (d - chrono::Duration::days(1)).to_string();
                }
            }
            let mut r = event_row(
                &s.name,
                e["title"].as_str().unwrap_or(""),
                e["source_url"]
                    .as_str()
                    .filter(|v| !v.is_empty())
                    .unwrap_or(&s.query),
                e["description"].as_str().unwrap_or(""),
                start,
                &end,
                e["location"].as_str().unwrap_or(""),
            );
            r["event_status"] = e["status"].clone();
            rows.push(r);
        }
        return Ok(upcoming(rows));
    }
    let html = String::from_utf8(bytes).map_err(|_| "Publisher page must be UTF-8")?;
    let s = s.clone();
    let enrich_poker=s.topic=="poker" && s.query=="https://www.cardplayer.com/poker-tournaments";
    let rows=tokio::task::spawn_blocking(move || parse_events(&html, &s))
        .await
        .map_err(|_| "Event parser interrupted")??;
    Ok(if enrich_poker {crate::poker_locations::enrich(state,rows).await} else {rows})
}
fn event_row(
    publisher: &str,
    title: &str,
    url: &str,
    description: &str,
    start: &str,
    end: &str,
    location: &str,
) -> Value {
    let mut r = row(
        "events",
        title,
        url,
        description,
        &format!("{start} → {end} · {location} · {publisher}"),
        None,
    );
    r["id"] = json!(format!(
        "event:{:x}",
        Sha256::digest(format!("{url}|{start}"))
    ));
    r["start_date"] = json!(start);
    r["end_date"] = json!(end);
    r["location"] = json!(location);
    if let Some(country)=crate::poker_locations::location_country(location){r["country_code"]=json!(country);}
    r
}
fn upcoming(rows: Vec<Value>) -> Vec<Value> {
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let mut rows = rows
        .into_iter()
        .filter(|r| {
            let end = r["end_date"].as_str().unwrap_or("");
            let date = end.get(..10).unwrap_or("");
            let start = r["start_date"]
                .as_str()
                .and_then(|s| s.get(..10))
                .unwrap_or("");
            chrono::NaiveDate::parse_from_str(start, "%Y-%m-%d").is_ok()
                && chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").is_ok()
                && start <= date
                && if end.contains('T') {
                    chrono::DateTime::parse_from_rfc3339(end).is_ok_and(|t| t >= chrono::Utc::now())
                } else {
                    date >= today.as_str()
                }
                && !r["event_status"]
                    .as_str()
                    .unwrap_or("")
                    .to_lowercase()
                    .contains("cancel")
        })
        .collect::<Vec<_>>();
    let mut seen = std::collections::HashSet::new();
    rows.retain(|r| seen.insert(format!("{}|{}", r["url"], r["start_date"])));
    rows.sort_by_key(|r| r["start_date"].as_str().unwrap_or("").to_owned());
    rows.truncate(100);
    rows
}
fn parse_events(html: &str, s: &Stream) -> Result<Vec<Value>> {
    let doc = dom_query::Document::from(html);
    let mut rows = vec![];
    fn walk(v: &Value, rows: &mut Vec<Value>, publisher: &str, base: &str, depth: usize) {
        if depth > 16 || rows.len() >= 100 {
            return;
        }
        match v {
            Value::Array(a) => {
                for v in a.iter().take(500) {
                    walk(v, rows, publisher, base, depth + 1)
                }
            }
            Value::Object(o) => {
                let event = o.get("@type").is_some_and(|v| {
                    v.as_str().is_some_and(|s| s.ends_with("Event"))
                        || v.as_array().is_some_and(|a| {
                            a.iter()
                                .any(|v| v.as_str().is_some_and(|s| s.ends_with("Event")))
                        })
                });
                if event {
                    if let (Some(name), Some(start)) = (v["name"].as_str(), v["startDate"].as_str())
                    {
                        let end = v["endDate"].as_str().unwrap_or(start);
                        let url = v["url"].as_str().unwrap_or(base);
                        if valid_public_link(url) {
                            let location = format!(
                                "{} {}",
                                v["location"]["name"].as_str().unwrap_or(""),
                                v["location"]["address"]
                                    .as_str()
                                    .map(str::to_owned)
                                    .unwrap_or_else(|| v["location"]["address"]
                                        .as_object()
                                        .map(|a| [
                                            "addressLocality",
                                            "addressRegion",
                                            "addressCountry"
                                        ]
                                        .iter()
                                        .filter_map(|k| a.get(*k).and_then(Value::as_str))
                                        .collect::<Vec<_>>()
                                        .join(", "))
                                        .unwrap_or_default())
                            );
                            let mut r = event_row(
                                publisher,
                                &clean(name, 500),
                                url,
                                &clean(v["description"].as_str().unwrap_or(""), 3000),
                                start,
                                end,
                                &location,
                            );
                            r["event_status"] = v["eventStatus"].clone();
                            if let Some(country)=crate::poker_locations::country(&v["location"]["address"]["addressCountry"]) {r["country_code"]=json!(country);}
                            rows.push(r);
                        }
                    }
                }
                for child in o.values().take(100) {
                    if child.is_array() || child.is_object() {
                        walk(child, rows, publisher, base, depth + 1)
                    }
                }
            }
            _ => {}
        }
    }
    for script in doc
        .select("script[type='application/ld+json']")
        .iter()
        .take(30)
    {
        if let Ok(v) = serde_json::from_str::<Value>(&script.text()) {
            walk(&v, &mut rows, &s.name, &s.query, 0);
        }
    }
    if s.query.contains("cardplayer.com/poker-tournaments") {
        for tr in doc.select("table.cp-poker-table tbody tr").iter().take(500) {
            let cells = tr.select("td");
            let date = cells.first().text().to_string();
            let dates = date.split(" - ").collect::<Vec<_>>();
            if dates.len() != 2 {
                continue;
            }
            let parse = |s: &str| chrono::NaiveDate::parse_from_str(s.trim(), "%b %e, '%y").ok();
            let (Some(start), Some(end)) = (parse(dates[0]), parse(dates[1])) else {
                continue;
            };
            let link = tr.select("a[data-series_id]");
            let Some(url) = link.attr("href") else {
                continue;
            };
            if !valid_public_link(&url) {
                continue;
            }
            let location = cells
                .iter()
                .nth(2)
                .map(|c| c.text().split_whitespace().collect::<Vec<_>>().join(" "))
                .unwrap_or_default();
            rows.push(event_row(&s.name,&link.text().split_whitespace().collect::<Vec<_>>().join(" "),&url,"Multi-organizer publisher calendar; confirm current details with the organizer before registration or travel.",&start.to_string(),&end.to_string(),&location));
        }
    }
    if s.query == "https://www.wsop.com/schedule/" {
        for li in doc.select("li").iter() {
            let title = li.select(".series-name").text().to_string();
            if title.is_empty() {
                continue;
            }
            let dates = li.select(".date").text().to_string();
            let dates = dates.split(" - ").collect::<Vec<_>>();
            if dates.len() != 2 {
                continue;
            }
            let parse = |v: &str| chrono::NaiveDate::parse_from_str(v.trim(), "%b %d %Y").ok();
            if let (Some(a), Some(b), Some(href)) = (
                parse(dates[0]),
                parse(dates[1]),
                li.select("a").attr("href"),
            ) {
                if let Ok(url) = reqwest::Url::parse(&s.query).and_then(|u| u.join(&href)) {
                    rows.push(event_row(
                        &s.name,
                        &title,
                        url.as_str(),
                        "Official WSOP schedule; confirm current venue and registration details.",
                        &a.to_string(),
                        &b.to_string(),
                        &li.select(".venue")
                            .text()
                            .split_whitespace()
                            .collect::<Vec<_>>()
                            .join(" "),
                    ));
                }
            }
        }
    }
    if s.query == "https://www.pokerstarslive.com/" {
        for slide in doc.select(".slick-slide").iter().take(100) {
            let title = slide.select("h1").text().to_string();
            let text = slide.select("p").text().to_string();
            if title.is_empty() {
                continue;
            }
            if let (Some((a, b)), Some(href)) = (human_range(&text), slide.select("a").attr("href"))
            {
                if let Ok(url) = reqwest::Url::parse(&s.query).and_then(|u| u.join(&href)) {
                    rows.push(event_row(
                        &s.name,
                        &title,
                        url.as_str(),
                        &text,
                        &a,
                        &b,
                        "See official festival venue",
                    ));
                }
            }
        }
    }
    if rows.is_empty() && s.query == "https://www.worldagexpo.com/" {
        let text = doc
            .select(".block-hero .copy")
            .text()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if let Some((a, b)) = human_range(&text) {
            rows.push(event_row(
                &s.name,
                "World Ag Expo",
                &s.query,
                &text,
                &a,
                &b,
                "Tulare, California, United States",
            ));
        }
    }
    if rows.is_empty() && s.query == "https://agritechindia.com/" {
        let text = doc
            .select(".foodex-info")
            .text()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if let Some((start, end)) = human_range(&text) {
            rows.push(event_row(
                &s.name,
                &doc.select("title").text(),
                &s.query,
                &text,
                &start,
                &end,
                "Bangalore, India",
            ));
        }
    }
    if rows.is_empty() {
        return Err("No supported dated events found in publisher page; source layout may have changed. Existing cache retained.".into());
    }
    Ok(upcoming(rows))
}
fn human_range(text: &str) -> Option<(String, String)> {
    let normalized = text.replace(['–', '—'], "-");
    let words = normalized.split_whitespace().collect::<Vec<_>>();
    for w in words.windows(3) {
        let range = w[0].split('-').collect::<Vec<_>>();
        if range.len() == 2 {
            let parse = |d: &str| {
                chrono::NaiveDate::parse_from_str(&format!("{} {} {}", d, w[1], w[2]), "%e %B %Y")
                    .ok()
            };
            if let (Some(a), Some(b)) = (parse(range[0]), parse(range[1])) {
                if b >= a {
                    return Some((a.to_string(), b.to_string()));
                }
            }
        }
        let range = w[1].trim_end_matches(',').split('-').collect::<Vec<_>>();
        if range.len() == 2 {
            let parse = |d: &str| {
                chrono::NaiveDate::parse_from_str(&format!("{} {} {}", d, w[0], w[2]), "%e %B %Y")
                    .ok()
            };
            if let (Some(a), Some(b)) = (parse(range[0]), parse(range[1])) {
                if b >= a {
                    return Some((a.to_string(), b.to_string()));
                }
            }
        }
    }
    None
}

#[tauri::command]
pub fn save_discovery_x_token(state: State<AppState>, value: String) -> Result<()> {
    let value = zeroize::Zeroizing::new(value);
    if value.len() < 20 || value.len() > 2048 || value.chars().any(char::is_whitespace) {
        return Err("Enter an X developer bearer token".into());
    }
    keyring::Entry::new("com.personal.commandcenter.api", "discovery-x")
        .map_err(|e| e.to_string())?
        .set_password(&value)
        .map_err(|_| "OS secure store could not save token")?;
    state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .set_integration_flag("discovery_x_configured", true)
}
#[tauri::command]
pub fn remove_discovery_x_token(state: State<AppState>) -> Result<()> {
    state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .set_integration_flag("discovery_x_configured", false)?;
    match keyring::Entry::new("com.personal.commandcenter.api", "discovery-x")
        .map_err(|e| e.to_string())?
        .delete_credential()
    {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(_) => Err("OS secure store could not remove token".into()),
    }
}

pub(crate) async fn refresh_batch(state: &AppState, ids: Option<Vec<String>>) -> Result<Value> {
    use futures_util::{stream, StreamExt};
    let epoch = state.data_epoch.load(Ordering::SeqCst);
    let streams = list(state)?
        .into_iter()
        .filter(|s| {
            s.enabled
                && !["linkedin", "import"].contains(&s.provider.as_str())
                && ids.as_ref().is_none_or(|ids| ids.contains(&s.id))
        })
        .collect::<Vec<_>>();
    if ids.as_ref().is_some_and(|v| v.len() > 64) {
        return Err("Too many sources".into());
    }
    let cancel = tokio_util::sync::CancellationToken::new();
    {
        let mut jobs = state.jobs.lock().map_err(|_| "Jobs unavailable")?;
        if jobs.contains_key("hub:batch") {
            return Err("Feed refresh already running".into());
        }
        jobs.insert("hub:batch".into(), cancel.clone());
    }
    let mut groups = std::collections::BTreeMap::<String, Vec<Stream>>::new();
    for s in streams {
        let key = if ["rss", "youtube_feed", "events", "calendar"].contains(&s.provider.as_str()) {
            s.id.clone()
        } else {
            s.provider.clone()
        };
        groups.entry(key).or_default().push(s);
    }
    let results = stream::iter(groups.into_values())
        .map(|sources| {
            let cancel = cancel.clone();
            async move {
                let mut rows = vec![];
                for s in sources {
                    if cancel.is_cancelled() {
                        break;
                    }
                    let result = refresh(state, &s.id).await;
                    rows.push(json!({"id":s.id,"ok":result.is_ok(),"error":result.err()}));
                }
                rows
            }
        })
        .buffer_unordered(3)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    state
        .jobs
        .lock()
        .map_err(|_| "Jobs unavailable")?
        .remove("hub:batch");
    let summary = json!({"at":command_center_core::now(),"refreshed":results.iter().filter(|r|r["ok"]==true).count(),"failed":results.iter().filter(|r|r["ok"]==false).count(),"cancelled":cancel.is_cancelled()});
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    if state.data_epoch.load(Ordering::SeqCst) == epoch {
        repo.set_setting("discovery_hub_last_refresh", &summary.to_string())?;
    }
    Ok(
        json!({"refreshed":results.iter().filter(|r|r["ok"]==true).count(),"failed":results.iter().filter(|r|r["ok"]==false).count(),"cancelled":cancel.is_cancelled(),"results":results}),
    )
}
#[tauri::command]
pub async fn refresh_discovery_hub(
    state: State<'_, AppState>,
    ids: Option<Vec<String>>,
) -> Result<Value> {
    refresh_batch(&state, ids).await
}
#[tauri::command]
pub fn cancel_discovery_hub(state: State<AppState>) -> Result<()> {
    for (id, token) in state.jobs.lock().map_err(|_| "Jobs unavailable")?.iter() {
        if id.starts_with("hub:") {
            token.cancel();
        }
    }
    Ok(())
}
#[tauri::command]
pub fn set_discovery_auto_refresh(state: State<AppState>, minutes: u32) -> Result<()> {
    if ![0, 30, 60, 180].contains(&minutes) {
        return Err("Choose off, 30, 60 or 180 minutes".into());
    }
    state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .set_setting("discovery_hub_auto_refresh", &minutes.to_string())
}
pub async fn poll(app: &tauri::AppHandle) -> Result<()> {
    use tauri::Manager;
    let state = app.state::<AppState>();
    let due = {
        let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
        let minutes = repo
            .setting("discovery_hub_auto_refresh")?
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(0);
        let last = repo
            .setting("discovery_hub_auto_last")?
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(0);
        let now = chrono::Utc::now().timestamp();
        if minutes >= 30 && now - last >= minutes * 60 {
            repo.set_setting("discovery_hub_auto_last", &now.to_string())?;
            true
        } else {
            false
        }
    };
    if due {
        refresh_batch(&state, None).await?;
    }
    Ok(())
}
#[tauri::command]
pub fn create_discovery_search(state: State<AppState>, query: String) -> Result<Vec<String>> {
    let query = query.trim();
    if query.is_empty() || query.len() > 200 {
        return Err("Enter a public search query up to 200 bytes".into());
    }
    let mut rows = list(&state)?;
    let extra = ["github", "models", "news", "arxiv", "bluesky"]
        .iter()
        .filter(|p| !rows.iter().any(|s| s.id == format!("search-{p}")))
        .count();
    if rows.len() + extra > 64 {
        return Err("Remove some sources before adding a live search".into());
    }
    let mut ids = vec![];
    for provider in ["github", "models", "news", "arxiv", "bluesky"] {
        let id = format!("search-{provider}");
        rows.retain(|s| s.id != id);
        state
            .repo
            .lock()
            .map_err(|_| "Database unavailable")?
            .cache_feed(&format!("hub:{id}"), &json!([]))?;
        let q = if provider == "arxiv" {
            format!("all:\"{}\"", query.replace(['"', '\\'], " "))
        } else {
            query.to_owned()
        };
        let mut s = stream(
            &id,
            &format!("Search {provider}: {query}"),
            "custom",
            provider,
            &q,
            match provider {
                "github" => "repos",
                "models" => "models",
                "arxiv" => "papers",
                "bluesky" => "social",
                _ => "news",
            },
            "world",
        );
        if provider == "models" {
            s.options.sort = "trendingScore".into();
        }
        rows.push(s);
        ids.push(id);
    }
    if rows.len() > 64 {
        return Err("Remove some sources before adding a live search".into());
    }
    state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .set_setting(
            "discovery_hub_streams",
            &serde_json::to_string(&rows).map_err(|e| e.to_string())?,
        )?;
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_validate_and_public_events_are_not_fake_bookmarks() {
        let rows = defaults();
        assert_eq!(
            rows.iter()
                .map(|s| &s.id)
                .collect::<std::collections::HashSet<_>>()
                .len(),
            rows.len()
        );
        assert!(rows.len() >= 25);
        for s in &rows {
            validate(s).unwrap();
        }
        assert!(
            rows.iter()
                .filter(|s| s.topic == "poker" && s.kind == "events")
                .count()
                >= 4
        );
        assert!(rows
            .iter()
            .filter(|s| ["x", "linkedin", "reddit"].contains(&s.provider.as_str()))
            .all(|s| !s.enabled));
    }
    #[test]
    fn event_markup_filters_ended_cancelled_and_preserves_source_dates() {
        let s = stream(
            "test",
            "Publisher",
            "poker",
            "events",
            "https://publisher.example/events",
            "events",
            "world",
        );
        let html = r#"<script type="application/ld+json">{"@type":"ItemList","itemListElement":[{"@type":"Event","name":"Future","url":"https://publisher.example/future","startDate":"2099-03-01","endDate":"2099-03-05","location":{"name":"Venue","address":{"addressCountry":"IN"}}},{"@type":"Event","name":"Past","startDate":"2000-01-01"},{"@type":"Event","name":"Cancelled","startDate":"2099-01-01","eventStatus":"https://schema.org/EventCancelled"}]}</script>"#;
        let rows = parse_events(html, &s).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["end_date"], "2099-03-05");
        assert!(rows[0]["location"].as_str().unwrap().contains("IN"));
        assert_eq!(rows[0]["country_code"],"IN");
        assert!(parse_events("<h1>No dated events</h1>", &s).is_err());
    }
    #[test]
    fn publisher_date_ranges_are_strict() {
        assert_eq!(
            human_range("Festival Dates: January 8-17, 2099").unwrap(),
            ("2099-01-08".into(), "2099-01-17".into())
        );
        assert_eq!(human_range("20-22 August 2099").unwrap().0, "2099-08-20");
        assert!(human_range("31-32 February 2099").is_none());
        assert!(human_range("17-8 January 2099").is_none());
    }
    #[test]
    fn custom_sources_reject_credentials_and_invalid_channels() {
        let mut s = defaults().remove(0);
        s.provider = "rss".into();
        s.query = "https://user:secret@example.com/rss".into();
        assert!(validate(&s).is_err());
        s.provider = "youtube_feed".into();
        s.query = "invalid".into();
        assert!(validate(&s).is_err());
    }
    #[tokio::test]
    #[ignore = "Live internet test: explicitly run with --ignored; isolated temp database, no credentials"]
    async fn live_public_aggregation() {
        use std::{collections::HashMap, sync::Mutex};
        let dir = tempfile::tempdir().unwrap();
        let state = AppState {
            repo: Mutex::new(
                command_center_core::Repository::open(dir.path().join("qa.db")).unwrap(),
            ),
            peer_sync: Mutex::new(crate::peer_sync::Runtime::default()),
            project_watchers: Mutex::new(HashMap::new()),
            pending_update: Mutex::new(None),
            automation_lock: tokio::sync::Mutex::new(()),
            backup_lock: tokio::sync::Mutex::new(()),
            data_epoch: std::sync::atomic::AtomicU64::new(0),
            pending: Mutex::new(None),
            large_import: Mutex::new(None),
            google_cache: Mutex::new(None),
            vault: Mutex::new(None),
            jobs: Mutex::new(HashMap::new()),
        };
        let ids = if std::env::var("PCC_LIVE_SKIP_REDDIT").is_ok() {
            Some(
                defaults()
                    .into_iter()
                    .filter(|s| s.provider != "reddit" && s.enabled)
                    .map(|s| s.id)
                    .collect(),
            )
        } else {
            None
        };
        let result = refresh_batch(&state, ids).await.unwrap();
        let status = status(&state).unwrap();
        let summary=status["streams"].as_array().unwrap().iter().map(|r|json!({"id":r["stream"]["id"],"provider":r["stream"]["provider"],"topic":r["stream"]["topic"],"enabled":r["stream"]["enabled"],"health":r["health"],"count":r["cache"]["items"].as_array().map_or(0,Vec::len),"samples":r["cache"]["items"].as_array().map(|a|a.iter().take(2).map(|i|json!({"title":i["title"],"url":i["url"],"start_date":i["start_date"],"end_date":i["end_date"]})).collect::<Vec<_>>()).unwrap_or_default()})).collect::<Vec<_>>();
        let mut hf = stream(
            "qa-model-filters",
            "Filter QA",
            "ai",
            "models",
            "",
            "models",
            "world",
        );
        hf.options.task = "text-generation".into();
        hf.options.language = "transformers".into();
        hf.options.license = "apache-2.0".into();
        hf.options.sort = "downloads".into();
        let filtered_models = fetch(&state, &hf).await.unwrap();
        assert!(!filtered_models.is_empty());
        assert!(filtered_models
            .iter()
            .all(|r| r["metadata"]["task"] == "text-generation"
                && r["metadata"]["license"] == "apache-2.0"
                && r["metadata"]["library"] == "transformers"));
        let mut gh = stream(
            "qa-repo-filters",
            "Filter QA",
            "code",
            "github",
            "topic:robotics",
            "repos",
            "world",
        );
        gh.options.language = "Python".into();
        gh.options.license = "mit".into();
        gh.options.sort = "stars".into();
        let filtered_repos = fetch(&state, &gh).await.unwrap();
        assert!(!filtered_repos.is_empty());
        assert!(filtered_repos
            .iter()
            .all(|r| r["metadata"]["language"] == "Python" && r["metadata"]["license"] == "MIT"));
        let mcp = status["streams"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["stream"]["id"] == "mcp-public-registry")
            .unwrap();
        let tools = mcp["cache"]["items"].as_array().unwrap();
        assert!(!tools.is_empty());
        let identities = tools
            .iter()
            .map(|item| item["id"].as_str().unwrap())
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(
            identities.len(),
            tools.len(),
            "MCP versions must not overwrite each other's metadata"
        );
        for item in tools {
            assert_eq!(
                item["metric"],
                format!("Version {}", item["metadata"]["version"].as_str().unwrap())
            );
            let url = reqwest::Url::parse(item["url"].as_str().unwrap()).unwrap();
            assert!(url.query_pairs().any(|(key, value)| key == "version"
                && value == item["metadata"]["version"].as_str().unwrap()));
        }
        let report = json!({"checked_at":command_center_core::now(),"isolated_temp_database":true,"live_public_requests":true,"credentials_used":false,"filter_checks":{"huggingface_task_library_license":filtered_models.len(),"github_language_license":filtered_repos.len()},"result":result,"sources":summary});
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../target/live-discovery/native-results.json");
        std::fs::create_dir_all(out.parent().unwrap()).unwrap();
        std::fs::write(out, serde_json::to_string_pretty(&report).unwrap()).unwrap();
        for id in [
            "github-top",
            "hf-trending",
            "robotics-news",
            "mit-lectures",
            "poker-wpt",
            "agritech-india",
        ] {
            let r = summary.iter().find(|r| r["id"] == id).unwrap();
            assert!(r["count"].as_u64().unwrap() > 0, "{id}: {}", r["health"]);
        }
    }
}

fn youtube_input(raw: &str) -> Result<String> {
    if raw.starts_with("UC")
        && raw.len() == 24
        && raw
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_".contains(&c))
    {
        return Ok(raw.into());
    }
    let path = if raw.starts_with('@') {
        raw.to_owned()
    } else {
        let u = reqwest::Url::parse(raw)
            .map_err(|_| "Enter a YouTube @handle, channel URL or UC channel ID")?;
        if u.scheme() != "https"
            || !["www.youtube.com", "youtube.com"].contains(&u.host_str().unwrap_or(""))
            || !u.username().is_empty()
            || u.password().is_some()
            || u.query().is_some()
            || u.fragment().is_some()
        {
            return Err("Use an official YouTube channel URL without credentials/query".into());
        }
        u.path().trim_matches('/').to_owned()
    };
    if let Some(id) = path.strip_prefix("channel/") {
        return youtube_input(id);
    }
    if !path.starts_with('@')
        || path.len() < 2
        || path.len() > 120
        || !path[1..]
            .chars()
            .all(|c| c.is_alphanumeric() || "-_ .".contains(c) && c != ' ')
    {
        return Err("Use a YouTube @handle, /channel/UC… URL or UC channel ID".into());
    }
    Ok(path)
}
async fn resolve_youtube(raw: &str) -> Result<String> {
    let input = youtube_input(raw)?;
    if input.starts_with("UC") {
        return Ok(input);
    }
    let url = reqwest::Url::parse(&format!("https://www.youtube.com/{input}"))
        .map_err(|_| "Invalid channel handle")?;
    let mut response = crate::research::public_client(&url)
        .await?
        .get(url)
        .send()
        .await
        .map_err(|_| "YouTube channel lookup failed")?;
    if !response.status().is_success() {
        return Err(format!(
            "YouTube channel lookup returned {}",
            response.status()
        ));
    }
    let mut bytes = vec![];
    let mut received = 0;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "Channel lookup interrupted")?
    {
        received += chunk.len();
        if received > 8 * 1024 * 1024 {
            break;
        }
        bytes.extend_from_slice(&chunk);
        let text = String::from_utf8_lossy(&bytes);
        if let Some(id) = channel_from_html(&text) {
            return Ok(id);
        }
        if bytes.len() > 256 * 1024 {
            bytes.drain(..bytes.len() - 256 * 1024);
        }
    }
    Err("Could not resolve the public channel handle; try the channel's UC ID".into())
}
fn channel_from_html(html: &str) -> Option<String> {
    // externalId is channel metadata. Do not pick recommended-video channelId values.
    let id = html.split("\"externalId\":\"").nth(1)?.split('"').next()?;
    (id.starts_with("UC")
        && id.len() == 24
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_".contains(&c)))
    .then(|| id.to_owned())
}

/// Remote source window has no capabilities; only the local `main` window has IPC access.
#[tauri::command]
pub async fn open_discovery_reader(app: tauri::AppHandle, url: String) -> Result<()> {
    use tauri::Manager;
    static ALLOWED: std::sync::OnceLock<std::sync::Mutex<String>> = std::sync::OnceLock::new();
    let allowed = ALLOWED.get_or_init(|| std::sync::Mutex::new(String::new()));
    let mut u = reqwest::Url::parse(&url).map_err(|_| "Invalid source URL")?;
    if u.scheme() == "http" && u.host_str() == Some("arxiv.org") {
        let _ = u.set_scheme("https");
    }
    crate::research::public_addresses(&u).await?;
    let reader_title = format!(
        "{} · Source reader",
        u.host_str().unwrap_or("Public source")
    );
    *allowed.lock().map_err(|_| "Reader unavailable")? = u
        .host_str()
        .ok_or("Source URL has no host")?
        .trim_start_matches("www.")
        .to_owned();
    if let Some(window) = app.get_webview_window("discovery-reader") {
        window.navigate(u).map_err(|e| e.to_string())?;
        window.set_title(&reader_title).map_err(|e| e.to_string())?;
        window.show().map_err(|e| e.to_string())?;
        window.set_focus().map_err(|e| e.to_string())?;
        return Ok(());
    }
    tauri::WebviewWindowBuilder::new(&app, "discovery-reader", tauri::WebviewUrl::External(u))
        .title(&reader_title)
        .inner_size(1120., 800.)
        .on_download(|_, _| false)
        .on_navigation(|u| {
            u.scheme() == "https"
                && u.username().is_empty()
                && u.password().is_none()
                && ALLOWED
                    .get()
                    .and_then(|m| m.lock().ok())
                    .is_some_and(|host| {
                        u.host_str()
                            .is_some_and(|h| h.trim_start_matches("www.") == host.as_str())
                    })
        })
        .build()
        .map_err(|e| e.to_string())?;
    Ok(())
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RedditAccess {
    client_id: String,
    refresh_token: String,
}
impl Drop for RedditAccess {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.client_id.zeroize();
        self.refresh_token.zeroize();
    }
}
#[tauri::command]
pub fn save_discovery_reddit_access(state: State<AppState>, access: RedditAccess) -> Result<()> {
    if access.client_id.len() < 5
        || access.client_id.len() > 200
        || access.refresh_token.len() < 15
        || access.refresh_token.len() > 4096
        || access.client_id.chars().any(char::is_whitespace)
        || access.refresh_token.chars().any(char::is_whitespace)
    {
        return Err("Enter your approved Reddit app client ID and read-only refresh token".into());
    }
    let encoded = zeroize::Zeroizing::new(
        serde_json::to_string(&access).map_err(|_| "Invalid access configuration")?,
    );
    keyring::Entry::new("com.personal.commandcenter.api", "discovery-reddit")
        .map_err(|e| e.to_string())?
        .set_password(&encoded)
        .map_err(|_| "OS secure store could not save Reddit access")?;
    state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .set_integration_flag("discovery_reddit_configured", true)
}
#[tauri::command]
pub fn remove_discovery_reddit_access(state: State<AppState>) -> Result<()> {
    state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .set_integration_flag("discovery_reddit_configured", false)?;
    match keyring::Entry::new("com.personal.commandcenter.api", "discovery-reddit")
        .map_err(|e| e.to_string())?
        .delete_credential()
    {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(_) => Err("OS secure store could not remove Reddit access".into()),
    }
}
async fn reddit_authorized(state: &AppState, s: &Stream) -> Result<Option<Vec<Value>>> {
    if state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .setting("discovery_reddit_configured")?
        .as_deref()
        != Some("true")
    {
        return Ok(None);
    }
    let secret = match keyring::Entry::new("com.personal.commandcenter.api", "discovery-reddit")
        .map_err(|_| "Secure store unavailable")?
        .get_password()
    {
        Ok(s) => zeroize::Zeroizing::new(s),
        Err(keyring::Error::NoEntry) => return Ok(None),
        Err(_) => return Err("Reddit secure store access failed".into()),
    };
    let access: RedditAccess =
        serde_json::from_str(&secret).map_err(|_| "Invalid saved Reddit access")?;
    let endpoint = reqwest::Url::parse("https://www.reddit.com/api/v1/access_token").unwrap();
    let c = crate::research::public_client(&endpoint).await?;
    let response = c
        .post(endpoint)
        .basic_auth(&access.client_id, Some(""))
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", access.refresh_token.as_str()),
        ])
        .send()
        .await
        .map_err(|_| "Reddit read-token refresh failed")?;
    let auth = zeroize::Zeroizing::new(
        crate::connectors::json_response(response)
            .await?
            .to_string(),
    );
    let auth: Value =
        serde_json::from_str(&auth).map_err(|_| "Invalid Reddit authorization response")?;
    let scope = auth["scope"].as_str().unwrap_or("");
    if !scope.split_whitespace().any(|s| s == "read" || s == "*") {
        return Err("Reddit authorization must include read scope".into());
    }
    let token = zeroize::Zeroizing::new(
        auth["access_token"]
            .as_str()
            .ok_or("Reddit did not return a read token")?
            .to_owned(),
    );
    let order = if ["hot", "new", "top"].contains(&s.options.sort.as_str()) {
        s.options.sort.as_str()
    } else {
        "hot"
    };
    let endpoint = reqwest::Url::parse(&format!(
        "https://oauth.reddit.com/r/{}/{order}.json?limit=30&t=week",
        s.query
    ))
    .map_err(|_| "Invalid subreddit")?;
    let response = crate::research::public_client(&endpoint)
        .await?
        .get(endpoint)
        .bearer_auth(token.as_str())
        .send()
        .await
        .map_err(|_| "Reddit read request failed")?;
    let v = crate::connectors::json_response(response).await?;
    let mut rows = vec![];
    for r in v["data"]["children"]
        .as_array()
        .ok_or("Reddit response has no post array")?
    {
        let p = &r["data"];
        if p["over_18"] == true || p["stickied"] == true {
            continue;
        }
        let path = p["permalink"].as_str().unwrap_or("");
        if !path.starts_with("/r/") {
            continue;
        }
        let date = p["created_utc"]
            .as_f64()
            .and_then(|t| chrono::DateTime::from_timestamp(t as i64, 0))
            .map(|t| t.to_rfc3339());
        rows.push(row(
            "reddit",
            p["title"].as_str().unwrap_or(""),
            &format!("https://www.reddit.com{path}"),
            p["selftext"].as_str().unwrap_or(""),
            &format!(
                "{} points · {} comments · r/{}",
                p["score"].as_i64().unwrap_or(0),
                p["num_comments"].as_u64().unwrap_or(0),
                s.query
            ),
            date.as_deref(),
        ));
    }
    Ok(Some(rows))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportedLink {
    title: String,
    url: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    topic: String,
    #[serde(default)]
    published_at: Option<String>,
}
fn import_links(state: &AppState, items: Vec<ImportedLink>) -> Result<usize> {
    if items.is_empty() || items.len() > 500 {
        return Err("Import 1–500 public links at a time".into());
    }
    let mut incoming = vec![];
    for i in items {
        let topic = if i.topic.is_empty() {
            "custom"
        } else {
            &i.topic
        };
        if i.title.trim().is_empty()
            || i.title.len() > 2000
            || i.description.len() > 12000
            || !topics_valid(topic)
            || !valid_public_link(&i.url)
            || i.url.len() > 2000
            || i.published_at
                .as_ref()
                .is_some_and(|s| chrono::DateTime::parse_from_rfc3339(s).is_err())
        {
            return Err("Each link needs a title, public HTTP(S) URL without credentials, supported topic and optional RFC3339 publication date".into());
        }
        let u = reqwest::Url::parse(&i.url).unwrap();
        let host = u.host_str().unwrap_or("").trim_start_matches("www.");
        let provider = match host {
            "x.com" | "twitter.com" => "x",
            "linkedin.com" => "linkedin",
            "reddit.com" | "old.reddit.com" => "reddit",
            "bsky.app" => "bluesky",
            "youtube.com" | "youtu.be" => "youtube",
            "github.com" => "github",
            _ => "import",
        };
        let mut r = row(
            provider,
            &i.title,
            &i.url,
            &i.description,
            "Imported link · user-supplied summary",
            i.published_at.as_deref(),
        );
        r["verification"]=json!("Imported locally. Content, date and title are user-supplied, not fetched or independently verified.");
        r["imported"] = json!(true);
        r["topic"] = json!(topic);
        r["kind"] = json!(if provider == "youtube" {
            "videos"
        } else if provider == "github" {
            "repos"
        } else {
            "social"
        });
        incoming.push(r);
    }
    let mut streams = list(state)?;
    if !streams.iter().any(|s| s.id == "imported-links") {
        let mut s = stream(
            "imported-links",
            "Imported & saved social links",
            "custom",
            "import",
            "",
            "social",
            "world",
        );
        s.enabled = false;
        streams.push(s);
    }
    if streams.len() > 64 {
        return Err("Source limit reached".into());
    }
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    let existing = repo
        .feeds()?
        .into_iter()
        .find(|f| f.source == "hub:imported-links")
        .and_then(|f| f.items.as_array().cloned())
        .unwrap_or_default();
    let count = incoming.len();
    let mut seen = std::collections::HashSet::new();
    incoming.extend(existing);
    incoming.retain(|i| seen.insert(i["url"].as_str().unwrap_or("").to_owned()));
    incoming.truncate(1000);
    repo.set_setting(
        "discovery_hub_streams",
        &serde_json::to_string(&streams).map_err(|e| e.to_string())?,
    )?;
    repo.cache_feed("hub:imported-links", &json!(incoming))?;
    Ok(count)
}
fn topics_valid(s: &str) -> bool {
    [
        "ai",
        "code",
        "robotics",
        "agriculture",
        "poker",
        "learning",
        "custom",
    ]
    .contains(&s)
}
#[tauri::command]
pub fn save_discovery_imports(state: State<AppState>, items: Vec<ImportedLink>) -> Result<usize> {
    import_links(&state, items)
}
#[tauri::command]
pub async fn choose_discovery_link_import(state: State<'_, AppState>) -> Result<Option<usize>> {
    let Some(file) = rfd::AsyncFileDialog::new()
        .set_title("Import public links JSON")
        .add_filter("Public links JSON", &["json"])
        .pick_file()
        .await
    else {
        return Ok(None);
    };
    let path = file.path().to_owned();
    let items=tokio::task::spawn_blocking(move||->Result<Vec<ImportedLink>>{use std::io::Read;let mut bytes=vec![];std::fs::File::open(path).map_err(|_|"Could not open chosen file")?.take(4*1024*1024+1).read_to_end(&mut bytes).map_err(|_|"Could not read chosen file")?;if bytes.len()>4*1024*1024{return Err("Link import exceeds 4 MiB".into());}serde_json::from_slice(&bytes).map_err(|_|"Expected a JSON array of public link objects with title, url, optional description, topic and published_at".to_string())}).await.map_err(|_|"Import reader interrupted")??;
    import_links(&state, items).map(Some)
}

#[cfg(test)]
pub(crate) mod failure_tests {
    use super::*;
    pub(crate) fn state() -> AppState {
        use std::{collections::HashMap, sync::Mutex};
        AppState {
            repo: Mutex::new(command_center_core::Repository::open(":memory:").unwrap()),
            peer_sync: Mutex::new(crate::peer_sync::Runtime::default()),
            project_watchers: Mutex::new(HashMap::new()),
            pending_update: Mutex::new(None),
            automation_lock: tokio::sync::Mutex::new(()),
            backup_lock: tokio::sync::Mutex::new(()),
            data_epoch: std::sync::atomic::AtomicU64::new(0),
            pending: Mutex::new(None),
            large_import: Mutex::new(None),
            google_cache: Mutex::new(None),
            vault: Mutex::new(None),
            jobs: Mutex::new(HashMap::new()),
        }
    }
    #[tokio::test]
    async fn failure_keeps_last_cache_and_reports_health() {
        let state = state();
        let mut source = stream(
            "fixture", "Fixture", "code", "linkedin", "", "social", "world",
        );
        source.enabled = true;
        let cache = json!([{"id":"fixture","title":"Last successful result"}]);
        {
            let repo = state.repo.lock().unwrap();
            repo.set_setting("discovery_hub_streams", &json!([source]).to_string())
                .unwrap();
            repo.cache_feed("hub:fixture", &cache).unwrap();
        }
        assert!(refresh(&state, "fixture").await.is_err());
        let repo = state.repo.lock().unwrap();
        assert_eq!(repo.feeds().unwrap()[0].items, cache);
        let health: Value =
            serde_json::from_str(&repo.setting("hub_health:fixture").unwrap().unwrap()).unwrap();
        assert_eq!(health["status"], "error");
        assert!(state.jobs.lock().unwrap().is_empty());
    }
    #[test]
    fn link_import_validates_whole_batch_before_writing() {
        let state = state();
        let items = vec![
            ImportedLink {
                title: "Valid post".into(),
                url: "https://www.linkedin.com/posts/public".into(),
                description: "Own summary".into(),
                topic: "robotics".into(),
                published_at: None,
            },
            ImportedLink {
                title: "Unsafe".into(),
                url: "javascript:alert(1)".into(),
                description: "".into(),
                topic: "custom".into(),
                published_at: None,
            },
        ];
        assert!(import_links(&state, items).is_err());
        assert!(state.repo.lock().unwrap().feeds().unwrap().is_empty());
        let count = import_links(
            &state,
            vec![ImportedLink {
                title: "Public post".into(),
                url: "https://x.com/example/status/1".into(),
                description: "User supplied".into(),
                topic: "ai".into(),
                published_at: None,
            }],
        )
        .unwrap();
        assert_eq!(count, 1);
        let rows = status(&state).unwrap();
        let s = rows["streams"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["stream"]["id"] == "imported-links")
            .unwrap();
        assert_eq!(s["stream"]["enabled"], false);
        assert_eq!(s["cache"]["items"][0]["imported"], true);
        assert_eq!(s["cache"]["items"][0]["source"], "x");
    }
    #[test]
    fn youtube_handles_accept_only_channel_locations() {
        assert!(youtube_input("@MITCSAIL").is_ok());
        assert_eq!(
            youtube_input("https://www.youtube.com/channel/UCEBb1b_L6zDS3xTUrIALZOw").unwrap(),
            "UCEBb1b_L6zDS3xTUrIALZOw"
        );
        assert!(youtube_input("https://youtube.com.evil.example/@person").is_err());
        assert!(youtube_input("https://user:pass@youtube.com/@person").is_err());
        assert!(youtube_input("https://www.youtube.com/watch?v=video").is_err());
        assert_eq!(channel_from_html(r#"{"channelId":"UCrecommended000000000000","channelMetadataRenderer":{"externalId":"UCEBb1b_L6zDS3xTUrIALZOw"}}"#).unwrap(),"UCEBb1b_L6zDS3xTUrIALZOw");
    }
}

#[cfg(test)]
mod watched_event_tests {
    use super::*;
    #[test]
    fn event_details_alert_once_and_keep_saved_plans() {
        let mut repo = command_center_core::Repository::open(":memory:").unwrap();
        let old = json!({"id":"fixture","title":"Tournament","start_date":"2026-12-01","description":"Registration soon","url":"https://example.com/tournament"});
        let mut record = command_center_core::workspace::Record::new(
            "poker",
            "My tournament plan",
            &repo.device_id,
        );
        record
            .fields
            .insert("hub_watch_stream".into(), "fixture-stream".into());
        record
            .fields
            .insert("hub_watch_id".into(), "fixture".into());
        record.fields.insert(
            "hub_watch_snapshot".into(),
            event_observation(&old).to_string(),
        );
        record.fields.insert("date".into(), "2026-12-01".into());
        let saved = repo.save_record(record).unwrap();
        let mut next = old.clone();
        next["start_date"] = json!("2026-12-02");
        next["description"] = json!("Registration open; satellite available");
        review_event_changes(&mut repo, "fixture-stream", &[next.clone()]).unwrap();
        review_event_changes(&mut repo, "fixture-stream", &[next]).unwrap();
        review_event_changes(&mut repo, "fixture-stream", &[]).unwrap();
        let rows = repo.records("", false).unwrap();
        assert_eq!(
            rows.iter().filter(|r| r.module == "notifications").count(),
            1
        );
        assert_eq!(
            rows.iter().find(|r| r.id == saved.id).unwrap().fields["date"],
            "2026-12-01"
        );
    }
}

#[cfg(test)]
mod first_install_tests {
    use super::*;
    #[test]
    fn fresh_profile_connects_public_defaults_without_accounts_and_keeps_owner_policy() {
        let repo = command_center_core::Repository::open(":memory:").unwrap();
        assert!(initialize_public_defaults(&repo).unwrap());
        assert_eq!(
            repo.setting("discovery_hub_auto_refresh")
                .unwrap()
                .as_deref(),
            Some("30")
        );
        assert!(repo.feeds().unwrap().is_empty());
        assert!(repo.list("", false).unwrap().is_empty());
        assert!(repo.records("", false).unwrap().is_empty());
        for (flag, _) in command_center_core::integrations::FLAGS {
            assert!(repo.setting(flag).unwrap().is_none());
        }
        let sources: Vec<Stream> =
            serde_json::from_str(&repo.setting("discovery_hub_streams").unwrap().unwrap()).unwrap();
        assert_eq!(sources.iter().filter(|s| s.enabled).count(), 35);
        assert!(sources.iter().any(|s| s.provider == "apps" && s.enabled));
        assert!(sources.iter().any(|s| s.provider == "mcp" && s.enabled));
        assert!(!sources.iter().any(|s| s.enabled
            && ["x", "linkedin", "reddit", "youtube", "patents", "schemes"]
                .contains(&s.provider.as_str())));
        repo.set_setting("discovery_hub_auto_refresh", "0").unwrap();
        assert!(!initialize_public_defaults(&repo).unwrap());
        assert_eq!(
            repo.setting("discovery_hub_auto_refresh")
                .unwrap()
                .as_deref(),
            Some("0")
        );
        let restored = command_center_core::Repository::open(":memory:").unwrap();
        restored.set_setting("discovery_hub_streams", "[]").unwrap();
        assert!(!initialize_public_defaults(&restored).unwrap());
        assert!(restored
            .setting("discovery_hub_auto_refresh")
            .unwrap()
            .is_none());
        assert_eq!(
            restored
                .setting("discovery_hub_streams")
                .unwrap()
                .as_deref(),
            Some("[]")
        );
    }
}

#[cfg(test)]
mod public_retry_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    #[tokio::test]
    async fn transient_public_reads_retry_once_but_auth_and_private_reads_do_not() {
        let calls = AtomicUsize::new(0);
        let result = retry_public_read(true, || async {
            if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                Err("error sending request".into())
            } else {
                Ok(7)
            }
        })
        .await
        .unwrap();
        assert_eq!(result, 7);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        let calls = AtomicUsize::new(0);
        let result = retry_public_read::<(), _, _>(true, || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Err("Source request failed: 403 Forbidden".into())
        })
        .await;
        assert!(result.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let calls = AtomicUsize::new(0);
        let result = retry_public_read::<(), _, _>(false, || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Err("error sending request".into())
        })
        .await;
        assert!(result.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}

#[cfg(test)]
mod media_post_tests {
    use super::*;
    #[test]
    fn article_only_bluesky_posts_remain_named_savable_and_source_attributed() {
        let p = json!({"record":{"text":"","embed":{"external":{"title":"Real publisher headline","description":"Publisher abstract","uri":"https://publisher.example/article"}}}});
        let (title, description, meta) = social_content(&p, "bluesky", "publisher.example");
        assert_eq!(title, "Real publisher headline");
        assert!(description.contains("Publisher abstract"));
        assert_eq!(meta["attached_url"], "https://publisher.example/article");
        assert_eq!(meta["original_text_supplied"], false);
        let item = row("bluesky", &title, "https://bsky.app/profile/publisher.example/post/abc", &description, "0 likes", None);
        let dir = tempfile::tempdir().unwrap();
        let mut repo = command_center_core::Repository::open(dir.path().join("qa.db")).unwrap();
        let now = command_center_core::now();
        let saved = repo.save(command_center_core::Item {
            id: uuid::Uuid::new_v4().to_string(), kind: "saved".into(),
            title: item["title"].as_str().unwrap().into(), body: description,
            pinned: false, priority: "normal".into(), status: "active".into(),
            due: None, recurrence: "none".into(), url: item["url"].as_str().map(str::to_owned),
            created_at: now.clone(), updated_at: now, device_id: repo.device_id.clone(),
            revision: 1, deleted_at: None,
        }).unwrap();
        assert_eq!(saved.title, "Real publisher headline");
        assert_eq!(repo.list("Real publisher", false).unwrap().len(), 1);
    }
    #[test]
    fn media_and_quote_posts_do_not_invent_body_or_blank_titles() {
        let p=json!({"record":{"text":""},"embed":{"images":[{"alt":"Field robot inspecting crops"}],"record":{"value":{"text":"Source quoted text"}}}});
        let (title,description,meta)=social_content(&p,"bluesky","robot.example");
        assert!(title.contains("no text supplied"));assert!(description.contains("Field robot inspecting crops"));assert!(description.contains("Quoted post: Source quoted text"));assert_eq!(meta["original_text_supplied"],false);
        let p=json!({"content":"<p></p>","media_attachments":[{"description":"Source photo description"}]});
        let (title,description,_)=social_content(&p,"mastodon","farmer@example.social");
        assert!(!title.is_empty());assert!(description.contains("Source photo description"));
        let (title,description,_)=social_content(&json!({}),"bluesky","unknown.example");assert!(!title.is_empty());assert!(description.contains("no text"));
    }
}

#[cfg(test)]
mod poker_live_audit {
    use super::*;
    #[tokio::test]
    #[ignore = "Explicit live public poker schedule audit; isolated memory database, no account/key"]
    async fn live_poker_schedules() {
        let state=crate::discovery_hub::failure_tests::state();
        let mut sources=vec![];
        for id in ["poker-wpt","poker-wsop","poker-stars","poker-all"] {
            let source=list(&state).unwrap().into_iter().find(|s|s.id==id).unwrap();
            let result=refresh(&state,id).await;
            sources.push(match result {
                Ok(value)=>json!({"stream":source,"health":value["health"],"cache":value["feed"]}),
                Err(error)=>json!({"stream":source,"error":error,"cache":null}),
            });
        }
        let out=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../target/poker-live");
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(out.join("native-schedules.json"),serde_json::to_string_pretty(&json!({"checked_at":command_center_core::now(),"isolated_database":true,"accounts_used":false,"api_keys_used":false,"streams":sources,"active":[],"auto_refresh_minutes":0})).unwrap()).unwrap();
        for source in &sources {assert!(source.get("error").is_none(),"{}: {}",source["stream"]["id"],source["error"]);assert!(source["cache"]["items"].as_array().unwrap().len()>0);}
    }
}
