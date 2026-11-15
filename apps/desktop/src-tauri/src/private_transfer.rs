//! Explicit, one-session category transfers. No plaintext journal or automatic vault replication.
use crate::{peer_sync, sync_wire::Channel, AppState, Result};
use command_center_core::archive::VaultEntry;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::Duration;
use tokio::sync::oneshot;
use zeroize::Zeroizing;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Offer {
    pub category: String,
    pub count: usize,
    pub bytes: usize,
}
pub struct Approval {
    pub label: String,
    pub category: String,
    pub count: usize,
    pub reply: oneshot::Sender<bool>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Chunk {
    data: Vec<u8>,
    last: bool,
}
pub fn valid(category: &str) -> bool {
    ["document", "finance", "subscription", "company"].contains(&category)
}
pub fn collect(state: &AppState, category: &str) -> Result<Zeroizing<Vec<u8>>> {
    if !valid(category) {
        return Err("Choose a private data category".into());
    }
    let key = crate::vault::session_key(state)?;
    let rows = state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .vault_rows()?;
    let mut entries = vec![];
    for (id, bytes) in rows {
        let entry = crate::vault::reveal(&key, &id, &bytes)?;
        if entry.category == category {
            entries.push(entry);
        }
    }
    if entries.is_empty() || entries.len() > 50 {
        return Err("Transfer 1–50 records in a category per session".into());
    }
    let raw = Zeroizing::new(serde_json::to_vec(&entries).map_err(|e| e.to_string())?);
    if raw.len() > 64 * 1024 * 1024 {
        return Err("Private category exceeds the 64 MiB transfer limit".into());
    }
    Ok(raw)
}
pub async fn send(
    state: &AppState,
    peer: &str,
    channel: &mut Channel,
    category: String,
    raw: Zeroizing<Vec<u8>>,
) -> Result<usize> {
    let count = serde_json::from_slice::<Vec<VaultEntry>>(&raw)
        .map_err(|e| e.to_string())?
        .len();
    channel
        .send(&Offer {
            category,
            count,
            bytes: raw.len(),
        })
        .await?;
    let accepted: bool = channel.receive().await?;
    if !accepted {
        return Err("Recipient declined, timed out, or has a locked vault".into());
    }
    let chunks = raw.chunks(256 * 1024);
    let total = chunks.len();
    for (index, data) in chunks.enumerate() {
        peer_sync::check_live_peer(state, peer, &channel.remote_key)?;
        let _ = crate::vault::session_key(state)?;
        channel
            .send(&Chunk {
                data: data.to_vec(),
                last: index + 1 == total,
            })
            .await?;
    }
    let result: std::result::Result<usize, String> = channel.receive().await?;
    result
}
pub async fn receive(
    state: &AppState,
    channel: &mut Channel,
    peer: &str,
    label: &str,
) -> Result<()> {
    let offer: Offer = channel.receive().await?;
    if !valid(&offer.category)
        || offer.count == 0
        || offer.count > 50
        || offer.bytes == 0
        || offer.bytes > 64 * 1024 * 1024
    {
        return Err("Invalid private transfer offer".into());
    }
    let id = uuid::Uuid::new_v4().to_string();
    let (tx, rx) = oneshot::channel();
    {
        let mut rt = state.peer_sync.lock().map_err(|_| "Sync unavailable")?;
        if rt.private_pending.len() >= 4 {
            return Err("Too many pending private transfers".into());
        }
        rt.private_pending.insert(
            id.clone(),
            Approval {
                label: label.into(),
                category: offer.category.clone(),
                count: offer.count,
                reply: tx,
            },
        );
    }
    let accepted = tokio::time::timeout(Duration::from_secs(45), rx)
        .await
        .ok()
        .and_then(|r| r.ok())
        .unwrap_or(false);
    state
        .peer_sync
        .lock()
        .map_err(|_| "Sync unavailable")?
        .private_pending
        .remove(&id);
    let key = crate::vault::session_key(state);
    channel.send(&(accepted && key.is_ok())).await?;
    if !accepted {
        return Err("Private transfer declined".into());
    }
    let key = key?;
    let epoch = state.data_epoch.load(std::sync::atomic::Ordering::SeqCst);
    let mut raw = Zeroizing::new(Vec::with_capacity(offer.bytes));
    loop {
        peer_sync::check_live_peer(state, peer, &channel.remote_key)?;
        let chunk: Chunk = channel.receive().await?;
        if chunk.data.is_empty()
            || chunk.data.len() > 256 * 1024
            || raw.len() + chunk.data.len() > offer.bytes
        {
            return Err("Invalid private transfer chunk".into());
        }
        raw.extend_from_slice(&chunk.data);
        if chunk.last {
            break;
        }
    }
    let result = (|| -> Result<usize> {
        if raw.len() != offer.bytes {
            return Err("Incomplete private transfer".into());
        }
        let entries: Vec<VaultEntry> =
            serde_json::from_slice(&raw).map_err(|_| "Invalid private transfer data")?;
        if entries.len() != offer.count {
            return Err("Private transfer count mismatch".into());
        }
        peer_sync::check_live_peer(state, peer, &channel.remote_key)?;
        let _ = crate::vault::session_key(state)?;
        let mut repo = state.repo.lock().map_err(|_| "Database unavailable")?;
        if state.data_epoch.load(std::sync::atomic::Ordering::SeqCst) != epoch {
            return Err("Data reset during transfer".into());
        }
        let existing = repo
            .vault_rows()?
            .into_iter()
            .collect::<std::collections::BTreeMap<_, _>>();
        let mut staged = vec![];
        let mut ids = std::collections::BTreeSet::new();
        for mut incoming in entries {
            incoming.validate()?;
            if incoming.category != offer.category || !ids.insert(incoming.id.clone()) {
                return Err("Category or identity mismatch".into());
            }
            let raw = Zeroizing::new(serde_json::to_vec(&incoming).map_err(|e| e.to_string())?);
            if let Some(bytes) = existing.get(&incoming.id) {
                let local = crate::vault::reveal(&key, &incoming.id, bytes)?;
                if serde_json::to_vec(&local).map_err(|e| e.to_string())? == *raw {
                    continue;
                }
                // Stable conflict-copy ID makes retries idempotent without overwriting either version.
                let digest = Sha256::digest(&*raw);
                let mut id = [0u8; 16];
                id.copy_from_slice(&digest[..16]);
                id[6] = (id[6] & 15) | 80;
                id[8] = (id[8] & 63) | 128;
                incoming.id = uuid::Uuid::from_bytes(id).to_string();
                incoming.title = format!(
                    "{} · incoming copy",
                    incoming.title.chars().take(100).collect::<String>()
                );
                if let Some(bytes) = existing.get(&incoming.id) {
                    let prior = crate::vault::reveal(&key, &incoming.id, bytes)?;
                    if serde_json::to_vec(&prior).map_err(|e| e.to_string())?
                        == serde_json::to_vec(&incoming).map_err(|e| e.to_string())?
                    {
                        continue;
                    }
                    return Err(
                        "A prior incoming copy was edited; review it before retrying".into(),
                    );
                }
            }
            staged.push((incoming.id.clone(), crate::vault::seal(&key, &incoming)?));
        }
        let count = staged.len();
        repo.save_vault_batch(&staged)?;
        Ok(count)
    })();
    channel.send(&result).await?;
    result.map(|_| ())
}
