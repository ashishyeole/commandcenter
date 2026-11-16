//! Read-only GitHub App OAuth with PKCE. Private repository responses remain transient.
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
#[derive(Serialize, Deserialize, Default)]
struct Credentials {
    client_id: String,
    client_secret: String,
    access_token: String,
    refresh_token: String,
    expires: i64,
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
    keyring::Entry::new("com.personal.commandcenter.github", &device).map_err(|e| e.to_string())
}
fn read(state: &AppState) -> Result<Credentials> {
    let raw = Zeroizing::new(
        entry(state)?
            .get_password()
            .map_err(|_| "Configure GitHub desktop OAuth credentials first")?,
    );
    serde_json::from_str(&raw).map_err(|_| "Invalid saved account credentials".into())
}
fn client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .https_only(true)
        .user_agent("PersonalCommandCenter/0.5")
        .default_headers({
            let mut h = reqwest::header::HeaderMap::new();
            h.insert(
                reqwest::header::ACCEPT,
                reqwest::header::HeaderValue::from_static("application/json"),
            );
            h.insert(
                "X-GitHub-Api-Version",
                reqwest::header::HeaderValue::from_static("2026-03-10"),
            );
            h
        })
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())
}
async fn response(mut r: reqwest::Response) -> Result<Value> {
    if !r.status().is_success() {
        return Err(format!("GitHub returned HTTP {}. Reconnect for expired authorization; retry later for quota errors.",r.status()));
    }
    let mut data = Zeroizing::new(vec![]);
    while let Some(c) = r.chunk().await.map_err(|_| "GitHub response interrupted")? {
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
            .setting("github_enabled")?
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
            jobs.remove("github:request");
        }
    }
}
fn job(state: &AppState) -> Result<(Job<'_>, tokio_util::sync::CancellationToken)> {
    let mut jobs = state.jobs.lock().map_err(|_| "Job state unavailable")?;
    if jobs.contains_key("github:request") {
        return Err("A GitHub account operation is already running".into());
    }
    let token = tokio_util::sync::CancellationToken::new();
    jobs.insert("github:request".into(), token.clone());
    Ok((Job(state), token))
}
#[tauri::command]
pub fn github_account_status(state: State<AppState>) -> Result<Value> {
    let enabled = state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .setting("github_enabled")?
        .as_deref()
        == Some("true");
    let c = read(&state).ok();
    Ok(
        json!({"configured":c.is_some(),"enabled":enabled,"client_id":c.as_ref().map(|c|&c.client_id),"connected":enabled&&c.as_ref().is_some_and(|c|!c.access_token.is_empty()),"callback":"http://127.0.0.1:48673/"}),
    )
}
#[tauri::command]
pub fn configure_github_account(
    state: State<AppState>,
    client_id: String,
    client_secret: String,
) -> Result<()> {
    if client_id.is_empty()
        || client_id.len() > 200
        || client_secret.len() > 1000
        || client_secret.is_empty()
        || !client_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-')
    {
        return Err("Enter your GitHub App client ID and client secret".into());
    }
    if state
        .jobs
        .lock()
        .map_err(|_| "Job state unavailable")?
        .contains_key("github:request")
    {
        return Err("Finish or cancel the GitHub request first".into());
    }
    let c = Credentials {
        client_id,
        client_secret,
        access_token: String::new(),
        refresh_token: String::new(),
        expires: 0,
    };
    let raw = Zeroizing::new(serde_json::to_string(&c).map_err(|e| e.to_string())?);
    entry(&state)?
        .set_password(&raw)
        .map_err(|_| "OS secure store refused credentials")?;
    state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .set_integration_flag("github_enabled", false)
}
#[tauri::command]
pub async fn connect_github_account(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    consent: bool,
) -> Result<Value> {
    if !consent {
        return Err(
            "Approve GitHub App browser sign-in with read-only repository permissions".into(),
        );
    }
    let (_job, cancel) = job(&state)?;
    let epoch = state.data_epoch.load(Ordering::SeqCst);
    let mut c = read(&state)?;
    let work = async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:48673")
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
        let mut url = reqwest::Url::parse("https://github.com/login/oauth/authorize")
            .map_err(|e| e.to_string())?;
        url.query_pairs_mut().extend_pairs([
            ("client_id", c.client_id.as_str()),
            ("redirect_uri", redirect.as_str()),
            ("response_type", "code"),
            ("scope", "offline_access"),
            ("state", csrf.as_str()),
            ("code_challenge", challenge.as_str()),
            ("code_challenge_method", "S256"),
            ("prompt", "select_account"),
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
                .post("https://github.com/login/oauth/access_token")
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
        let key = entry(&state)?;
        let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
        if state.data_epoch.load(Ordering::SeqCst) != epoch || cancel.is_cancelled() {
            return Err("Sign-in cancelled or workspace reset".into());
        }
        let raw = Zeroizing::new(serde_json::to_string(&c).map_err(|e| e.to_string())?);
        key.set_password(&raw)
            .map_err(|_| "Cannot save authorization to OS secure storage")?;
        repo.set_integration_flag("github_enabled", true)?;
        Ok(
            json!({"connected":true,"notice":"Only read endpoints are implemented. Configure the GitHub App with read-only contents, issues, pull requests and metadata; install it on selected repositories only."}),
        )
    };
    tokio::select! {_=cancel.cancelled()=>Err("GitHub sign-in cancelled".into()),r=work=>r}
}
async fn access(state: &AppState, epoch: u64) -> Result<Zeroizing<String>> {
    active(state, epoch)?;
    let mut c = read(state)?;
    if c.access_token.is_empty() {
        return Err("Connect GitHub first".into());
    }
    if c.expires > chrono::Utc::now().timestamp() + 60 {
        return Ok(Zeroizing::new(c.access_token.clone()));
    }
    if c.refresh_token.is_empty() {
        return Err("GitHub authorization expired; reconnect".into());
    }
    let v = response(
        client()?
            .post("https://github.com/login/oauth/access_token")
            .form(&[
                ("client_id", c.client_id.as_str()),
                ("client_secret", c.client_secret.as_str()),
                ("grant_type", "refresh_token"),
                ("refresh_token", c.refresh_token.as_str()),
            ])
            .send()
            .await
            .map_err(|_| "GitHub token refresh failed")?,
    )
    .await?;
    c.access_token = v["access_token"]
        .as_str()
        .ok_or("Reconnect GitHub; token refresh failed")?
        .into();
    c.refresh_token = v["refresh_token"]
        .as_str()
        .ok_or("Provider omitted rotated refresh token")?
        .into();
    c.expires =
        chrono::Utc::now().timestamp() + v["expires_in"].as_i64().unwrap_or(28800).clamp(60, 86400);
    let key = entry(state)?;
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    if state.data_epoch.load(Ordering::SeqCst) != epoch
        || repo.setting("github_enabled")?.as_deref() != Some("true")
    {
        return Err("Account access revoked or workspace reset".into());
    }
    key.set_password(&Zeroizing::new(
        serde_json::to_string(&c).map_err(|e| e.to_string())?,
    ))
    .map_err(|_| "Cannot store refreshed credential")?;
    Ok(Zeroizing::new(c.access_token.clone()))
}
fn segment(s: &str) -> Result<()> {
    if s.is_empty()
        || s.len() > 100
        || s == "."
        || s == ".."
        || !s
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    {
        return Err("Enter an owner and repository name, without a URL".into());
    }
    Ok(())
}
#[tauri::command]
pub async fn read_github_account(
    state: State<'_, AppState>,
    resource: String,
    owner: String,
    repository: String,
    page: u32,
    consent: bool,
) -> Result<Value> {
    if !consent {
        return Err("Approve this private repository read".into());
    }
    if page == 0 || page > 100 {
        return Err("Page must be between 1 and 100".into());
    }
    let (_job, cancel) = job(&state)?;
    let epoch = state.data_epoch.load(Ordering::SeqCst);
    let work = async {
        let token = access(&state, epoch).await?;
        let path = match resource.as_str() {
            "repositories" => format!("/user/repos?per_page=20&page={page}&sort=updated"),
            "installations" => format!("/user/installations?per_page=20&page={page}"),
            "metadata" | "commits" | "issues" | "pulls" => {
                segment(&owner)?;
                segment(&repository)?;
                let suffix = if resource == "metadata" {
                    String::new()
                } else {
                    format!(
                        "/{resource}?per_page=20&page={page}{}",
                        if resource == "issues" || resource == "pulls" {
                            "&state=all"
                        } else {
                            ""
                        }
                    )
                };
                format!("/repos/{owner}/{repository}{suffix}")
            }
            _ => return Err("Unsupported GitHub read".into()),
        };
        let value = response(
            client()?
                .get(format!("https://api.github.com{path}"))
                .bearer_auth(token.as_str())
                .send()
                .await
                .map_err(|_| "GitHub request failed")?,
        )
        .await?;
        active(&state, epoch)?;
        // Read-only endpoints only. Private content is never saved in ordinary records or AI context.
        Ok(
            json!({"resource":resource,"page":page,"observed_at":command_center_core::now(),"source_url":format!("https://api.github.com{path}"),"data":value,"privacy":"transient_private_view","notice":"Only selected installed repositories and app permissions are accessible. Repository writes and source-file download are not exposed."}),
        )
    };
    tokio::select! {biased;_=cancel.cancelled()=>Err("GitHub read cancelled".into()),r=work=>r}
}
#[tauri::command]
pub async fn disconnect_github_account(state: State<'_, AppState>, revoke: bool) -> Result<()> {
    let c = read(&state).ok();
    state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .set_integration_flag("github_enabled", false)?;
    if let Some(t) = state
        .jobs
        .lock()
        .map_err(|_| "Job state unavailable")?
        .get("github:request")
    {
        t.cancel();
    }
    match entry(&state)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => (),
        Err(_) => return Err("Access disabled; secure credential removal failed".into()),
    }
    if revoke {
        if let Some(c) = c {
            if !c.access_token.is_empty() {
                let r=client()?.delete(format!("https://api.github.com/applications/{}/grant",c.client_id)).basic_auth(&c.client_id,Some(&c.client_secret)).json(&json!({"access_token":c.access_token})).send().await.map_err(|_|"Disconnected locally, but provider revocation failed; revoke the app in GitHub settings")?;
                if !r.status().is_success() && r.status().as_u16() != 404 {
                    return Err("Disconnected locally; provider rejected revocation. Revoke access in GitHub settings.".into());
                }
            }
        }
    }
    Ok(())
}
