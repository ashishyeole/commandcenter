use crate::{
    sync_wire::{self, Channel},
    AppState, Result,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
use command_center_core::sync::{valid_scope, Batch};
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::{Duration, Instant},
};
use tauri::{Manager, State};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{oneshot, Semaphore},
};
use tokio_util::sync::CancellationToken;
use zeroize::{Zeroize, Zeroizing};
const SERVICE: &str = "_pcc-sync._tcp.local.";
const PAIR_NOISE: &str = "Noise_XXpsk3_25519_ChaChaPoly_SHA256";
const SYNC_NOISE: &str = "Noise_KK_25519_ChaChaPoly_SHA256";
#[derive(Default)]
pub struct Runtime {
    pub stop: Option<CancellationToken>,
    pub port: Option<u16>,
    mdns: Option<ServiceDaemon>,
    service: Option<String>,
    candidates: BTreeMap<String, Vec<String>>,
    candidate_keys: BTreeMap<String, Vec<String>>,
    invite: Option<Invite>,
    pending: BTreeMap<String, Approval>,
    pub busy: bool,
    pub private_pending: BTreeMap<String, crate::private_transfer::Approval>,
}
#[derive(Clone)]
struct Invite {
    id: String,
    secret: Zeroizing<[u8; 32]>,
    expires: Instant,
    scopes: Vec<String>,
}
struct Approval {
    device: String,
    label: String,
    key: String,
    verification: String,
    scopes: Vec<String>,
    reply: oneshot::Sender<bool>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Trust {
    device: String,
    label: String,
    public_key: String,
    scopes: Vec<String>,
    endpoint: String,
    #[serde(default)]
    foreground_only: bool,
    paired_at: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Prelude {
    version: u32,
    mode: String,
    id: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PairCode {
    version: u32,
    invitation: String,
    secret: String,
    public_key: String,
    port: u16,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Hello {
    device: String,
    label: String,
    scopes: Vec<String>,
    received: i64,
    #[serde(default)]
    mobile: bool,
    #[serde(default)]
    deliveries: Vec<String>,
    port: u16,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PairReply {
    accepted: bool,
    hello: Option<Hello>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Round {
    ack: i64,
    batch: Batch,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ack {
    through: i64,
}
fn fail(e: impl std::fmt::Display) -> String {
    e.to_string()
}
fn fingerprint(key: &[u8]) -> String {
    format!("{:x}", Sha256::digest(key))
}
fn local_address(address: &str, allow_mesh: bool) -> Result<SocketAddr> {
    let addr: SocketAddr = address
        .parse()
        .map_err(|_| "Use a numeric LAN address and port, such as 192.168.1.10:41234")?;
    if addr.port() == 0 {
        return Err("Invalid port".into());
    }
    let local = match addr.ip() {
        IpAddr::V4(ip) => {
            ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || (allow_mesh && (u32::from(ip) & 0xffc00000) == 0x64400000)
        }
        IpAddr::V6(_) => false,
    };
    if !local {
        return Err("Only private IPv4 addresses are enabled. Explicit private-mesh mode also permits 100.64.0.0/10; globally routable endpoints remain disabled".into());
    }
    Ok(addr)
}
fn check_scopes(scopes: &[String]) -> Result<()> {
    if scopes.is_empty() || scopes.len() > 32 || scopes.iter().any(|s| !valid_scope(s)) {
        return Err(
            "Choose ordinary-data categories; vault, credentials, caches and paths are excluded"
                .into(),
        );
    }
    Ok(())
}
fn own(state: &AppState) -> Result<(String, String)> {
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    Ok((
        repo.device_id.clone(),
        repo.setting("sync_label")?.unwrap_or("My device".into()),
    ))
}
fn identity(state: &AppState) -> Result<Zeroizing<Vec<u8>>> {
    let (device, _) = own(state)?;
    let entry =
        keyring::Entry::new("com.personal.commandcenter.peer.identity", &device).map_err(fail)?;
    match entry.get_secret() {
        Ok(raw) if raw.len() == 64 => Ok(Zeroizing::new(raw)),
        Ok(_) => Err("Invalid stored sync identity; recover or revoke the identity".into()),
        Err(keyring::Error::NoEntry) => {
            let mut pair = snow::Builder::new(SYNC_NOISE.parse().map_err(fail)?)
                .generate_keypair()
                .map_err(fail)?;
            let mut raw = Zeroizing::new(pair.private.clone());
            raw.extend_from_slice(&pair.public);
            pair.private.zeroize();
            entry.set_secret(&raw).map_err(fail)?;
            Ok(raw)
        }
        Err(e) => Err(format!("OS secure storage unavailable: {e}")),
    }
}
fn peer_ids(state: &AppState) -> Result<Vec<String>> {
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    serde_json::from_str(&repo.setting("paired_peers")?.unwrap_or("[]".into())).map_err(fail)
}
fn trust_entry(local: &str, peer: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(&format!("com.personal.commandcenter.peer.{local}"), peer).map_err(fail)
}
fn trusted(state: &AppState, peer: &str) -> Result<Trust> {
    uuid::Uuid::parse_str(peer).map_err(|_| "Invalid peer identity")?;
    if !peer_ids(state)?.iter().any(|id| id == peer) {
        return Err("Device is not paired or has been revoked".into());
    }
    let (local, _) = own(state)?;
    let raw = trust_entry(&local, peer)?
        .get_password()
        .map_err(|_| "Peer trust is missing from OS secure storage")?;
    let trust: Trust = serde_json::from_str(&raw).map_err(fail)?;
    if trust.device != peer {
        return Err("Peer trust identity mismatch".into());
    }
    check_scopes(&trust.scopes)?;
    let bytes = B64.decode(&trust.public_key).map_err(fail)?;
    if bytes.len() != 32 {
        return Err("Invalid pinned peer key".into());
    }
    Ok(trust)
}
fn store_trust(state: &AppState, trust: &Trust) -> Result<()> {
    uuid::Uuid::parse_str(&trust.device).map_err(fail)?;
    check_scopes(&trust.scopes)?;
    let (local, _) = own(state)?;
    if trust.device == local {
        return Err("Cannot pair a device with itself or a cloned device identity".into());
    }
    let mut peers = peer_ids(state)?;
    if peers.len() >= 8 && !peers.contains(&trust.device) {
        return Err("At most eight paired devices".into());
    }
    if peers.contains(&trust.device) {
        let old = trusted(state, &trust.device)?;
        if old.public_key != trust.public_key {
            return Err("Device key changed; revoke the old device before pairing again".into());
        }
    }
    trust_entry(&local, &trust.device)?
        .set_password(&serde_json::to_string(trust).map_err(fail)?)
        .map_err(fail)?;
    if !peers.contains(&trust.device) {
        peers.push(trust.device.clone());
    }
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    repo.set_setting(
        "paired_peers",
        &serde_json::to_string(&peers).map_err(fail)?,
    )?;
    repo.reset_peer_progress(&trust.device)
}
fn hello(state: &AppState, scopes: Vec<String>, peer: Option<&str>) -> Result<Hello> {
    let (device, label) = own(state)?;
    let received = if let Some(peer) = peer {
        state
            .repo
            .lock()
            .map_err(|_| "Database unavailable")?
            .sync_progress(peer)?
            .1
    } else {
        0
    };
    let port = state
        .peer_sync
        .lock()
        .map_err(|_| "Sync unavailable")?
        .port
        .ok_or("Start LAN sync first")?;
    let deliveries = if peer.is_some() && scopes.iter().any(|s| s == "task") {
        state
            .repo
            .lock()
            .map_err(|_| "Database unavailable")?
            .recent_deliveries()?
    } else {
        vec![]
    };
    Ok(Hello {
        deliveries,
        mobile: false,
        device,
        label,
        scopes,
        received,
        port,
    })
}
fn validate_hello(value: &Hello) -> Result<()> {
    if value.deliveries.len() > 128 || value.deliveries.iter().any(|v| v.len() > 512) {
        return Err("Invalid notification receipt list".into());
    }
    uuid::Uuid::parse_str(&value.device).map_err(fail)?;
    if value.label.trim().is_empty()
        || value.label.len() > 80
        || value.received < 0
        || value.port == 0
    {
        return Err("Invalid peer greeting".into());
    }
    check_scopes(&value.scopes)
}
fn alive(state: &AppState, trust: &Trust) -> Result<()> {
    if state
        .peer_sync
        .lock()
        .map_err(|_| "Sync unavailable")?
        .stop
        .as_ref()
        .is_none_or(|s| s.is_cancelled())
    {
        return Err("Device sync is stopped".into());
    }
    let current = trusted(state, &trust.device)?;
    if current.public_key != trust.public_key || current.scopes != trust.scopes {
        return Err("Peer permissions changed; reconnect".into());
    }
    Ok(())
}
fn record_health(state: &AppState, peer: &str, result: &Result<usize>) -> Result<()> {
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    repo.set_setting(&format!("sync_health:{peer}"),&json!({"at":command_center_core::now(),"status":if result.is_ok(){"synced"}else{"failed"},"changed":result.as_ref().ok(),"error":result.as_ref().err()}).to_string())
}
async fn sync_responder(state: &AppState, mut channel: Channel, trust: Trust) -> Result<usize> {
    let request: Hello = channel.receive().await?;
    validate_hello(&request)?;
    if request.device != trust.device || request.scopes != trust.scopes {
        return Err("Peer identity or scope mismatch; re-pair to change categories".into());
    }
    save_receipts(state, &trust, &request.deliveries)?;
    let mut remote_cursor = request.received;
    channel
        .send(&hello(state, trust.scopes.clone(), Some(&trust.device))?)
        .await?;
    let mut changed = 0;
    for _ in 0..32 {
        alive(state, &trust)?;
        let batch: Batch = channel.receive().await?;
        alive(state, &trust)?;
        let reply = {
            let mut repo = state.repo.lock().map_err(|_| "Database unavailable")?;
            changed += repo.apply_sync_batch(&trust.device, &batch, &trust.scopes)?;
            repo.sync_batch(remote_cursor, &trust.scopes)?
        };
        let complete = !batch.more && !reply.more;
        let sent = reply.through;
        channel
            .send(&Round {
                ack: batch.through,
                batch: reply,
            })
            .await?;
        let ack: Ack = channel.receive().await?;
        if ack.through != sent {
            return Err("Peer acknowledgement mismatch".into());
        }
        alive(state, &trust)?;
        state
            .repo
            .lock()
            .map_err(|_| "Database unavailable")?
            .sync_ack(&trust.device, ack.through)?;
        remote_cursor = ack.through;
        if complete {
            break;
        }
    }
    Ok(changed)
}
async fn sync_initiator(state: &AppState, mut channel: Channel, trust: &Trust) -> Result<usize> {
    channel
        .send(&hello(state, trust.scopes.clone(), Some(&trust.device))?)
        .await?;
    let remote: Hello = channel.receive().await?;
    validate_hello(&remote)?;
    if remote.device != trust.device || remote.scopes != trust.scopes {
        return Err("Peer identity or scope mismatch; re-pair to change categories".into());
    }
    save_receipts(state, trust, &remote.deliveries)?;
    let mut remote_cursor = remote.received;
    let mut changed = 0;
    for _ in 0..32 {
        alive(state, trust)?;
        let batch = state
            .repo
            .lock()
            .map_err(|_| "Database unavailable")?
            .sync_batch(remote_cursor, &trust.scopes)?;
        let sent = batch.through;
        let more = batch.more;
        channel.send(&batch).await?;
        let response: Round = channel.receive().await?;
        if response.ack != sent {
            return Err("Peer acknowledgement mismatch".into());
        }
        alive(state, trust)?;
        {
            let mut repo = state.repo.lock().map_err(|_| "Database unavailable")?;
            changed += repo.apply_sync_batch(&trust.device, &response.batch, &trust.scopes)?;
            repo.sync_ack(&trust.device, sent)?;
        }
        channel
            .send(&Ack {
                through: response.batch.through,
            })
            .await?;
        remote_cursor = sent;
        if !more && !response.batch.more {
            break;
        }
    }
    Ok(changed)
}
async fn accept_connection(
    app: tauri::AppHandle,
    mut stream: TcpStream,
    address: SocketAddr,
) -> Result<()> {
    let state = app.state::<AppState>();
    local_address(&address.to_string(), mesh_enabled(&state)?)?;
    let prelude: Prelude =
        tokio::time::timeout(Duration::from_secs(5), sync_wire::read_prelude(&mut stream))
            .await
            .map_err(|_| "Connection timed out")??;
    if prelude.version != 1 {
        return Err("Unsupported peer protocol".into());
    }
    let key = identity(&state)?;
    if prelude.mode == "pair" {
        let invite = {
            let runtime = state.peer_sync.lock().map_err(|_| "Sync unavailable")?;
            runtime
                .invite
                .as_ref()
                .filter(|i| i.id == prelude.id && i.expires > Instant::now())
                .cloned()
                .ok_or("No matching active invitation")?
        };
        let noise = snow::Builder::new(PAIR_NOISE.parse().map_err(fail)?)
            .local_private_key(&key[..32])
            .map_err(fail)?
            .psk(3, &invite.secret)
            .map_err(fail)?
            .prologue(b"PCC peer protocol 1 pairing")
            .map_err(fail)?
            .build_responder()
            .map_err(fail)?;
        let mut channel = sync_wire::handshake(stream, noise, false).await?;
        let candidate: Hello = channel.receive().await?;
        validate_hello(&candidate)?;
        let scopes: Vec<String> = invite
            .scopes
            .iter()
            .filter(|s| candidate.scopes.contains(s))
            .cloned()
            .collect();
        check_scopes(&scopes)?;
        let approval_id = uuid::Uuid::new_v4().to_string();
        let (sender, receiver) = oneshot::channel();
        let public_key = B64.encode(&channel.remote_key);
        {
            let mut runtime = state.peer_sync.lock().map_err(|_| "Sync unavailable")?;
            if runtime
                .invite
                .as_ref()
                .is_none_or(|i| i.id != invite.id || i.expires <= Instant::now())
            {
                return Err("Invitation expired or already used".into());
            }
            runtime.invite = None;
            runtime.pending.insert(
                approval_id.clone(),
                Approval {
                    device: candidate.device.clone(),
                    label: candidate.label.clone(),
                    key: public_key.clone(),
                    verification: channel.verification.clone(),
                    scopes: scopes.clone(),
                    reply: sender,
                },
            );
        }
        let accepted = tokio::time::timeout(Duration::from_secs(45), receiver)
            .await
            .ok()
            .and_then(|r| r.ok())
            .unwrap_or(false);
        state
            .peer_sync
            .lock()
            .map_err(|_| "Sync unavailable")?
            .pending
            .remove(&approval_id);
        if !accepted {
            channel
                .send(&PairReply {
                    accepted: false,
                    hello: None,
                })
                .await?;
            return Err("Pairing declined or timed out".into());
        }
        let trust = Trust {
            device: candidate.device,
            label: candidate.label,
            public_key,
            scopes: scopes.clone(),
            endpoint: SocketAddr::new(address.ip(), candidate.port).to_string(),
            foreground_only: candidate.mobile || candidate.port == 1,
            paired_at: command_center_core::now(),
        };
        store_trust(&state, &trust)?;
        channel
            .send(&PairReply {
                accepted: true,
                hello: Some(hello(&state, scopes, None)?),
            })
            .await?;
        Ok(())
    } else if prelude.mode == "sync" || prelude.mode == "private-copy" {
        let trust = peer_ids(&state)?
            .into_iter()
            .filter_map(|id| trusted(&state, &id).ok())
            .find(|t| {
                B64.decode(&t.public_key)
                    .ok()
                    .is_some_and(|k| fingerprint(&k) == prelude.id)
            })
            .ok_or("Unknown peer key")?;
        let remote = B64.decode(&trust.public_key).map_err(fail)?;
        let prologue: &[u8] = if prelude.mode == "private-copy" {
            b"PCC peer protocol 1 private-copy"
        } else {
            b"PCC peer protocol 1 sync"
        };
        let noise = snow::Builder::new(SYNC_NOISE.parse().map_err(fail)?)
            .local_private_key(&key[..32])
            .map_err(fail)?
            .remote_public_key(&remote)
            .map_err(fail)?
            .prologue(prologue)
            .map_err(fail)?
            .build_responder()
            .map_err(fail)?;
        let mut channel = sync_wire::handshake(stream, noise, false).await?;
        if channel.remote_key != remote {
            return Err("Peer key mismatch".into());
        }
        alive(&state, &trust)?;
        if prelude.mode == "private-copy" {
            return crate::private_transfer::receive(
                &state,
                &mut channel,
                &trust.device,
                &trust.label,
            )
            .await;
        }
        let result = sync_responder(&state, channel, trust.clone()).await;
        let _ = record_health(&state, &trust.device, &result);
        result.map(|_| ())
    } else {
        Err("Unsupported peer operation".into())
    }
}
#[tauri::command]
pub fn peer_sync_status(state: State<AppState>) -> Result<Value> {
    let peers=peer_ids(&state)?.into_iter().map(|id|{match trusted(&state,&id){Ok(t)=>{let repo=state.repo.lock().map_err(|_|"Database unavailable")?;Ok(json!({"device":t.device,"label":t.label,"public_key":t.public_key,"fingerprint":fingerprint(&B64.decode(&t.public_key).map_err(fail)?),"scopes":t.scopes,"endpoint":t.endpoint,"foreground_only":t.foreground_only,"paired_at":t.paired_at,"pending":repo.sync_pending(&id)?,"health":repo.setting(&format!("sync_health:{id}"))?.and_then(|s|serde_json::from_str::<Value>(&s).ok())}))},Err(e)=>Ok(json!({"device":id,"label":"Unavailable trust record","error":e}))}}).collect::<Result<Vec<_>>>()?;
    let (conflicts, device, label) = {
        let mut repo = state.repo.lock().map_err(|_| "Database unavailable")?;
        repo.prepare_sync()?;
        (
            repo.sync_conflicts()?,
            repo.device_id.clone(),
            repo.setting("sync_label")?.unwrap_or("My device".into()),
        )
    };
    let mesh = mesh_enabled(&state)?;
    let (automatic, verification) = {
        let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
        (
            repo.setting("automatic_peer_sync")?.as_deref() == Some("true"),
            repo.setting("pair_verification")?,
        )
    };
    let runtime = state.peer_sync.lock().map_err(|_| "Sync unavailable")?;
    let pending:Vec<Value>=runtime.pending.iter().map(|(id,p)|json!({"id":id,"device":p.device,"label":p.label,"fingerprint":p.key,"verification":p.verification,"scopes":p.scopes})).collect();
    Ok(
        json!({"running":runtime.port.is_some(),"port":runtime.port,"device":device,"label":label,"candidates":runtime.candidates,"pending_pairings":pending,"private_transfers":runtime.private_pending.iter().map(|(id,p)|json!({"id":id,"label":p.label,"category":p.category,"count":p.count})).collect::<Vec<_>>(),"peers":peers,"conflicts":conflicts,"automatic":automatic,"mesh":mesh,"busy":runtime.busy,"verification":verification}),
    )
}
#[tauri::command]
pub async fn start_peer_sync(app: tauri::AppHandle, label: String) -> Result<u16> {
    if label.trim().is_empty() || label.len() > 80 {
        return Err("Use a device label of 1–80 bytes".into());
    }
    let state = app.state::<AppState>();
    let identity_key = identity(&state)?;
    // Reserve startup before yielding, to avoid two listeners from duplicate clicks.
    {
        let mut rt = state.peer_sync.lock().map_err(|_| "Sync unavailable")?;
        if let Some(port) = rt.port {
            return Ok(port);
        }
        if rt.busy {
            return Err("Sync is starting or busy".into());
        }
        rt.busy = true;
    }
    let result=async {
  let listener=TcpListener::bind("0.0.0.0:0").await.map_err(fail)?;let port=listener.local_addr().map_err(fail)?.port();let stop=CancellationToken::new();
  let mdns=ServiceDaemon::new().map_err(fail)?;let instance=uuid::Uuid::new_v4().to_string();let host=format!("pcc-{instance}.local.");
  let info=ServiceInfo::new(SERVICE,&instance,&host,"",port,[("protocol".to_owned(),"1".to_owned()),("key".to_owned(),fingerprint(&identity_key[32..]))].into_iter().collect::<std::collections::HashMap<String,String>>()).map_err(fail)?.enable_addr_auto();let fullname=info.get_fullname().to_string();mdns.register(info).map_err(fail)?;let browsing=mdns.browse(SERVICE).map_err(fail)?;
  {let mut rt=state.peer_sync.lock().map_err(|_|"Sync unavailable")?;rt.port=Some(port);rt.stop=Some(stop.clone());rt.mdns=Some(mdns);rt.service=Some(fullname.clone());}
  state.repo.lock().map_err(|_|"Database unavailable")?.set_setting("sync_label",label.trim())?;
  let server_app=app.clone();let server_stop=stop.clone();let slots=Arc::new(Semaphore::new(4));
  tauri::async_runtime::spawn(async move{loop{tokio::select!{_=server_stop.cancelled()=>break,result=listener.accept()=>{let Ok((stream,address))=result else{break;};let Ok(permit)=slots.clone().try_acquire_owned()else{continue;};let app=server_app.clone();let cancel=server_stop.clone();tauri::async_runtime::spawn(async move{let _permit=permit;tokio::select!{_=cancel.cancelled()=>{},_=tokio::time::timeout(Duration::from_secs(600),accept_connection(app,stream,address))=>{}}});}}}});
  let discovery_app=app.clone();tauri::async_runtime::spawn(async move{loop{tokio::select!{_=stop.cancelled()=>break,event=browsing.recv_async()=>{match event{Ok(ServiceEvent::ServiceResolved(service))if service.get_fullname()!=fullname=>{let endpoints=service.get_addresses_v4().iter().map(|ip|format!("{ip}:{}",service.get_port())).filter(|s|local_address(s,false).is_ok()).collect::<Vec<_>>();let state=discovery_app.state::<AppState>();if let Ok(mut rt)=state.peer_sync.lock(){if rt.candidates.len()<64{if let Some(key)=service.get_properties().get_property_val_str("key"){if key.len()==64{rt.candidate_keys.insert(key.to_owned(),endpoints.clone());}}rt.candidates.insert(service.get_fullname().to_owned(),endpoints);}};},Ok(ServiceEvent::ServiceRemoved(_,name))=>{let state=discovery_app.state::<AppState>();if let Ok(mut rt)=state.peer_sync.lock(){rt.candidates.remove(&name);};},Err(_)=>break,_=>{}}}}}});
  Ok(port)
 }.await;
    state.peer_sync.lock().map_err(|_| "Sync unavailable")?.busy = false;
    result
}
#[tauri::command]
pub fn stop_peer_sync(state: State<AppState>) -> Result<()> {
    halt(&state)
}
pub fn halt(state: &AppState) -> Result<()> {
    let mut rt = state.peer_sync.lock().map_err(|_| "Sync unavailable")?;
    if let Some(stop) = rt.stop.take() {
        stop.cancel();
    }
    if let Some(mdns) = rt.mdns.take() {
        if let Some(name) = rt.service.take() {
            let _ = mdns.unregister(&name);
        }
        let _ = mdns.shutdown();
    }
    rt.port = None;
    rt.invite = None;
    rt.pending.clear();
    rt.private_pending.clear();
    rt.candidates.clear();
    rt.candidate_keys.clear();
    Ok(())
}
#[tauri::command]
pub fn create_pair_invitation(state: State<AppState>, scopes: Vec<String>) -> Result<Value> {
    check_scopes(&scopes)?;
    let key = identity(&state)?;
    let mut rt = state.peer_sync.lock().map_err(|_| "Sync unavailable")?;
    let port = rt.port.ok_or("Start LAN sync first")?;
    let mut secret = Zeroizing::new([0u8; 32]);
    OsRng.fill_bytes(secret.as_mut());
    let id = uuid::Uuid::new_v4().to_string();
    let code = PairCode {
        version: 1,
        invitation: id.clone(),
        secret: B64.encode(secret.as_ref()),
        public_key: B64.encode(&key[32..]),
        port,
    };
    rt.invite = Some(Invite {
        id,
        secret,
        expires: Instant::now() + Duration::from_secs(300),
        scopes,
    });
    Ok(
        json!({"code":format!("pcc1.{}",B64.encode(serde_json::to_vec(&code).map_err(fail)?)),"expires_at":(chrono::Utc::now()+chrono::Duration::minutes(5)).to_rfc3339(),"fingerprint":fingerprint(&key[32..]),"port":port}),
    )
}
#[tauri::command]
pub fn approve_pairing(state: State<AppState>, id: String, approved: bool) -> Result<()> {
    let pending = state
        .peer_sync
        .lock()
        .map_err(|_| "Sync unavailable")?
        .pending
        .remove(&id)
        .ok_or("Pairing request expired")?;
    pending
        .reply
        .send(approved)
        .map_err(|_| "Pairing connection closed".into())
}
#[tauri::command]
pub async fn join_peer(
    app: tauri::AppHandle,
    address: String,
    code: String,
    scopes: Vec<String>,
) -> Result<Value> {
    check_scopes(&scopes)?;
    let state = app.state::<AppState>();
    let address = local_address(&address, mesh_enabled(&state)?)?;
    let code = Zeroizing::new(code);
    if code.len() > 2048 {
        return Err("Invalid pairing code".into());
    }
    let decoded = Zeroizing::new(
        B64.decode(code.strip_prefix("pcc1.").ok_or("Use a PCC pairing code")?)
            .map_err(|_| "Invalid pairing code")?,
    );
    let mut invite: PairCode =
        serde_json::from_slice(&decoded).map_err(|_| "Invalid pairing code")?;
    if invite.version != 1 || invite.port != address.port() {
        return Err("Pairing protocol or port differs from invitation".into());
    }
    let psk = Zeroizing::new(
        B64.decode(&invite.secret)
            .map_err(|_| "Invalid invitation secret")?,
    );
    invite.secret.zeroize();
    let psk: &[u8; 32] = psk
        .as_slice()
        .try_into()
        .map_err(|_| "Invalid invitation secret length")?;
    let expected = B64.decode(&invite.public_key).map_err(fail)?;
    if expected.len() != 32 {
        return Err("Invalid invitation identity".into());
    }
    let cancel = state
        .peer_sync
        .lock()
        .map_err(|_| "Sync unavailable")?
        .stop
        .clone()
        .ok_or("Start LAN sync first")?;
    let key = identity(&state)?;
    state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .set_setting("pair_verification", "")?;
    let work = async {
        let mut stream = tokio::time::timeout(Duration::from_secs(10), TcpStream::connect(address))
            .await
            .map_err(|_| "Peer connection timed out")?
            .map_err(fail)?;
        sync_wire::write_prelude(
            &mut stream,
            &Prelude {
                version: 1,
                mode: "pair".into(),
                id: invite.invitation,
            },
        )
        .await?;
        let noise = snow::Builder::new(PAIR_NOISE.parse().map_err(fail)?)
            .local_private_key(&key[..32])
            .map_err(fail)?
            .psk(3, psk)
            .map_err(fail)?
            .prologue(b"PCC peer protocol 1 pairing")
            .map_err(fail)?
            .build_initiator()
            .map_err(fail)?;
        let mut channel = sync_wire::handshake(stream, noise, true).await?;
        if channel.remote_key != expected {
            return Err("Invitation identity mismatch; do not approve this connection".into());
        }
        let verification = channel.verification.clone();
        channel.send(&hello(&state, scopes.clone(), None)?).await?;
        // Expose only the comparison code while waiting; no records are exchanged during pairing.
        state
            .repo
            .lock()
            .map_err(|_| "Database unavailable")?
            .set_setting("pair_verification", &verification)?;
        let reply: PairReply = channel.receive().await?;
        if !reply.accepted {
            return Err("Other device declined pairing".into());
        }
        let remote = reply.hello.ok_or("Missing peer identity")?;
        validate_hello(&remote)?;
        if remote.scopes.iter().any(|s| !scopes.contains(s)) {
            return Err("Peer expanded pairing scope".into());
        }
        store_trust(
            &state,
            &Trust {
                device: remote.device.clone(),
                label: remote.label.clone(),
                public_key: B64.encode(&expected),
                scopes: remote.scopes,
                endpoint: address.to_string(),
                foreground_only: remote.mobile || remote.port == 1,
                paired_at: command_center_core::now(),
            },
        )?;
        Ok(json!({"device":remote.device,"label":remote.label,"verification":verification}))
    };
    tokio::select! {_=cancel.cancelled()=>Err("Pairing cancelled".into()),result=work=>result}
}
#[tauri::command]
pub async fn sync_peer_now(
    app: tauri::AppHandle,
    peer_id: String,
    address: String,
) -> Result<usize> {
    let state = app.state::<AppState>();
    let address = local_address(&address, mesh_enabled(&state)?)?;
    let mut trust = trusted(&state, &peer_id)?;
    let cancel = {
        let mut rt = state.peer_sync.lock().map_err(|_| "Sync unavailable")?;
        if rt.busy {
            return Err("Another sync operation is active".into());
        }
        let cancel = rt.stop.clone().ok_or("Start LAN sync first")?;
        rt.busy = true;
        cancel
    };
    let work = async {
        let key = identity(&state)?;
        let remote = B64.decode(&trust.public_key).map_err(fail)?;
        let mut stream = tokio::time::timeout(Duration::from_secs(10), TcpStream::connect(address))
            .await
            .map_err(|_| "Peer connection timed out")?
            .map_err(fail)?;
        sync_wire::write_prelude(
            &mut stream,
            &Prelude {
                version: 1,
                mode: "sync".into(),
                id: fingerprint(&key[32..]),
            },
        )
        .await?;
        let noise = snow::Builder::new(SYNC_NOISE.parse().map_err(fail)?)
            .local_private_key(&key[..32])
            .map_err(fail)?
            .remote_public_key(&remote)
            .map_err(fail)?
            .prologue(b"PCC peer protocol 1 sync")
            .map_err(fail)?
            .build_initiator()
            .map_err(fail)?;
        let channel = sync_wire::handshake(stream, noise, true).await?;
        if channel.remote_key != remote {
            return Err("Pinned device key mismatch".into());
        }
        alive(&state, &trust)?;
        let changed = sync_initiator(&state, channel, &trust).await?;
        trust.endpoint = address.to_string();
        let (local, _) = own(&state)?;
        trust_entry(&local, &peer_id)?
            .set_password(&serde_json::to_string(&trust).map_err(fail)?)
            .map_err(fail)?;
        Ok(changed)
    };
    let result =
        tokio::select! {_=cancel.cancelled()=>Err("Sync cancelled".into()),result=work=>result};
    state.peer_sync.lock().map_err(|_| "Sync unavailable")?.busy = false;
    let _ = record_health(&state, &peer_id, &result);
    result
}
#[tauri::command]
pub fn revoke_peer(state: State<AppState>, peer_id: String) -> Result<()> {
    uuid::Uuid::parse_str(&peer_id).map_err(fail)?;
    let (local, _) = own(&state)?;
    let mut peers = peer_ids(&state)?;
    peers.retain(|id| id != &peer_id);
    // Remove the allowlist first: even a secure-store deletion failure revokes live authorization.
    {
        let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
        repo.set_setting(
            "paired_peers",
            &serde_json::to_string(&peers).map_err(fail)?,
        )?;
        repo.forget_peer_progress(&peer_id)?;
    }
    match trust_entry(&local, &peer_id)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("Device revoked; OS trust cleanup failed: {e}")),
    }
}
#[tauri::command]
pub fn resolve_peer_conflict(
    state: State<AppState>,
    id: String,
    choice: String,
    revision: i64,
    edited: Option<Value>,
) -> Result<()> {
    state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .resolve_sync_conflict(&id, &choice, revision, edited)
}
#[tauri::command]
pub fn set_automatic_sync(state: State<AppState>, enabled: bool) -> Result<()> {
    state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .set_setting(
            "automatic_peer_sync",
            if enabled { "true" } else { "false" },
        )
}
pub async fn poll(app: &tauri::AppHandle) -> Result<()> {
    let state = app.state::<AppState>();
    {
        let rt = state.peer_sync.lock().map_err(|_| "Sync unavailable")?;
        if rt.port.is_none() || rt.busy {
            return Ok(());
        }
    }
    if state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .setting("automatic_peer_sync")?
        .as_deref()
        != Some("true")
    {
        return Ok(());
    }
    let (local, _) = own(&state)?;
    for id in peer_ids(&state)? {
        if id < local {
            continue;
        }
        let trust = trusted(&state, &id)?;
        if trust.foreground_only {
            continue;
        }
        let key = fingerprint(&B64.decode(&trust.public_key).map_err(fail)?);
        let found = state
            .peer_sync
            .lock()
            .map_err(|_| "Sync unavailable")?
            .candidate_keys
            .get(&key)
            .and_then(|v| v.first())
            .cloned();
        let address = found.unwrap_or(trust.endpoint);
        if !address.is_empty() {
            let _ = sync_peer_now(app.clone(), id, address).await;
        }
    }
    Ok(())
}
/// Best effort: receipts prevent repeat delivery after a successful exchange; simultaneous offline alerts can duplicate.
pub fn reachable_notification_peers(state: &AppState) -> Result<Vec<String>> {
    if state
        .peer_sync
        .lock()
        .map_err(|_| "Sync unavailable")?
        .port
        .is_none()
    {
        return Ok(vec![]);
    }
    let mut peers = vec![];
    for id in peer_ids(state)? {
        let Ok(trust) = trusted(state, &id) else {
            continue;
        };
        if trust.foreground_only || !trust.scopes.iter().any(|s| s == "task") {
            continue;
        }
        let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
        let recent = repo
            .setting(&format!("sync_health:{id}"))?
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .is_some_and(|v| {
                v["status"] == "synced"
                    && v["at"]
                        .as_str()
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                        .is_some_and(|d| {
                            (chrono::Utc::now() - d.with_timezone(&chrono::Utc)).num_seconds() < 150
                        })
            });
        if recent {
            peers.push(id);
        }
    }
    Ok(peers)
}

/// Identity replacement is explicit and never restores trust from an archive.
#[tauri::command]
pub fn rotate_peer_identity(state: State<AppState>, confirmation: String) -> Result<()> {
    if confirmation != "ROTATE SYNC IDENTITY" {
        return Err(
            "Type ROTATE SYNC IDENTITY to revoke every local pairing and replace this device key"
                .into(),
        );
    }
    halt(&state)?;
    let (local, _) = own(&state)?;
    let peers = peer_ids(&state)?;
    {
        let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
        repo.set_setting("paired_peers", "[]")?;
        repo.set_setting("automatic_peer_sync", "false")?;
        for id in &peers {
            repo.forget_peer_progress(id)?;
        }
    }
    let mut cleanup = vec![];
    for id in peers {
        if let Err(e) = trust_entry(&local, &id)?.delete_credential() {
            if !matches!(e, keyring::Error::NoEntry) {
                cleanup.push(e.to_string());
            }
        }
    }
    let entry =
        keyring::Entry::new("com.personal.commandcenter.peer.identity", &local).map_err(fail)?;
    // Replace atomically in secure storage; ordinary records and causal identity stay intact.
    let mut pair = snow::Builder::new(SYNC_NOISE.parse().map_err(fail)?)
        .generate_keypair()
        .map_err(fail)?;
    let mut raw = Zeroizing::new(pair.private.clone());
    raw.extend_from_slice(&pair.public);
    pair.private.zeroize();
    entry
        .set_secret(&raw)
        .map_err(|e| format!("Pairings revoked; key replacement failed: {e}"))?;
    if !cleanup.is_empty() {
        return Err(format!(
            "Identity replaced and trust revoked; stale secure-store cleanup: {}",
            cleanup.join("; ")
        ));
    }
    Ok(())
}

pub(crate) fn check_live_peer(state: &AppState, peer: &str, pinned: &[u8]) -> Result<()> {
    let trust = trusted(state, peer)?;
    if B64.decode(&trust.public_key).map_err(fail)? != pinned {
        return Err("Peer key changed during private transfer".into());
    }
    alive(state, &trust)
}
#[tauri::command]
pub fn approve_private_transfer(state: State<AppState>, id: String, approved: bool) -> Result<()> {
    if approved {
        let _ = crate::vault::session_key(&state)?;
    }
    let pending = state
        .peer_sync
        .lock()
        .map_err(|_| "Sync unavailable")?
        .private_pending
        .remove(&id)
        .ok_or("Transfer approval expired")?;
    pending
        .reply
        .send(approved)
        .map_err(|_| "Transfer is no longer waiting".into())
}
#[tauri::command]
pub async fn send_private_category(
    state: State<'_, AppState>,
    peer_id: String,
    address: String,
    category: String,
) -> Result<usize> {
    let trust = trusted(&state, &peer_id)?;
    if trust.foreground_only {
        return Err("Private category transfers currently require two desktop apps".into());
    }
    let address = local_address(&address, mesh_enabled(&state)?)?;
    let raw = crate::private_transfer::collect(&state, &category)?;
    let cancel = {
        let mut rt = state.peer_sync.lock().map_err(|_| "Sync unavailable")?;
        if rt.busy {
            return Err("Another sync is active".into());
        }
        let cancel = rt.stop.clone().ok_or("Enable LAN sync first")?;
        rt.busy = true;
        cancel
    };
    let work = async {
        alive(&state, &trust)?;
        let key = identity(&state)?;
        let remote = B64.decode(&trust.public_key).map_err(fail)?;
        let mut stream = tokio::time::timeout(Duration::from_secs(10), TcpStream::connect(address))
            .await
            .map_err(|_| "Peer connection timed out")?
            .map_err(fail)?;
        sync_wire::write_prelude(
            &mut stream,
            &Prelude {
                version: 1,
                mode: "private-copy".into(),
                id: fingerprint(&key[32..]),
            },
        )
        .await?;
        let noise = snow::Builder::new(SYNC_NOISE.parse().map_err(fail)?)
            .local_private_key(&key[..32])
            .map_err(fail)?
            .remote_public_key(&remote)
            .map_err(fail)?
            .prologue(b"PCC peer protocol 1 private-copy")
            .map_err(fail)?
            .build_initiator()
            .map_err(fail)?;
        let mut channel = sync_wire::handshake(stream, noise, true).await?;
        if channel.remote_key != remote {
            return Err("Pinned device key mismatch".into());
        }
        alive(&state, &trust)?;
        crate::private_transfer::send(&state, &peer_id, &mut channel, category, raw).await
    };
    let result = tokio::select! {_=cancel.cancelled()=>Err("Private transfer cancelled".into()),result=work=>result};
    state.peer_sync.lock().map_err(|_| "Sync unavailable")?.busy = false;
    result
}

fn mesh_enabled(state: &AppState) -> Result<bool> {
    Ok(state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .setting("allow_private_mesh")?
        .as_deref()
        == Some("true"))
}
#[tauri::command]
pub fn set_private_mesh(state: State<AppState>, enabled: bool) -> Result<()> {
    halt(&state)?;
    state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .set_setting("allow_private_mesh", if enabled { "true" } else { "false" })
}

#[tauri::command]
pub async fn export_sync_diagnostics(state: State<'_, AppState>) -> Result<Option<String>> {
    // Deliberately omit labels, addresses, public keys, invitation secrets and record contents.
    let data = {
        let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
        let peers = repo.peer_status()?;
        json!({"format":"pcc-sync-diagnostics","version":1,"created_at":command_center_core::now(),"app_version":env!("CARGO_PKG_VERSION"),"device_hash":fingerprint(repo.device_id.as_bytes()),"peers":peers.iter().map(|p|json!({"peer_hash":fingerprint(p["device"].as_str().unwrap_or("").as_bytes()),"sent":p["sent"],"received":p["received"],"pending":p["pending"],"last_exchange":p["last_exchange"]})).collect::<Vec<_>>(),"unresolved_conflicts":repo.sync_conflicts()?.len(),"runtime_verified":false})
    };
    let Some(file) = rfd::AsyncFileDialog::new()
        .set_title("Export content-free sync diagnostics")
        .set_file_name("command-center-sync-diagnostics.json")
        .save_file()
        .await
    else {
        return Ok(None);
    };
    crate::atomic_write(
        file.path(),
        &serde_json::to_vec_pretty(&data).map_err(fail)?,
    )?;
    Ok(Some(file.path().display().to_string()))
}

fn save_receipts(state: &AppState, trust: &Trust, receipts: &[String]) -> Result<()> {
    if !trust.scopes.iter().any(|s| s == "task") {
        return Ok(());
    }
    state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .set_setting(
            &format!("peer_deliveries:{}", trust.device),
            &serde_json::to_string(receipts).map_err(fail)?,
        )
}
