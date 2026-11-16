//! Ciphertext-only bounded WebDAV transfer; staged manifest verification and complete readback.
use crate::{large_archives::Cancellable, AppState, Result};
use command_center_core::{now, stream_archive::BYTE_LIMIT};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tauri::Manager;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;
const JOB: &str = "backup:webdav-stream";
struct Job {
    app: tauri::AppHandle,
    token: CancellationToken,
}
impl Job {
    fn start(app: &tauri::AppHandle) -> Result<Self> {
        let state = app.state::<AppState>();
        let mut jobs = state.jobs.lock().map_err(|_| "Jobs unavailable")?;
        if jobs.contains_key(JOB) {
            return Err("A streaming WebDAV transfer is already active".into());
        }
        let token = CancellationToken::new();
        jobs.insert(JOB.into(), token.clone());
        Ok(Self {
            app: app.clone(),
            token,
        })
    }
}
impl Drop for Job {
    fn drop(&mut self) {
        if let Ok(mut jobs) = self.app.state::<AppState>().jobs.lock() {
            jobs.remove(JOB);
        }
    }
}
fn credentials(device: &str) -> Result<crate::webdav_backup::Credentials> {
    let raw = Zeroizing::new(
        crate::webdav_backup::entry(device)?
            .get_password()
            .map_err(|_| "WebDAV credentials unavailable")?,
    );
    serde_json::from_str(&raw).map_err(|_| "Invalid WebDAV credentials".into())
}
fn client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .https_only(true)
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(1800))
        .build()
        .map_err(|e| e.to_string())
}
async fn send(
    request: reqwest::RequestBuilder,
    cancel: &CancellationToken,
) -> Result<reqwest::Response> {
    tokio::select! {_=cancel.cancelled()=>Err("WebDAV transfer cancelled; remote ciphertext may remain".into()),r=request.send()=>r.map_err(|_|"WebDAV request failed".to_string())}
}
async fn checksum(
    mut response: reqwest::Response,
    expected: u64,
    cancel: &CancellationToken,
    mut output: Option<tokio::fs::File>,
) -> Result<(String, Option<String>)> {
    if cancel.is_cancelled() {
        return Err("WebDAV transfer cancelled".into());
    }
    if !response.status().is_success() {
        return Err(format!("WebDAV read returned HTTP {}", response.status()));
    }
    if expected > BYTE_LIMIT {
        return Err("Remote archive exceeds 8 GiB".into());
    }
    let etag = response
        .headers()
        .get(reqwest::header::ETAG)
        .and_then(|v| v.to_str().ok())
        .filter(|v| v.starts_with('"') && v.ends_with('"'))
        .map(str::to_owned);
    let mut bytes = 0u64;
    let mut digest = Sha256::new();
    loop {
        let chunk = tokio::select! {_=cancel.cancelled()=>return Err("Streaming WebDAV transfer cancelled; a remote encrypted file may remain".into()),v=response.chunk()=>v.map_err(|_|"WebDAV read interrupted")?};
        let Some(chunk) = chunk else { break };
        bytes = bytes
            .checked_add(chunk.len() as u64)
            .ok_or("Archive size overflow")?;
        if bytes > expected || bytes > BYTE_LIMIT {
            return Err("Remote archive size changed".into());
        }
        digest.update(&chunk);
        if let Some(output) = output.as_mut() {
            output
                .write_all(&chunk)
                .await
                .map_err(|_| "Could not stage downloaded archive")?;
        }
    }
    if bytes != expected {
        return Err("Remote archive is truncated or changed".into());
    }
    if let Some(output) = output.as_mut() {
        output
            .sync_all()
            .await
            .map_err(|_| "Could not flush archive")?;
    }
    Ok((format!("{:x}", digest.finalize()), etag))
}
pub(crate) async fn execute(
    app: tauri::AppHandle,
    config: crate::backups::BackupSchedule,
    device: String,
    destination: String,
    epoch: u64,
) -> Result<Value> {
    let job = Job::start(&app)?;
    let token = job.token.clone();
    let root = crate::webdav_backup::endpoint(&destination)?;
    let password = Zeroizing::new(
        crate::backups::key(&device)?
            .get_password()
            .map_err(|_| "Backup recovery password unavailable")?,
    );
    let stage_app = app.clone();
    let stage_token = token.clone();
    let stage_password = Zeroizing::new(password.to_string());
    let include_vault = config.include_vault;
    let (temp, count, size, digest) = tokio::task::spawn_blocking(move || -> Result<_> {
        use std::io::{Read, Write};
        let state = stage_app.state::<AppState>();
        let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
        if state.data_epoch.load(std::sync::atomic::Ordering::SeqCst) != epoch {
            return Err("Workspace reset before backup".into());
        }
        let key = if include_vault {
            Some(Zeroizing::new(
                keyring::Entry::new("com.personal.commandcenter.vault", &repo.device_id)
                    .map_err(|e| e.to_string())?
                    .get_secret()
                    .map_err(|_| "Approved vault key unavailable")?,
            ))
        } else {
            None
        };
        let mut temp = tempfile::NamedTempFile::new().map_err(|e| e.to_string())?;
        {
            let mut writer = std::io::BufWriter::new(Cancellable {
                inner: temp.as_file_mut(),
                token: stage_token.clone(),
            });
            repo.export_stream(
                &mut writer,
                &stage_password,
                key.as_deref().map(|v| v.as_slice()),
            )?;
            writer.flush().map_err(|e| e.to_string())?;
        }
        temp.as_file().sync_all().map_err(|e| e.to_string())?;
        let summary = repo.inspect_stream(
            std::io::BufReader::new(Cancellable {
                inner: temp.reopen().map_err(|e| e.to_string())?,
                token: stage_token.clone(),
            }),
            &stage_password,
        )?;
        let size = temp.as_file().metadata().map_err(|e| e.to_string())?.len();
        let mut reader = Cancellable {
            inner: temp.reopen().map_err(|e| e.to_string())?,
            token: stage_token,
        };
        let mut hash = Sha256::new();
        let mut chunk = [0; 65536];
        loop {
            let n = reader.read(&mut chunk).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            hash.update(&chunk[..n]);
        }
        Ok((
            temp,
            summary.records,
            size,
            format!("{:x}", hash.finalize()),
        ))
    })
    .await
    .map_err(|_| "Streaming backup staging interrupted")??;
    let state = app.state::<AppState>();
    if state.data_epoch.load(std::sync::atomic::Ordering::SeqCst) != epoch || token.is_cancelled() {
        return Err("Backup reset or cancelled before upload".into());
    }
    let auth = credentials(&device)?;
    let remote_password = Zeroizing::new(auth.password);
    let http = client()?;
    let name = format!(
        "command-center-{device}-{}-{}.paccs",
        chrono::Utc::now().format("%Y%m%dT%H%M%SZ"),
        uuid::Uuid::new_v4()
    );
    let target = root.join(&name).map_err(|e| e.to_string())?;
    let file = tokio::fs::File::from_std(temp.reopen().map_err(|e| e.to_string())?);
    let stream_token = token.clone();
    let stream =
        futures_util::stream::try_unfold((file, stream_token), |(mut file, token)| async move {
            if token.is_cancelled() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Interrupted,
                    "Backup cancelled",
                ));
            }
            let mut chunk = vec![0; 65536];
            let n = file.read(&mut chunk).await?;
            if n == 0 {
                return Ok(None);
            }
            chunk.truncate(n);
            Ok(Some((chunk, (file, token))))
        });
    let request = http
        .put(target.clone())
        .basic_auth(&auth.username, Some(remote_password.as_str()))
        .header(reqwest::header::IF_NONE_MATCH, "*")
        .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
        .header(reqwest::header::CONTENT_LENGTH, size)
        .body(reqwest::Body::wrap_stream(stream));
    let response = tokio::select! {_=token.cancelled()=>return Err("Upload cancelled; partial ciphertext may remain remotely".into()),r=request.send()=>r.map_err(|_|"Streaming WebDAV upload failed")?};
    if !response.status().is_success() {
        return Err(format!("WebDAV upload returned HTTP {}", response.status()));
    }
    let response = send(
        http.get(target.clone())
            .basic_auth(&auth.username, Some(remote_password.as_str())),
        &token,
    )
    .await?;
    let (remote_hash, _) = checksum(response, size, &token, None).await?;
    if remote_hash != digest {
        return Err("Remote archive checksum did not match verified local ciphertext".into());
    }
    let info = json!({"sha256":digest,"verified_at":now(),"path":target.as_str(),"streaming":true,"format":"PCCSTR01","backend":"webdav","encrypted":true,"records":count,"includes_vault":config.include_vault,"modules":[],"bytes":size});
    let mut owned: Vec<Value> = {
        let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
        if state.data_epoch.load(std::sync::atomic::Ordering::SeqCst) != epoch {
            return Err("Workspace reset during upload; verified remote archive may remain".into());
        }
        let mut list: Vec<Value> =
            serde_json::from_str(&repo.setting("scheduled_archives")?.unwrap_or("[]".into()))
                .map_err(|_| "Invalid backup inventory")?;
        list.push(info.clone());
        repo.set_setting("last_backup", &info.to_string())?;
        repo.set_setting(
            "scheduled_archives",
            &serde_json::to_string(&list).map_err(|e| e.to_string())?,
        )?;
        list
    };
    let mut warnings = vec![];
    if config.retain > 0 {
        let candidates = owned
            .iter()
            .filter(|v| {
                v["backend"] == "webdav"
                    && v["path"]
                        .as_str()
                        .is_some_and(|p| p.starts_with(root.as_str()))
            })
            .count();
        let mut remove = candidates.saturating_sub(config.retain as usize);
        let mut removed = std::collections::HashSet::new();
        for old in &owned {
            if remove == 0 {
                break;
            }
            let Some(raw) = old["path"].as_str() else {
                continue;
            };
            if old["backend"] != "webdav"
                || !raw.starts_with(root.as_str())
                || raw == target.as_str()
            {
                continue;
            }
            let Ok(url) = reqwest::Url::parse(raw) else {
                continue;
            };
            if url.origin() != root.origin()
                || !url
                    .path()
                    .rsplit('/')
                    .next()
                    .is_some_and(|n| n.starts_with(&format!("command-center-{device}-")))
            {
                continue;
            }
            let result = async {
                let bytes = old["bytes"]
                    .as_u64()
                    .ok_or("Older backup has no exact size")?;
                let response = send(
                    http.get(url.clone())
                        .basic_auth(&auth.username, Some(remote_password.as_str())),
                    &token,
                )
                .await?;
                let (hash, etag) = checksum(response, bytes, &token, None).await?;
                if old["sha256"] != hash {
                    return Err("Older backup changed; kept it".into());
                }
                let etag = etag.ok_or("No strong ETag; kept older backup")?;
                if state.data_epoch.load(std::sync::atomic::Ordering::SeqCst) != epoch
                    || token.is_cancelled()
                {
                    return Err("Retention cancelled".into());
                }
                let response = send(
                    http.delete(url)
                        .basic_auth(&auth.username, Some(remote_password.as_str()))
                        .header(reqwest::header::IF_MATCH, etag),
                    &token,
                )
                .await?;
                if !response.status().is_success() {
                    return Err("Server refused conditional retention delete".into());
                }
                Ok::<(), String>(())
            }
            .await;
            match result {
                Ok(()) => {
                    removed.insert(raw.to_owned());
                    remove -= 1;
                }
                Err(e) => warnings.push(e),
            }
        }
        owned.retain(|v| !v["path"].as_str().is_some_and(|p| removed.contains(p)));
    }
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    if state.data_epoch.load(std::sync::atomic::Ordering::SeqCst) != epoch {
        return Err("Workspace reset before backup metadata update".into());
    }
    repo.set_setting(
        "scheduled_archives",
        &serde_json::to_string(&owned).map_err(|e| e.to_string())?,
    )?;
    Ok(json!({"path":target.as_str(),"rotation_warnings":warnings}))
}
#[tauri::command]
pub async fn download_webdav_stream(
    app: tauri::AppHandle,
    path: String,
    consent: bool,
) -> Result<Option<String>> {
    if !consent {
        return Err("Approve downloading this owned encrypted backup".into());
    }
    let state = app.state::<AppState>();
    let (device, destination, info, epoch) = {
        let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
        let owned: Vec<Value> =
            serde_json::from_str(&repo.setting("scheduled_archives")?.unwrap_or("[]".into()))
                .map_err(|_| "Invalid backup inventory")?;
        let info = owned
            .into_iter()
            .find(|v| v["path"] == path && v["streaming"] == true && v["backend"] == "webdav")
            .ok_or("Choose an app-owned streaming backup")?;
        (
            repo.device_id.clone(),
            repo.setting("backup_webdav")?
                .ok_or("Reconfigure the original WebDAV destination")?,
            info,
            state.data_epoch.load(std::sync::atomic::Ordering::SeqCst),
        )
    };
    let root = crate::webdav_backup::endpoint(&destination)?;
    let url = reqwest::Url::parse(&path).map_err(|_| "Invalid owned backup URL")?;
    if url.origin() != root.origin() || !url.as_str().starts_with(root.as_str()) {
        return Err("Backup is outside the current authorized destination".into());
    }
    let Some(selected) = rfd::AsyncFileDialog::new()
        .set_file_name("command-center-restored.paccs")
        .save_file()
        .await
    else {
        return Ok(None);
    };
    let job = Job::start(&app)?;
    let auth = credentials(&device)?;
    let secret = Zeroizing::new(auth.password);
    let http = client()?;
    let parent = selected.path().parent().ok_or("Invalid destination")?;
    let temp = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    let file = tokio::fs::File::from_std(temp.reopen().map_err(|e| e.to_string())?);
    let response = send(
        http.get(url)
            .basic_auth(auth.username, Some(secret.as_str())),
        &job.token,
    )
    .await?;
    let (digest, _) = checksum(
        response,
        info["bytes"].as_u64().ok_or("Missing backup size")?,
        &job.token,
        Some(file),
    )
    .await?;
    if info["sha256"] != digest {
        return Err("Downloaded backup checksum changed; destination was not replaced".into());
    }
    if state.data_epoch.load(std::sync::atomic::Ordering::SeqCst) != epoch
        || job.token.is_cancelled()
    {
        return Err("Download reset or cancelled".into());
    }
    temp.persist(selected.path()).map_err(|e| e.to_string())?;
    Ok(Some("Encrypted backup downloaded and checksum verified. Open it in Large streaming backups to verify the password and review restore.".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    async fn fixture() -> reqwest::Response {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0; 1024];
            stream.read(&mut request).await.unwrap();
            stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\nETag: \"fixture\"\r\nConnection: close\r\n\r\nciphertext").await.unwrap();
        });
        reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .get(format!("http://{address}/owned.paccs"))
            .send()
            .await
            .unwrap()
    }
    #[tokio::test]
    async fn readback_and_download_reject_changed_sizes_and_honor_cancel() {
        let token = CancellationToken::new();
        let temp = tempfile::NamedTempFile::new().unwrap();
        let file = tokio::fs::File::from_std(temp.reopen().unwrap());
        let (hash, etag) = checksum(fixture().await, 10, &token, Some(file))
            .await
            .unwrap();
        assert_eq!(hash, format!("{:x}", Sha256::digest(b"ciphertext")));
        assert_eq!(etag.as_deref(), Some("\"fixture\""));
        assert_eq!(std::fs::read(temp.path()).unwrap(), b"ciphertext");
        assert!(checksum(fixture().await, 9, &token, None).await.is_err());
        assert!(checksum(fixture().await, 11, &token, None).await.is_err());
        let response = fixture().await;
        let token = CancellationToken::new();
        token.cancel();
        assert!(checksum(response, 10, &token, None).await.is_err());
    }
}
