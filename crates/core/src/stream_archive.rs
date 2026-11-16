//! PCCSTR01: encrypted, bounded per-record frames and an authenticated terminal manifest.
use crate::{
    archive::VaultEntry,
    attachments::Attachment,
    err,
    workspace::{CachedFeed, Change, PortableWorkspace, Record},
    Item, Repository, Result,
};
use aes_gcm::{
    aead::{Aead, KeyInit, Payload},
    Aes256Gcm, Nonce,
};
use rand::{rngs::OsRng, RngCore};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    io::{Read, Seek, SeekFrom, Write},
};
use zeroize::Zeroizing;
const MAGIC: &[u8; 8] = b"PCCSTR01";
const FRAME_LIMIT: usize = 24 * 1024 * 1024;
pub const BYTE_LIMIT: u64 = 8 * 1024 * 1024 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", deny_unknown_fields)]
enum Frame {
    Item(Item),
    Record(Record),
    Attachment(Attachment),
    History(Change),
    Vault(VaultEntry),
    Preferences(serde_json::Value),
    Feed(CachedFeed),
    Collections(Vec<crate::collections::Collection>),
    End { count: u64, sha256: String },
}
#[derive(Default, Clone, Serialize)]
pub struct Summary {
    pub records: u64,
    pub vault_records: u64,
    pub history_events: u64,
    pub config_frames: u64,
    pub new_records: u64,
    pub existing_records: u64,
    pub imported: u64,
}
fn cipher(password: &str, header: &[u8; 28]) -> Result<Aes256Gcm> {
    let mut key = Zeroizing::new([0u8; 32]);
    argon2::Argon2::new(
        argon2::Algorithm::Argon2id,
        argon2::Version::V0x13,
        argon2::Params::new(19456, 2, 1, Some(32)).map_err(err)?,
    )
    .hash_password_into(password.as_bytes(), &header[8..24], &mut *key)
    .map_err(err)?;
    Aes256Gcm::new_from_slice(&*key).map_err(err)
}
fn nonce(header: &[u8; 28], seq: u64) -> [u8; 12] {
    let mut n = [0; 12];
    n[..4].copy_from_slice(&header[24..]);
    n[4..].copy_from_slice(&seq.to_be_bytes());
    n
}
struct Encoder<W> {
    out: W,
    header: [u8; 28],
    cipher: Aes256Gcm,
    seq: u64,
    hash: Sha256,
    bytes: u64,
}
impl<W: Write> Encoder<W> {
    fn new(mut out: W, password: &str) -> Result<Self> {
        if password.chars().count() < 12 {
            return Err("Use a backup password of at least 12 characters".into());
        }
        let mut header = [0; 28];
        header[..8].copy_from_slice(MAGIC);
        OsRng.fill_bytes(&mut header[8..]);
        out.write_all(&header).map_err(err)?;
        let cipher = cipher(password, &header)?;
        Ok(Self {
            out,
            header,
            cipher,
            seq: 0,
            hash: Sha256::new(),
            bytes: 28,
        })
    }
    fn emit(&mut self, frame: &Frame) -> Result<()> {
        let plain = Zeroizing::new(serde_json::to_vec(frame).map_err(err)?);
        if plain.len() > FRAME_LIMIT {
            return Err("Record exceeds streaming frame limit".into());
        }
        let end = matches!(frame, Frame::End { .. });
        if !end {
            validate(frame)?;
            self.hash.update(&*plain);
        }
        let bytes = self
            .cipher
            .encrypt(
                Nonce::from_slice(&nonce(&self.header, self.seq)),
                Payload {
                    msg: &plain,
                    aad: &self.header,
                },
            )
            .map_err(|_| "Archive encryption failed")?;
        self.bytes += 4 + bytes.len() as u64;
        if self.bytes > BYTE_LIMIT {
            return Err("Streaming archive exceeds 8 GiB".into());
        }
        self.out
            .write_all(&(bytes.len() as u32).to_be_bytes())
            .map_err(err)?;
        self.out.write_all(&bytes).map_err(err)?;
        self.seq += 1;
        if self.seq > 1_000_001 {
            return Err("Streaming archive exceeds frame limit".into());
        }
        Ok(())
    }
    fn finish(mut self) -> Result<()> {
        self.emit(&Frame::End {
            count: self.seq,
            sha256: format!("{:x}", self.hash.clone().finalize()),
        })?;
        self.out.flush().map_err(err)
    }
}
fn validate(frame: &Frame) -> Result<()> {
    match frame {
        Frame::Item(v) => v.validate(),
        Frame::Record(v) => v.validate(),
        Frame::Attachment(v) => v.validate().map(|_| ()),
        Frame::Vault(v) => v.validate(),
        Frame::History(v) => {
            for id in [&v.event_id, &v.entity_id, &v.device_id] {
                uuid::Uuid::parse_str(id).map_err(err)?;
            }
            chrono::DateTime::parse_from_rfc3339(&v.timestamp).map_err(err)?;
            if v.payload.len() > 12 * 1024 * 1024
                || !["upsert", "delete"].contains(&v.operation.as_str())
            {
                return Err("Invalid archive history".into());
            }
            let payload: serde_json::Value =
                serde_json::from_str(&v.payload).map_err(|_| "Invalid history record payload")?;
            if payload["id"].as_str() != Some(v.entity_id.as_str()) {
                return Err("History payload belongs to a different entity".into());
            }
            if payload.get("kind").is_some() {
                serde_json::from_value::<Item>(payload)
                    .map_err(err)?
                    .validate()?;
            } else {
                serde_json::from_value::<Record>(payload)
                    .map_err(err)?
                    .validate()?;
            }
            Ok(())
        }
        Frame::Preferences(v) => crate::workspace::validate_portable_extras(&PortableWorkspace {
            preferences: Some(v.clone()),
            ..Default::default()
        }),
        Frame::Feed(v) => crate::workspace::validate_portable_extras(&PortableWorkspace {
            feeds: vec![v.clone()],
            ..Default::default()
        }),
        Frame::Collections(v) => crate::collections::validate_collections(v),
        Frame::End { .. } => Ok(()),
    }
}
fn decode<R: Read>(
    mut input: R,
    password: &str,
    mut accept: impl FnMut(Frame) -> Result<()>,
) -> Result<()> {
    let mut header = [0; 28];
    input
        .read_exact(&mut header)
        .map_err(|_| "Incomplete streaming archive header")?;
    if &header[..8] != MAGIC {
        return Err("Unsupported streaming archive format/version".into());
    }
    let cipher = cipher(password, &header)?;
    let mut hash = Sha256::new();
    let mut seen = HashSet::new();
    let mut bytes = 28u64;
    for seq in 0..=1_000_000u64 {
        let mut size = [0; 4];
        input
            .read_exact(&mut size)
            .map_err(|_| "Archive is truncated; authenticated end manifest missing")?;
        let size = u32::from_be_bytes(size) as usize;
        bytes += 4 + size as u64;
        if !(16..=FRAME_LIMIT + 16).contains(&size) || bytes > BYTE_LIMIT {
            return Err("Archive exceeds bounded frame/total size".into());
        }
        let mut encrypted = vec![0; size];
        input
            .read_exact(&mut encrypted)
            .map_err(|_| "Truncated archive frame")?;
        let plain = Zeroizing::new(
            cipher
                .decrypt(
                    Nonce::from_slice(&nonce(&header, seq)),
                    Payload {
                        msg: &encrypted,
                        aad: &header,
                    },
                )
                .map_err(|_| "Wrong password or damaged streaming archive")?,
        );
        let frame: Frame = serde_json::from_slice(&plain)
            .map_err(|_| "Unsupported or invalid streaming record")?;
        if let Frame::End { count, sha256 } = frame {
            if count != seq || sha256 != format!("{:x}", hash.finalize()) {
                return Err("Streaming manifest mismatch".into());
            }
            let mut extra = [0];
            if input.read(&mut extra).map_err(err)? != 0 {
                return Err("Unexpected data after archive manifest".into());
            }
            return Ok(());
        }
        validate(&frame)?;
        let identity = match &frame {
            Frame::Item(v) => Some(format!("entity:{}", v.id)),
            Frame::Record(v) => Some(format!("entity:{}", v.id)),
            Frame::Attachment(v) => Some(format!("entity:{}", v.id)),
            Frame::Vault(v) => Some(format!("entity:{}", v.id)),
            Frame::History(v) => Some(format!("history:{}", v.event_id)),
            Frame::Preferences(_) => Some("preferences".into()),
            Frame::Collections(_) => Some("collections".into()),
            Frame::Feed(v) => Some(format!("feed:{}", v.source)),
            _ => None,
        };
        if identity.is_some_and(|id| !seen.insert(id)) {
            return Err("Duplicate streaming archive identity".into());
        }
        hash.update(&*plain);
        accept(frame)?;
    }
    Err("Archive exceeds frame count limit".into())
}
const ITEM_SQL:&str="SELECT json_object('id',id,'kind',kind,'title',title,'body',body,'pinned',json(CASE pinned WHEN 1 THEN 'true' ELSE 'false' END),'priority',priority,'status',status,'due',due,'recurrence',recurrence,'url',url,'created_at',created_at,'updated_at',updated_at,'device_id',device_id,'revision',revision,'deleted_at',deleted_at) FROM items ORDER BY id";
const RECORD_SQL:&str="SELECT json_object('id',id,'module',module,'title',title,'summary',summary,'status',status,'pinned',json(CASE pinned WHEN 1 THEN 'true' ELSE 'false' END),'tags',json(tags),'links',json(links),'fields',json(fields),'created_at',created_at,'updated_at',updated_at,'device_id',device_id,'revision',revision,'deleted_at',deleted_at) FROM records ORDER BY id";
impl Repository {
    pub fn export_stream<W: Write>(
        &self,
        out: W,
        password: &str,
        vault_key: Option<&[u8]>,
    ) -> Result<()> {
        let tx = self.conn.unchecked_transaction().map_err(err)?;
        let mut encoder = Encoder::new(out, password)?;
        for (sql,kind) in [(ITEM_SQL,0),(RECORD_SQL,1),("SELECT payload FROM attachments ORDER BY id",2),("SELECT json_object('event_id',event_id,'entity_id',entity_id,'device_id',device_id,'revision',revision,'operation',operation,'timestamp',timestamp,'payload',payload) FROM changes ORDER BY sequence",3),("SELECT json_object('source',source,'fetched_at',fetched_at,'items',json(payload)) FROM feed_cache ORDER BY source",4)] {
  let mut stmt=tx.prepare(sql).map_err(err)?;let rows=stmt.query_map([],|r|r.get::<_,String>(0)).map_err(err)?;for row in rows{let raw=row.map_err(err)?;let frame=match kind{0=>Frame::Item(serde_json::from_str(&raw).map_err(err)?),1=>Frame::Record(serde_json::from_str(&raw).map_err(err)?),2=>Frame::Attachment(serde_json::from_str(&raw).map_err(err)?),3=>Frame::History(serde_json::from_str(&raw).map_err(err)?),_=>Frame::Feed(serde_json::from_str(&raw).map_err(err)?)};encoder.emit(&frame)?;}
 }
        if let Some(raw) = self.setting("preferences")? {
            let value: serde_json::Value = serde_json::from_str(&raw).map_err(err)?;
            let safe = [
                "quiet_start",
                "quiet_end",
                "enabled_modules",
                "dashboard_widgets",
                "interests",
                "backup_reminder_days",
                "ui_scale",
                "high_contrast",
                "reduce_motion",
            ]
            .iter()
            .filter_map(|k| value.get(*k).map(|v| ((*k).into(), v.clone())))
            .collect::<serde_json::Map<_, _>>();
            encoder.emit(&Frame::Preferences(safe.into()))?;
        }
        encoder.emit(&Frame::Collections(self.research_collections()?))?;
        if let Some(key) = vault_key {
            let mut stmt = tx
                .prepare("SELECT id,ciphertext FROM vault ORDER BY id")
                .map_err(err)?;
            let rows = stmt
                .query_map([], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))
                })
                .map_err(err)?;
            for row in rows {
                let (id, bytes) = row.map_err(err)?;
                encoder.emit(&Frame::Vault(crate::vault_codec::reveal(key, &id, &bytes)?))?;
            }
        }
        encoder.finish()?;
        tx.commit().map_err(err)
    }
    /// Deliberate full-copy restore: fresh IDs and remapped explicit relationships/history.
    pub fn import_stream_copies<R: Read + Seek>(
        &mut self,
        mut input: R,
        password: &str,
        vault_key: Option<&[u8]>,
        expected_sha256: Option<&str>,
    ) -> Result<Summary> {
        input.seek(SeekFrom::Start(0)).map_err(err)?;
        let mut reader = HashReader {
            inner: &mut input,
            hash: Sha256::new(),
        };
        let mut mapping = HashMap::new();
        let mut fresh = HashSet::new();
        decode(&mut reader, password, |frame| {
            if let Some(old) = frame_id(&frame) {
                let id = loop {
                    let id = uuid::Uuid::new_v4().to_string();
                    let exists:bool=self.conn.query_row("SELECT EXISTS(SELECT 1 FROM items WHERE id=?1 UNION ALL SELECT 1 FROM records WHERE id=?1 UNION ALL SELECT 1 FROM attachments WHERE id=?1 UNION ALL SELECT 1 FROM vault WHERE id=?1)",[&id],|r|r.get(0)).map_err(err)?;
                    if !exists && fresh.insert(id.clone()) {
                        break id;
                    }
                };
                mapping.insert(old.to_owned(), id);
            }
            Ok(())
        })?;
        let checksum = format!("{:x}", reader.hash.finalize());
        if expected_sha256.is_some_and(|expected| expected != checksum) {
            return Err("Reviewed archive checksum changed".into());
        }
        input.seek(SeekFrom::Start(0)).map_err(err)?;
        // Bind the second pass to the first even when the caller has no preview checksum.
        self.import_stream_mapped(input, password, vault_key, Some(&checksum), Some(&mapping))
    }
    pub fn inspect_stream<R: Read>(&self, input: R, password: &str) -> Result<Summary> {
        let mut summary = Summary::default();
        decode(input, password, |frame| {
            count(&self.conn, &frame, &mut summary)?;
            Ok(())
        })?;
        Ok(summary)
    }
    pub fn import_stream<R: Read>(
        &mut self,
        input: R,
        password: &str,
        vault_key: Option<&[u8]>,
    ) -> Result<Summary> {
        self.import_stream_checked(input, password, vault_key, None)
    }
    pub fn import_stream_checked<R: Read>(
        &mut self,
        input: R,
        password: &str,
        vault_key: Option<&[u8]>,
        expected_sha256: Option<&str>,
    ) -> Result<Summary> {
        self.import_stream_mapped(input, password, vault_key, expected_sha256, None)
    }
    fn import_stream_mapped<R: Read>(
        &mut self,
        input: R,
        password: &str,
        vault_key: Option<&[u8]>,
        expected_sha256: Option<&str>,
        mapping: Option<&HashMap<String, String>>,
    ) -> Result<Summary> {
        let device = self.device_id.clone();
        let mut input = HashReader {
            inner: input,
            hash: Sha256::new(),
        };
        let tx = self.conn.transaction().map_err(err)?;
        let mut summary = Summary::default();
        decode(&mut input, password, |frame| {
            let frame = if let Some(mapping) = mapping {
                let Some(frame) = copy_frame(frame, mapping, &device)? else {
                    return Ok(());
                };
                frame
            } else {
                frame
            };
            validate(&frame)?;
            count(&tx, &frame, &mut summary)?;
            match frame {
                Frame::Item(v) => {
                    if !exists(&tx, "items", &v.id)? {
                        crate::write(&tx, &v)?;
                        summary.imported += 1;
                    }
                }
                Frame::Record(v) => {
                    if !exists(&tx, "records", &v.id)? {
                        crate::workspace::write_record(&tx, &v)?;
                        summary.imported += 1;
                    }
                }
                Frame::Attachment(v) => {
                    summary.imported += crate::attachments::insert(&tx, &v)? as u64;
                }
                Frame::Vault(v) => {
                    if !exists(&tx, "vault", &v.id)? {
                        let key = vault_key
                            .ok_or("Unlock this device's vault before importing private records")?;
                        let encrypted = crate::vault_codec::seal(key, &v)?;
                        tx.execute(
                            "INSERT INTO vault VALUES(?1,?2,?3)",
                            params![v.id, encrypted, crate::now()],
                        )
                        .map_err(err)?;
                        summary.imported += 1;
                    }
                }
                Frame::History(v) => {
                    tx.execute("INSERT OR IGNORE INTO changes(event_id,entity_id,device_id,revision,operation,timestamp,payload) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![v.event_id,v.entity_id,v.device_id,v.revision,v.operation,v.timestamp,v.payload]).map_err(err)?;
                }
                Frame::Feed(v) => {
                    tx.execute(
                        "INSERT OR IGNORE INTO feed_cache VALUES(?1,?2,?3)",
                        params![v.source, v.fetched_at, v.items.to_string()],
                    )
                    .map_err(err)?;
                }
                Frame::Preferences(v) => {
                    tx.execute(
                        "INSERT OR IGNORE INTO settings VALUES('preferences',?1)",
                        [v.to_string()],
                    )
                    .map_err(err)?;
                }
                Frame::Collections(v) => {
                    tx.execute(
                        "INSERT OR IGNORE INTO settings VALUES('research_collections',?1)",
                        [serde_json::to_string(&v).map_err(err)?],
                    )
                    .map_err(err)?;
                }
                Frame::End { .. } => {}
            }
            Ok(())
        })?;
        if expected_sha256
            .is_some_and(|expected| expected != format!("{:x}", input.hash.finalize()))
        {
            return Err("Reviewed archive checksum changed".into());
        }
        tx.commit().map_err(err)?;
        Ok(summary)
    }
}
fn exists(conn: &rusqlite::Connection, table: &str, id: &str) -> Result<bool> {
    // table is an internal constant, never renderer input.
    conn.query_row(
        &format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE id=?1)"),
        [id],
        |r| r.get(0),
    )
    .map_err(err)
}
fn count(conn: &rusqlite::Connection, frame: &Frame, s: &mut Summary) -> Result<()> {
    let entity = match frame {
        Frame::Item(v) => Some((&v.id, "item", "items")),
        Frame::Record(v) => Some((&v.id, "record", "records")),
        Frame::Attachment(v) => Some((&v.id, "attachment", "attachments")),
        Frame::Vault(v) => {
            s.vault_records += 1;
            Some((&v.id, "vault", "vault"))
        }
        _ => None,
    };
    if let Some((id, class, table)) = entity {
        crate::workspace::check_owner(conn, id, class)?;
        s.records += 1;
        if exists(conn, table, id)? {
            s.existing_records += 1;
        } else {
            s.new_records += 1;
        }
    } else if matches!(frame, Frame::History(_)) {
        s.history_events += 1;
    } else {
        s.config_frames += 1;
    }
    Ok(())
}

