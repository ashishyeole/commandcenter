//! Opt-in OS scheduler. No registration occurs until the owner presses Enable.
use crate::{AppState, Result};
use serde_json::{json, Value};
use tauri::{Manager, State};
fn label(app: &tauri::AppHandle) -> Result<String> {
    let s = format!("{}.reminders", app.config().identifier);
    if !s
        .bytes()
        .all(|c| c.is_ascii_alphanumeric() || b".-".contains(&c))
    {
        return Err("Invalid scheduler identifier".into());
    }
    Ok(s)
}
fn executable() -> Result<String> {
    let path = std::env::current_exe().map_err(|e| e.to_string())?;
    path.to_str()
        .map(str::to_owned)
        .ok_or("Application path must be valid Unicode".into())
}
#[cfg(target_os = "macos")]
fn agent_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf> {
    Ok(app
        .path()
        .home_dir()
        .map_err(|e| e.to_string())?
        .join("Library/LaunchAgents")
        .join(format!("{}.plist", label(app)?)))
}
fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn plist(id: &str, exe: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd"><plist version="1.0"><dict><key>Label</key><string>{}</string><key>ProgramArguments</key><array><string>{}</string><string>--reminder-check</string></array><key>StartInterval</key><integer>900</integer><key>ProcessType</key><string>Background</string><key>LimitLoadToSessionType</key><string>Aqua</string></dict></plist>"#,
        xml(id),
        xml(exe)
    )
}
#[tauri::command]
pub fn background_reminder_status(app: tauri::AppHandle, state: State<AppState>) -> Result<Value> {
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    Ok(
        json!({"enabled":repo.setting("background_reminders")?.as_deref()==Some("true"),"executable":executable()?,"label":label(&app)?,"interval_minutes":15,"last_check":repo.setting("background_reminder_check")?,"last_error":repo.setting("background_reminder_error")?}),
    )
}
#[tauri::command]
pub fn configure_background_reminders(
    app: tauri::AppHandle,
    state: State<AppState>,
    enabled: bool,
    consent: bool,
) -> Result<()> {
    if !consent {
        return Err("Approve changing the OS reminder schedule".into());
    }
    if !enabled {
        state
            .repo
            .lock()
            .map_err(|_| "Database unavailable")?
            .set_setting("background_reminders", "false")?;
    }
    let id = label(&app)?;
    let exe = executable()?;
    #[cfg(target_os = "macos")]
    {
        let path = agent_path(&app)?;
        if enabled {
            std::fs::create_dir_all(path.parent().ok_or("Invalid scheduler path")?)
                .map_err(|e| e.to_string())?;
            crate::atomic_write(&path, plist(&id, &exe).as_bytes())?;
            let result = std::process::Command::new("/bin/launchctl")
                .arg("load")
                .arg(&path)
                .output()
                .map_err(|e| e.to_string())?;
            if !result.status.success() {
                return Err("OS schedule registration failed. Disable it before retrying, or check macOS background-item settings.".into());
            }
        } else if path.exists() {
            let _ = std::process::Command::new("/bin/launchctl")
                .arg("unload")
                .arg(&path)
                .output();
            std::fs::remove_file(&path).map_err(|e| e.to_string())?;
        }
    }
    #[cfg(target_os = "windows")]
    {
        let result = if enabled {
            std::process::Command::new("schtasks.exe")
                .args([
                    "/Create",
                    "/F",
                    "/SC",
                    "MINUTE",
                    "/MO",
                    "15",
                    "/TN",
                    &id,
                    "/TR",
                    &format!("\"{exe}\" --reminder-check"),
                ])
                .output()
        } else {
            std::process::Command::new("schtasks.exe")
                .args(["/Delete", "/F", "/TN", &id])
                .output()
        }
        .map_err(|e| e.to_string())?;
        if !result.status.success() {
            return Err(
                "Windows could not update the reminder task; check Task Scheduler permissions."
                    .into(),
            );
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        return Err("Background reminders are supported on macOS and Windows".into());
    }
    state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .set_setting(
            "background_reminders",
            if enabled { "true" } else { "false" },
        )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn launch_agent_escapes_paths_and_has_no_shell() {
        let doc = plist("com.example.reminders", "/Applications/A&B.app/bin");
        assert!(doc.contains("A&amp;B"));
        assert!(doc.contains("--reminder-check"));
        assert!(doc.contains("<integer>900</integer>"));
        assert!(!doc.contains("/bin/sh"));
    }
}
