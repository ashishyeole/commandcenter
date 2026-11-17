#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod agent_plans;
mod agent_schedules;
mod agriculture;
mod app_market;
mod background_reminders;
mod backups;
mod bank_accounts;
mod bank_reconcile;
mod business_demo;
mod business_discovery;
mod apple_charts;
mod citation_review;
mod cloud_ai;
mod connectors;
mod discovery_hub;
mod document_index;
mod document_security;
mod documents;
mod event_recurrence;
mod event_sources;
mod extensions;
mod github_accounts;
mod google_accounts;
mod google_background;
mod hosted;
mod integration_controls;
mod invoices;
mod large_archives;
mod mcp_stdio;
mod mcp_tools;
mod model_catalogue;
mod model_intelligence;
mod notification_channels;
mod peer_sync;
mod performance;
mod poker_search;
mod poker_locations;
mod price_watch;
mod private_transfer;
mod productivity;
mod projects;
mod qa_profile;
mod rendered_audit;
mod research;
mod retention;
mod scholarly_sources;
mod sync_wire;
mod updates;
mod vault;
mod voice;
mod webdav_backup;
mod webdav_stream;
mod website_audit;
use command_center_core::{
    archive::{self, ExportData},
    workspace::Record,
    Item, Repository, Snapshot,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::HashMap, io::Write, path::Path, sync::Mutex};
use tauri::{Emitter, Manager, State};
use tauri_plugin_global_shortcut::GlobalShortcutExt;
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;
use zeroize::Zeroizing;
type Result<T> = std::result::Result<T, String>;
struct AppState {
    repo: Mutex<Repository>,
    peer_sync: Mutex<peer_sync::Runtime>,
    project_watchers: Mutex<HashMap<String, projects::WatchBinding>>,
    pending_update: Mutex<Option<updates::Candidate>>,
    automation_lock: tokio::sync::Mutex<()>,
    backup_lock: tokio::sync::Mutex<()>,
    data_epoch: std::sync::atomic::AtomicU64,
    pending: Mutex<Option<ExportData>>,
    large_import: Mutex<Option<large_archives::Pending>>,
    google_cache: Mutex<Option<google_background::Cache>>,
    vault: Mutex<Option<vault::VaultSession>>,
    jobs: Mutex<HashMap<String, tokio_util::sync::CancellationToken>>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Preferences {
    notifications: bool,
    quiet_start: u32,
    quiet_end: u32,
    enabled_modules: Vec<String>,
    dashboard_widgets: Vec<String>,
    interests: Vec<String>,
    backup_reminder_days: u32,
    escalate_critical: bool,
    ui_scale: u32,
    theme: String,
    high_contrast: bool,
    reduce_motion: bool,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            dashboard_widgets: [
                "capture", "summary", "pins", "tasks", "dates", "briefs", "feeds",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
            interests: vec![],
            backup_reminder_days: 30,
            ui_scale: 100,
            theme: "mono".into(),
            high_contrast: false,
            reduce_motion: false,
            escalate_critical: false,
            notifications: false,
            quiet_start: 22,
            quiet_end: 8,
            enabled_modules: command_center_core::workspace::MODULES
                .iter()
                .map(|s| s.to_string())
                .collect(),
        }
    }
}
fn preferences(repo: &Repository) -> Result<Preferences> {
    match repo.setting("preferences")? {
        Some(s) => serde_json::from_str(&s).map_err(|e| e.to_string()),
        None => Ok(Preferences::default()),
    }
}
#[tauri::command]
fn snapshot(state: State<AppState>, query: String) -> Result<Snapshot> {
    state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .snapshot(&query)
}
#[tauri::command]
fn save_item(state: State<AppState>, item: Item) -> Result<Item> {
    state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .save(item)
}
#[tauri::command]
fn complete_task(state: State<AppState>, id: String) -> Result<Item> {
    state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .complete(&id)
}
#[tauri::command]
fn workspace_snapshot(state: State<AppState>, query: String) -> Result<Value> {
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    Ok(
        json!({"watched_projects":repo.setting("watched_projects")?.and_then(|s|serde_json::from_str::<Value>(&s).ok()).unwrap_or(json!([])),"monitored_projects":repo.setting("monitored_projects")?.and_then(|s|serde_json::from_str::<Value>(&s).ok()).unwrap_or(json!([])),"discovery_state":repo.setting("discovery_state")?.and_then(|s|serde_json::from_str::<Value>(&s).ok()).unwrap_or(json!({})),"records":repo.records(&query,false)?,"feeds":repo.feeds()?,"preferences":preferences(&repo)?,"last_backup":repo.setting("last_backup")?.and_then(|s|serde_json::from_str::<Value>(&s).ok()),"history":repo.recent_history()?,"shortcut_error":repo.setting("shortcut_error")?,"notification_error":repo.setting("notification_error")?,"device_id":repo.device_id}),
    )
}
#[tauri::command]
fn search_records(state: State<AppState>, query: String) -> Result<Vec<Record>> {
    state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .records(&query, false)
}
#[tauri::command]
fn save_record(state: State<AppState>, record: Record) -> Result<Record> {
    state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .save_record(record)
}
#[tauri::command]
fn save_preferences(state: State<AppState>, value: Preferences) -> Result<()> {
    if value.dashboard_widgets.iter().any(|s| {
        ![
            "capture", "summary", "pins", "tasks", "dates", "briefs", "feeds",
        ]
        .contains(&s.as_str())
    }) || value.dashboard_widgets.len() > 7
        || value.interests.len() > 20
        || value.interests.iter().any(|s| s.len() > 80)
        || ![100, 115, 130, 150, 175, 200].contains(&value.ui_scale)
        || !["mono", "lumen", "slate", "midnight"].contains(&value.theme.as_str())
        || value.backup_reminder_days > 365
        || value.quiet_start > 23
        || value.quiet_end > 23
        || value
            .enabled_modules
            .iter()
            .any(|m| !command_center_core::workspace::MODULES.contains(&m.as_str()))
    {
        return Err("Invalid preferences".into());
    }
    state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .set_setting(
            "preferences",
            &serde_json::to_string(&value).map_err(|e| e.to_string())?,
        )
}
#[tauri::command]
fn trash(state: State<AppState>) -> Result<Value> {
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    Ok(
        json!({"items":repo.list("",true)?.into_iter().filter(|i|i.deleted_at.is_some()).collect::<Vec<_>>(),"records":repo.records("",true)?.into_iter().filter(|r|r.deleted_at.is_some()).collect::<Vec<_>>()}),
    )
}
#[tauri::command]
fn open_source(app: tauri::AppHandle, url: String) -> Result<()> {
    let parsed = url::Url::parse(&url).map_err(|_| "Invalid URL")?;
    if !["http", "https"].contains(&parsed.scheme())
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err("Only HTTP(S) sources without credentials can be opened".into());
    }
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| e.to_string())
}
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().ok_or("Invalid destination")?;
    let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    temp.write_all(bytes).map_err(|e| e.to_string())?;
    temp.as_file().sync_all().map_err(|e| e.to_string())?;
    temp.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}
