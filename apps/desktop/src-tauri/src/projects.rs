use crate::{AppState, Result};
use command_center_core::{now, workspace::Record};
use serde_json::{json, Value};
use std::{path::Path, process::Stdio, time::Duration};
use tauri::State;
use tokio::io::AsyncReadExt;
async fn git(path: &str, args: &[&str]) -> Result<String> {
    let mut command = tokio::process::Command::new("git");
    command
        .args([
            "--no-optional-locks",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.hooksPath=/dev/null",
            "-C",
            path,
        ])
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_CONFIG_COUNT")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut child = command
        .spawn()
        .map_err(|_| "Git is not installed or cannot be started")?;
    let stdout = child.stdout.take().ok_or("Cannot read Git output")?;
    let result = tokio::time::timeout(Duration::from_secs(12), async {
        let mut bytes = vec![];
        stdout
            .take(512 * 1024 + 1)
            .read_to_end(&mut bytes)
            .await
            .map_err(|e| e.to_string())?;
        if bytes.len() > 512 * 1024 {
            return Err("Git output exceeds limit".into());
        }
        let status = child.wait().await.map_err(|e| e.to_string())?;
        if !status.success() {
            return Err(
                "Git command failed. Select a valid repository with readable history.".into(),
            );
        }
        Ok(String::from_utf8_lossy(&bytes).trim().to_owned())
    })
    .await
    .map_err(|_| "Git observation timed out")?;
    result
}
#[tauri::command]
pub async fn choose_project_folder(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<Option<String>> {
    if let Some(folder) = rfd::AsyncFileDialog::new()
        .set_title("Authorize read-only Git observation for this folder")
        .pick_folder()
        .await
    {
        let canonical = folder.path().canonicalize().map_err(|e| e.to_string())?;
        if !Path::new(&canonical).join(".git").exists() {
            return Err("Select a Git repository root (a folder containing .git)".into());
        }
        let path = canonical.to_string_lossy().to_string();
        state
            .repo
            .lock()
            .map_err(|_| "Database unavailable")?
            .bind_folder(&project_id, &path)?;
        Ok(Some(path))
    } else {
        Ok(None)
    }
}
#[tauri::command]
pub fn project_folder(state: State<AppState>, project_id: String) -> Result<Option<String>> {
    state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .folder(&project_id)
}
#[tauri::command]
pub fn revoke_project_folder(state: State<AppState>, project_id: String) -> Result<()> {
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    let mut ids: Vec<String> =
        serde_json::from_str(&repo.setting("monitored_projects")?.unwrap_or("[]".into()))
            .map_err(|e| e.to_string())?;
    ids.retain(|id| id != &project_id);
    repo.set_setting(
        "monitored_projects",
        &serde_json::to_string(&ids).map_err(|e| e.to_string())?,
    )?;
    let mut watched: Vec<String> =
        serde_json::from_str(&repo.setting("watched_projects")?.unwrap_or("[]".into()))
            .map_err(|_| "Invalid watcher configuration")?;
    watched.retain(|id| id != &project_id);
    repo.set_setting(
        "watched_projects",
        &serde_json::to_string(&watched).map_err(|e| e.to_string())?,
    )?;
    state
        .project_watchers
        .lock()
        .map_err(|_| "Watcher state unavailable")?
        .remove(&project_id);
    repo.revoke_folder(&project_id)
}
#[tauri::command]
pub async fn observe_project(state: State<'_, AppState>, project_id: String) -> Result<Value> {
    let (path, device) = {
        let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
        (
            repo.folder(&project_id)?
                .ok_or("Choose and authorize a project folder first")?,
            repo.device_id.clone(),
        )
    };
    let branch = git(&path, &["rev-parse", "--abbrev-ref", "HEAD"]).await?;
    let status = git(&path, &["status", "--porcelain=v1", "--untracked-files=no"]).await?;
    let commits = git(&path, &["log", "-5", "--format=%h | %aI | %.160s"]).await?;
    let observation = json!({"branch":branch,"tracked_changes":status,"recent_commits":commits,"observed_at":now(),"device_id":device,"evidence":"Observed Git metadata; no inference about LLMs or IDEs"});
    let mut record = Record::new("sessions", "Observed Git session", &device);
    record.status = "observed".into();
    record.links.push(project_id);
    record.summary =
        format!("Branch: {branch}\n\nTracked changes:\n{status}\n\nRecent commits:\n{commits}");
    record.fields.insert("branch".into(), branch);
    record
        .fields
        .insert("evidence".into(), "Observed, read-only Git metadata".into());
    let mut repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    if repo.folder(&record.links[0])?.as_deref() != Some(path.as_str())
        || !repo
            .records("", false)?
            .iter()
            .any(|r| r.id == record.links[0])
    {
        return Err("Project access was revoked during observation".into());
    }
    let previous = repo
        .records("", false)?
        .into_iter()
        .filter(|r| r.module == "sessions" && r.links == record.links && r.status == "observed")
        .max_by(|a, b| a.updated_at.cmp(&b.updated_at));
    if !previous.is_some_and(|r| r.summary == record.summary) {
        repo.save_record(record)?;
    }
    Ok(observation)
}
#[tauri::command]
pub fn set_project_monitoring(
    state: State<AppState>,
    project_id: String,
    enabled: bool,
) -> Result<()> {
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    if enabled && repo.folder(&project_id)?.is_none() {
        return Err("Authorize a repository folder first".into());
    }
    let mut ids: Vec<String> =
        serde_json::from_str(&repo.setting("monitored_projects")?.unwrap_or("[]".into()))
            .map_err(|e| e.to_string())?;
    ids.retain(|id| id != &project_id);
    if enabled {
        if ids.len() >= 8 {
            return Err("At most eight monitored projects per device".into());
        }
        ids.push(project_id);
    }
    repo.set_setting(
        "monitored_projects",
        &serde_json::to_string(&ids).map_err(|e| e.to_string())?,
    )
}
#[tauri::command]
pub fn resume_project(
    app: tauri::AppHandle,
    state: State<AppState>,
    project_id: String,
) -> Result<()> {
    use tauri_plugin_opener::OpenerExt;
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    if !repo
        .records("", false)?
        .iter()
        .any(|r| r.id == project_id && r.module == "projects")
    {
        return Err("Project no longer exists".into());
    }
    let path = repo
        .folder(&project_id)?
        .ok_or("Authorize a repository folder first")?;
    app.opener()
        .open_path(path, None::<&str>)
        .map_err(|e| e.to_string())
}
pub async fn poll(app: &tauri::AppHandle) -> Result<()> {
    use tauri::Manager;
    let ids: Vec<String> = {
        let state = app.state::<AppState>();
        let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
        let mut ids: Vec<String> =
            serde_json::from_str(&repo.setting("monitored_projects")?.unwrap_or("[]".into()))
                .map_err(|e| e.to_string())?;
        let watched: Vec<String> =
            serde_json::from_str(&repo.setting("watched_projects")?.unwrap_or("[]".into()))
                .map_err(|e| e.to_string())?;
        ids.extend(watched);
        ids.sort();
        ids.dedup();
        ids
    };
    for id in ids {
        let result = observe_project(app.state(), id.clone()).await;
        let state = app.state::<AppState>();
        let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
        if repo.folder(&id)?.is_some() {
            repo.set_setting(&format!("project_health:{id}"),&json!({"at":now(),"status":if result.is_ok(){"observed"}else{"failed"},"error":result.err()}).to_string())?;
        }
    }
    Ok(())
}

/// Deliberately separate from Git authorization: manifest contents can disclose dependencies.
#[tauri::command]
pub async fn inspect_project_manifests(
    state: State<'_, AppState>,
    project_id: String,
    consent: bool,
) -> Result<Value> {
    if !consent {
        return Err("Approve bounded manifest inspection for this project first".into());
    }
    let epoch = state.data_epoch.load(std::sync::atomic::Ordering::SeqCst);
    let path = state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .folder(&project_id)?
        .ok_or("Authorize a project folder first")?;
    let root = std::path::PathBuf::from(&path);
    let observations = tokio::task::spawn_blocking(move || -> Result<Vec<Value>> {
        use std::io::Read;
        let canonical = root.canonicalize().map_err(|_| "Project folder unavailable")?;
        let mut rows = vec![];
        // No recursive traversal, environment files, arbitrary workspace paths, scripts or execution.
        for (name, ecosystem) in [("package.json","JavaScript / TypeScript"),("Cargo.toml","Rust"),("pyproject.toml","Python"),("requirements.txt","Python"),("go.mod","Go"),("Package.swift","Swift"),("pubspec.yaml","Dart / Flutter"),("build.gradle.kts","Gradle / Kotlin"),("build.gradle","Gradle"),("CMakeLists.txt","CMake"),(".vscode/extensions.json","VS Code recommendations")] {
            let file = canonical.join(name);
            if !file.exists() { continue; }
            let resolved = file.canonicalize().map_err(|_| "Manifest cannot be resolved")?;
            if !resolved.starts_with(&canonical) { return Err("A manifest points outside the authorized project folder".into()); }
            let mut opened = std::fs::File::open(&resolved).map_err(|_| "Manifest cannot be opened")?;
            let metadata = opened.metadata().map_err(|_| "Cannot inspect manifest metadata")?;
            if !metadata.is_file() || metadata.len() > 256*1024 { rows.push(json!({"file":name,"ecosystem":ecosystem,"status":"skipped: not a regular file or exceeds 256 KiB"}));continue; }
            let mut bytes = vec![];
            (&mut opened).take(256*1024+1).read_to_end(&mut bytes).map_err(|_| "Cannot read manifest")?;
            if bytes.len()>256*1024 { return Err("Manifest changed beyond size limit".into()); }
            let text = std::str::from_utf8(&bytes).map_err(|_| "Manifest must be UTF-8")?;
            let mut dependencies:Vec<String> = vec![];
            if name=="package.json" || name==".vscode/extensions.json" {
                if let Ok(value)=serde_json::from_str::<Value>(text) {
                    for key in ["dependencies","devDependencies","peerDependencies"] {
                        if let Some(map)=value.get(key).and_then(Value::as_object) {dependencies.extend(map.keys().filter(|k|k.len()<=180).take(150).cloned());}
                    }
                    if let Some(values)=value.get("recommendations").and_then(Value::as_array) {dependencies.extend(values.iter().filter_map(Value::as_str).filter(|s|s.len()<=180).take(150).map(str::to_owned));}
                }
            }
            dependencies.sort();dependencies.dedup();dependencies.truncate(200);
            use sha2::{Digest,Sha256};
            rows.push(json!({"file":name,"ecosystem":ecosystem,"status":"observed","bytes":bytes.len(),"sha256":format!("{:x}",Sha256::digest(&bytes)),"dependency_names":dependencies}));
        }
        Ok(rows)
    }).await.map_err(|_| "Manifest inspection interrupted")??;
    let mut repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    if epoch != state.data_epoch.load(std::sync::atomic::Ordering::SeqCst)
        || repo.folder(&project_id)?.as_deref() != Some(path.as_str())
    {
        return Err("Project access changed during inspection".into());
    }
    if !repo
        .records("", false)?
        .iter()
        .any(|r| r.id == project_id && r.module == "projects")
    {
        return Err("Project no longer exists".into());
    }
    let result = json!({"observed_at":now(),"manifests":observations,"evidence":"File presence, checksums and selected dependency names only. No source execution, code upload, IDE usage or AI model usage inference."});
    let mut record = Record::new("sessions", "Observed project manifests", &repo.device_id);
    record.status = "observed".into();
    record.links.push(project_id);
    record.summary = serde_json::to_string_pretty(&result).map_err(|e| e.to_string())?;
    record.fields.insert(
        "evidence".into(),
        "Observed allowlisted project manifests with explicit approval".into(),
    );
    repo.save_record(record)?;
    Ok(result)
}

pub struct WatchBinding {
    pub root: String,
    pub _watcher: notify::RecommendedWatcher,
}
#[tauri::command]
pub fn set_project_watching(
    state: State<AppState>,
    project_id: String,
    enabled: bool,
) -> Result<()> {
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    if enabled && repo.folder(&project_id)?.is_none() {
        return Err("Authorize this project folder first".into());
    }
    let mut ids: Vec<String> =
        serde_json::from_str(&repo.setting("watched_projects")?.unwrap_or("[]".into()))
            .map_err(|_| "Invalid watcher configuration")?;
    ids.retain(|id| id != &project_id);
    if enabled {
        if ids.len() >= 8 {
            return Err("At most eight explicitly watched roots".into());
        }
        ids.push(project_id.clone());
    }
    repo.set_setting(
        "watched_projects",
        &serde_json::to_string(&ids).map_err(|e| e.to_string())?,
    )?;
    if !enabled {
        state
            .project_watchers
            .lock()
            .map_err(|_| "Watcher state unavailable")?
            .remove(&project_id);
    }
    Ok(())
}
pub fn start_watchers(app: tauri::AppHandle) {
    use tauri::Manager;
    tauri::async_runtime::spawn(async move {
        use notify::Watcher;
        let (sender, mut events) = tokio::sync::mpsc::channel::<String>(128);
        let mut timer = tokio::time::interval(Duration::from_secs(3));
        let mut pending = std::collections::HashSet::new();
        let mut last: std::collections::HashMap<String, std::time::Instant> =
            std::collections::HashMap::new();
        loop {
            tokio::select! {Some(id)=events.recv()=>{pending.insert(id);continue;},_=timer.tick()=>{}}
            let state = app.state::<AppState>();
            let wanted: std::collections::HashMap<String, String> = (|| -> Result<_> {
                let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
                let ids: Vec<String> =
                    serde_json::from_str(&repo.setting("watched_projects")?.unwrap_or("[]".into()))
                        .map_err(|_| "Invalid watcher configuration")?;
                let mut wanted = std::collections::HashMap::new();
                for id in ids.into_iter().take(8) {
                    if let Some(path) = repo.folder(&id)? {
                        wanted.insert(id, path);
                    }
                }
                Ok(wanted)
            })()
            .unwrap_or_default();
            {
                if let Ok(mut watchers) = state.project_watchers.lock() {
                    watchers.retain(|id, binding| wanted.get(id) == Some(&binding.root));
                    for (id, root) in &wanted {
                        if watchers.contains_key(id) {
                            continue;
                        }
                        let tx = sender.clone();
                        let project = id.clone();
                        let allowed = std::path::PathBuf::from(root);
                        let watch = notify::RecommendedWatcher::new(
                            move |event: notify::Result<notify::Event>| {
                                if let Ok(event) = event {
                                    if event.kind.is_access() {
                                        return;
                                    }
                                    if event.paths.iter().any(|p| {
                                        p.strip_prefix(&allowed).is_ok_and(|relative| {
                                            !relative.components().any(|c| {
                                                matches!(
                                                    c.as_os_str().to_str(),
                                                    Some(
                                                        "node_modules"
                                                            | "target"
                                                            | ".gradle"
                                                            | "dist"
                                                    )
                                                )
                                            })
                                        })
                                    }) {
                                        let _ = tx.try_send(project.clone());
                                    }
                                }
                            },
                            notify::Config::default().with_follow_symlinks(false),
                        );
                        if let Ok(mut watcher) = watch {
                            if watcher
                                .watch(std::path::Path::new(root), notify::RecursiveMode::Recursive)
                                .is_ok()
                            {
                                watchers.insert(
                                    id.clone(),
                                    WatchBinding {
                                        root: root.clone(),
                                        _watcher: watcher,
                                    },
                                );
                                pending.insert(id.clone());
                            }
                        }
                    }
                }
            }
            last.retain(|id, _| wanted.contains_key(id));
            pending.retain(|id| wanted.contains_key(id));
            let ready = pending
                .iter()
                .filter(|id| {
                    last.get(*id)
                        .is_none_or(|t| t.elapsed() >= Duration::from_secs(5))
                })
                .cloned()
                .collect::<Vec<_>>();
            for id in ready {
                pending.remove(&id);
                last.insert(id.clone(), std::time::Instant::now());
                let result = observe_project(app.state(), id.clone()).await;
                let repo = state.repo.lock();
                if let Ok(repo) = repo {
                    if repo.folder(&id).ok().flatten().is_some() {
                        let _=repo.set_setting(&format!("project_watch_health:{id}"),&json!({"at":now(),"status":if result.is_ok(){"observed"}else{"failed"},"error":result.err()}).to_string());
                    }
                }
            }
        }
    });
}
