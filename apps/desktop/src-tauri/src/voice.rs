use crate::{AppState, Result};
use serde_json::{json, Value};
use tauri::State;
#[cfg(target_os = "macos")]
extern "C" {
    fn pcc_voice_start();
    fn pcc_voice_stop();
    fn pcc_voice_clear();
    fn pcc_voice_snapshot() -> *mut std::ffi::c_char;
    fn pcc_voice_free(value: *mut std::ffi::c_char);
}
#[cfg(target_os = "windows")]
fn current() -> &'static std::sync::Mutex<Value> {
    static STATE: std::sync::OnceLock<std::sync::Mutex<Value>> = std::sync::OnceLock::new();
    STATE.get_or_init(||std::sync::Mutex::new(json!({"supported":true,"phase":"idle","text":"","error":"","engine":"Windows installed speech engine"})))
}
#[tauri::command]
pub fn desktop_voice_status() -> Result<Value> {
    #[cfg(target_os = "macos")]
    {
        unsafe {
            let ptr = pcc_voice_snapshot();
            if ptr.is_null() {
                return Err("Speech state unavailable".into());
            }
            let value = serde_json::from_slice(std::ffi::CStr::from_ptr(ptr).to_bytes())
                .map_err(|_| "Invalid speech state".into());
            pcc_voice_free(ptr);
            return value;
        }
    }
    #[cfg(target_os = "windows")]
    {
        return current()
            .lock()
            .map(|v| v.clone())
            .map_err(|_| "Speech state unavailable".into());
    }
    #[allow(unreachable_code)]
    Ok(
        json!({"supported":false,"phase":"unavailable","text":"","error":"Built-in dictation supports macOS and Windows."}),
    )
}
#[tauri::command]
pub fn desktop_voice_stop(state: State<AppState>, clear: bool) -> Result<()> {
    #[cfg(target_os = "macos")]
    unsafe {
        if clear {
            pcc_voice_clear()
        } else {
            pcc_voice_stop()
        }
    }
    if let Some(token) = state
        .jobs
        .lock()
        .map_err(|_| "Job state unavailable")?
        .get("desktop:voice")
    {
        token.cancel();
    }
    #[cfg(target_os = "windows")]
    {
        let mut value = current().lock().map_err(|_| "Speech state unavailable")?;
        value["phase"] = json!("finished");
        if clear {
            value["text"] = json!("");
        }
    }
    Ok(())
}
#[tauri::command]
pub fn desktop_voice_start(
    app: tauri::AppHandle,
    state: State<AppState>,
    consent: bool,
) -> Result<()> {
    if !consent {
        return Err("Approve on-device microphone capture first".into());
    }
    #[cfg(target_os = "macos")]
    {
        let _ = (&app, &state);
        let value = desktop_voice_status()?;
        if value["phase"] == "recording" || value["phase"] == "permission" {
            return Err("Speech capture is already active".into());
        }
        unsafe { pcc_voice_start() };
        return Ok(());
    }
    #[cfg(target_os = "windows")]
    {
        let token = tokio_util::sync::CancellationToken::new();
        {
            let mut jobs = state.jobs.lock().map_err(|_| "Job state unavailable")?;
            if jobs.contains_key("desktop:voice") {
                return Err("Speech capture is already active".into());
            }
            jobs.insert("desktop:voice".into(), token.clone());
        }
        *current().lock().map_err(|_| "Speech state unavailable")? = json!({"supported":true,"phase":"recording","text":"","error":"","engine":"Windows installed speech engine"});
        tauri::async_runtime::spawn(async move {
            use std::process::Stdio;
            use tauri::Manager;
            use tokio::io::AsyncReadExt;
            let work = async {
                // Constant source only: renderer text, commands, paths and arguments are never interpolated.
                let script = r#"[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false); try { Add-Type -AssemblyName System.Speech; $recognizer = New-Object System.Speech.Recognition.SpeechRecognitionEngine; $recognizer.LoadGrammar((New-Object System.Speech.Recognition.DictationGrammar)); $recognizer.SetInputToDefaultAudioDevice(); $recognized = $recognizer.Recognize([TimeSpan]::FromSeconds(30)); if ($recognized) { @{text=$recognized.Text;error=''} | ConvertTo-Json -Compress } else { @{text='';error='No speech recognized'} | ConvertTo-Json -Compress }; $recognizer.Dispose() } catch { @{text='';error='Installed Windows speech engine or microphone unavailable. Enable local speech and microphone access in Windows Settings.'} | ConvertTo-Json -Compress }"#;
                let root =
                    std::env::var_os("SystemRoot").ok_or("Windows system directory unavailable")?;
                let exe = std::path::PathBuf::from(root)
                    .join("System32/WindowsPowerShell/v1.0/powershell.exe");
                let mut child = tokio::process::Command::new(exe)
                    .args(["-NoProfile", "-NonInteractive", "-Command", script])
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .kill_on_drop(true)
                    .spawn()
                    .map_err(|_| "Cannot start installed speech engine")?;
                let mut bytes = vec![];
                child
                    .stdout
                    .take()
                    .ok_or("Speech output unavailable")?
                    .take(65537)
                    .read_to_end(&mut bytes)
                    .await
                    .map_err(|_| "Speech capture interrupted")?;
                if bytes.len() > 65536 {
                    return Err("Speech output exceeds limit".to_string());
                }
                let status = child
                    .wait()
                    .await
                    .map_err(|_| "Speech engine interrupted")?;
                if !status.success() {
                    return Err("Speech engine failed".into());
                }
                serde_json::from_slice::<Value>(&bytes)
                    .map_err(|_| "Invalid speech engine response".into())
            };
            let result: Result<Value> = tokio::select! {_=token.cancelled()=>Err("Speech capture stopped".into()),r=tokio::time::timeout(std::time::Duration::from_secs(45),work)=>r.map_err(|_|"Speech recording limit reached".to_string()).and_then(|r|r)};
            if !token.is_cancelled() {
                if let Ok(mut value) = current().lock() {
                    match result {
                        Ok(v) => {
                            value["phase"] = json!("finished");
                            value["text"] = v["text"].clone();
                            value["error"] = v["error"].clone();
                        }
                        Err(e) => {
                            value["phase"] = json!("error");
                            value["error"] = json!(e);
                        }
                    }
                }
            }
            let app_state = app.state::<AppState>();
            if let Ok(mut jobs) = app_state.jobs.lock() {
                jobs.remove("desktop:voice");
            };
        });
        return Ok(());
    }
    #[allow(unreachable_code)]
    {
        let _ = (app, state);
        Err("Built-in speech is unavailable on this platform".into())
    }
}