struct HashReader<R> {
    inner: R,
    hash: Sha256,
}
impl<R: Read> Read for HashReader<R> {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(bytes)?;
        self.hash.update(&bytes[..n]);
        Ok(n)
    }
}

#[cfg(test)]
mod hostile_tests {
    use super::*;
    fn item(repo: &Repository) -> Item {
        Item {
            id: uuid::Uuid::new_v4().to_string(),
            kind: "note".into(),
            title: "Fixture".into(),
            body: "".into(),
            pinned: false,
            priority: "normal".into(),
            status: "active".into(),
            due: None,
            recurrence: "none".into(),
            url: None,
            created_at: crate::now(),
            updated_at: crate::now(),
            device_id: repo.device_id.clone(),
            revision: 1,
            deleted_at: None,
        }
    }
    #[test]
    fn authenticated_duplicate_ids_roll_back_all_prior_frames() {
        let mut target = Repository::open(":memory:").unwrap();
        let i = item(&target);
        let mut bytes = vec![];
        let mut e = Encoder::new(&mut bytes, "synthetic-stream-password").unwrap();
        e.emit(&Frame::Item(i.clone())).unwrap();
        e.emit(&Frame::Item(i)).unwrap();
        e.finish().unwrap();
        assert!(target
            .import_stream(&bytes[..], "synthetic-stream-password", None)
            .is_err());
        assert!(target.list("", true).unwrap().is_empty());
    }
    #[test]
    fn unknown_versions_and_cancellation_do_not_import_partial_data() {
        let mut source = Repository::open(":memory:").unwrap();
        source.save(item(&source)).unwrap();
        let mut bytes = vec![];
        source
            .export_stream(&mut bytes, "synthetic-stream-password", None)
            .unwrap();
        let mut future = bytes.clone();
        future[7] = b'2';
        let mut target = Repository::open(":memory:").unwrap();
        assert!(target
            .import_stream(&future[..], "synthetic-stream-password", None)
            .is_err());
        struct Fail {
            bytes: std::io::Cursor<Vec<u8>>,
            remaining: usize,
        }
        impl Read for Fail {
            fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
                if self.remaining == 0 {
                    return Err(std::io::Error::other("Cancelled fixture"));
                }
                let n = out.len().min(self.remaining);
                let read = self.bytes.read(&mut out[..n])?;
                self.remaining -= read;
                Ok(read)
            }
        }
        let remaining = bytes.len() - 10;
        assert!(target
            .import_stream(
                Fail {
                    bytes: std::io::Cursor::new(bytes),
                    remaining
                },
                "synthetic-stream-password",
                None
            )
            .is_err());
        assert!(target.list("", true).unwrap().is_empty());
    }
}

