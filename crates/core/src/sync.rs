//! Version-vector replication for explicitly paired peers. Transport and trust stay native.
use crate::{err, now, workspace::Record, Item, Repository, Result};
use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;
pub type Clock = BTreeMap<String, u64>;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub id: String,
    pub entity_id: String,
    pub kind: String,
    pub clock: Clock,
    pub base_clock: Clock,
    pub base: Option<Value>,
    pub payload: Value,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Batch {
    pub protocol: u32,
    pub after: i64,
    pub through: i64,
    pub more: bool,
    pub events: Vec<Event>,
}
#[derive(Serialize)]
pub struct Conflict {
    pub id: String,
    pub peer_id: String,
    pub entity_id: String,
    pub local: Value,
    pub incoming: Value,
    pub created_at: String,
}
fn encode(v: &impl Serialize) -> Result<String> {
    serde_json::to_string(v).map_err(err)
}
fn decode<T: serde::de::DeserializeOwned>(s: &str) -> Result<T> {
    serde_json::from_str(s).map_err(err)
}
fn normalized(v: &Value) -> Value {
    let mut v = v.clone();
    if let Some(m) = v.as_object_mut() {
        for k in ["revision", "updated_at", "device_id"] {
            m.remove(k);
        }
    }
    v
}
fn dominates(a: &Clock, b: &Clock) -> bool {
    b.iter().all(|(k, v)| a.get(k).copied().unwrap_or(0) >= *v)
}
fn union(a: &Clock, b: &Clock) -> Clock {
    let mut result = a.clone();
    for (k, v) in b {
        result
            .entry(k.clone())
            .and_modify(|n| *n = (*n).max(*v))
            .or_insert(*v);
    }
    result
}
fn advance(clock: &Clock, device: &str) -> Result<Clock> {
    let mut next = clock.clone();
    let n = next
        .get(device)
        .copied()
        .unwrap_or(0)
        .checked_add(1)
        .ok_or("Version counter exhausted")?;
    next.insert(device.into(), n);
    if next.len() > 64 {
        return Err("Sync supports up to 64 causal device identities per record".into());
    }
    Ok(next)
}
pub fn valid_scope(scope: &str) -> bool {
    ["note", "task", "saved", "attachment"].contains(&scope)
        || (crate::workspace::MODULES.contains(&scope) && scope != "notifications")
}
fn validate_payload(kind: &str, payload: &Value) -> Result<()> {
    if encode(payload)?.len()
        > if kind == "attachment" {
            15 * 1024 * 1024
        } else {
            300_000
        }
    {
        return Err("Sync entity exceeds 300 KB".into());
    }
    if kind == "attachment" {
        attachment_value(payload)?.validate()?;
    } else if ["note", "task", "saved"].contains(&kind) {
        let i: Item = serde_json::from_value(payload.clone()).map_err(err)?;
        i.validate()?;
        if i.kind != kind {
            return Err("Entity kind mismatch".into());
        }
    } else {
        let r: Record = serde_json::from_value(payload.clone()).map_err(err)?;
        r.validate()?;
        if r.module != kind || !valid_scope(kind) {
            return Err("Entity module not syncable".into());
        }
    }
    Ok(())
}
fn attachment_value(value: &Value) -> Result<crate::attachments::Attachment> {
    let mut value = value.clone();
    if let Some(m) = value.as_object_mut() {
        for k in ["revision", "updated_at", "device_id"] {
            m.remove(k);
        }
    }
    serde_json::from_value(value).map_err(err)
}
fn attachment_scope(conn: &rusqlite::Connection, value: &Value, scopes: &[String]) -> Result<bool> {
    let owner = value["owner_id"]
        .as_str()
        .ok_or("Attachment owner missing")?;
    let kind: Option<String> = conn
        .query_row(
            "SELECT kind FROM items WHERE id=?1 UNION ALL SELECT module FROM records WHERE id=?1",
            [owner],
            |r| r.get(0),
        )
        .optional()
        .map_err(err)?;
    Ok(kind.is_some_and(|k| scopes.contains(&k)))
}
impl Event {
    fn validate(&self) -> Result<()> {
        Uuid::parse_str(&self.id).map_err(err)?;
        Uuid::parse_str(&self.entity_id).map_err(err)?;
        if self.clock.is_empty()
            || self.clock.len() > 64
            || self.base_clock.len() > 64
            || !dominates(&self.clock, &self.base_clock)
            || self.clock == self.base_clock
        {
            return Err("Invalid causal version".into());
        }
        for (k, v) in self.clock.iter().chain(self.base_clock.iter()) {
            Uuid::parse_str(k).map_err(err)?;
            if *v == 0 || *v > i64::MAX as u64 {
                return Err("Invalid version counter".into());
            }
        }
        validate_payload(&self.kind, &self.payload)?;
        if self.payload["id"].as_str() != Some(&self.entity_id) {
            return Err("Entity ID mismatch".into());
        }
        if let Some(base) = &self.base {
            validate_payload(&self.kind, base)?;
            if base["id"] != self.payload["id"] {
                return Err("Base identity mismatch".into());
            }
        } else if !self.base_clock.is_empty() {
            return Err("Missing causal base".into());
        }
        Ok(())
    }
}
fn head(tx: &Transaction<'_>, id: &str) -> Result<Option<(String, Clock, Value)>> {
    let raw: Option<(String, String, String)> = tx
        .query_row(
            "SELECT kind,clock,payload FROM sync_heads WHERE entity_id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(err)?;
    raw.map(|(k, c, p)| Ok((k, decode(&c)?, decode(&p)?)))
        .transpose()
}
fn put_head(tx: &Transaction<'_>, event: &Event, payload: &Value) -> Result<()> {
    tx.execute("INSERT INTO sync_heads VALUES(?1,?2,?3,?4) ON CONFLICT(entity_id) DO UPDATE SET kind=excluded.kind,clock=excluded.clock,payload=excluded.payload",params![event.entity_id,event.kind,encode(&event.clock)?,encode(payload)?]).map_err(err)?;
    Ok(())
}
fn append(tx: &Transaction<'_>, e: &Event) -> Result<()> {
    let raw = encode(e)?;
    let hash = format!("{:x}", Sha256::digest(raw.as_bytes()));
    tx.execute("INSERT INTO sync_seen VALUES(?1,?2)", params![e.id, hash])
        .map_err(err)?;
    tx.execute("INSERT INTO sync_events(event_id,entity_id,kind,clock,base_clock,base_payload,payload) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![e.id,e.entity_id,e.kind,encode(&e.clock)?,encode(&e.base_clock)?,e.base.as_ref().map(encode).transpose()?,encode(&e.payload)?]).map_err(err)?;
    Ok(())
}
fn materialize(tx: &Transaction<'_>, kind: &str, payload: &Value) -> Result<Value> {
    let id = payload["id"].as_str().ok_or("Missing entity ID")?;
    if kind == "attachment" {
        let a = attachment_value(payload)?;
        a.validate()?;
        crate::workspace::check_owner(tx, &a.id, "attachment")?;
        let existing: Option<String> = tx
            .query_row(
                "SELECT payload FROM attachments WHERE id=?1",
                [&a.id],
                |r| r.get(0),
            )
            .optional()
            .map_err(err)?;
        if let Some(raw) = existing.as_ref() {
            let old: crate::attachments::Attachment = decode(raw)?;
            if old.owner_id != a.owner_id
                || old.sha256 != a.sha256
                || old.previous_id != a.previous_id
                || old.created_at != a.created_at
            {
                return Err("Attachment identity/content is immutable; add a new version".into());
            }
        } else {
            let count: i64 = tx
                .query_row("SELECT COUNT(*) FROM attachments", [], |r| r.get(0))
                .map_err(err)?;
            if count >= 1000 {
                return Err("Attachment limit reached; nothing in this batch was committed".into());
            }
            crate::attachments::insert(tx, &a)?;
        }
        tx.execute(
            "UPDATE attachments SET payload=?2 WHERE id=?1",
            params![a.id, encode(&a)?],
        )
        .map_err(err)?;
        tx.execute("DELETE FROM attachments_fts WHERE id=?1", [&a.id])
            .map_err(err)?;
        tx.execute(
            "INSERT INTO attachments_fts(id,title,extracted_text) VALUES(?1,?2,?3)",
            params![a.id, a.filename, a.extracted_text],
        )
        .map_err(err)?;
        let revision:i64=tx.query_row("SELECT COALESCE(json_extract(payload,'$.revision'),0) FROM sync_heads WHERE entity_id=?1",[id],|r|r.get(0)).optional().map_err(err)?.unwrap_or(0);
        let mut value = payload.clone();
        value["revision"] = Value::from(revision.checked_add(1).ok_or("Revision exhausted")?);
        return Ok(value);
    }
    let table = if ["note", "task", "saved"].contains(&kind) {
        "items"
    } else {
        "records"
    };
    let revision: Option<i64> = tx
        .query_row(
            &format!("SELECT revision FROM {table} WHERE id=?1"),
            [id],
            |r| r.get(0),
        )
        .optional()
        .map_err(err)?;
    let mut value = payload.clone();
    value["revision"] = Value::from(
        revision
            .unwrap_or(0)
            .checked_add(1)
            .ok_or("Revision exhausted")?,
    );
    if table == "items" {
        let item: Item = serde_json::from_value(value.clone()).map_err(err)?;
        item.validate()?;
        crate::write(tx, &item)?;
    } else {
        let record: Record = serde_json::from_value(value.clone()).map_err(err)?;
        record.validate()?;
        crate::workspace::write_record(tx, &record)?;
    }
    Ok(value)
}
fn merge_value(base: &Value, local: &Value, incoming: &Value, key: &str) -> Option<Value> {
    if local == incoming {
        return Some(local.clone());
    }
    if local == base {
        return Some(incoming.clone());
    }
    if incoming == base {
        return Some(local.clone());
    }
    if key == "fields" {
        let (b, l, r) = (base.as_object()?, local.as_object()?, incoming.as_object()?);
        let keys: BTreeSet<_> = b.keys().chain(l.keys()).chain(r.keys()).collect();
        let mut out = serde_json::Map::new();
        for k in keys {
            let value = merge_value(
                b.get(k).unwrap_or(&Value::Null),
                l.get(k).unwrap_or(&Value::Null),
                r.get(k).unwrap_or(&Value::Null),
                k,
            )?;
            if !value.is_null() {
                out.insert(k.clone(), value);
            }
        }
        return Some(Value::Object(out));
    }
    if key == "tags" || key == "links" {
        let set = |v: &Value| -> Option<BTreeSet<String>> {
            v.as_array()?
                .iter()
                .map(|v| v.as_str().map(str::to_owned))
                .collect()
        };
        let (b, l, r) = (set(base)?, set(local)?, set(incoming)?);
        let mut result: BTreeSet<_> = l.intersection(&r).cloned().collect();
        result.extend(l.difference(&b).cloned());
        result.extend(r.difference(&b).cloned());
        return Some(serde_json::json!(result));
    }
    None
}
fn merge(base: &Value, local: &Value, incoming: &Value) -> Option<Value> {
    let (b, l, r) = (normalized(base), normalized(local), normalized(incoming));
    // Delete versus independent edit is always visible; never revive or erase silently.
    if l["deleted_at"] != r["deleted_at"] && l != b && r != b {
        return None;
    }
    let (b, l, r) = (b.as_object()?, l.as_object()?, r.as_object()?);
    let mut out = incoming.clone();
    for k in b
        .keys()
        .chain(l.keys())
        .chain(r.keys())
        .collect::<BTreeSet<_>>()
    {
        let v = merge_value(
            b.get(k).unwrap_or(&Value::Null),
            l.get(k).unwrap_or(&Value::Null),
            r.get(k).unwrap_or(&Value::Null),
            k,
        )?;
        out[k] = v;
    }
    Some(out)
}
impl Repository {
    /// Capture edits from all ordinary write paths before exporting or applying peer changes.
    pub fn prepare_sync(&mut self) -> Result<()> {
        let entities: Vec<(String, Value)> = self
            .list("", true)?
            .into_iter()
            .map(|i| Ok((i.kind.clone(), serde_json::to_value(i).map_err(err)?)))
            .chain(
                self.records("", true)?
                    .into_iter()
                    .filter(|r| valid_scope(&r.module))
                    .map(|r| Ok((r.module.clone(), serde_json::to_value(r).map_err(err)?))),
            )
            .collect::<Result<_>>()?;
        let device = self.device_id.clone();
        let tx = self.conn.transaction().map_err(err)?;
        let mut documents = tx
            .prepare("SELECT payload FROM attachments ORDER BY rowid")
            .map_err(err)?;
        let rows = documents
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(err)?;
        let attachments = rows.map(|r| -> Result<(String, Value)> {
            Ok(("attachment".to_string(), decode::<Value>(&r.map_err(err)?)?))
        });
        for entity in entities.into_iter().map(Ok).chain(attachments) {
            let (kind, mut payload) = entity?;
            let id = payload["id"].as_str().ok_or("Missing identity")?.to_owned();
            let prior = head(&tx, &id)?;
            if prior
                .as_ref()
                .is_some_and(|(_, _, p)| normalized(p) == normalized(&payload))
            {
                continue;
            }
            if kind == "attachment" {
                payload["revision"] = Value::from(
                    prior
                        .as_ref()
                        .and_then(|(_, _, p)| p["revision"].as_i64())
                        .unwrap_or(0)
                        .checked_add(1)
                        .ok_or("Revision exhausted")?,
                );
            }
            let base_clock = prior
                .as_ref()
                .map(|(_, c, _)| c.clone())
                .unwrap_or_default();
            let event = Event {
                id: Uuid::new_v4().to_string(),
                entity_id: id.into(),
                kind,
                clock: advance(&base_clock, &device)?,
                base_clock,
                base: prior.map(|(_, _, p)| p),
                payload,
            };
            event.validate()?;
            append(&tx, &event)?;
            put_head(&tx, &event, &event.payload)?;
        }
        drop(documents);
        tx.commit().map_err(err)
    }
    pub fn sync_batch(&mut self, after: i64, scopes: &[String]) -> Result<Batch> {
        if after < 0 || scopes.iter().any(|s| !valid_scope(s)) {
            return Err("Invalid cursor or scope".into());
        }
        self.prepare_sync()?;
        let mut stmt=self.conn.prepare("SELECT sequence,event_id,entity_id,kind,clock,base_clock,base_payload,payload FROM sync_events WHERE sequence>?1 ORDER BY sequence LIMIT 64").map_err(err)?;
        let rows = stmt
            .query_map([after], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, Option<String>>(6)?,
                    r.get::<_, String>(7)?,
                ))
            })
            .map_err(err)?;
        let mut through = after;
        let mut events = vec![];
        let mut bytes = 0;
        for row in rows {
            let (seq, id, entity_id, kind, c, bc, b, p) = row.map_err(err)?;
            if scopes.contains(&kind)
                && (kind != "attachment"
                    || attachment_scope(&self.conn, &decode::<Value>(&p)?, scopes)?)
            {
                let event = Event {
                    id,
                    entity_id,
                    kind,
                    clock: decode(&c)?,
                    base_clock: decode(&bc)?,
                    base: b.as_deref().map(decode).transpose()?,
                    payload: decode(&p)?,
                };
                let size = encode(&event)?.len();
                if !events.is_empty()
                    && (event.kind == "attachment" || bytes + size > 3 * 1024 * 1024)
                {
                    break;
                }
                bytes += size;
                let document = event.kind == "attachment";
                events.push(event);
                if document {
                    through = seq;
                    break;
                }
            }
            through = seq;
        }
        let more = self
            .conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sync_events WHERE sequence>?1)",
                [through],
                |r| r.get(0),
            )
            .map_err(err)?;
        Ok(Batch {
            protocol: 1,
            after,
            through,
            more,
            events,
        })
    }
    pub fn sync_progress(&self, peer: &str) -> Result<(i64, i64)> {
        self.conn
            .query_row(
                "SELECT sent,received FROM peer_progress WHERE peer_id=?1",
                [peer],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(err)
            .map(|v| v.unwrap_or((0, 0)))
    }
    pub fn sync_ack(&self, peer: &str, through: i64) -> Result<()> {
        let max: i64 = self
            .conn
            .query_row(
                "SELECT COALESCE(MAX(sequence),0) FROM sync_events",
                [],
                |r| r.get(0),
            )
            .map_err(err)?;
        if through < 0 || through > max {
            return Err("Invalid acknowledgement".into());
        }
        self.conn.execute("INSERT INTO peer_progress(peer_id,sent,received,last_sync) VALUES(?1,?2,0,?3) ON CONFLICT(peer_id) DO UPDATE SET sent=MAX(sent,excluded.sent),last_sync=excluded.last_sync",params![peer,through,now()]).map_err(err)?;
        Ok(())
    }
    pub fn apply_sync_batch(
        &mut self,
        peer: &str,
        batch: &Batch,
        scopes: &[String],
    ) -> Result<usize> {
        Uuid::parse_str(peer).map_err(err)?;
        if batch.protocol != 1
            || batch.after < 0
            || batch.through < batch.after
            || batch.events.len() > 64
            || encode(batch)?.len()
                > if scopes.iter().any(|s| s == "attachment") {
                    32 * 1024 * 1024
                } else {
                    4 * 1024 * 1024
                }
        {
            return Err("Invalid sync batch".into());
        }
        let cursor = self.sync_progress(peer)?.1;
        if batch.after != cursor {
            return Err("Sync cursor changed; reconnect to resume".into());
        }
        for e in &batch.events {
            e.validate()?;
            if !scopes.contains(&e.kind) {
                return Err("Peer attempted data outside approved scope".into());
            }
        }
        self.prepare_sync()?;
        let device = self.device_id.clone();
        let tx = self.conn.transaction().map_err(err)?;
        let mut changed = 0;
        let conflicts: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM sync_conflicts WHERE resolved_at IS NULL",
                [],
                |r| r.get(0),
            )
            .map_err(err)?;
        if conflicts + batch.events.len() as i64 > 2000 {
            return Err("Resolve existing sync conflicts before receiving more data".into());
        }
        for event in &batch.events {
            if event.kind == "attachment" && !attachment_scope(&tx, &event.payload, scopes)? {
                return Err(
                    "Attachment owner category is not approved or its record has not synced yet"
                        .into(),
                );
            }
            let digest = format!("{:x}", Sha256::digest(encode(event)?.as_bytes()));
            let seen: Option<String> = tx
                .query_row(
                    "SELECT digest FROM sync_seen WHERE event_id=?1",
                    [&event.id],
                    |r| r.get(0),
                )
                .optional()
                .map_err(err)?;
            if let Some(seen) = seen {
                if seen != digest {
                    return Err("Immutable event ID reused with different data".into());
                }
                continue;
            }
            let prior = head(&tx, &event.entity_id)?;
            if let Some((kind, clock, payload)) = &prior {
                if kind != &event.kind {
                    return Err("Peer changed an entity type".into());
                }
                if dominates(clock, &event.clock) {
                    if clock == &event.clock && normalized(payload) != normalized(&event.payload) {
                        return Err("Identical causal version has conflicting payloads".into());
                    }
                } else if dominates(&event.clock, clock) {
                    let value = materialize(&tx, &event.kind, &event.payload)?;
                    put_head(&tx, event, &value)?;
                    changed += 1;
                } else {
                    let merged = if normalized(payload) == normalized(&event.payload) {
                        Some(payload.clone())
                    } else if dominates(clock, &event.base_clock) {
                        event
                            .base
                            .as_ref()
                            .and_then(|b| merge(b, payload, &event.payload))
                    } else {
                        None
                    };
                    if let Some(mut value) = merged {
                        value["updated_at"] = Value::String(now());
                        value["device_id"] = Value::String(device.clone());
                        validate_payload(&event.kind, &value)?;
                        let value = materialize(&tx, &event.kind, &value)?;
                        let resolved = Event {
                            id: Uuid::new_v4().to_string(),
                            entity_id: event.entity_id.clone(),
                            kind: event.kind.clone(),
                            clock: advance(&union(clock, &event.clock), &device)?,
                            base_clock: clock.clone(),
                            base: Some(payload.clone()),
                            payload: value.clone(),
                        };
                        append(&tx, &resolved)?;
                        put_head(&tx, &resolved, &value)?;
                        changed += 1;
                    } else {
                        tx.execute(
                            "INSERT INTO sync_conflicts VALUES(?1,?2,?3,?4,?5,?6,NULL)",
                            params![
                                Uuid::new_v4().to_string(),
                                peer,
                                event.entity_id,
                                encode(event)?,
                                encode(payload)?,
                                now()
                            ],
                        )
                        .map_err(err)?;
                    }
                }
            } else {
                let value = materialize(&tx, &event.kind, &event.payload)?;
                put_head(&tx, event, &value)?;
                changed += 1;
            }
            append(&tx, event)?;
        }
        tx.execute("INSERT INTO peer_progress(peer_id,sent,received,last_sync) VALUES(?1,0,?2,?3) ON CONFLICT(peer_id) DO UPDATE SET received=excluded.received,last_sync=excluded.last_sync",params![peer,batch.through,now()]).map_err(err)?;
        tx.commit().map_err(err)?;
        Ok(changed)
    }
    pub fn peer_status(&self) -> Result<Vec<Value>> {
        let mut stmt = self
            .conn
            .prepare("SELECT peer_id,sent,received,last_sync FROM peer_progress ORDER BY peer_id")
            .map_err(err)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, Option<String>>(3)?,
                ))
            })
            .map_err(err)?;
        rows.map(|row|{let (peer,sent,received,last)=row.map_err(err)?;Ok(serde_json::json!({"device":peer,"sent":sent,"received":received,"last_exchange":last,"pending":self.sync_pending(&peer)?}))}).collect()
    }
    pub fn sync_conflicts(&self) -> Result<Vec<Conflict>> {
        let mut stmt=self.conn.prepare("SELECT id,peer_id,entity_id,local_payload,event,created_at FROM sync_conflicts WHERE resolved_at IS NULL ORDER BY created_at").map_err(err)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                ))
            })
            .map_err(err)?;
        rows.map(|row| {
            let (id, peer_id, entity_id, local, event, created_at) = row.map_err(err)?;
            let event: Event = decode(&event)?;
            Ok(Conflict {
                id,
                peer_id,
                entity_id: entity_id.clone(),
                local: self
                    .conn
                    .query_row(
                        "SELECT payload FROM sync_heads WHERE entity_id=?1",
                        [&entity_id],
                        |r| r.get::<_, String>(0),
                    )
                    .optional()
                    .map_err(err)?
                    .map(|s| decode(&s))
                    .transpose()?
                    .unwrap_or(decode(&local)?),
                incoming: event.payload,
                created_at,
            })
        })
        .map(|r: Result<Conflict>| {
            r.map(|mut c| {
                for value in [&mut c.local, &mut c.incoming] {
                    if let Some(m) = value.as_object_mut() {
                        if m.contains_key("content_base64") {
                            m.remove("content_base64");
                            m.remove("extracted_text");
                        }
                    }
                }
                c
            })
        })
        .collect()
    }
    pub fn resolve_sync_conflict(
        &mut self,
        id: &str,
        choice: &str,
        expected_revision: i64,
        edited: Option<Value>,
    ) -> Result<()> {
        self.prepare_sync()?;
        let device = self.device_id.clone();
        let tx = self.conn.transaction().map_err(err)?;
        let raw: String = tx
            .query_row(
                "SELECT event FROM sync_conflicts WHERE id=?1 AND resolved_at IS NULL",
                [id],
                |r| r.get(0),
            )
            .map_err(err)?;
        let event: Event = decode(&raw)?;
        let (kind, clock, local) =
            head(&tx, &event.entity_id)?.ok_or("Local record unavailable")?;
        if local["revision"].as_i64() != Some(expected_revision) {
            return Err("Local record changed; reload conflict before resolving".into());
        }
        if kind == "attachment" && choice == "edited" {
            return Err("Choose a stored attachment version; editing file payloads in conflict JSON is not supported".into());
        }
        let mut value = match choice {
            "local" => local.clone(),
            "incoming" => event.payload.clone(),
            "edited" => edited.ok_or("Provide merged content")?,
            _ => return Err("Choose local, incoming or edited content".into()),
        };
        if value["id"] != local["id"] {
            return Err("Cannot change conflict identity".into());
        }
        value["device_id"] = Value::String(device.clone());
        value["updated_at"] = Value::String(now());
        validate_payload(&kind, &value)?;
        let value = materialize(&tx, &kind, &value)?;
        let resolved = Event {
            id: Uuid::new_v4().to_string(),
            entity_id: event.entity_id,
            kind,
            clock: advance(&union(&clock, &event.clock), &device)?,
            base_clock: clock,
            base: Some(local),
            payload: value.clone(),
        };
        append(&tx, &resolved)?;
        put_head(&tx, &resolved, &value)?;
        tx.execute(
            "UPDATE sync_conflicts SET resolved_at=?2 WHERE id=?1",
            params![id, now()],
        )
        .map_err(err)?;
        tx.commit().map_err(err)
    }
    pub fn reset_peer_progress(&self, peer: &str) -> Result<()> {
        Uuid::parse_str(peer).map_err(err)?;
        self.conn.execute("INSERT INTO peer_progress(peer_id,sent,received,last_sync) VALUES(?1,0,0,NULL) ON CONFLICT(peer_id) DO UPDATE SET sent=0,received=0,last_sync=NULL",[peer]).map_err(err)?;
        Ok(())
    }
    pub fn forget_peer_progress(&self, peer: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM peer_progress WHERE peer_id=?1", [peer])
            .map_err(err)?;
        Ok(())
    }
    pub fn sync_pending(&self, peer: &str) -> Result<i64> {
        let sent = self.sync_progress(peer)?.0;
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM sync_events WHERE sequence>?1",
                [sent],
                |r| r.get(0),
            )
            .map_err(err)
    }
}
impl Repository {
    pub fn peer_has_current_entity(&self, peer: &str, id: &str) -> Result<bool> {
        self.conn.query_row("SELECT EXISTS(SELECT 1 FROM peer_progress p WHERE p.peer_id=?1 AND p.sent >= (SELECT MAX(sequence) FROM sync_events WHERE entity_id=?2))",params![peer,id],|r|r.get(0)).map_err(err)
    }
}
