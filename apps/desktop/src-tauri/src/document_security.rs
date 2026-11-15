//! Optional local antivirus scans of an explicitly selected ordinary attachment.
use crate::{AppState, Result};
use serde_json::{json, Value};
use std::io::Write;
use tauri::State;
fn scanner() -> Option<std::path::PathBuf> {
    #[cfg(target_os = "windows")]
    let paths = std::env::var_os("ProgramFiles")
        .map(|p| vec![std::path::PathBuf::from(p).join("ClamAV/clamscan.exe")])
        .unwrap_or_default();
    #[cfg(not(target_os = "windows"))]
    let paths = [
        "/opt/homebrew/bin/clamscan",
        "/usr/local/bin/clamscan",
        "/usr/bin/clamscan",
    ]
    .into_iter()
    .map(std::path::PathBuf::from)
    .collect::<Vec<_>>();
    paths.into_iter().find(|p| p.is_file())
}
#[tauri::command]
pub fn attachment_security_status(state: State<AppState>, id: String) -> Result<Value> {
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    let a = repo.attachment(&id)?;
    let receipt = repo
        .setting(&format!("attachment_scan:{id}:{}", a.sha256))?
        .and_then(|s| serde_json::from_str::<Value>(&s).ok());
    Ok(
        json!({"scanner_available":scanner().is_some(),"sha256":a.sha256,"receipt":receipt,"notice":"A clean scan is not proof of safety. Scan results apply only to these exact bytes and the installed signature database."}),
    )
}
struct Guard<'a> {
    state: &'a AppState,
    id: String,
}
impl Drop for Guard<'_> {
    fn drop(&mut self) {
        if let Ok(mut jobs) = self.state.jobs.lock() {
            jobs.remove(&self.id);
        }
    }
}
#[tauri::command]
pub async fn scan_attachment(
    state: State<'_, AppState>,
    id: String,
    consent: bool,
) -> Result<Value> {
    if !consent {
        return Err("Approve scanning this selected ordinary attachment with local ClamAV".into());
    }
    let executable=scanner().ok_or("Install ClamAV and its current signature database separately. No scanner is installed or updated automatically.")?;
    let job_id = format!("scan:{id}");
    let cancel = tokio_util::sync::CancellationToken::new();
    {
        let mut jobs = state.jobs.lock().map_err(|_| "Job state unavailable")?;
        if jobs.contains_key(&job_id) {
            return Err("This attachment is already being scanned".into());
        }
        jobs.insert(job_id.clone(), cancel.clone());
    }
    let _guard = Guard {
        state: &state,
        id: job_id,
    };
    let epoch = state.data_epoch.load(std::sync::atomic::Ordering::SeqCst);
    let a = state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .attachment(&id)?;
    let bytes = a.validate()?;
    let mut temporary =
        tempfile::NamedTempFile::new().map_err(|_| "Cannot create private scan temporary file")?;
    temporary
        .write_all(&bytes)
        .map_err(|_| "Cannot prepare attachment scan")?;
    temporary.flush().map_err(|_| "Cannot flush scan file")?;
    let work = async {
        use tokio::io::AsyncReadExt;
        let mut child = tokio::process::Command::new(executable)
            .args([
                "--stdout",
                "--no-summary",
                "--max-filesize=12M",
                "--max-scansize=24M",
                "--max-recursion=8",
                "--max-files=500",
                "--alert-exceeds-max=yes",
                "--",
            ])
            .arg(temporary.path())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| "Cannot start local scanner")?;
        let stdout = child.stdout.take().ok_or("Scanner stdout unavailable")?;
        let stderr = child.stderr.take().ok_or("Scanner stderr unavailable")?;
        async fn read<R: tokio::io::AsyncRead + Unpin>(stream: R) -> Result<Vec<u8>> {
            let mut out = vec![];
            stream
                .take(16385)
                .read_to_end(&mut out)
                .await
                .map_err(|_| "Scanner output interrupted")?;
            if out.len() > 16384 {
                return Err("Scanner output exceeds bound".into());
            }
            Ok(out)
        }
        let (out, error, status) = tokio::try_join!(read(stdout), read(stderr), async {
            child
                .wait()
                .await
                .map_err(|_| "Scanner interrupted".to_string())
        })?;
        let output = format!(
            "{}\n{}",
            String::from_utf8_lossy(&out),
            String::from_utf8_lossy(&error)
        )
        .replace(
            temporary.path().to_string_lossy().as_ref(),
            "[selected attachment]",
        );
        Ok(
            json!({"at":command_center_core::now(),"engine":"locally installed ClamAV","status":match status.code(){Some(0)=>"no_known_threat_detected",Some(1)=>"flagged_or_scan_limit",_=>"scanner_error"},"exit_code":status.code(),"output":output,"sha256":a.sha256,"notice":"Original attachment retained unchanged. A flagged or incomplete scan requires review; the app never executes the file. The scanner/signature database must be maintained separately."}),
        )
    };
    let result: Result<Value> = tokio::select! {biased;_=cancel.cancelled()=>Err("Local scan cancelled; no clean result established".into()),r=tokio::time::timeout(std::time::Duration::from_secs(120),work)=>r.map_err(|_|"Scanner timed out; scan is incomplete".to_string()).and_then(|r|r)};
    let receipt = match result {
        Ok(v) => v,
        Err(e) => {
            json!({"at":command_center_core::now(),"status":"scanner_error","error":e,"sha256":a.sha256})
        }
    };
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    if epoch != state.data_epoch.load(std::sync::atomic::Ordering::SeqCst) {
        return Err("Workspace reset during scan".into());
    }
    repo.set_setting(
        &format!("attachment_scan:{id}:{}", a.sha256),
        &receipt.to_string(),
    )?;
    Ok(receipt)
}
