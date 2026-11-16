//! Explicit synthetic local benchmark. Never reads or modifies the personal database.
use crate::Result;
use serde_json::{json, Value};
#[tauri::command]
pub async fn benchmark_local_workspace() -> Result<Value> {
    tokio::task::spawn_blocking(||{
 let dir=tempfile::tempdir().map_err(|e|e.to_string())?;let mut repo=command_center_core::Repository::open(dir.path().join("benchmark.db"))?;let mut capture=vec![];let mut dashboard=vec![];let mut search=vec![];
 for n in 0..500{let now=command_center_core::now();let item=command_center_core::Item{id:uuid::Uuid::new_v4().to_string(),kind:"note".into(),title:format!("Synthetic robotics research {n}"),body:"Benchmark fixture: agriculture, robotics, repositories and learning.".repeat(10),pinned:n%10==0,priority:"normal".into(),status:"active".into(),due:None,recurrence:"none".into(),url:None,created_at:now.clone(),updated_at:now,device_id:repo.device_id.clone(),revision:1,deleted_at:None};let t=std::time::Instant::now();repo.save(item)?;if n>=400{capture.push(t.elapsed().as_secs_f64()*1000.);}}
 for _ in 0..30{let t=std::time::Instant::now();repo.snapshot("")?;dashboard.push(t.elapsed().as_secs_f64()*1000.);let t=std::time::Instant::now();repo.list("robotics",false)?;search.push(t.elapsed().as_secs_f64()*1000.);}
 let summarize=|mut values:Vec<f64>|{values.sort_by(f64::total_cmp);json!({"samples":values.len(),"p50_ms":values[values.len()/2],"p95_ms":values[(values.len()*95/100).min(values.len()-1)],"max_ms":values.last()})};
 Ok(json!({"checked_at":command_center_core::now(),"synthetic_records":500,"capture":summarize(capture),"dashboard_snapshot":summarize(dashboard),"keyword_search":summarize(search),"scope":"Temporary SQLite database on this machine. Measures storage/API latency, not native render time, screen-reader usability, mobile performance or a production load guarantee. No personal records or account requests."}))
 }).await.map_err(|_|"Benchmark interrupted".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn synthetic_benchmark_reports_actual_latency() {
        let value = benchmark_local_workspace().await.unwrap();
        assert_eq!(value["synthetic_records"], 500);
        assert_eq!(value["capture"]["samples"], 100);
        assert!(value["keyword_search"]["p95_ms"].as_f64().unwrap() >= 0.);
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../target/remaining-code/local-benchmark.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, serde_json::to_string_pretty(&value).unwrap()).unwrap();
    }
}