fn frame_id(f: &Frame) -> Option<&str> {
    match f {
        Frame::Item(v) => Some(&v.id),
        Frame::Record(v) => Some(&v.id),
        Frame::Attachment(v) => Some(&v.id),
        Frame::Vault(v) => Some(&v.id),
        _ => None,
    }
}
fn remap(id: &mut String, map: &HashMap<String, String>) {
    if let Some(new) = map.get(id) {
        *id = new.clone();
    }
}
fn copied_title(title: &mut String) {
    let suffix = " (imported copy)";
    let mut end = title.len().min(500 - suffix.len());
    while !title.is_char_boundary(end) {
        end -= 1;
    }
    *title = format!("{}{suffix}", &title[..end]);
}
fn copy_item(v: &mut Item, map: &HashMap<String, String>, device: Option<&str>) {
    remap(&mut v.id, map);
    if let Some(parent) = v.recurrence.strip_prefix("after:") {
        if let Some(new) = map.get(parent) {
            v.recurrence = format!("after:{new}");
        }
    }
    if let Some(device) = device {
        copied_title(&mut v.title);
        v.revision = 1;
        v.device_id = device.into();
        v.created_at = crate::now();
        v.updated_at = crate::now();
    }
}
fn copy_record(v: &mut Record, map: &HashMap<String, String>, device: Option<&str>) {
    remap(&mut v.id, map);
    for id in &mut v.links {
        remap(id, map);
    }
    if let Some(id) = v.fields.get_mut("workspace_owner") {
        remap(id, map);
    }
    if let Some(device) = device {
        copied_title(&mut v.title);
        v.revision = 1;
        v.device_id = device.into();
        v.created_at = crate::now();
        v.updated_at = crate::now();
    }
}
fn copy_frame(mut f: Frame, map: &HashMap<String, String>, device: &str) -> Result<Option<Frame>> {
    match &mut f {
        Frame::Item(v) => copy_item(v, map, Some(device)),
        Frame::Record(v) => copy_record(v, map, Some(device)),
        Frame::Attachment(v) => {
            remap(&mut v.id, map);
            remap(&mut v.owner_id, map);
            if let Some(id) = &mut v.previous_id {
                remap(id, map);
            }
        }
        Frame::Vault(v) => {
            remap(&mut v.id, map);
            copied_title(&mut v.title);
            v.updated_at = crate::now();
        }
        Frame::History(v) => {
            if !map.contains_key(&v.entity_id) {
                return Ok(None);
            }
            remap(&mut v.entity_id, map);
            v.event_id = uuid::Uuid::new_v4().to_string();
            let value: serde_json::Value = serde_json::from_str(&v.payload).map_err(err)?;
            if value.get("kind").is_some() {
                let mut payload: Item = serde_json::from_value(value).map_err(err)?;
                copy_item(&mut payload, map, None);
                v.payload = serde_json::to_string(&payload).map_err(err)?;
            } else {
                let mut payload: Record = serde_json::from_value(value).map_err(err)?;
                copy_record(&mut payload, map, None);
                v.payload = serde_json::to_string(&payload).map_err(err)?;
            }
        }
        Frame::Preferences(_) | Frame::Feed(_) | Frame::Collections(_) => return Ok(None),
        Frame::End { .. } => {}
    }
    Ok(Some(f))
}
