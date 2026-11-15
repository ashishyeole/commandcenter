//! Explicit local extraction through independently installed, fixed-path utilities.
use crate::{AppState, Result};
use serde_json::{json, Value};
use std::io::Write;
use tauri::State;
fn executable(kind: &str) -> Option<std::path::PathBuf> {
    let names: &[&str] = match kind {
        "pdf" => &[
            "/opt/homebrew/bin/pdftotext",
            "/usr/local/bin/pdftotext",
            "/usr/bin/pdftotext",
            r"C:\Program Files\poppler\Library\bin\pdftotext.exe",
        ],
        _ => &[
            "/opt/homebrew/bin/tesseract",
            "/usr/local/bin/tesseract",
            "/usr/bin/tesseract",
            r"C:\Program Files\Tesseract-OCR\tesseract.exe",
        ],
    };
    names
        .iter()
        .map(std::path::PathBuf::from)
        .find(|p| p.is_file())
}
#[tauri::command]
pub fn document_index_status() -> Value {
    json!({"pdf_available":executable("pdf").is_some(),"ocr_available":executable("ocr").is_some(),"notice":"Optional Poppler pdftotext / Tesseract English language data must be installed and maintained separately. No utilities are downloaded or executed by this status check."})
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
pub async fn index_document(
    state: State<'_, AppState>,
    id: String,
    consent: bool,
) -> Result<Value> {
    if !consent {
        return Err(
            "Approve extracting and indexing this selected ordinary document locally".into(),
        );
    }
    let epoch = state.data_epoch.load(std::sync::atomic::Ordering::SeqCst);
    let a = state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .attachment(&id)?;
    let bytes = a.validate()?;
    if a.deleted_at.is_some() {
        return Err("Restore this document before indexing".into());
    }
    let kind = if bytes.starts_with(b"%PDF-") {
        "pdf"
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") || bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        "ocr"
    } else {
        return Err("Explicit extraction supports PDF, PNG and JPEG; plain text files are indexed when attached".into());
    };
    let program = executable(kind).ok_or(
        "Install the required Poppler or Tesseract utility separately in its standard location",
    )?;
    let job_id = format!("index:{id}");
    let cancel = tokio_util::sync::CancellationToken::new();
    {
        let mut jobs = state.jobs.lock().map_err(|_| "Job state unavailable")?;
        if jobs.len() >= 4 || jobs.contains_key(&job_id) {
            return Err("Finish or cancel an active extraction first".into());
        }
        jobs.insert(job_id.clone(), cancel.clone());
    }
    let _guard = Guard {
        state: &state,
        id: job_id,
    };
    let mut file =
        tempfile::NamedTempFile::new().map_err(|_| "Cannot create extraction temporary file")?;
    file.write_all(&bytes)
        .map_err(|_| "Cannot prepare extraction")?;
    file.flush().map_err(|_| "Cannot flush extraction file")?;
    let work = async {
        use tokio::io::AsyncReadExt;
        let mut command = tokio::process::Command::new(program);
        if kind == "pdf" {
            command
                .args(["-f", "1", "-l", "100", "-enc", "UTF-8", "-nopgbrk"])
                .arg(file.path())
                .arg("-");
        } else {
            command
                .arg(file.path())
                .args(["stdout", "-l", "eng", "--psm", "3"]);
        }
        let mut child = command
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| "Cannot start local extraction utility")?;
        async fn read<R: tokio::io::AsyncRead + Unpin>(stream: R, max: u64) -> Result<Vec<u8>> {
            let mut bytes = vec![];
            stream
                .take(max + 1)
                .read_to_end(&mut bytes)
                .await
                .map_err(|_| "Extraction output interrupted")?;
            if bytes.len() as u64 > max {
                return Err("Extraction output exceeds bound; use a smaller document".into());
            }
            Ok(bytes)
        }
        let stdout = child.stdout.take().ok_or("Extraction stdout missing")?;
        let stderr = child.stderr.take().ok_or("Extraction stderr missing")?;
        let (output, _, status) =
            tokio::try_join!(read(stdout, 1024 * 1024), read(stderr, 16384), async {
                child
                    .wait()
                    .await
                    .map_err(|_| "Extraction interrupted".to_string())
            })?;
        if !status.success() {
            return Err::<String,String>("Local extraction failed. Encrypted/scanned PDFs or missing OCR language data may require preparation in a trusted document application.".into());
        }
        String::from_utf8(output).map_err(|_| "Extractor did not return UTF-8".into())
    };
    let mut text: String = tokio::select! {biased;_=cancel.cancelled()=>return Err("Document extraction cancelled".into()),result=tokio::time::timeout(std::time::Duration::from_secs(60),work)=>result.map_err(|_|"Document extraction exceeded 60 seconds")??};
    let truncated = text.len() > 100000;
    let mut end = text.len().min(100000);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    if text.trim().is_empty() {
        return Err("No text extracted. PDF text extraction does not OCR scanned pages; attach a PNG/JPEG for OCR.".into());
    }
    let mut repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    if epoch != state.data_epoch.load(std::sync::atomic::Ordering::SeqCst) {
        return Err("Workspace reset during extraction".into());
    }
    repo.index_attachment_text(&id, &a.sha256, &text)?;
    let receipt = json!({"at":command_center_core::now(),"engine":if kind=="pdf"{"local Poppler pdftotext"}else{"local Tesseract (English)"},"sha256":a.sha256,"indexed_bytes":text.len(),"truncated":truncated,"pdf_page_limit":100,"notice":"Extracted/OCR text may contain errors. Original bytes are unchanged. No document was uploaded."});
    repo.set_setting(&format!("attachment_index:{id}"), &receipt.to_string())?;
    Ok(receipt)
}
