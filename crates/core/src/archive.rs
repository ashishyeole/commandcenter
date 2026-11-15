use crate::{err, now, Item, Result};
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashSet},
    io::{Cursor, Read, Write},
};
use zeroize::Zeroizing;
const MAX: u64 = 64 * 1024 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    format: String,
    schema: u32,
    exported_at: String,
    count: usize,
    hashes: BTreeMap<String, String>,
}
fn hash(b: &[u8]) -> String {
    format!("{:x}", Sha256::digest(b))
}
fn csv(s: &str) -> String {
    let safe = if s.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{s}")
    } else {
        s.to_owned()
    };
    format!("\"{}\"", safe.replace('"', "\"\""))
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VaultEntry {
    pub id: String,
    pub title: String,
    pub category: String,
    pub body: String,
    pub amount: Option<f64>,
    pub due: Option<String>,
    pub filename: Option<String>,
    pub file_base64: Option<String>,
    pub updated_at: String,
}
impl VaultEntry {
    pub fn validate(&self) -> Result<()> {
        uuid::Uuid::parse_str(&self.id).map_err(err)?;
        if self.title.trim().is_empty()
            || self.title.len() > 500
            || self.body.len() > 100000
            || !["document", "finance", "subscription", "company"].contains(&self.category.as_str())
            || self
                .file_base64
                .as_ref()
                .is_some_and(|s| s.len() > 14 * 1024 * 1024)
            || self.filename.as_ref().is_some_and(|s| s.len() > 500)
            || self.amount.is_some_and(|n| !n.is_finite())
        {
            return Err("Invalid vault record".into());
        }
        if let Some(date) = &self.due {
            chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").map_err(err)?;
        }
        chrono::DateTime::parse_from_rfc3339(&self.updated_at).map_err(err)?;
        Ok(())
    }
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportData {
    pub items: Vec<Item>,
    #[serde(default)]
    pub vault: Vec<VaultEntry>,
    #[serde(default)]
    pub workspace: crate::workspace::PortableWorkspace,
}
pub fn export(items: &[Item]) -> Result<Vec<u8>> {
    export_all(&ExportData {
        items: items.to_vec(),
        workspace: Default::default(),
        vault: vec![],
    })
}
pub fn export_all(data: &ExportData) -> Result<Vec<u8>> {
    crate::workspace::validate_portable_extras(&data.workspace)?;
    let mut attachment_ids = HashSet::new();
    for a in &data.workspace.attachments {
        a.validate()?;
        if !attachment_ids.insert(&a.id) {
            return Err("Duplicate attachment ID".into());
        }
    }
    let items = &data.items;
    for record in &data.workspace.records {
        record.validate()?;
    }
    if data.workspace.records.len() > 10000 || data.workspace.history.len() > 100000 {
        return Err("Workspace exceeds archive record limits".into());
    }
    if items.len() > 10000 {
        return Err("Archive exceeds 10,000 record limit".into());
    }
    for i in items {
        i.validate()?;
    }
    let mut files = BTreeMap::new();
    for a in &data.workspace.attachments {
        files.insert(
            format!("attachments/{}/{}", a.id, a.filename),
            a.validate()?,
        );
    }
    files.insert(
        "data.json".to_owned(),
        serde_json::to_vec_pretty(items).map_err(err)?,
    );
    let mut table="id,kind,title,body,pinned,priority,status,due,recurrence,url,created_at,updated_at,device_id,revision,deleted_at\r\n".to_owned();
    for i in items {
        table.push_str(
            &[
                &i.id,
                &i.kind,
                &i.title,
                &i.body,
                &i.pinned.to_string(),
                &i.priority,
                &i.status,
                i.due.as_deref().unwrap_or(""),
                &i.recurrence,
                i.url.as_deref().unwrap_or(""),
                &i.created_at,
                &i.updated_at,
                &i.device_id,
                &i.revision.to_string(),
                i.deleted_at.as_deref().unwrap_or(""),
            ]
            .iter()
            .map(|s| csv(s))
            .collect::<Vec<_>>()
            .join(","),
        );
        table.push_str("\r\n");
        if i.kind == "note" {
            files.insert(
                format!("notes/{}.md", i.id),
                format!("# {}\n\n{}\n", i.title, i.body).into_bytes(),
            );
        }
    }
    files.insert("items.csv".into(), table.into_bytes());
    files.insert(
        "workspace.json".into(),
        serde_json::to_vec_pretty(&data.workspace).map_err(err)?,
    );
    let mut record_csv = "id,module,title,status,summary,created_at,updated_at\r\n".to_owned();
    for r in &data.workspace.records {
        record_csv.push_str(
            &[
                &r.id,
                &r.module,
                &r.title,
                &r.status,
                &r.summary,
                &r.created_at,
                &r.updated_at,
            ]
            .iter()
            .map(|v| csv(v))
            .collect::<Vec<_>>()
            .join(","),
        );
        record_csv.push_str("\r\n");
    }
    files.insert("workspaces.csv".into(), record_csv.into_bytes());
    for entry in &data.vault {
        entry.validate()?;
    }
    files.insert(
        "vault.json".into(),
        serde_json::to_vec(&data.vault).map_err(err)?,
    );
    let manifest = Manifest {
        format: "personal-command-center".into(),
        schema: 4,
        exported_at: now(),
        count: items.len(),
        hashes: files.iter().map(|(n, b)| (n.clone(), hash(b))).collect(),
    };
    files.insert(
        "manifest.json".into(),
        serde_json::to_vec_pretty(&manifest).map_err(err)?,
    );
    let total: usize = files.values().map(Vec::len).sum();
    if total as u64 > MAX {
        return Err("Archive exceeds 64 MiB prototype limit".into());
    }
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in files {
        zip.start_file(
            name,
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated),
        )
        .map_err(err)?;
        zip.write_all(&bytes).map_err(err)?;
    }
    Ok(zip.finish().map_err(err)?.into_inner())
}
pub fn inspect(bytes: &[u8]) -> Result<Vec<Item>> {
    Ok(inspect_all(bytes)?.items)
}
pub fn inspect_all(bytes: &[u8]) -> Result<ExportData> {
    if bytes.len() as u64 > MAX {
        return Err("Archive exceeds 64 MiB".into());
    }
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(err)?;
    if zip.len() > 10006 {
        return Err("Too many archive entries".into());
    }
    let mut files = BTreeMap::new();
    let mut total: u64 = 0;
    let mut actual_total: u64 = 0;
    for idx in 0..zip.len() {
        let mut f = zip.by_index(idx).map_err(err)?;
        total = total.checked_add(f.size()).ok_or("Archive size overflow")?;
        if total > MAX {
            return Err("Expanded archive exceeds 64 MiB".into());
        }
        let name = f.name().map_err(err)?.into_owned();
        if f.enclosed_name().is_none() || files.contains_key(&name) {
            return Err("Unsafe or duplicate archive path".into());
        }
        let mut data = Vec::new();
        (&mut f).take(MAX + 1).read_to_end(&mut data).map_err(err)?;
        actual_total = actual_total
            .checked_add(data.len() as u64)
            .ok_or("Expanded archive overflow")?;
        if data.len() as u64 > MAX || actual_total > MAX || data.len() as u64 != f.size() {
            return Err("Entry size mismatch or expansion limit exceeded".into());
        }
        files.insert(name, data);
    }
    let manifest: Manifest =
        serde_json::from_slice(files.get("manifest.json").ok_or("Missing manifest")?)
            .map_err(err)?;
    if ![1, 2, 3, 4].contains(&manifest.schema) || manifest.format != "personal-command-center" {
        return Err("Unsupported archive version".into());
    }
    if files.len() != manifest.hashes.len() + 1 {
        return Err("Unlisted files in archive".into());
    }
    for (name, expected) in &manifest.hashes {
        if hash(files.get(name).ok_or("Missing archive file")?) != *expected {
            return Err("Checksum mismatch".into());
        }
    }
    let items: Vec<Item> =
        serde_json::from_slice(files.get("data.json").ok_or("Missing data")?).map_err(err)?;
    if items.len() != manifest.count || items.len() > 10000 {
        return Err("Invalid record count".into());
    }
    let mut ids = HashSet::new();
    for i in &items {
        i.validate()?;
        if !ids.insert(&i.id) {
            return Err("Duplicate record ID".into());
        }
    }
    let workspace = if manifest.schema >= 2 {
        serde_json::from_slice::<crate::workspace::PortableWorkspace>(
            files
                .get("workspace.json")
                .ok_or("Missing workspace data")?,
        )
        .map_err(err)?
    } else {
        Default::default()
    };
    crate::workspace::validate_portable_extras(&workspace)?;
    let mut attachment_ids = HashSet::new();
    for a in &workspace.attachments {
        let bytes = a.validate()?;
        if !attachment_ids.insert(&a.id) {
            return Err("Duplicate attachment ID".into());
        }
        if manifest.schema >= 4
            && files.get(&format!("attachments/{}/{}", a.id, a.filename)) != Some(&bytes)
        {
            return Err("Missing or mismatched original attachment".into());
        }
    }

    if workspace.records.len() > 10000 || workspace.history.len() > 100000 {
        return Err("Workspace archive limit exceeded".into());
    }
    for r in &workspace.records {
        r.validate()?;
        if !ids.insert(&r.id) {
            return Err("Duplicate record ID".into());
        }
    }
    let vault: Vec<VaultEntry> = if let Some(bytes) = files.get("vault.json") {
        serde_json::from_slice(bytes).map_err(err)?
    } else {
        vec![]
    };
    for a in &workspace.attachments {
        if !ids.insert(&a.id) {
            return Err("Attachment ID collides with another record".into());
        }
    }
    for entry in &vault {
        entry.validate()?;
        if !ids.insert(&entry.id) {
            return Err("Duplicate vault record ID".into());
        }
    }
    Ok(ExportData {
        items,
        workspace,
        vault,
    })
}
fn key(password: &str, salt: &[u8]) -> Result<Zeroizing<[u8; 32]>> {
    let mut key = Zeroizing::new([0u8; 32]);
    argon2::Argon2::default()
        .hash_password_into(password.as_bytes(), salt, &mut *key)
        .map_err(err)?;
    Ok(key)
}
pub fn encrypt(bytes: &[u8], password: &str) -> Result<Vec<u8>> {
    if password.chars().count() < 12 {
        return Err("Use a backup password of at least 12 characters".into());
    }
    let mut salt = [0u8; 16];
    let mut nonce = [0u8; 12];
    OsRng.fill_bytes(&mut salt);
    OsRng.fill_bytes(&mut nonce);
    let key = key(password, &salt)?;
    let cipher = Aes256Gcm::new_from_slice(&*key).map_err(err)?;
    let encrypted = cipher
        .encrypt(Nonce::from_slice(&nonce), bytes)
        .map_err(|_| "Encryption failed")?;
    let mut out = b"PACC01".to_vec();
    out.extend(salt);
    out.extend(nonce);
    out.extend(encrypted);
    Ok(out)
}
pub fn decrypt(bytes: &[u8], password: &str) -> Result<Vec<u8>> {
    if bytes.len() < 50 || !bytes.starts_with(b"PACC01") || bytes.len() as u64 > MAX + 50 {
        return Err("Invalid encrypted backup".into());
    }
    let key = key(password, &bytes[6..22])?;
    Aes256Gcm::new_from_slice(&*key)
        .map_err(err)?
        .decrypt(Nonce::from_slice(&bytes[22..34]), &bytes[34..])
        .map_err(|_| "Wrong password or damaged backup".into())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn portable_roundtrip_and_tampering() {
        let i = crate::tests::item();
        let zip = export(&[i.clone()]).unwrap();
        assert_eq!(inspect(&zip).unwrap(), vec![i]);
        let encrypted = encrypt(&zip, "a long test password").unwrap();
        assert!(decrypt(&encrypted, "incorrect password").is_err());
        assert_eq!(decrypt(&encrypted, "a long test password").unwrap(), zip);
        let mut damaged = encrypted;
        let n = damaged.len();
        damaged[n - 1] ^= 1;
        assert!(decrypt(&damaged, "a long test password").is_err());
    }
    #[test]
    fn rejects_modified_payload_and_future_schema() {
        let original = export(&[crate::tests::item()]).unwrap();
        for target in ["data.json", "manifest.json"] {
            let mut input = zip::ZipArchive::new(Cursor::new(&original)).unwrap();
            let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
            for n in 0..input.len() {
                let mut file = input.by_index(n).unwrap();
                let name = file.name().unwrap().into_owned();
                let mut bytes = Vec::new();
                file.read_to_end(&mut bytes).unwrap();
                if name == target {
                    if target == "data.json" {
                        bytes = b"[]".to_vec();
                    } else {
                        let mut m: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                        m["schema"] = 99.into();
                        bytes = serde_json::to_vec(&m).unwrap();
                    }
                }
                output
                    .start_file(name, zip::write::SimpleFileOptions::default())
                    .unwrap();
                output.write_all(&bytes).unwrap();
            }
            assert!(inspect(&output.finish().unwrap().into_inner()).is_err());
        }
    }
    #[test]
    fn csv_formula_protection() {
        assert_eq!(csv("=1+2"), "\"'=1+2\"");
    }
}
