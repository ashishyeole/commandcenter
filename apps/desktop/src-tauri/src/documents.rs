use crate::{AppState, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use command_center_core::{attachments::Attachment, now};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tauri::State;
#[tauri::command]
pub fn list_attachments(
    state: State<AppState>,
    owner: Option<String>,
    query: String,
    deleted: bool,
) -> Result<Vec<Value>> {
    state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .attachment_summaries(owner.as_deref(), &query, deleted)
}
#[tauri::command]
pub async fn attach_document(
    state: State<'_, AppState>,
    owner: String,
    previous: Option<String>,
) -> Result<Option<String>> {
    uuid::Uuid::parse_str(&owner).map_err(|_| "Save the owning record first")?;
    let epoch = state.data_epoch.load(std::sync::atomic::Ordering::SeqCst);
    let Some(file) = rfd::AsyncFileDialog::new()
        .set_title("Attach an ordinary local document — use Vault for sensitive files")
        .pick_file()
        .await
    else {
        return Ok(None);
    };
    if std::fs::metadata(file.path())
        .map_err(|e| e.to_string())?
        .len()
        > 10 * 1024 * 1024
    {
        return Err("Each attachment must be at most 10 MiB".into());
    }
    let bytes = std::fs::read(file.path()).map_err(|e| e.to_string())?;
    if bytes.len() > 10 * 1024 * 1024 {
        return Err("Document grew beyond 10 MiB".into());
    }
    let filename = file.file_name();
    let ext = file
        .path()
        .extension()
        .and_then(|v| v.to_str())
        .unwrap_or("")
        .to_lowercase();
    let mime = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        "image/jpeg"
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        "image/gif"
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        "image/webp"
    } else if bytes.starts_with(b"%PDF-") {
        "application/pdf"
    } else if [
        "txt", "md", "csv", "json", "rs", "ts", "tsx", "js", "py", "yaml", "yml", "toml", "log",
    ]
    .contains(&ext.as_str())
        && std::str::from_utf8(&bytes).is_ok()
    {
        "text/plain"
    } else {
        "application/octet-stream"
    };
    let extracted_text = if mime == "text/plain" {
        let text = std::str::from_utf8(&bytes).map_err(|_| "Invalid text")?;
        let mut end = text.len().min(100000);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text[..end].to_owned()
    } else {
        String::new()
    };
    let a = Attachment {
        id: uuid::Uuid::new_v4().to_string(),
        owner_id: owner,
        filename,
        mime: mime.into(),
        sha256: format!("{:x}", Sha256::digest(&bytes)),
        content_base64: STANDARD.encode(&bytes),
        created_at: now(),
        previous_id: previous,
        deleted_at: None,
        extracted_text,
    };
    let mut repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    if state.data_epoch.load(std::sync::atomic::Ordering::SeqCst) != epoch {
        return Err("Workspace reset during file selection".into());
    }
    repo.add_attachment(&a)?;
    Ok(Some(a.id))
}
#[tauri::command]
pub fn preview_attachment(state: State<AppState>, id: String) -> Result<Value> {
    let a = state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .attachment(&id)?;
    let bytes = a.validate()?;
    // Preview only raster formats whose signature is checked independently of stored MIME.
    let safe = match a.mime.as_str() {
        "image/png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/jpeg" => bytes.starts_with(&[0xff, 0xd8, 0xff]),
        "image/gif" => bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"),
        "image/webp" => bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP"),
        _ => false,
    };
    Ok(
        json!({"image":if safe{Some(format!("data:{};base64,{}",a.mime,a.content_base64))}else{None},"text":a.extracted_text,"filename":a.filename}),
    )
}
#[tauri::command]
pub async fn export_attachment(state: State<'_, AppState>, id: String) -> Result<Option<String>> {
    let a = state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .attachment(&id)?;
    let bytes = a.validate()?;
    let Some(file) = rfd::AsyncFileDialog::new()
        .set_title("Export original attachment")
        .set_file_name(&a.filename)
        .save_file()
        .await
    else {
        return Ok(None);
    };
    crate::atomic_write(file.path(), &bytes)?;
    Ok(Some(file.path().display().to_string()))
}
#[tauri::command]
pub fn trash_attachment(state: State<AppState>, id: String, deleted: bool) -> Result<()> {
    state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .trash_attachment(&id, deleted)
}