#[tauri::command]
async fn export_archive(
    state: State<'_, AppState>,
    password: String,
    include_vault: bool,
    modules: Vec<String>,
    include_dependencies: Option<bool>,
) -> Result<Option<String>> {
    let password = Zeroizing::new(password);
    if include_vault && password.is_empty() {
        return Err("Vault records require an encrypted export".into());
    }
    let mut data = {
        let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
        ExportData {
            items: repo.list("", true)?,
            workspace: repo.portable_workspace()?,
            vault: vec![],
        }
    };
    if !modules.is_empty() {
        data.workspace.preferences = None;
        data.workspace.discovery_sources = None;
        data.workspace.feeds.clear();
        let mut selected: std::collections::HashSet<String> = data
            .items
            .iter()
            .filter(|i| modules.contains(&i.kind))
            .map(|i| i.id.clone())
            .chain(
                data.workspace
                    .records
                    .iter()
                    .filter(|r| modules.contains(&r.module))
                    .map(|r| r.id.clone()),
            )
            .collect();
        if include_dependencies.unwrap_or(false) {
            loop {
                let before = selected.len();
                for r in &data.workspace.records {
                    if selected.contains(&r.id) {
                        selected.extend(r.links.clone());
                    }
                }
                for i in &data.items {
                    if selected.contains(&i.id) {
                        if let Some(parent) = i.recurrence.strip_prefix("after:") {
                            selected.insert(parent.into());
                        }
                    }
                }
                if selected.len() == before {
                    break;
                }
            }
        }
        data.items.retain(|i| selected.contains(&i.id));
        data.workspace.records.retain(|r| selected.contains(&r.id));
        let ids: Vec<&str> = data
            .items
            .iter()
            .map(|i| i.id.as_str())
            .chain(data.workspace.records.iter().map(|r| r.id.as_str()))
            .collect();
        data.workspace
            .history
            .retain(|e| ids.contains(&e.entity_id.as_str()));
        data.workspace
            .attachments
            .retain(|a| ids.contains(&a.owner_id.as_str()));
    }
    if include_vault {
        let key = vault::session_key(&state)?;
        for (id, bytes) in state
            .repo
            .lock()
            .map_err(|_| "Database unavailable")?
            .vault_rows()?
        {
            data.vault.push(vault::reveal(&key, &id, &bytes)?);
        }
    }
    let count = data.items.len()
        + data.workspace.records.len()
        + data.vault.len()
        + data.workspace.attachments.len();
    let zip = Zeroizing::new(archive::export_all(&data)?);
    archive::inspect_all(&zip)?;
    let encrypted = !password.is_empty();
    let bytes = Zeroizing::new(if encrypted {
        archive::encrypt(&zip, &password)?
    } else {
        zip.to_vec()
    });
    if let Some(file) = rfd::AsyncFileDialog::new()
        .set_file_name(if encrypted {
            "command-center.pacc"
        } else {
            "command-center.zip"
        })
        .save_file()
        .await
    {
        atomic_write(file.path(), &bytes)?;
        let read = Zeroizing::new(std::fs::read(file.path()).map_err(|e| e.to_string())?);
        let check = Zeroizing::new(if encrypted {
            archive::decrypt(&read, &password)?
        } else {
            read.to_vec()
        });
        archive::inspect_all(&check)?;
        state.repo.lock().map_err(|_|"Database unavailable")?.set_setting("last_backup",&json!({"verified_at":command_center_core::now(),"path":file.path().display().to_string(),"encrypted":encrypted,"records":count,"includes_vault":include_vault,"modules":modules,"bytes":bytes.len()}).to_string())?;
        Ok(Some(format!(
            "Verified {count} records → {}",
            file.path().display()
        )))
    } else {
        Ok(None)
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MobileExchange {
    format: String,
    schema: u32,
    items: Vec<Item>,
}
fn read_mobile(bytes: &[u8]) -> Result<ExportData> {
    let exchange: MobileExchange = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    if exchange.format != "personal-command-center-mobile"
        || exchange.schema != 1
        || exchange.items.len() > 10000
    {
        return Err("Unsupported mobile exchange".into());
    }
    let mut ids = std::collections::HashSet::new();
    for item in &exchange.items {
        item.validate()?;
        if !ids.insert(&item.id) {
            return Err("Duplicate mobile record ID".into());
        }
    }
    Ok(ExportData {
        items: exchange.items,
        workspace: Default::default(),
        vault: vec![],
    })
}
#[tauri::command]
async fn export_mobile(state: State<'_, AppState>) -> Result<Option<String>> {
    let items = state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .list("", false)?;
    let data = serde_json::to_vec_pretty(&MobileExchange {
        format: "personal-command-center-mobile".into(),
        schema: 1,
        items,
    })
    .map_err(|e| e.to_string())?;
    if let Some(file) = rfd::AsyncFileDialog::new()
        .set_file_name("command-center-mobile.json")
        .save_file()
        .await
    {
        atomic_write(file.path(), &data)?;
        Ok(Some(file.path().display().to_string()))
    } else {
        Ok(None)
    }
}
#[derive(Serialize)]
struct Preview {
    total: usize,
    new_records: usize,
    conflicts: usize,
    duplicates: usize,
    vault_records: usize,
    history_events: usize,
    cached_feeds: usize,
    has_preferences: bool,
}
#[tauri::command]
async fn inspect_archive(state: State<'_, AppState>, password: String) -> Result<Option<Preview>> {
    *state.pending.lock().map_err(|_| "Import unavailable")? = None;

    let password = Zeroizing::new(password);
    if let Some(file) = rfd::AsyncFileDialog::new()
        .add_filter("Command Center backup", &["zip", "pacc", "json"])
        .pick_file()
        .await
    {
        if std::fs::metadata(file.path())
            .map_err(|e| e.to_string())?
            .len()
            > 64 * 1024 * 1024 + 50
        {
            return Err("Archive exceeds 64 MiB".into());
        }
        let bytes = Zeroizing::new(std::fs::read(file.path()).map_err(|e| e.to_string())?);
        let encrypted = bytes.starts_with(b"PACC01");
        let zip = Zeroizing::new(if encrypted {
            archive::decrypt(&bytes, &password)?
        } else {
            bytes.to_vec()
        });
        let data = if file.path().extension().and_then(|s| s.to_str()) == Some("json") {
            read_mobile(&zip)?
        } else {
            archive::inspect_all(&zip)?
        };
        if !data.vault.is_empty() {
            if !encrypted {
                return Err("Sensitive vault imports must be password encrypted".into());
            }
            let _ = vault::session_key(&state)?;
        }
        let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
        let items = repo.list("", true)?;
        let records = repo.records("", true)?;
        let vault_rows = repo.vault_rows()?;
        let mut p = Preview {
            total: data.items.len()
                + data.workspace.records.len()
                + data.vault.len()
                + data.workspace.attachments.len(),
            new_records: 0,
            conflicts: 0,
            duplicates: 0,
            vault_records: data.vault.len(),
            history_events: data.workspace.history.len(),
            cached_feeds: data.workspace.feeds.len(),
            has_preferences: data.workspace.preferences.is_some()
                || data.workspace.discovery_sources.is_some(),
        };
        for i in &data.items {
            match items.iter().find(|e| e.id == i.id) {
                None => p.new_records += 1,
                Some(e) if e == i => p.duplicates += 1,
                Some(_) => p.conflicts += 1,
            }
        }
        for r in &data.workspace.records {
            match records.iter().find(|e| e.id == r.id) {
                None => p.new_records += 1,
                Some(e) if e == r => p.duplicates += 1,
                Some(_) => p.conflicts += 1,
            }
        }
        let existing_attachments = repo.attachments(None, "", true)?;
        for a in &data.workspace.attachments {
            match existing_attachments.iter().find(|v| v.id == a.id) {
                None => p.new_records += 1,
                Some(v) if v == a => p.duplicates += 1,
                Some(_) => p.conflicts += 1,
            }
        }
        for v in &data.vault {
            if vault_rows.iter().any(|(id, _)| id == &v.id) {
                p.conflicts += 1;
            } else {
                p.new_records += 1;
            }
        }
        *state.pending.lock().map_err(|_| "Import unavailable")? = Some(data);
        Ok(Some(p))
    } else {
        Ok(None)
    }
}
#[tauri::command]
fn cancel_import(state: State<AppState>) -> Result<()> {
    *state.pending.lock().map_err(|_| "Import unavailable")? = None;

    Ok(())
}
#[tauri::command]
fn apply_import(
    state: State<AppState>,
    conflict_mode: String,
    restore_configuration: Option<bool>,
) -> Result<usize> {
    if !["keep", "copy"].contains(&conflict_mode.as_str()) {
        return Err("Choose a supported conflict policy".into());
    }
    let mut data = state
        .pending
        .lock()
        .map_err(|_| "Import unavailable")?
        .take()
        .ok_or("Preview an archive first")?;
    if !restore_configuration.unwrap_or(false) {
        data.workspace.preferences = None;
        data.workspace.discovery_sources = None;
        data.workspace.feeds.clear();
    }
    if conflict_mode == "copy" {
        let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
        let existing_items = repo.list("", true)?;
        let existing_records = repo.records("", true)?;
        let existing_vault = repo.vault_rows()?;
        let existing_attachments = repo.attachments(None, "", true)?;
        let mut ids = HashMap::new();
        for i in &data.items {
            if existing_items.iter().any(|e| e.id == i.id && e != i) {
                ids.insert(i.id.clone(), uuid::Uuid::new_v4().to_string());
            }
        }
        for r in &data.workspace.records {
            if existing_records.iter().any(|e| e.id == r.id && e != r) {
                ids.insert(r.id.clone(), uuid::Uuid::new_v4().to_string());
            }
        }
        for v in &data.vault {
            if existing_vault.iter().any(|(id, _)| id == &v.id) {
                ids.insert(v.id.clone(), uuid::Uuid::new_v4().to_string());
            }
        }
        // Copy incoming link owners too, so an existing duplicate cannot silently keep
        // a link to the old conflicting entity when restoring the incoming graph.
        loop {
            let mut added = false;
            for record in &data.workspace.records {
                if !ids.contains_key(&record.id)
                    && existing_records.iter().any(|e| e.id == record.id)
                    && record.links.iter().any(|id| ids.contains_key(id))
                {
                    ids.insert(record.id.clone(), uuid::Uuid::new_v4().to_string());
                    added = true;
                }
            }
            if !added {
                break;
            }
        }
        for a in &data.workspace.attachments {
            if ids.contains_key(&a.owner_id)
                || existing_attachments.iter().any(|v| v.id == a.id && v != a)
            {
                ids.insert(a.id.clone(), uuid::Uuid::new_v4().to_string());
            }
        }
        for a in &mut data.workspace.attachments {
            if let Some(id) = ids.get(&a.id) {
                a.id = id.clone();
            }
            if let Some(id) = ids.get(&a.owner_id) {
                a.owner_id = id.clone();
            }
            if let Some(id) = a.previous_id.as_ref().and_then(|id| ids.get(id)) {
                a.previous_id = Some(id.clone());
            }
        }
        for i in &mut data.items {
            if let Some(new) = i
                .recurrence
                .strip_prefix("after:")
                .and_then(|id| ids.get(id))
            {
                i.recurrence = format!("after:{new}");
            }
            if let Some(new) = ids.get(&i.id) {
                i.id = new.clone();
            }
        }
        for r in &mut data.workspace.records {
            if let Some(new) = ids.get(&r.id) {
                r.id = new.clone();
            }
            for id in &mut r.links {
                if let Some(new) = ids.get(id) {
                    *id = new.clone();
                }
            }
        }
        for v in &mut data.vault {
            if let Some(new) = ids.get(&v.id) {
                v.id = new.clone();
            }
        }
        for e in &mut data.workspace.history {
            if let Some(new) = ids.get(&e.entity_id) {
                e.entity_id = new.clone();
                e.event_id = uuid::Uuid::new_v4().to_string();
                let mut payload: Value =
                    serde_json::from_str(&e.payload).map_err(|_| "Invalid conflict history")?;
                if let Some(object) = payload.as_object_mut() {
                    object.insert("id".into(), json!(new));
                    if let Some(links) = object.get_mut("links").and_then(Value::as_array_mut) {
                        for link in links {
                            if let Some(new_id) = link.as_str().and_then(|id| ids.get(id)) {
                                *link = json!(new_id);
                            }
                        }
                    }
                }
                e.payload = payload.to_string();
            }
        }
    }
    let mut vault_rows = vec![];
    if !data.vault.is_empty() {
        let key = vault::session_key(&state)?;
        for entry in &data.vault {
            vault_rows.push((entry.id.clone(), vault::seal(&key, entry)?));
        }
    }
    state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .import_snapshot(&data.items, &data.workspace, &vault_rows)
}
#[tauri::command]
fn data_inventory(app: tauri::AppHandle, state: State<AppState>) -> Result<Value> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    let files=["command-center.db","command-center.db-wal","command-center.db-shm"].iter().map(|n|json!({"path":dir.join(n).display().to_string(),"bytes":std::fs::metadata(dir.join(n)).map(|m|m.len()).unwrap_or(0)})).collect::<Vec<_>>();
    Ok(
        json!({"files":files,"vault_records":repo.vault_rows()?.len(),"ordinary_records":repo.list("",true)?.len()+repo.records("",true)?.len(),"attachments":repo.attachment_summaries(None,"",true)?.len(),"last_backup":repo.setting("last_backup")?.and_then(|s|serde_json::from_str::<Value>(&s).ok()),"note":"No models or source repositories are stored here. Linked source folders and exported backups are never deleted by reset."}),
    )
}
#[tauri::command]
fn reset_local_data(
    state: State<AppState>,
    confirmation: String,
    sensitive_confirmation: String,
) -> Result<()> {
    if confirmation != "DELETE LOCAL DATA" {
        return Err("Type DELETE LOCAL DATA to confirm reset".into());
    }
    voice::desktop_voice_stop(state.clone(), true)?;
    peer_sync::halt(&state)?;
    let mut repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    if state
        .jobs
        .lock()
        .map_err(|_| "Job state unavailable")?
        .contains_key("update:download")
        && repo
            .setting("update_progress")?
            .is_some_and(|v| v.contains("\"installing\""))
    {
        return Err("Wait for application installation to finish before resetting data".into());
    }
    if !repo.vault_rows()?.is_empty() && sensitive_confirmation != "DELETE VAULT TOO" {
        return Err("Type DELETE VAULT TOO to confirm sensitive record deletion".into());
    }
    for token in state.jobs.lock().map_err(|_| "AI unavailable")?.values() {
        token.cancel();
    }
    state
        .project_watchers
        .lock()
        .map_err(|_| "Watcher state unavailable")?
        .clear();
    *state
        .pending_update
        .lock()
        .map_err(|_| "Update state unavailable")? = None;
    repo.reset()?;
    state
        .data_epoch
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    *state.pending.lock().map_err(|_| "Import unavailable")? = None;
    *state
        .large_import
        .lock()
        .map_err(|_| "Import unavailable")? = None;
    *state.vault.lock().map_err(|_| "Vault unavailable")? = None;
    *state
        .google_cache
        .lock()
        .map_err(|_| "Account cache unavailable")? = None;
    Ok(())
}
fn poll_reminders(app: &tauri::AppHandle) -> Result<()> {
    use chrono::Timelike;
    let state = app.state::<AppState>();
    let coordinating_peers = peer_sync::reachable_notification_peers(&state)?;
    let mut repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    if !coordinating_peers.is_empty() {
        repo.prepare_sync()?;
    }
    if !repo.acquire_reminder_poll(chrono::Utc::now().timestamp())? {
        return Ok(());
    }
    repo.activate_conditions()?;
    let p = preferences(&repo)?;
    if !p.notifications {
        return Ok(());
    }
    let local = chrono::Local::now();
    let hour = local.hour();
    let quiet = if p.quiet_start > p.quiet_end {
        hour >= p.quiet_start || hour < p.quiet_end
    } else {
        hour >= p.quiet_start && hour < p.quiet_end
    };
    if quiet {
        return Ok(());
    }
    let today = local.format("%Y-%m-%d").to_string();
    let device = repo.device_id.clone();
    for item in repo.list("", false)?.into_iter().filter(|i| {
        i.kind == "task" && i.status == "active" && i.due.as_ref().is_some_and(|d| d <= &today)
    }) {
        let snooze = repo
            .setting(&format!("snooze:{}", item.id))?
            .unwrap_or_default();
        let policy = repo.task_reminder_policy(&item.id)?;
        let Some(delivery) = command_center_core::reminders::delivery_key(
            &item,
            &policy,
            &today,
            chrono::Utc::now(),
            &snooze,
            p.escalate_critical,
        ) else {
            continue;
        };
        if repo.delivery_exists(&delivery)? {
            continue;
        }
        if coordinating_peers.iter().any(|peer| {
            repo.peer_has_current_entity(peer, &item.id)
                .unwrap_or(false)
                && repo
                    .setting(&format!("peer_deliveries:{peer}"))
                    .ok()
                    .flatten()
                    .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
                    .is_some_and(|receipts| receipts.contains(&delivery))
        }) {
            repo.mark_delivered(&delivery)?;
            continue;
        }

        app.notification()
            .builder()
            .title("A task needs your attention")
            .body("Open Personal Command Center to review your due tasks.")
            .show()
            .map_err(|e| e.to_string())?;
        repo.mark_delivered(&delivery)?;
        let mut n = Record::new("notifications", &item.title, &device);
        n.status = "delivered".into();
        n.links.push(item.id);
        n.summary = format!(
            "Due {}. Desktop notification submitted at {}. Delivery is subject to OS settings.",
            item.due.unwrap_or_default(),
            command_center_core::now()
        );
        repo.save_record(n)?;
    }
    Ok(())
}
fn main() {
    let background = std::env::args().any(|a| a == "--reminder-check");
    let mut context = tauri::generate_context!();
    match qa_profile::identifier(&std::env::args().collect::<Vec<_>>(), cfg!(debug_assertions)) {
        Ok(Some(identifier)) => {
            context.config_mut().identifier = identifier;
            for window in &mut context.config_mut().app.windows {
                window.title = "Personal Command Center — Isolated QA".into();
            }
        }
        Ok(None) => {}
        Err(error) => { eprintln!("{error}"); std::process::exit(2); }
    }
    if background {
        for window in &mut context.config_mut().app.windows {
            window.visible = false;
        }
    }
    let command_handler: fn(tauri::ipc::Invoke<tauri::Wry>) -> bool = tauri::generate_handler![
        cloud_ai::save_cloud_key,
        cloud_ai::disconnect_cloud,
        citation_review::cloud_evidence_review,
        citation_review::confirm_cloud_evidence,
        cloud_ai::cloud_context_preview,
        cloud_ai::ask_cloud,
        documents::list_attachments,
        documents::attach_document,
        documents::preview_attachment,
        documents::export_attachment,
        documents::trash_attachment,
        backups::backup_schedule_snapshot,
        webdav_stream::download_webdav_stream,
        backups::choose_backup_destination,
        webdav_backup::save_webdav_destination,
        backups::save_backup_schedule,
        backups::forget_backup_schedule,
        backups::run_scheduled_backup,
        productivity::mark_discovery,
        background_reminders::background_reminder_status,
        background_reminders::configure_background_reminders,
        poker_search::poker_search_plan,
        productivity::automation_snapshot,
        productivity::research_collections,
        productivity::save_research_collections,
        productivity::save_automation,
        productivity::run_automation,
        productivity::plan_tasks,
        productivity::save_workspace_document,
        productivity::export_document,
        productivity::snooze_task,
        productivity::task_reminder_status,
        productivity::update_task_reminder,
        peer_sync::set_automatic_sync,
        peer_sync::set_private_mesh,
        peer_sync::export_sync_diagnostics,
        peer_sync::peer_sync_status,
        peer_sync::start_peer_sync,
        peer_sync::stop_peer_sync,
        peer_sync::create_pair_invitation,
        peer_sync::approve_pairing,
        peer_sync::join_peer,
        peer_sync::sync_peer_now,
        peer_sync::revoke_peer,
        peer_sync::approve_private_transfer,
        peer_sync::send_private_category,
        peer_sync::rotate_peer_identity,
        peer_sync::resolve_peer_conflict,
        snapshot,
        price_watch::refresh_price_watch,
        performance::benchmark_local_workspace,
        save_item,
        complete_task,
        trash,
        open_source,
        workspace_snapshot,
        search_records,
        save_record,
        save_preferences,
        export_archive,
        large_archives::export_large_archive,
        large_archives::preview_large_archive,
        large_archives::apply_large_archive,
        large_archives::review_large_archive_records,
        large_archives::discard_large_archive,
        export_mobile,
        inspect_archive,
        apply_import,
        cancel_import,
        data_inventory,
        reset_local_data,
        connectors::refresh_source,
        connectors::refresh_public_channel,
        discovery_hub::discovery_hub,
        discovery_hub::add_research_starters,
        discovery_hub::save_discovery_imports,
        discovery_hub::choose_discovery_link_import,
        discovery_hub::open_discovery_reader,
        discovery_hub::save_discovery_reddit_access,
        discovery_hub::remove_discovery_reddit_access,
        discovery_hub::refresh_discovery_hub,
        discovery_hub::cancel_discovery_hub,
        discovery_hub::set_discovery_auto_refresh,
        discovery_hub::create_discovery_search,
        discovery_hub::save_discovery_stream,
        discovery_hub::remove_discovery_stream,
        discovery_hub::refresh_discovery_stream,
        discovery_hub::save_discovery_x_token,
        discovery_hub::remove_discovery_x_token,
        app_market::app_market_snapshots,
        app_market::configure_market_feed,
        app_market::remove_market_feed,
        app_market::refresh_market_feed,
        app_market::import_app_market_snapshot,
        agent_schedules::agent_schedule_status,
        agent_schedules::configure_agent_schedule,
        document_index::document_index_status,
        document_index::index_document,
        business_demo::export_business_demo,
        model_intelligence::model_evidence,
        model_intelligence::inspect_public_model,
        connectors::save_youtube_key,
        connectors::remove_youtube_key,
        connectors::cancel_ai,
        projects::choose_project_folder,
        projects::project_folder,
        projects::revoke_project_folder,
        website_audit::audit_public_website,
        google_background::google_background_status,
        google_background::save_google_background,
        google_background::read_google_background,
        google_accounts::google_account_status,
        google_accounts::configure_google_account,
        google_accounts::connect_google_account,
        google_accounts::read_google_account,
        google_accounts::read_google_message,
        google_accounts::disconnect_google_account,
        mcp_tools::mcp_connections,
        mcp_tools::mcp_health,
        mcp_tools::save_mcp_connection,
        mcp_tools::remove_mcp_connection,
        mcp_tools::preview_mcp_call,
        mcp_tools::run_mcp_request,
        voice::desktop_voice_status,
        voice::desktop_voice_start,
        voice::desktop_voice_stop,
        invoices::invoice_totals,
        invoices::save_invoice,
        invoices::export_invoice,
        agent_plans::save_agent_plan,
        agent_plans::preview_agent_plan,
        agent_plans::run_agent_plan,
        bank_reconcile::choose_statement,
        bank_reconcile::preview_statement,
        bank_reconcile::apply_statement,
        integration_controls::integration_inventory,
        integration_controls::ai_context_policy,
        integration_controls::set_ai_context_exclusion,
        integration_controls::pause_data_connections,
        bank_accounts::bank_connection_status,
        bank_accounts::start_bank_enrollment,
        bank_accounts::complete_bank_enrollment,
        bank_accounts::cancel_bank_enrollment,
        bank_accounts::configure_bank_connection,
        bank_accounts::disconnect_bank_connection,
        bank_accounts::read_bank_connection,
        business_discovery::cached_business_search,
        business_discovery::search_public_businesses,
        business_discovery::cancel_public_businesses,
        business_discovery::save_discovered_lead,
        projects::set_project_watching,
        extensions::list_extensions,
        extensions::validate_extension_manifest,
        extensions::export_extension_manifest,
        extensions::install_extension,
        extensions::configure_extension,
        extensions::preview_extension,
        extensions::run_extension,
        extensions::pull_extension_image,
        github_accounts::github_account_status,
        github_accounts::configure_github_account,
        github_accounts::connect_github_account,
        github_accounts::read_github_account,
        github_accounts::disconnect_github_account,
        hosted::browse_extension_catalog,
        hosted::hosted_catalog_action,
        hosted::hosted_export,
        hosted::hosted_reset,
        notification_channels::reminder_channels,
        notification_channels::configure_reminder_channel,
        notification_channels::disable_reminder_channel,
        notification_channels::send_personal_reminder,
        event_sources::event_source_status,
        event_sources::save_event_source,
        event_sources::remove_event_source,
        event_sources::refresh_event_source,
        event_sources::save_calendar_event,
        agriculture::configure_agriculture_key,
        agriculture::remove_agriculture_key,
        agriculture::agriculture_status,
        agriculture::fetch_agriculture,
        agriculture::save_agriculture_observation,
        document_security::attachment_security_status,
        document_security::scan_attachment,
        retention::preview_retention,
        retention::preview_sync_compaction,
        retention::apply_sync_compaction,
        retention::apply_retention,
        retention::export_diagnostics,
        hosted::hosted_status,
        hosted::configure_hosted,
        hosted::disconnect_hosted,
        hosted::hosted_read,
        hosted::hosted_save_job,
        hosted::hosted_delete,
        hosted::hosted_send_archive,
        hosted::hosted_download_archive,
        updates::updater_status,
        updates::save_update_publisher,
        updates::disable_updates,
        updates::check_signed_update,
        updates::install_signed_update,
        projects::observe_project,
        projects::inspect_project_manifests,
        projects::set_project_monitoring,
        projects::resume_project,
        vault::unlock_vault,
        vault::lock_vault,
        vault::vault_entries,
        vault::save_vault_entry,
        vault::attach_vault_file,
        vault::export_vault_file
    ];
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _, event| {
                    if event.state() == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.unminimize();
                            let _ = window.set_focus();
                            let _ = window.emit("open-command-palette", ());
                        }
                    }
                })
                .build(),
        )
        .setup(move |app| {
            let dir = app.path().app_data_dir()?;
            if let Some(expected) = qa_profile::identifier(&std::env::args().collect::<Vec<_>>(), cfg!(debug_assertions)).map_err(std::io::Error::other)? {
                if dir.file_name().and_then(|v| v.to_str()) != Some(expected.as_str()) {
                    return Err(std::io::Error::other("QA profile path mismatch; refusing to open any database").into());
                }
            }
            std::fs::create_dir_all(&dir)?;
            let repo =
                Repository::open(dir.join("command-center.db")).map_err(std::io::Error::other)?;
            if !background {
                discovery_hub::initialize_public_defaults(&repo).map_err(std::io::Error::other)?;
            }
            app.manage(AppState {
                repo: Mutex::new(repo),
                peer_sync: Mutex::new(peer_sync::Runtime::default()),
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
            });
            if background {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let allowed = handle
                        .state::<AppState>()
                        .repo
                        .lock()
                        .ok()
                        .and_then(|repo| repo.setting("background_reminders").ok().flatten())
                        .as_deref()
                        == Some("true");
                    if allowed {
                        let result = poll_reminders(&handle);
                        if let Ok(repo) = handle.state::<AppState>().repo.lock() {
                            let _ = repo.set_setting(
                                "background_reminder_check",
                                &command_center_core::now(),
                            );
                            let _ = repo.set_setting(
                                "background_reminder_error",
                                result.err().as_deref().unwrap_or(""),
                            );
                        }
                    }
                    handle.exit(0);
                });
                return Ok(());
            }
            if let Err(error) = app
                .global_shortcut()
                .register("CommandOrControl+Shift+Space")
            {
                let state = app.state::<AppState>();
                if let Ok(repo) = state.repo.lock() {
                    let _ = repo.set_setting("shortcut_error", &error.to_string());
                };
            }
            let peer_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                    let _ = peer_sync::poll(&peer_handle).await;
                }
            });
            let backup_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                    let _ = backups::poll(&backup_handle).await;
                }
            });
            let automation_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                    let _ = productivity::poll(&automation_handle).await;
                    let _ = agent_schedules::poll(&automation_handle).await;
                }
            });
            let discovery_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    let _ = discovery_hub::poll(&discovery_handle).await;
                    tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                }
            });
            projects::start_watchers(app.handle().clone());
            let project_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(120)).await;
                    let _ = projects::poll(&project_handle).await;
                    let _ = event_sources::poll(&project_handle).await;
                    let _ = google_background::poll(&project_handle).await;
                }
            });
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                    let _ = notification_channels::poll(&handle).await;
                    if let Err(error) = poll_reminders(&handle) {
                        let state = handle.state::<AppState>();
                        if let Ok(repo) = state.repo.lock() {
                            let _ = repo.set_setting("notification_error", &error);
                        };
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(move |invoke: tauri::ipc::Invoke<tauri::Wry>| {
            // Original-source windows are untrusted remote content, never app IPC clients.
            if invoke.message.webview_ref().label() != "main" {
                invoke
                    .resolver
                    .reject("Native commands are available only in the local main window");
                return true;
            }
            command_handler(invoke)
        })
        .run(context)
        .expect("Could not launch Personal Command Center");
}

#[cfg(test)]
mod startup_config_tests {
    #[test]
    fn bundled_updater_configuration_deserializes_without_network_defaults() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let updater: tauri_plugin_updater::Config =
            serde_json::from_value(config["plugins"]["updater"].clone()).unwrap();
        assert!(updater.endpoints.is_empty());
        assert!(updater.pubkey.is_empty()); // Owner-approved key is injected per reviewed update request.
    }
}
