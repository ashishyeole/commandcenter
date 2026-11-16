//! Local statement reconciliation. Plaintext is transient; writes use the unlocked vault only.
use crate::{AppState, Result};
use command_center_core::{
    archive::VaultEntry,
    vault_codec::{reveal, seal},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tauri::State;
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transaction {
    pub external_id: String,
    pub date: String,
    pub cents: i64,
    pub direction: String,
    pub memo: String,
    pub account: String,
    pub currency: String,
    pub entity: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    pub external_id: String,
    pub action: String,
    pub target: Option<String>,
}
fn validate(rows: &[Transaction]) -> Result<()> {
    if rows.is_empty() || rows.len() > 500 {
        return Err("Review between one and 500 statement rows at a time".into());
    }
    let mut seen = std::collections::HashSet::new();
    for r in rows {
        if r.external_id.is_empty()
            || r.external_id.len() > 160
            || !seen.insert(&r.external_id)
            || !(1..=1_000_000_000_000).contains(&r.cents)
            || !["income", "expense"].contains(&r.direction.as_str())
            || !["USD", "INR", "EUR", "GBP", "CAD", "AUD"].contains(&r.currency.as_str())
            || r.memo.len() > 2000
            || r.cents <= 0
            || r.cents > 1_000_000_000_000
            || [&r.account, &r.entity]
                .iter()
                .any(|s| s.trim().is_empty() || s.len() > 100)
            || r.external_id.chars().any(char::is_control)
        {
            return Err("Invalid/duplicate statement row, currency, account or amount".into());
        }
        chrono::NaiveDate::parse_from_str(&r.date, "%Y-%m-%d")
            .map_err(|_| "Statement dates must be YYYY-MM-DD")?;
    }
    Ok(())
}
fn reference(r: &Transaction) -> String {
    format!(
        "{:x}",
        Sha256::digest(format!(
            "{}\0{}\0{}\0{}",
            r.entity, r.account, r.currency, r.external_id
        ))
    )
}
fn preview(rows: &[Transaction], entries: &[VaultEntry]) -> Result<Value> {
    validate(rows)?;
    let result = rows
        .iter()
        .map(|r| {
            let parsed = entries
                .iter()
                .filter_map(|e| serde_json::from_str::<Value>(&e.body).ok().map(|v| (e, v)))
                .filter(|(_, v)| v["schema"] == "pcc-ledger-1" && v["kind"] == "transaction")
                .collect::<Vec<_>>();
            let duplicate = parsed
                .iter()
                .find(|(_, v)| v["statement_reference"].as_str() == Some(&reference(r)))
                .map(|(e, _)| e.id.clone());
            let matches = parsed
                .iter()
                .filter(|(_, v)| {
                    v["date"] == r.date
                        && v["cents"] == r.cents
                        && v["direction"] == r.direction
                        && v["account"] == r.account
                        && v["currency"] == r.currency
                        && v["entity"] == r.entity
                        && v["statement_reference"].is_null()
                })
                .map(|(e, _)| json!({"id":e.id,"title":e.title,"updated_at":e.updated_at}))
                .collect::<Vec<_>>();
            json!({"transaction":r,"duplicate":duplicate,"matches":matches})
        })
        .collect::<Vec<_>>();
    // Bind review to all current private entry versions, without exposing their contents.
    let versions = entries
        .iter()
        .map(|e| (&e.id, &e.updated_at, &e.body))
        .collect::<Vec<_>>();
    let hash = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&json!([rows, versions])).map_err(|e| e.to_string())?)
    );
    Ok(json!({"rows":result,"sha256":hash}))
}
#[tauri::command]
pub fn preview_statement(state: State<AppState>, rows: Vec<Transaction>) -> Result<Value> {
    let key = crate::vault::session_key(&state)?;
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    let entries = repo
        .vault_rows()?
        .iter()
        .map(|(id, b)| reveal(&key, id, b))
        .collect::<Result<Vec<_>>>()?;
    preview(&rows, &entries)
}
#[tauri::command]
pub async fn choose_statement(state: State<'_, AppState>) -> Result<Option<Vec<Transaction>>> {
    crate::vault::session_key(&state)?;
    let Some(file) = rfd::AsyncFileDialog::new()
        .set_title("Select statement JSON — read into unlocked vault review")
        .add_filter("Statement JSON", &["json"])
        .pick_file()
        .await
    else {
        return Ok(None);
    };
    use std::io::Read;
    let mut bytes = zeroize::Zeroizing::new(vec![]);
    std::fs::File::open(file.path())
        .map_err(|_| "Could not open selected statement")?
        .take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Could not read selected statement")?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err("Statement exceeds 4 MiB".into());
    }
    let rows:Vec<Transaction>=serde_json::from_slice(&bytes).map_err(|_|"Expected a JSON array: external_id, date, cents, direction, memo, account, currency, entity")?;
    validate(&rows)?;
    crate::vault::session_key(&state)?;
    Ok(Some(rows))
}
#[tauri::command]
pub fn apply_statement(
    state: State<AppState>,
    rows: Vec<Transaction>,
    decisions: Vec<Decision>,
    reviewed_sha256: String,
    consent: bool,
) -> Result<Value> {
    if !consent || decisions.len() != rows.len() {
        return Err("Review one decision for every statement row".into());
    }
    let key = crate::vault::session_key(&state)?;
    let mut repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    let raw = repo.vault_rows()?;
    let entries = raw
        .iter()
        .map(|(id, b)| reveal(&key, id, b))
        .collect::<Result<Vec<_>>>()?;
    let review = preview(&rows, &entries)?;
    if review["sha256"] != reviewed_sha256 {
        return Err("Statement or vault changed; preview again".into());
    }
    let mut seen = std::collections::HashSet::new();
    let mut targets = std::collections::HashSet::new();
    let mut writes = vec![];
    let mut created = 0;
    let mut reconciled = 0;
    for r in &rows {
        let decisions = decisions
            .iter()
            .filter(|d| d.external_id == r.external_id)
            .collect::<Vec<_>>();
        if decisions.len() != 1 || !seen.insert(&r.external_id) {
            return Err("Duplicate or missing row decision".into());
        }
        let d = decisions[0];
        if d.action == "skip" {
            continue;
        }
        let row = review["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["transaction"]["external_id"] == r.external_id)
            .unwrap();
        if !row["duplicate"].is_null() {
            return Err("Already imported statement row must be skipped".into());
        }
        let (mut entry, mut data) = if d.action == "reconcile" {
            let target = d
                .target
                .as_deref()
                .ok_or("Choose the exact matching ledger entry")?;
            if !targets.insert(target.to_owned())
                || !row["matches"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|v| v["id"] == target)
            {
                return Err(
                    "Ledger target is not an unassigned exact account/date/amount match".into(),
                );
            }
            let e = entries.iter().find(|e| e.id == target).unwrap().clone();
            let v = serde_json::from_str::<Value>(&e.body).map_err(|_| "Invalid ledger entry")?;
            reconciled += 1;
            (e, v)
        } else if d.action == "create" {
            created += 1;
            (
                VaultEntry {
                    id: uuid::Uuid::new_v4().to_string(),
                    title: format!("Statement: {} · {}", r.account, r.date),
                    category: "finance".into(),
                    body: String::new(),
                    amount: Some(r.cents as f64 / 100.),
                    due: Some(r.date.clone()),
                    filename: None,
                    file_base64: None,
                    updated_at: command_center_core::now(),
                },
                json!({"schema":"pcc-ledger-1","kind":"transaction","entity":r.entity,"account":r.account,"currency":r.currency,"category":"Uncategorized","date":r.date,"direction":r.direction,"cents":r.cents,"memo":r.memo}),
            )
        } else {
            return Err("Choose skip, create or reconcile".into());
        };
        data["reconciled"] = json!(true);
        data["statement_reference"] = json!(reference(r));
        data["statement_memo"] = json!(r.memo);
        entry.body = data.to_string();
        entry.updated_at = command_center_core::now();
        entry.validate()?;
        let previous = raw
            .iter()
            .find(|(id, _)| id == &entry.id)
            .map(|(_, b)| b.clone());
        writes.push((entry.id.clone(), seal(&key, &entry)?, previous));
    }
    repo.save_vault_batch_checked(&writes)?;
    Ok(json!({"created":created,"reconciled":reconciled,"skipped":rows.len()-created-reconciled}))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preview_requires_exact_account_amount_and_deduplicates() {
        let r = Transaction {
            external_id: "bank-row-1".into(),
            date: "2026-11-16".into(),
            cents: 1250,
            direction: "expense".into(),
            memo: "QA".into(),
            account: "QA account".into(),
            currency: "USD".into(),
            entity: "Personal".into(),
        };
        let mut e=VaultEntry{id:uuid::Uuid::new_v4().to_string(),title:"QA".into(),category:"finance".into(),body:json!({"schema":"pcc-ledger-1","kind":"transaction","date":r.date,"cents":1250,"direction":"expense","account":r.account,"currency":"USD","entity":"Personal"}).to_string(),..Default::default()};
        assert_eq!(
            preview(&[r.clone()], &[e.clone()]).unwrap()["rows"][0]["matches"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        let mut v: Value = serde_json::from_str(&e.body).unwrap();
        v["statement_reference"] = json!(reference(&r));
        e.body = v.to_string();
        assert_eq!(
            preview(&[r.clone()], &[e.clone()]).unwrap()["rows"][0]["duplicate"],
            e.id
        );
        assert!(validate(&[r.clone(), r]).is_err());
    }
}
