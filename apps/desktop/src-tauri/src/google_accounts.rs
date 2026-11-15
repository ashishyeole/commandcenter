//! Read-only, user-owned desktop OAuth. No account data is written to ordinary SQLite.
use crate::{AppState, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{sync::atomic::Ordering, time::Duration};
use tauri::State;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use zeroize::{Zeroize, Zeroizing};
const MAIL: &str = "https://www.googleapis.com/auth/gmail.metadata";
const MAIL_BODY: &str = "https://www.googleapis.com/auth/gmail.readonly";
const CAL: &str = "https://www.googleapis.com/auth/calendar.events.readonly";
#[derive(Serialize, Deserialize, Default)]
struct Credentials {
    client_id: String,
    client_secret: String,
    access_token: String,
    refresh_token: String,
    expires: i64,
    scopes: String,
}
impl Drop for Credentials {
    fn drop(&mut self) {
        self.client_secret.zeroize();
        self.access_token.zeroize();
        self.refresh_token.zeroize();
    }
}
fn entry(state: &AppState) -> Result<keyring::Entry> {
    let device = state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .device_id
        .clone();
    keyring::Entry::new("com.personal.commandcenter.google", &device).map_err(|e| e.to_string())
}
fn read(state: &AppState) -> Result<Credentials> {
    let raw = Zeroizing::new(
        entry(state)?
            .get_password()
            .map_err(|_| "Configure Google desktop OAuth credentials first")?,
    );
    serde_json::from_str(&raw).map_err(|_| "Invalid saved account credentials".into())
}
fn client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())
}
async fn response(mut r: reqwest::Response) -> Result<Value> {
    if !r.status().is_success() {
        return Err(format!("Google returned HTTP {}. Reconnect for expired authorization; retry later for quota errors.",r.status()));
    }
    let mut data = Zeroizing::new(vec![]);
    while let Some(c) = r.chunk().await.map_err(|_| "Google response interrupted")? {
        if data.len() + c.len() > 4 * 1024 * 1024 {
            return Err("Provider response exceeds 4 MiB".into());
        }
        data.extend_from_slice(&c);
    }
    serde_json::from_slice(&data).map_err(|_| "Invalid provider response".into())
}
fn nonce() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}
fn active(state: &AppState, epoch: u64) -> Result<()> {
    if state.data_epoch.load(Ordering::SeqCst) != epoch
        || state
            .repo
            .lock()
            .map_err(|_| "Database unavailable")?
            .setting("google_enabled")?
            .as_deref()
            != Some("true")
    {
        return Err("Account access revoked or workspace reset".into());
    }
    Ok(())
}
struct Job<'a>(&'a AppState);
impl Drop for Job<'_> {
    fn drop(&mut self) {
        if let Ok(mut jobs) = self.0.jobs.lock() {
            jobs.remove("google:request");
        }
    }
}
fn job(state: &AppState) -> Result<(Job<'_>, tokio_util::sync::CancellationToken)> {
    let mut jobs = state.jobs.lock().map_err(|_| "Job state unavailable")?;
    if jobs.contains_key("google:request") {
        return Err("A Google account operation is already running".into());
    }
    let token = tokio_util::sync::CancellationToken::new();
    jobs.insert("google:request".into(), token.clone());
    Ok((Job(state), token))
}
#[tauri::command]
pub fn google_account_status(state: State<AppState>) -> Result<Value> {
    let enabled = state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .setting("google_enabled")?
        .as_deref()
        == Some("true");
    let body_approved = state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .setting("google_body_approved")?
        .as_deref()
        == Some("true");
    let saved = read(&state).ok();
    Ok(
        json!({"body_approved":body_approved,"configured":saved.is_some(),"connected":enabled&&saved.as_ref().is_some_and(|s|!s.refresh_token.is_empty()||!s.access_token.is_empty()),"scopes":saved.as_ref().map(|s|s.scopes.as_str()).unwrap_or("")}),
    )
}
#[tauri::command]
pub fn configure_google_account(
    state: State<AppState>,
    client_id: String,
    client_secret: String,
) -> Result<()> {
    let (_job, _) = job(&state)?;
    let secret = Zeroizing::new(client_secret);
    if client_id.len() > 300
        || !client_id.ends_with(".apps.googleusercontent.com")
        || client_id.chars().any(char::is_whitespace)
        || secret.len() > 1000
    {
        return Err("Use a Google OAuth Desktop app client ID and its client secret".into());
    }
    state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .set_setting("google_enabled", "false")?;
    state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .set_setting("google_body_approved", "false")?;
    let c = Credentials {
        client_id,
        client_secret: secret.to_string(),
        access_token: String::new(),
        refresh_token: String::new(),
        expires: 0,
        scopes: String::new(),
    };
    let raw = Zeroizing::new(serde_json::to_string(&c).map_err(|e| e.to_string())?);
    entry(&state)?
        .set_password(&raw)
        .map_err(|_| "OS secure store refused OAuth configuration".into())
}
#[tauri::command]
pub async fn connect_google_account(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    mail: bool,
    calendar: bool,
    mail_bodies: Option<bool>,
    consent: bool,
) -> Result<Value> {
    if !consent || (!mail && !calendar) {
        return Err("Select read-only scopes and approve browser sign-in".into());
    }
    let (_job, cancel) = job(&state)?;
    let epoch = state.data_epoch.load(Ordering::SeqCst);
    let mut c = read(&state)?;
    let work = async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|_| "Cannot open temporary loopback OAuth callback")?;
        let redirect = format!(
            "http://127.0.0.1:{}/",
            listener
                .local_addr()
                .map_err(|_| "Callback unavailable")?
                .port()
        );
        let csrf = nonce();
        let verifier = Zeroizing::new(nonce());
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let scopes = [
            (
                mail,
                if mail_bodies.unwrap_or(false) {
                    MAIL_BODY
                } else {
                    MAIL
                },
            ),
            (calendar, CAL),
        ]
        .into_iter()
        .filter(|(b, _)| *b)
        .map(|(_, s)| s)
        .collect::<Vec<_>>()
        .join(" ");
        let mut url = reqwest::Url::parse("https://accounts.google.com/o/oauth2/v2/auth")
            .map_err(|e| e.to_string())?;
        url.query_pairs_mut().extend_pairs([
            ("client_id", c.client_id.as_str()),
            ("redirect_uri", redirect.as_str()),
            ("response_type", "code"),
            ("scope", scopes.as_str()),
            ("state", csrf.as_str()),
            ("code_challenge", challenge.as_str()),
            ("code_challenge_method", "S256"),
            ("access_type", "offline"),
            ("prompt", "consent"),
        ]);
        use tauri_plugin_opener::OpenerExt;
        app.opener()
            .open_url(url.as_str(), None::<&str>)
            .map_err(|_| "Cannot open system browser for OAuth")?;
        let callback = async {
            for _ in 0..16 {
                let (mut socket, address) = listener
                    .accept()
                    .await
                    .map_err(|_| "OAuth callback failed")?;
                if !address.ip().is_loopback() {
                    continue;
                }
                let mut bytes = Zeroizing::new(vec![]);
                let mut chunk = [0u8; 1024];
                let read = tokio::time::timeout(Duration::from_secs(5), async {
                    loop {
                        let n = socket
                            .read(&mut chunk)
                            .await
                            .map_err(|_| "Callback read failed")?;
                        if n == 0 {
                            return Err("Callback closed".to_string());
                        }
                        bytes.extend_from_slice(&chunk[..n]);
                        if bytes.len() > 16384 {
                            return Err("Callback too large".into());
                        }
                        if bytes.windows(4).any(|w| w == b"\r\n\r\n") {
                            return Ok(());
                        }
                    }
                })
                .await;
                if !matches!(read, Ok(Ok(()))) {
                    continue;
                }
                let raw = std::str::from_utf8(&bytes).map_err(|_| "Invalid callback encoding")?;
                let parts = raw
                    .lines()
                    .next()
                    .unwrap_or("")
                    .split_whitespace()
                    .collect::<Vec<_>>();
                let parsed = parts.get(1).and_then(|p| {
                    reqwest::Url::parse(&format!("{redirect}{}", p.trim_start_matches('/'))).ok()
                });
                let valid = parts.first() == Some(&"GET")
                    && parts.get(1).is_some_and(|p| p.starts_with("/?"))
                    && parsed.as_ref().is_some_and(|p| {
                        p.path() == "/"
                            && p.query_pairs().filter(|(k, _)| k == "state").count() == 1
                            && p.query_pairs().any(|(k, v)| k == "state" && v == csrf)
                    });
                let reply = if valid {
                    "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nCache-Control: no-store\r\nConnection: close\r\n\r\nAuthorization received. Return to Personal Command Center."
                } else {
                    "HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\nInvalid authorization callback."
                };
                let _ = socket.write_all(reply.as_bytes()).await;
                if !valid {
                    continue;
                }
                let p = parsed.ok_or("Invalid callback")?;
                if p.query_pairs().any(|(k, _)| k == "error") {
                    return Err("Authorization declined by the user or provider".into());
                }
                return p
                    .query_pairs()
                    .find(|(k, _)| k == "code")
                    .map(|(_, v)| Zeroizing::new(v.into_owned()))
                    .ok_or_else(|| "No authorization code returned".to_string());
            }
            Err("Too many invalid callbacks".into())
        };
        let code: Zeroizing<String> = tokio::time::timeout(Duration::from_secs(180), callback)
            .await
            .map_err(|_| "Sign-in timed out after three minutes")??;
        let token = response(
            client()?
                .post("https://oauth2.googleapis.com/token")
                .form(&[
                    ("client_id", c.client_id.as_str()),
                    ("client_secret", c.client_secret.as_str()),
                    ("code", code.as_str()),
                    ("code_verifier", verifier.as_str()),
                    ("redirect_uri", redirect.as_str()),
                    ("grant_type", "authorization_code"),
                ])
                .send()
                .await
                .map_err(|_| "OAuth token exchange failed")?,
        )
        .await?;
        c.access_token = token["access_token"]
            .as_str()
            .ok_or("Provider returned no access token")?
            .into();
        c.refresh_token = token["refresh_token"].as_str().unwrap_or("").into();
        c.expires = chrono::Utc::now().timestamp()
            + token["expires_in"]
                .as_i64()
                .unwrap_or(3600)
                .clamp(60, 86400);
        let granted = token["scope"].as_str().unwrap_or(&scopes);
        c.scopes = scopes
            .split_whitespace()
            .filter(|required| scope_allows(granted, required))
            .collect::<Vec<_>>()
            .join(" ");
        let key = entry(&state)?;
        let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
        if state.data_epoch.load(Ordering::SeqCst) != epoch || cancel.is_cancelled() {
            return Err("Sign-in cancelled or workspace reset".into());
        }
        let raw = Zeroizing::new(serde_json::to_string(&c).map_err(|e| e.to_string())?);
        key.set_password(&raw)
            .map_err(|_| "Cannot save authorization to OS secure storage")?;
        repo.set_setting(
            "google_body_approved",
            if mail && mail_bodies.unwrap_or(false) && scope_allows(&c.scopes, MAIL_BODY) {
                "true"
            } else {
                "false"
            },
        )?;
        repo.set_setting("google_enabled", "true")?;
        Ok(json!({"connected":true,"scopes":c.scopes}))
    };
    tokio::select! {_=cancel.cancelled()=>Err("Google sign-in cancelled".into()),r=work=>r}
}
async fn access(state: &AppState, epoch: u64, required: &str) -> Result<Zeroizing<String>> {
    active(state, epoch)?;
    let mut c = read(state)?;
    if !scope_allows(&c.scopes, required) {
        return Err("Reconnect and approve the required read-only scope".into());
    }
    if c.expires < chrono::Utc::now().timestamp() + 60 {
        if c.refresh_token.is_empty() {
            return Err("Authorization expired; reconnect your account".into());
        }
        let token = response(
            client()?
                .post("https://oauth2.googleapis.com/token")
                .form(&[
                    ("client_id", c.client_id.as_str()),
                    ("client_secret", c.client_secret.as_str()),
                    ("refresh_token", c.refresh_token.as_str()),
                    ("grant_type", "refresh_token"),
                ])
                .send()
                .await
                .map_err(|_| "Token refresh failed")?,
        )
        .await?;
        c.access_token = token["access_token"]
            .as_str()
            .ok_or("No refreshed access token")?
            .into();
        c.expires = chrono::Utc::now().timestamp()
            + token["expires_in"]
                .as_i64()
                .unwrap_or(3600)
                .clamp(60, 86400);
        let key = entry(state)?;
        let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
        if state.data_epoch.load(Ordering::SeqCst) != epoch
            || repo.setting("google_enabled")?.as_deref() != Some("true")
        {
            return Err("Account access revoked during refresh".into());
        }
        let raw = Zeroizing::new(serde_json::to_string(&c).map_err(|e| e.to_string())?);
        key.set_password(&raw)
            .map_err(|_| "Cannot store refreshed authorization")?;
    }
    Ok(Zeroizing::new(c.access_token.clone()))
}
#[tauri::command]
pub async fn read_google_account(
    state: State<'_, AppState>,
    kind: String,
    page_token: Option<String>,
    consent: bool,
) -> Result<Value> {
    if !consent
        || !["mail", "calendar"].contains(&kind.as_str())
        || page_token.as_ref().is_some_and(|p| p.len() > 4096)
    {
        return Err("Approve a supported read-only account request".into());
    }
    let (_job, cancel) = job(&state)?;
    let epoch = state.data_epoch.load(Ordering::SeqCst);
    let work = async {
        let token = access(&state, epoch, if kind == "mail" { MAIL } else { CAL }).await?;
        let client = client()?;
        let mut url = reqwest::Url::parse(if kind == "mail" {
            "https://gmail.googleapis.com/gmail/v1/users/me/messages"
        } else {
            "https://www.googleapis.com/calendar/v3/calendars/primary/events"
        })
        .map_err(|e| e.to_string())?;
        if let Some(page) = &page_token {
            url.query_pairs_mut().append_pair("pageToken", page);
        }
        let start = chrono::Utc::now()
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .ok_or("Invalid calendar window")?
            .and_utc();
        let min = start.to_rfc3339();
        let max = (start + chrono::Duration::days(30)).to_rfc3339();
        if kind == "mail" {
            url.query_pairs_mut()
                .extend_pairs([("labelIds", "INBOX"), ("maxResults", "20")]);
        } else {
            url.query_pairs_mut().extend_pairs([
                ("singleEvents", "true"),
                ("orderBy", "startTime"),
                ("timeMin", min.as_str()),
                ("timeMax", max.as_str()),
                ("maxResults", "100"),
                (
                    "fields",
                    "items(id,summary,start,end,status,htmlLink),nextPageToken,timeZone",
                ),
            ]);
        }
        let value = response(
            client
                .get(url)
                .bearer_auth(token.as_str())
                .send()
                .await
                .map_err(|_| "Google account request failed")?,
        )
        .await?;
        let mut rows = vec![];
        if kind == "mail" {
            for item in value["messages"].as_array().into_iter().flatten().take(20) {
                active(&state, epoch)?;
                let id = item["id"].as_str().ok_or("Invalid message identifier")?;
                if id.len() > 128 || !id.bytes().all(|b| b.is_ascii_alphanumeric()) {
                    return Err("Invalid message identifier".into());
                }
                let mut url = reqwest::Url::parse(&format!(
                    "https://gmail.googleapis.com/gmail/v1/users/me/messages/{id}"
                ))
                .map_err(|e| e.to_string())?;
                url.query_pairs_mut().extend_pairs([
                    ("format", "metadata"),
                    ("metadataHeaders", "Subject"),
                    ("metadataHeaders", "From"),
                    ("metadataHeaders", "Date"),
                    ("fields", "id,payload(headers)"),
                ]);
                let message = response(
                    client
                        .get(url)
                        .bearer_auth(token.as_str())
                        .send()
                        .await
                        .map_err(|_| "Email metadata request failed")?,
                )
                .await?;
                let header = |name: &str| {
                    message["payload"]["headers"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .find(|h| {
                            h["name"]
                                .as_str()
                                .is_some_and(|n| n.eq_ignore_ascii_case(name))
                        })
                        .and_then(|h| h["value"].as_str())
                        .unwrap_or("")
                        .chars()
                        .take(1000)
                        .collect::<String>()
                };
                rows.push(json!({"id":id,"title":header("Subject"),"from":header("From"),"date":header("Date")}));
            }
        } else {
            rows = value["items"].as_array().cloned().unwrap_or_default();
        }
        active(&state, epoch)?;
        Ok(
            json!({"kind":kind,"rows":rows,"next_page_token":value["nextPageToken"],"fetched_at":command_center_core::now(),"scope":if kind=="mail"{"Inbox headers only; no body, attachment download, marking read or sending"}else{"Primary calendar, 30 days from today (UTC); read only"}}),
        )
    };
    tokio::select! {_=cancel.cancelled()=>Err("Account request cancelled".into()),r=work=>r}
}
#[tauri::command]
pub async fn disconnect_google_account(state: State<'_, AppState>, revoke: bool) -> Result<()> {
    state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .set_setting("google_enabled", "false")?;
    state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .set_setting("google_body_approved", "false")?;
    if let Some(token) = state
        .jobs
        .lock()
        .map_err(|_| "Job state unavailable")?
        .get("google:request")
    {
        token.cancel();
    }
    let saved = read(&state).ok();
    match entry(&state)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => (),
        Err(_) => return Err("Access disabled; OS credential removal failed".into()),
    }
    if revoke {
        if let Some(c) = saved {
            let token = if c.refresh_token.is_empty() {
                &c.access_token
            } else {
                &c.refresh_token
            };
            if !token.is_empty() {
                let r=client()?.post("https://oauth2.googleapis.com/revoke").form(&[("token",token.as_str())]).send().await.map_err(|_|"Disconnected locally; provider revocation failed. Remove access in Google account settings.")?;
                if !r.status().is_success() {
                    return Err("Disconnected locally; provider revocation was not confirmed. Remove access in Google account settings.".into());
                }
            }
        }
    }
    Ok(())
}

