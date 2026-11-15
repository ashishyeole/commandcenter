use crate::{AppState, Result};
use command_center_core::{
    now,
    stream_archive::{Summary, BYTE_LIMIT},
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, SeekFrom, Write};
use tauri::{Manager, State};
use zeroize::Zeroizing;
pub struct Pending {
    file: tempfile::NamedTempFile,
    sha256: String,
    id: String,
    vault_records: u64,
    epoch: u64,
}
#[tauri::command]
pub async fn export_large_archive(
    app: tauri::AppHandle,
    password: String,
    include_vault: bool,
) -> Result<Option<String>> {
    let job = Job::start(&app)?;
    let epoch = app
        .state::<AppState>()
        .data_epoch
        .load(std::sync::atomic::Ordering::SeqCst);
    let password = Zeroizing::new(password);
    if password.chars().count() < 12 {
        return Err("Use a backup password of at least 12 characters".into());
    }
    let Some(file) = rfd::AsyncFileDialog::new()
        .set_file_name("command-center-stream.paccs")
        .save_file()
        .await
    else {
        return Ok(None);
    };
    let path = file.path().to_path_buf();
    tauri::async_runtime::spawn_blocking(move||{
  let _job=job;let token=_job.token.clone();let state=app.state::<AppState>();let key=if include_vault{Some(crate::vault::session_key(&state)?)}else{None};let parent=path.parent().ok_or("Invalid backup destination")?;let mut temp=tempfile::NamedTempFile::new_in(parent).map_err(|e|e.to_string())?;
  let summary={let repo=state.repo.lock().map_err(|_|"Database unavailable")?;{let mut output=std::io::BufWriter::new(Cancellable{inner:temp.as_file_mut(),token:token.clone()});repo.export_stream(&mut output,&password,key.as_deref().map(|k|k.as_slice()))?;output.flush().map_err(|e|e.to_string())?;}temp.as_file().sync_all().map_err(|e|e.to_string())?;repo.inspect_stream(std::io::BufReader::new(Cancellable{inner:temp.reopen().map_err(|e|e.to_string())?,token:token.clone()}),&password)?};let bytes=temp.as_file().metadata().map_err(|e|e.to_string())?.len();let repo=state.repo.lock().map_err(|_|"Database unavailable")?;if state.data_epoch.load(std::sync::atomic::Ordering::SeqCst)!=epoch{return Err("Data reset during export; archive was not published".into());}if token.is_cancelled(){return Err("Archive export cancelled".into());}temp.persist(&path).map_err(|e|e.to_string())?;
  repo.set_setting("last_backup",&json!({"verified_at":now(),"path":path.display().to_string(),"encrypted":true,"records":summary.records,"includes_vault":include_vault,"modules":[],"bytes":bytes,"format":"PCCSTR01"}).to_string())?;
  Ok(Some(format!("Verified {} streamed records → {}",summary.records,path.display())))
 }).await.map_err(|_|"Streaming export interrupted".to_string())?
}
#[tauri::command]
pub async fn preview_large_archive(
    app: tauri::AppHandle,
    password: String,
) -> Result<Option<Value>> {
    let job = Job::start(&app)?;
    let epoch = app
        .state::<AppState>()
        .data_epoch
        .load(std::sync::atomic::Ordering::SeqCst);
    let password = Zeroizing::new(password);
    let Some(selected) = rfd::AsyncFileDialog::new()
        .add_filter("Streaming encrypted backup", &["paccs"])
        .pick_file()
        .await
    else {
        return Ok(None);
    };
    let path = selected.path().to_path_buf();
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "Streaming archive".into());
    tauri::async_runtime::spawn_blocking(move||{
  let _job=job;let token=_job.token.clone();let state=app.state::<AppState>();let mut source=std::fs::File::open(path).map_err(|e|e.to_string())?;let mut magic=[0;8];source.read_exact(&mut magic).map_err(|_|"Incomplete archive header")?;if &magic!=b"PCCSTR01"{return Err("Choose a PCCSTR01 streaming backup (.paccs); use standard Import for ZIP/PACC01 archives".into());}source.seek(SeekFrom::Start(0)).map_err(|e|e.to_string())?;
  let mut temp=tempfile::NamedTempFile::new().map_err(|e|e.to_string())?;let copied=std::io::copy(&mut Cancellable{inner:source,token:token.clone()}.take(BYTE_LIMIT+1),temp.as_file_mut()).map_err(|e|e.to_string())?;if copied>BYTE_LIMIT{return Err("Streaming archive exceeds 8 GiB".into());}
  let mut reader=Cancellable{inner:temp.reopen().map_err(|e|e.to_string())?,token:token.clone()};let mut hash=Sha256::new();let mut buffer=[0;65536];loop{let n=reader.read(&mut buffer).map_err(|e|e.to_string())?;if n==0{break;}hash.update(&buffer[..n]);}let sha256=format!("{:x}",hash.finalize());
  let summary=state.repo.lock().map_err(|_|"Database unavailable")?.inspect_stream(std::io::BufReader::new(Cancellable{inner:temp.reopen().map_err(|e|e.to_string())?,token:token.clone()}),&password)?;
  let id=uuid::Uuid::new_v4().to_string();let result=json!({"name":name,"id":id,"sha256":sha256,"summary":summary,"bytes":copied,"policy":"Import only missing IDs; existing records and permissions are kept. No connection permissions, keys or schedules are restored. Corrupt, incomplete or modified archives roll back completely."});
  let _repo=state.repo.lock().map_err(|_|"Database unavailable")?;if state.data_epoch.load(std::sync::atomic::Ordering::SeqCst)!=epoch{return Err("Data reset during preview; select the archive again".into());}
  let mut pending=state.large_import.lock().map_err(|_|"Import state unavailable")?;
  if token.is_cancelled(){return Err("Archive preview cancelled".into());}
  *pending=Some(Pending{file:temp,sha256,id,vault_records:summary.vault_records,epoch});Ok(Some(result))
 }).await.map_err(|_|"Streaming preview interrupted".to_string())?
}
#[tauri::command]
pub async fn apply_large_archive(
    app: tauri::AppHandle,
    id: String,
    reviewed_sha256: String,
    password: String,
    confirmation: String,
) -> Result<Summary> {
    if confirmation != "IMPORT MISSING RECORDS" {
        return Err("Review the streaming import policy first".into());
    }
    let password = Zeroizing::new(password);
    let job = Job::start(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let _job = job;
        let token = _job.token.clone();
        let state = app.state::<AppState>();
        let p = state
            .large_import
            .lock()
            .map_err(|_| "Import state unavailable")?
            .take()
            .ok_or("Select and verify the archive again")?;
        if p.id != id || p.sha256 != reviewed_sha256 {
            return Err("Archive review changed; preview again".into());
        }
        let key = if p.vault_records > 0 {
            Some(crate::vault::session_key(&state)?)
        } else {
            None
        };
        let mut repo = state.repo.lock().map_err(|_| "Database unavailable")?;
        if state.data_epoch.load(std::sync::atomic::Ordering::SeqCst) != p.epoch {
            return Err("Data reset since preview; verify the archive again".into());
        }
        repo.import_stream_checked(
            std::io::BufReader::new(Cancellable {
                inner: p.file.reopen().map_err(|e| e.to_string())?,
                token: token.clone(),
            }),
            &password,
            key.as_deref().map(|k| k.as_slice()),
            Some(&reviewed_sha256),
        )
    })
    .await
    .map_err(|_| "Streaming import interrupted".to_string())?
}
#[tauri::command]
pub fn discard_large_archive(state: State<AppState>) -> Result<()> {
    *state
        .large_import
        .lock()
        .map_err(|_| "Import state unavailable")? = None;
    Ok(())
}

