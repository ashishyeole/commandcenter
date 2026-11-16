//! Explicit visible incognito page inspection. Remote webview receives no native capabilities.
use crate::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Signals {
    title: String,
    description: String,
    viewport: bool,
    language: bool,
    h1: u32,
    images: u32,
    missing_alt: u32,
    unnamed_links: u32,
    unnamed_buttons: u32,
    unlabelled_inputs: u32,
}
const SCRIPT: &str = r#"setTimeout(()=>{const all=s=>Array.from(document.querySelectorAll(s));const named=e=>!!((e.getAttribute('aria-label')||e.getAttribute('title')||e.textContent||'').trim()||e.querySelector('img[alt]')?.getAttribute('alt'));const result={title:document.title.slice(0,300),description:(document.querySelector('meta[name="description"]')?.content||'').slice(0,500),viewport:(document.querySelector('meta[name="viewport"]')?.content||'').includes('width=device-width'),language:!!document.documentElement.lang,h1:all('h1').length,images:all('img').length,missing_alt:all('img:not([alt])').length,unnamed_links:all('a[href]').filter(e=>!named(e)).length,unnamed_buttons:all('button,[role="button"]').filter(e=>!named(e)).length,unlabelled_inputs:all('input:not([type="hidden"]):not([type="submit"]):not([type="button"]),select,textarea').filter(e=>!(e.labels?.length||e.getAttribute('aria-label')||e.getAttribute('aria-labelledby')||e.getAttribute('title'))).length};location.href='https://pcc-audit.invalid/result?data='+encodeURIComponent(JSON.stringify(result));},1500);"#;
fn report(url: &str, s: Signals) -> Result<Value> {
    if s.title.len() > 1200
        || s.description.len() > 2000
        || [
            s.h1,
            s.images,
            s.missing_alt,
            s.unnamed_links,
            s.unnamed_buttons,
            s.unlabelled_inputs,
        ]
        .iter()
        .any(|n| *n > 20000)
    {
        return Err("Rendered page signals exceed limits".into());
    }
    let checks = vec![
        json!({"label":"Page title present","passed":!s.title.trim().is_empty(),"weight":15,"evidence":s.title}),
        json!({"label":"Description present","passed":!s.description.trim().is_empty(),"weight":10,"evidence":s.description}),
        json!({"label":"Mobile viewport declared","passed":s.viewport,"weight":15,"evidence":"Rendered DOM viewport metadata"}),
        json!({"label":"Document language declared","passed":s.language,"weight":10,"evidence":"Rendered html lang"}),
        json!({"label":"One primary heading","passed":s.h1==1,"weight":10,"evidence":format!("{} h1 elements",s.h1)}),
        json!({"label":"Images declare alt text","passed":s.missing_alt==0,"weight":10,"evidence":format!("{} of {} images missing alt",s.missing_alt,s.images)}),
        json!({"label":"Links have names","passed":s.unnamed_links==0,"weight":10,"evidence":format!("{} possibly unnamed links",s.unnamed_links)}),
        json!({"label":"Buttons have names","passed":s.unnamed_buttons==0,"weight":10,"evidence":format!("{} possibly unnamed buttons",s.unnamed_buttons)}),
        json!({"label":"Form inputs have labels","passed":s.unlabelled_inputs==0,"weight":10,"evidence":format!("{} possibly unlabelled inputs",s.unlabelled_inputs)}),
    ];
    let score: u64 = checks
        .iter()
        .filter(|v| v["passed"] == false)
        .map(|v| v["weight"].as_u64().unwrap_or(0))
        .sum();
    Ok(
        json!({"url":url,"observed_at":command_center_core::now(),"opportunity_signal":score,"checks":checks,"mode":"rendered","scope":"Visible incognito desktop webview; page scripts/subresources run under browser rules, with no native app access or existing signed-in session. Samples DOM 1.5 seconds after load. Heuristic names/labels are not a full accessibility audit; score indicates review opportunities, not business value, security or revenue. No forms submitted, crawling, downloads or outreach."}),
    )
}
pub(crate) async fn inspect(app: tauri::AppHandle, url: reqwest::Url) -> Result<Value> {
    crate::research::public_addresses(&url).await?;
    let host = url.host_str().ok_or("Missing host")?.to_owned();
    let capture_host = host.clone();
    let phase = Arc::new(AtomicBool::new(false));
    let capture_phase = phase.clone();
    let (tx, rx) = tokio::sync::oneshot::channel::<String>();
    let sender = Arc::new(Mutex::new(Some(tx)));
    let label = format!("website-audit-{}", uuid::Uuid::new_v4());
    let window =
        tauri::WebviewWindowBuilder::new(&app, &label, tauri::WebviewUrl::External(url.clone()))
            .title("Public website audit · isolated session")
            .inner_size(1100., 800.)
            .incognito(true)
            .on_download(|_, _| false)
            .on_navigation(move |u| {
                if u.host_str() == Some("pcc-audit.invalid") {
                    if phase.load(Ordering::SeqCst) && u.as_str().len() < 16000 {
                        if let Some((_, data)) = u.query_pairs().find(|(k, _)| k == "data") {
                            if let Ok(mut sender) = sender.lock() {
                                if let Some(tx) = sender.take() {
                                    let _ = tx.send(data.into_owned());
                                }
                            }
                        }
                    }
                    return false;
                }
                u.scheme() == "https"
                    && u.host_str() == Some(host.as_str())
                    && u.username().is_empty()
                    && u.password().is_none()
            })
            .on_page_load(move |window, payload| {
                if matches!(payload.event(), tauri::webview::PageLoadEvent::Finished)
                    && payload.url().host_str() == Some(capture_host.as_str())
                    && !capture_phase.swap(true, Ordering::SeqCst)
                {
                    let _ = window.eval(SCRIPT);
                }
            })
            .build()
            .map_err(|e| e.to_string())?;
    let result = tokio::time::timeout(std::time::Duration::from_secs(30), rx).await;
    let _ = window.close();
    let raw = result
        .map_err(|_| {
            "Rendered page timed out; use its final public HTTPS URL or try static inspection"
        })?
        .map_err(|_| "Rendered page closed before inspection")?;
    let signals: Signals =
        serde_json::from_str(&raw).map_err(|_| "Unsupported rendered page response")?;
    report(url.as_str(), signals)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rendered_signals_have_bounded_explained_score() {
        let s = Signals {
            title: "QA".into(),
            description: "QA page".into(),
            viewport: true,
            language: true,
            h1: 1,
            images: 1,
            missing_alt: 1,
            unnamed_links: 0,
            unnamed_buttons: 0,
            unlabelled_inputs: 2,
        };
        let r = report("https://example.com/", s).unwrap();
        assert_eq!(r["opportunity_signal"], 20);
        assert_eq!(r["checks"].as_array().unwrap().len(), 9);
        assert_eq!(r["mode"], "rendered");
    }
}
