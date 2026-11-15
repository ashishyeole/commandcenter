use crate::{AppState, Result};
use command_center_core::archive::VaultEntry;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::State;
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Line {
    description: String,
    quantity: u32,
    unit_cents: i64,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Invoice {
    schema: String,
    number: String,
    entity: String,
    client: String,
    client_address: String,
    currency: String,
    issued: String,
    due: String,
    status: String,
    tax_basis_points: u32,
    cost_cents: i64,
    lines: Vec<Line>,
    notes: String,
    payment_instructions: String,
}
fn totals(i: &Invoice) -> Result<Value> {
    if i.schema != "pcc-invoice-1"
        || i.number.trim().is_empty()
        || i.number.len() > 80
        || i.entity.trim().is_empty()
        || i.entity.len() > 300
        || i.client.trim().is_empty()
        || i.client.len() > 300
        || i.client_address.len() > 2000
        || i.notes.len() > 10000
        || i.payment_instructions.len() > 4000
        || !["USD", "INR", "EUR", "GBP", "CAD", "AUD"].contains(&i.currency.as_str())
        || !["draft", "sent", "paid", "void"].contains(&i.status.as_str())
        || i.tax_basis_points > 10000
        || !(0..=100_000_000_000_000).contains(&i.cost_cents)
        || i.lines.is_empty()
        || i.lines.len() > 100
    {
        return Err("Invalid invoice fields, currency, status or limits".into());
    }
    let issue = chrono::NaiveDate::parse_from_str(&i.issued, "%Y-%m-%d")
        .map_err(|_| "Invalid issue date")?;
    let due =
        chrono::NaiveDate::parse_from_str(&i.due, "%Y-%m-%d").map_err(|_| "Invalid due date")?;
    if due < issue {
        return Err("Due date cannot precede issue date".into());
    }
    let mut subtotal = 0i64;
    for l in &i.lines {
        if l.description.trim().is_empty()
            || l.description.len() > 1000
            || l.quantity == 0
            || l.quantity > 100000
            || !(0..=1_000_000_000_000).contains(&l.unit_cents)
        {
            return Err(
                "Invalid invoice line; quantities are whole units and prices use integer cents"
                    .into(),
            );
        }
        subtotal = subtotal
            .checked_add(
                l.unit_cents
                    .checked_mul(i64::from(l.quantity))
                    .ok_or("Invoice amount overflow")?,
            )
            .ok_or("Invoice amount overflow")?;
    }
    if subtotal > 100_000_000_000_000 {
        return Err("Invoice exceeds amount limit".into());
    }
    let tax = subtotal
        .checked_mul(i64::from(i.tax_basis_points))
        .and_then(|n| n.checked_add(5000))
        .ok_or("Tax amount overflow")?
        / 10000;
    Ok(
        json!({"subtotal_cents":subtotal,"tax_cents":tax,"total_cents":subtotal+tax,"estimated_margin_cents":subtotal-i.cost_cents}),
    )
}
#[tauri::command]
pub fn invoice_totals(invoice: Invoice) -> Result<Value> {
    totals(&invoice)
}
#[tauri::command]
pub fn save_invoice(
    state: State<AppState>,
    id: Option<String>,
    expected_updated_at: Option<String>,
    invoice: Invoice,
) -> Result<()> {
    let amounts = totals(&invoice)?;
    let key = crate::vault::session_key(&state)?;
    let id = id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    uuid::Uuid::parse_str(&id).map_err(|_| "Invalid invoice ID")?;
    let mut repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    let previous = repo.vault_ciphertext(&id)?;
    if let Some(bytes) = &previous {
        let original = crate::vault::reveal(&key, &id, bytes)?;
        let existing: Invoice = serde_json::from_str(&original.body)
            .map_err(|_| "This private record is not an invoice")?;
        totals(&existing)?;
        if expected_updated_at.as_deref() != Some(original.updated_at.as_str()) {
            return Err("Invoice changed; reload before saving".into());
        }
    }
    for (other, bytes) in repo.vault_rows()? {
        if other != id {
            let entry = crate::vault::reveal(&key, &other, &bytes)?;
            if let Ok(existing) = serde_json::from_str::<Invoice>(&entry.body) {
                if existing
                    .entity
                    .trim()
                    .eq_ignore_ascii_case(invoice.entity.trim())
                    && existing
                        .number
                        .trim()
                        .eq_ignore_ascii_case(invoice.number.trim())
                {
                    return Err("This entity already has an invoice with that number".into());
                }
            }
        }
    }
    let entry = VaultEntry {
        id: id.clone(),
        title: format!(
            "Invoice {} · {}",
            invoice.number,
            invoice.client.chars().take(70).collect::<String>()
        ),
        category: "finance".into(),
        body: serde_json::to_string(&invoice).map_err(|e| e.to_string())?,
        amount: amounts["total_cents"].as_i64().map(|n| n as f64 / 100.0),
        due: Some(invoice.due),
        filename: None,
        file_base64: None,
        updated_at: command_center_core::now(),
    };
    repo.save_vault_if_unchanged(&id, &crate::vault::seal(&key, &entry)?, previous.as_deref())
}
fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
fn money(cents: i64, currency: &str) -> String {
    format!("{} {:.2}", currency, cents as f64 / 100.0)
}
#[tauri::command]
pub async fn export_invoice(
    state: State<'_, AppState>,
    id: String,
    plaintext_consent: bool,
) -> Result<Option<String>> {
    if !plaintext_consent {
        return Err("Approve creating a readable invoice outside the encrypted vault".into());
    }
    let key = crate::vault::session_key(&state)?;
    let epoch = state.data_epoch.load(std::sync::atomic::Ordering::SeqCst);
    let (invoice, updated) = {
        let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
        let bytes = repo
            .vault_ciphertext(&id)?
            .ok_or("Invoice no longer exists")?;
        let entry = crate::vault::reveal(&key, &id, &bytes)?;
        let invoice: Invoice = serde_json::from_str(&entry.body)
            .map_err(|_| "This private record is not an invoice")?;
        (invoice, entry.updated_at)
    };
    let amount = totals(&invoice)?;
    let lines = invoice
        .lines
        .iter()
        .map(|l| {
            format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                esc(&l.description),
                l.quantity,
                money(l.unit_cents, &invoice.currency),
                money(l.unit_cents * i64::from(l.quantity), &invoice.currency)
            )
        })
        .collect::<String>();
    let html=format!("<!doctype html><html lang=\"en\"><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'\"><title>Invoice {}</title><style>body{{font:16px system-ui;color:#17212b;max-width:900px;margin:48px auto;padding:24px}}header{{display:flex;justify-content:space-between;border-bottom:3px solid #244e62}}table{{border-collapse:collapse;width:100%;margin:32px 0}}th,td{{padding:12px;text-align:left;border-bottom:1px solid #ddd}}td:not(:first-child){{text-align:right}}pre{{font:inherit;white-space:pre-wrap}}.totals{{text-align:right}}small{{color:#586877}}@media print{{body{{margin:0}}}}</style><header><div><h1>Invoice {}</h1><p>{}</p></div><div><p>Status: {}</p><p>Issued {}<br>Due {}</p></div></header><h2>Bill to</h2><strong>{}</strong><pre>{}</pre><table><thead><tr><th>Description</th><th>Quantity</th><th>Unit price</th><th>Amount</th></tr></thead><tbody>{}</tbody></table><section class=\"totals\"><p>Subtotal: {}</p><p>Tax ({:.2}%): {}</p><h2>Total: {}</h2></section><h3>Payment instructions</h3><pre>{}</pre><h3>Notes</h3><pre>{}</pre><small>Prepared from locally entered records. Payment status is manually recorded; this document does not confirm bank settlement.</small></html>",esc(&invoice.number),esc(&invoice.number),esc(&invoice.entity),esc(&invoice.status),esc(&invoice.issued),esc(&invoice.due),esc(&invoice.client),esc(&invoice.client_address),lines,money(amount["subtotal_cents"].as_i64().unwrap_or(0),&invoice.currency),invoice.tax_basis_points as f64/100.0,money(amount["tax_cents"].as_i64().unwrap_or(0),&invoice.currency),money(amount["total_cents"].as_i64().unwrap_or(0),&invoice.currency),esc(&invoice.payment_instructions),esc(&invoice.notes));
    if let Some(file) = rfd::AsyncFileDialog::new()
        .set_title("Export readable invoice HTML — sensitive plaintext copy")
        .add_filter("HTML", &["html"])
        .set_file_name("invoice.html")
        .save_file()
        .await
    {
        let current_key = crate::vault::session_key(&state)?;
        if *current_key != *key
            || epoch != state.data_epoch.load(std::sync::atomic::Ordering::SeqCst)
        {
            return Err("Vault session changed during export".into());
        }
        let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
        let bytes = repo
            .vault_ciphertext(&id)?
            .ok_or("Invoice no longer exists")?;
        if crate::vault::reveal(&key, &id, &bytes)?.updated_at != updated {
            return Err("Invoice changed while choosing a destination; export again".into());
        }
        crate::atomic_write(file.path(), html.as_bytes())?;
        return Ok(Some(file.path().to_string_lossy().into()));
    }
    Ok(None)
}
