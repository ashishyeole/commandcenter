use crate::{AppState, Result};
use serde::{Deserialize, Serialize};
use tauri::State;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Draft {
    pub headline: String,
    pub introduction: String,
    pub services: Vec<String>,
    pub contact: String,
    pub accent: String,
}
fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
#[tauri::command]
pub async fn export_business_demo(
    state: State<'_, AppState>,
    record_id: String,
    revision: i64,
    draft: Draft,
    consent: bool,
) -> Result<Option<String>> {
    if !consent {
        return Err(
            "Approve exporting this reviewed proposal demo; it will not be published or sent"
                .into(),
        );
    }
    if draft.headline.trim().is_empty()
        || draft.headline.len() > 300
        || draft.introduction.len() > 8000
        || draft.contact.len() > 1000
        || draft.services.len() > 12
        || draft
            .services
            .iter()
            .any(|s| s.trim().is_empty() || s.len() > 500)
        || !matches!(draft.accent.as_str(), "teal" | "blue" | "plum")
    {
        return Err("Invalid or oversized demo content".into());
    }
    let epoch = state.data_epoch.load(std::sync::atomic::Ordering::SeqCst);
    let record = {
        let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
        repo.records("", false)?
            .into_iter()
            .find(|r| r.id == record_id && r.module == "crm" && r.revision == revision)
            .ok_or("Lead changed; save edits and reopen before exporting")?
    };
    let color = match draft.accent.as_str() {
        "blue" => "#234fa0",
        "plum" => "#71365f",
        _ => "#155b56",
    };
    let services = draft
        .services
        .iter()
        .map(|s| format!("<li>{}</li>", escape(s)))
        .collect::<String>();
    let html = format!(
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; form-action 'none'; base-uri 'none'"><title>{name} — proposal concept</title><style>:root{{color-scheme:light}}*{{box-sizing:border-box}}body{{margin:0;font:18px/1.65 system-ui;color:#203232;background:#f7f8f5}}.draft{{padding:12px 24px;background:#fff0ce;font-size:14px}}nav,main,footer{{max-width:1100px;margin:auto;padding:32px}}nav{{font-weight:700;color:{color}}}header{{padding:64px 0;max-width:800px}}h1{{font-size:clamp(2.2rem,6vw,4.4rem);line-height:1.1;letter-spacing:-.04em;color:{color}}}h2{{font-size:1.7rem}}p,.contact{{white-space:pre-wrap;overflow-wrap:anywhere}}ul{{display:grid;grid-template-columns:repeat(auto-fit,minmax(220px,1fr));gap:20px;list-style:none;padding:0}}li{{background:white;padding:28px;border:1px solid #dce4df;border-radius:12px}}.contact{{background:{color};color:white;padding:32px;border-radius:12px}}footer{{font-size:14px}}@media print{{header{{padding:12px 0}}}}</style></head><body><aside class="draft">Proposal concept only · Not the business’s official website · Content and contact details require owner approval</aside><nav aria-label="Business identity">{name}</nav><main><header><h1>{headline}</h1><p>{introduction}</p></header><section><h2>Proposed services</h2><ul>{services}</ul></section><section><h2>Contact information for review</h2><div class="contact">{contact}</div></section></main><footer>Prepared as a local concept for review. No forms, tracking, remote assets, booking or payment features are active. No hosting or publication has occurred.</footer></body></html>"#,
        name = escape(&record.title),
        headline = escape(&draft.headline),
        introduction = escape(&draft.introduction),
        contact = escape(&draft.contact)
    );
    if let Some(file) = rfd::AsyncFileDialog::new()
        .set_title("Export reviewed website concept — local file only")
        .set_file_name("website-concept.html")
        .add_filter("HTML", &["html"])
        .save_file()
        .await
    {
        let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
        if epoch != state.data_epoch.load(std::sync::atomic::Ordering::SeqCst)
            || !repo
                .records("", false)?
                .iter()
                .any(|r| r.id == record_id && r.revision == revision)
        {
            return Err("Lead or workspace changed while choosing a file; review again".into());
        }
        crate::atomic_write(file.path(), html.as_bytes())?;
        return Ok(Some(file.path().display().to_string()));
    }
    Ok(None)
}