fn scope_allows(granted: &str, required: &str) -> bool {
    granted
        .split_whitespace()
        .any(|s| s == required || (required == MAIL && s == MAIL_BODY))
}
#[tauri::command]
pub async fn read_google_message(
    state: State<'_, AppState>,
    message_id: String,
    consent: bool,
) -> Result<Value> {
    if !consent
        || message_id.is_empty()
        || message_id.len() > 100
        || !message_id.bytes().all(|c| c.is_ascii_hexdigit())
    {
        return Err("Approve reading a selected Gmail message with a valid message ID".into());
    }
    if state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .setting("google_body_approved")?
        .as_deref()
        != Some("true")
    {
        return Err("Select body access and reconnect to approve it for this app".into());
    }
    let (_job, cancel) = job(&state)?;
    let epoch = state.data_epoch.load(Ordering::SeqCst);
    let work = async {
        let token = access(&state, epoch, MAIL_BODY).await?;
        let value = response(
            client()?
                .get(format!(
                    "https://gmail.googleapis.com/gmail/v1/users/me/messages/{message_id}"
                ))
                .bearer_auth(&*token)
                .query(&[("format", "full")])
                .send()
                .await
                .map_err(|_| "Gmail body request failed")?,
        )
        .await?;
        let (text, format, truncated) = message_text(&value["payload"])?;
        let title = value["payload"]["headers"]
            .as_array()
            .and_then(|headers| {
                headers.iter().find(|h| {
                    h["name"]
                        .as_str()
                        .is_some_and(|n| n.eq_ignore_ascii_case("Subject"))
                })
            })
            .and_then(|h| h["value"].as_str())
            .unwrap_or("Gmail message text preview")
            .chars()
            .take(500)
            .collect::<String>();
        active(&state, epoch)?;
        Ok(
            json!({"kind":"mail_body","rows":[{"id":message_id,"title":title,"body":text,"format":format,"truncated":truncated}],"fetched_at":command_center_core::now(),"scope":"Selected message text only. HTML converted to inert text; no images loaded, attachment files opened/saved, mail marked read or data sent to AI. Temporary view unless explicitly saved to your encrypted vault."}),
        )
    };
    tokio::select! {_=cancel.cancelled()=>Err("Account request cancelled".into()),r=work=>r}
}
fn message_text(payload: &Value) -> Result<(String, String, bool)> {
    fn visit(
        v: &Value,
        depth: usize,
        count: &mut usize,
        plain: &mut Vec<String>,
        html: &mut Vec<String>,
        bytes: &mut usize,
    ) -> Result<()> {
        *count += 1;
        if depth > 12 || *count > 200 {
            return Err("Message MIME structure exceeds safe preview limits".into());
        }
        let mime = v["mimeType"].as_str().unwrap_or("");
        let attached = v["filename"].as_str().is_some_and(|s| !s.is_empty())
            || v["headers"].as_array().is_some_and(|headers| {
                headers.iter().any(|h| {
                    h["name"]
                        .as_str()
                        .is_some_and(|n| n.eq_ignore_ascii_case("Content-Disposition"))
                        && h["value"].as_str().is_some_and(|v| {
                            v.split(';')
                                .next()
                                .unwrap_or("")
                                .trim()
                                .eq_ignore_ascii_case("attachment")
                        })
                })
            });
        if attached {
            return Ok(());
        }
        if v["filename"].as_str().unwrap_or("").is_empty()
            && ["text/plain", "text/html"].contains(&mime)
        {
            if let Some(data) = v["body"]["data"].as_str() {
                if data.len() > 1024 * 1024 {
                    return Err("Message text part exceeds 1 MiB preview limit".into());
                }
                let decoded = URL_SAFE_NO_PAD
                    .decode(data.trim_end_matches('='))
                    .map_err(|_| "Invalid encoded message text")?;
                *bytes += decoded.len();
                if *bytes > 1024 * 1024 {
                    return Err("Combined message text exceeds 1 MiB preview limit".into());
                }
                let text = String::from_utf8_lossy(&decoded).into_owned();
                if mime == "text/plain" {
                    plain.push(text)
                } else {
                    html.push(text)
                }
            }
        }
        for child in v["parts"].as_array().into_iter().flatten() {
            visit(child, depth + 1, count, plain, html, bytes)?;
        }
        Ok(())
    }
    let (mut plain, mut html, mut count, mut bytes) = (vec![], vec![], 0, 0);
    visit(payload, 0, &mut count, &mut plain, &mut html, &mut bytes)?;
    let (text, format) = if !plain.is_empty() {
        (plain.join("\n\n"), "text/plain")
    } else if !html.is_empty() {
        let markup = html.join("\n");
        if markup.bytes().filter(|c| *c == b'<').count() > 20_000 {
            return Err("Message HTML exceeds preview markup limits".into());
        }
        let doc = dom_query::Document::from(markup);
        doc.select("script,style,noscript,iframe,object,embed")
            .remove();
        (
            doc.select("body").text().to_string(),
            "HTML converted to text",
        )
    } else {
        (
            "No supported inline text body. Attachments are not downloaded.".into(),
            "no inline text",
        )
    };
    let mut end = text.len().min(12000);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let truncated = end < text.len();
    Ok((text[..end].to_owned(), format.into(), truncated))
}
#[cfg(test)]
mod message_tests {
    use super::*;
    #[test]
    fn body_scope_does_not_follow_from_metadata_grant() {
        assert!(!scope_allows(MAIL, MAIL_BODY));
        assert!(scope_allows(MAIL_BODY, MAIL));
        assert!(!scope_allows(CAL, MAIL_BODY));
    }
    #[test]
    fn mime_preview_prefers_plain_and_ignores_attachments() {
        let value = json!({"parts":[{"mimeType":"text/html","body":{"data":URL_SAFE_NO_PAD.encode("<b>HTML</b>")}},{"mimeType":"text/plain","body":{"data":URL_SAFE_NO_PAD.encode("Plain fixture")}},{"mimeType":"text/plain","filename":"attachment.txt","body":{"data":URL_SAFE_NO_PAD.encode("Ignored attachment")}}]});
        let (text, _, truncated) = message_text(&value).unwrap();
        assert_eq!(text, "Plain fixture");
        assert!(!truncated);
    }
    #[test]
    fn unnamed_attachment_subtrees_are_not_used_as_message_body() {
        let value = json!({"headers":[{"name":"Content-Disposition","value":"attachment"}],"parts":[{"mimeType":"text/plain","body":{"data":URL_SAFE_NO_PAD.encode("Do not preview attached content")}}]});
        let (text, _, _) = message_text(&value).unwrap();
        assert!(!text.contains("Do not preview"));
    }
    #[test]
    fn html_is_inert_and_preview_is_utf8_bounded() {
        let value = json!({"mimeType":"text/html","body":{"data":URL_SAFE_NO_PAD.encode("<p>Visible</p><script>hidden()</script><img src='https://example.test/tracker'>")}});
        let (text, _, _) = message_text(&value).unwrap();
        assert!(text.contains("Visible"));
        assert!(!text.contains("hidden"));
        let value = json!({"mimeType":"text/plain","body":{"data":URL_SAFE_NO_PAD.encode(format!("a{}","🧪".repeat(6000)))}});
        let (text, _, truncated) = message_text(&value).unwrap();
        assert!(truncated && text.len() <= 12000);
    }
}