const JOB: &str = "archive:stream";
struct Job {
    app: tauri::AppHandle,
    token: tokio_util::sync::CancellationToken,
}
impl Job {
    fn start(app: &tauri::AppHandle) -> Result<Self> {
        let state = app.state::<AppState>();
        let mut jobs = state.jobs.lock().map_err(|_| "Job state unavailable")?;
        if jobs.contains_key(JOB) {
            return Err("A streaming archive operation is already running".into());
        }
        let token = tokio_util::sync::CancellationToken::new();
        jobs.insert(JOB.into(), token.clone());
        Ok(Self {
            app: app.clone(),
            token,
        })
    }
}
impl Drop for Job {
    fn drop(&mut self) {
        let state = self.app.state::<AppState>();
        if let Ok(mut jobs) = state.jobs.lock() {
            jobs.remove(JOB);
        };
    }
}
struct Cancellable<T> {
    inner: T,
    token: tokio_util::sync::CancellationToken,
}
impl<T: Read> Read for Cancellable<T> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.token.is_cancelled() {
            return Err(std::io::Error::other("Archive operation cancelled"));
        }
        self.inner.read(buf)
    }
}
impl<T: Write> Write for Cancellable<T> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.token.is_cancelled() {
            return Err(std::io::Error::other("Archive operation cancelled"));
        }
        self.inner.write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        if self.token.is_cancelled() {
            return Err(std::io::Error::other("Archive operation cancelled"));
        }
        self.inner.flush()
    }
}

#[cfg(test)]
mod cancellation_tests {
    use super::*;
    #[test]
    fn cancelled_io_never_forwards_new_data() {
        let token = tokio_util::sync::CancellationToken::new();
        let mut reader = Cancellable {
            inner: std::io::Cursor::new(vec![1, 2, 3]),
            token: token.clone(),
        };
        let mut writer = Cancellable {
            inner: Vec::new(),
            token: token.clone(),
        };
        token.cancel();
        assert!(reader.read(&mut [0; 1]).is_err());
        assert!(writer.write(b"private").is_err());
        assert!(writer.inner.is_empty());
        assert!(writer.flush().is_err());
    }
}
