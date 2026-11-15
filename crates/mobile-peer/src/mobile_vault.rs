use super::{strarg, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use command_center_core::{
    archive::{self, ExportData, VaultEntry},
    vault_codec::{new_key, reveal, seal},
    Repository,
};
use serde_json::{json, Value};
use zeroize::Zeroizing;
pub fn dispatch(repo: &mut Repository, op: &str, req: &Value) -> Result<Value> {
    if op == "vault_info" {
        return Ok(json!({"count":repo.vault_rows()?.len()}));
    }
    if op == "vault_keygen" {
        if !repo.vault_rows()?.is_empty() {
            return Err("Vault data exists. Restore the original device key or an encrypted backup; never replace a lost key over existing data.".into());
        }
        return Ok(json!({"key":B64.encode(&*new_key())}));
    }
    let key = Zeroizing::new(
        B64.decode(strarg(req, "key")?)
            .map_err(|_| "Invalid vault key")?,
    );
    if key.len() != 32 {
        return Err("Invalid vault key length".into());
    }
    let rows = repo.vault_rows()?;
    if rows.len() > 1000 {
        return Err("Mobile vault exceeds 1000 entries".into());
    }
    let mut entries = vec![];
    for (id, bytes) in &rows {
        entries.push(reveal(&key, id, bytes)?);
    }
    match op {
        "vault_list" => Ok(json!({"entries":entries})),
        "vault_save" => {
            let mut entry: VaultEntry = serde_json::from_value(req["entry"].clone())
                .map_err(|_| "Invalid private entry")?;
            entry.validate()?;
            if let Some(existing) = entries.iter().find(|e| e.id == entry.id) {
                if req["expected_updated_at"].as_str() != Some(existing.updated_at.as_str()) {
                    return Err("Private entry changed; unlock and reload before saving".into());
                }
            } else if entries.len() >= 1000 {
                return Err("Mobile vault supports at most 1000 entries".into());
            }
            entry.updated_at = command_center_core::now();
            let encrypted = seal(&key, &entry)?;
            repo.save_vault_if_unchanged(
                &entry.id,
                &encrypted,
                rows.iter()
                    .find(|(id, _)| id == &entry.id)
                    .map(|(_, bytes)| bytes.as_slice()),
            )?;
            Ok(json!({"saved":true}))
        }
        "vault_export" => {
            let password = Zeroizing::new(strarg(req, "password")?.to_owned());
            let zip = Zeroizing::new(archive::export_all(&ExportData {
                items: vec![],
                workspace: Default::default(),
                vault: entries,
            })?);
            if zip.len() > 20 * 1024 * 1024 {
                return Err(
                    "Mobile private backup exceeds 20 MiB; export from desktop instead".into(),
                );
            }
            let bytes = archive::encrypt(&zip, &password)?;
            Ok(json!({"archive_base64":B64.encode(bytes)}))
        }
        "vault_preview_import" | "vault_import" => {
            let bytes = B64
                .decode(strarg(req, "archive_base64")?)
                .map_err(|_| "Invalid encrypted archive")?;
            if bytes.len() > 20 * 1024 * 1024 {
                return Err("Mobile private import exceeds 20 MiB".into());
            }
            let password = Zeroizing::new(strarg(req, "password")?.to_owned());
            let plain = Zeroizing::new(archive::decrypt(&bytes, &password)?);
            let imported = archive::inspect_all(&plain)?;
            if imported.vault.len() + entries.len() > 1000 {
                return Err("Combined vault exceeds 1000 entries".into());
            }
            if op == "vault_preview_import" {
                return Ok(
                    json!({"private_entries":imported.vault.len(),"ordinary_items_ignored":imported.items.len()+imported.workspace.records.len(),"existing_ids":imported.vault.iter().filter(|e|entries.iter().any(|x|x.id==e.id)).count(),"policy":"Identical private records are skipped; conflicting IDs become separate copies. Ordinary data is ignored."}),
                );
            }
            if req["confirmation"] != "IMPORT PRIVATE COPIES" {
                return Err("Confirm the reviewed private import".into());
            }
            let mut writes = vec![];
            for mut entry in imported.vault {
                if let Some(existing) = entries.iter().find(|e| e.id == entry.id) {
                    if serde_json::to_value(existing).map_err(|e| e.to_string())?
                        == serde_json::to_value(&entry).map_err(|e| e.to_string())?
                    {
                        continue;
                    }
                    entry.id = uuid::Uuid::new_v4().to_string();
                    entry.title = format!(
                        "{} (imported copy)",
                        entry.title.chars().take(110).collect::<String>()
                    );
                }
                // Cross-class UUID collisions are rejected transactionally by save_vault_batch.
                writes.push((entry.id.clone(), seal(&key, &entry)?));
            }
            repo.save_vault_batch(&writes)?;
            Ok(json!({"imported":writes.len()}))
        }
        _ => Err("Unsupported mobile vault operation".into()),
    }
}
