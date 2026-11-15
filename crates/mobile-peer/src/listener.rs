//! Foreground mobile receiver. Platform lifecycle callbacks explicitly stop it.
use super::*;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, LazyLock, Mutex,
};
use std::time::Instant;
use tokio::sync::{oneshot, Semaphore};
static ACTIVE: LazyLock<Mutex<Option<Arc<Server>>>> = LazyLock::new(|| Mutex::new(None));
struct Pending {
    id: String,
    trust: Trust,
    verification: String,
    reply: oneshot::Sender<bool>,
}
struct Server {
    path: String,
    device: String,
    label: String,
    port: u16,
    key: Zeroizing<Vec<u8>>,
    scopes: Vec<String>,
    mesh: bool,
    stop: AtomicBool,
    trust: Mutex<Vec<Trust>>,
    invitation: Mutex<Option<(String, Zeroizing<Vec<u8>>, Instant)>>,
    code: Mutex<Option<String>>,
    pending: Mutex<Option<Pending>>,
    health: Mutex<String>,
}
fn active() -> Result<Arc<Server>> {
    ACTIVE
        .lock()
        .map_err(|_| "Listener unavailable")?
        .clone()
        .ok_or("Listener is stopped".into())
}
pub fn port() -> u16 {
    ACTIVE
        .lock()
        .ok()
        .and_then(|s| {
            s.as_ref()
                .filter(|s| !s.stop.load(Ordering::SeqCst))
                .map(|s| s.port)
        })
        .unwrap_or(1)
}
pub fn stop() -> Result<Value> {
    if let Some(s) = ACTIVE.lock().map_err(|_| "Listener unavailable")?.take() {
        s.stop.store(true, Ordering::SeqCst);
        s.pending.lock().map_err(|_| "Listener unavailable")?.take();
    }
    Ok(json!({"running":false}))
}
pub fn status() -> Result<Value> {
    let Ok(s) = active() else {
        return Ok(json!({"running":false}));
    };
    {
        let mut invitation = s.invitation.lock().map_err(|_| "Listener unavailable")?;
        if invitation
            .as_ref()
            .is_some_and(|(_, _, expires)| *expires <= Instant::now())
        {
            invitation.take();
            s.code.lock().map_err(|_| "Listener unavailable")?.take();
        }
    }
    let pending = s.pending.lock().map_err(|_| "Listener unavailable")?;
    Ok(
        json!({"running":!s.stop.load(Ordering::SeqCst),"port":s.port,"code":*s.code.lock().map_err(|_|"Listener unavailable")?,"health":*s.health.lock().map_err(|_|"Listener unavailable")?,"pending":pending.as_ref().map(|p|json!({"id":p.id,"trust":p.trust,"verification":p.verification}))}),
    )
}
pub fn approve(request: &Value) -> Result<Value> {
    let s = active()?;
    let mut pending = s.pending.lock().map_err(|_| "Listener unavailable")?;
    if pending
        .as_ref()
        .is_none_or(|p| Some(p.id.as_str()) != request["id"].as_str())
    {
        return Err("Pairing request expired".into());
    }
    let p = pending.take().ok_or("Pairing request expired")?;
    p.reply
        .send(request["approved"].as_bool().unwrap_or(false))
        .map_err(|_| "Pairing connection closed")?;
    Ok(json!({"accepted":true}))
}
pub fn start(request: &Value) -> Result<Value> {
    stop()?;
    let key = Zeroizing::new(B64.decode(strarg(request, "identity")?).map_err(err)?);
    if key.len() != 64 {
        return Err("Invalid device identity".into());
    }
    let selected = scopes(&request["scopes"])?;
    let trust: Vec<Trust> = serde_json::from_value(request["trusts"].clone()).map_err(err)?;
    if trust.len() > 8 {
        return Err("At most eight trusted peers".into());
    }
    for t in &trust {
        uuid::Uuid::parse_str(&t.device).map_err(err)?;
        scopes(&json!(t.scopes))?;
        if B64.decode(&t.public_key).map_err(err)?.len() != 32 {
            return Err("Invalid peer key".into());
        }
    }
    let label = strarg(request, "label")?;
    if label.is_empty() || label.len() > 80 {
        return Err("Use a short device name".into());
    }
    let listener = std::net::TcpListener::bind("0.0.0.0:0").map_err(err)?;
    listener.set_nonblocking(true).map_err(err)?;
    let port = listener.local_addr().map_err(err)?.port();
    let mut pair = snow::Builder::new("Noise_KK_25519_ChaChaPoly_SHA256".parse().map_err(err)?)
        .generate_keypair()
        .map_err(err)?;
    let secret = Zeroizing::new(pair.private.clone());
    pair.private.zeroize();
    let invitation = uuid::Uuid::new_v4().to_string();
    let code = format!(
        "pcc1.{}",
        B64.encode(
            serde_json::to_vec(&PairCode {
                version: 1,
                invitation: invitation.clone(),
                secret: B64.encode(&*secret),
                public_key: B64.encode(&key[32..]),
                port
            })
            .map_err(err)?
        )
    );
    let s = Arc::new(Server {
        path: strarg(request, "path")?.into(),
        device: strarg(request, "device")?.into(),
        label: label.into(),
        port,
        key,
        scopes: selected,
        mesh: request["allow_mesh"].as_bool().unwrap_or(false),
        stop: AtomicBool::new(false),
        trust: Mutex::new(trust),
        invitation: Mutex::new(Some((
            invitation,
            secret,
            Instant::now() + Duration::from_secs(300),
        ))),
        code: Mutex::new(Some(code)),
        pending: Mutex::new(None),
        health: Mutex::new("Waiting for a trusted connection".into()),
    });
    *ACTIVE.lock().map_err(|_| "Listener unavailable")? = Some(s.clone());
    std::thread::spawn(move || {
        let result = (|| -> Result<()> {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(err)?;
            rt.block_on(async{
            let listener=tokio::net::TcpListener::from_std(listener).map_err(err)?;let slots=Arc::new(Semaphore::new(2));
            while !s.stop.load(Ordering::SeqCst){tokio::select!{
                _=tokio::time::sleep(Duration::from_millis(250))=>{},
                accepted=listener.accept()=>{let (stream,address)=accepted.map_err(err)?;let Ok(permit)=slots.clone().try_acquire_owned()else{continue;};let s=s.clone();tokio::spawn(async move{let _permit=permit;let cancellation=async{while !s.stop.load(Ordering::SeqCst){tokio::time::sleep(Duration::from_millis(100)).await;}};
                    let result=tokio::select!{_=cancellation=>Err("Listener stopped".into()),result=tokio::time::timeout(Duration::from_secs(600),receive(&s,stream,address))=>result.map_err(|_|"Session timed out".to_string()).and_then(|v|v)};
                    if let Ok(mut health)=s.health.lock(){*health=match result{Ok(n)=>format!("Committed {n} changes at {}",command_center_core::now()),Err(e)=>e};}
                });}
            }}Ok(())
        })
        })();
        if let Err(e) = result {
            if let Ok(mut h) = s.health.lock() {
                *h = e;
            }
        }
        s.stop.store(true, Ordering::SeqCst);
    });
    status()
}
fn repo(s: &Server) -> Result<Repository> {
    let mut r = Repository::open(&s.path)?;
    if r.setting("mobile_owner")?.as_deref() != Some(&s.device) {
        return Err("Database ownership changed".into());
    }
    r.device_id = s.device.clone();
    Ok(r)
}
fn hello(s: &Server, scopes: Vec<String>, received: i64) -> Hello {
    Hello {
        mobile: true,
        deliveries: vec![],
        device: s.device.clone(),
        label: s.label.clone(),
        scopes,
        received,
        port: s.port,
    }
}
fn live(s: &Server, trust: &Trust) -> Result<()> {
    if s.stop.load(Ordering::SeqCst)
        || !s
            .trust
            .lock()
            .map_err(|_| "Trust unavailable")?
            .iter()
            .any(|t| {
                t.device == trust.device
                    && t.public_key == trust.public_key
                    && t.scopes == trust.scopes
            })
    {
        return Err("Listener stopped or trust revoked".into());
    }
    Ok(())
}
async fn receive(
    s: &Arc<Server>,
    mut stream: tokio::net::TcpStream,
    remote_address: SocketAddr,
) -> Result<usize> {
    address(&remote_address.to_string(), s.mesh)?;
    let prelude: Value =
        tokio::time::timeout(Duration::from_secs(5), sync_wire::read_prelude(&mut stream))
            .await
            .map_err(|_| "Prelude timed out")??;
    if prelude["version"] != 1 {
        return Err("Unsupported protocol".into());
    }
    if prelude["mode"] == "pair" {
        let (id, secret) = {
            let invitation = s.invitation.lock().map_err(|_| "Invitation unavailable")?;
            let (id, secret, expires) = invitation
                .as_ref()
                .filter(|(id, _, e)| {
                    Some(id.as_str()) == prelude["id"].as_str() && *e > Instant::now()
                })
                .ok_or("No active invitation")?;
            let _ = expires;
            (id.clone(), secret.clone())
        };
        let psk: &[u8; 32] = secret.as_slice().try_into().map_err(|_| "Invalid secret")?;
        let noise = snow::Builder::new(
            "Noise_XXpsk3_25519_ChaChaPoly_SHA256"
                .parse()
                .map_err(err)?,
        )
        .local_private_key(&s.key[..32])
        .map_err(err)?
        .psk(3, psk)
        .map_err(err)?
        .prologue(b"PCC peer protocol 1 pairing")
        .map_err(err)?
        .build_responder()
        .map_err(err)?;
        let mut channel = sync_wire::handshake(stream, noise, false).await?;
        let candidate: Hello = channel.receive().await?;
        uuid::Uuid::parse_str(&candidate.device).map_err(err)?;
        if candidate.device == s.device
            || candidate.label.is_empty()
            || candidate.label.len() > 80
            || candidate.port == 0
        {
            return Err("Invalid device greeting".into());
        }
        scopes(&json!(candidate.scopes))?;
        let selected: Vec<String> = s
            .scopes
            .iter()
            .filter(|c| candidate.scopes.contains(c))
            .cloned()
            .collect();
        scopes(&json!(selected))?;
        let trust = Trust {
            device: candidate.device,
            label: candidate.label,
            public_key: B64.encode(&channel.remote_key),
            scopes: selected.clone(),
            endpoint: SocketAddr::new(remote_address.ip(), candidate.port).to_string(),
            paired_at: command_center_core::now(),
        };
        {
            let peers = s.trust.lock().map_err(|_| "Trust unavailable")?;
            if peers.len() >= 8 && !peers.iter().any(|t| t.device == trust.device) {
                return Err("Peer limit reached".into());
            }
            if peers
                .iter()
                .any(|t| t.device == trust.device && t.public_key != trust.public_key)
            {
                return Err("Revoke old peer key before pairing again".into());
            }
        }
        {
            let mut invitation = s.invitation.lock().map_err(|_| "Invitation unavailable")?;
            if invitation
                .as_ref()
                .is_none_or(|(v, _, e)| *v != id || *e <= Instant::now())
            {
                return Err("Invitation expired or used".into());
            }
            invitation.take();
        }
        s.code.lock().map_err(|_| "Listener unavailable")?.take();
        let approval = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();
        {
            let mut pending = s.pending.lock().map_err(|_| "Approval unavailable")?;
            if pending.is_some() {
                return Err("Another pairing is waiting".into());
            }
            *pending = Some(Pending {
                id: approval,
                trust: trust.clone(),
                verification: channel.verification.clone(),
                reply: tx,
            });
        }
        let approved = tokio::time::timeout(Duration::from_secs(45), rx)
            .await
            .ok()
            .and_then(|v| v.ok())
            .unwrap_or(false);
        s.pending.lock().map_err(|_| "Approval unavailable")?.take();
        if !approved {
            channel
                .send(&PairReply {
                    accepted: false,
                    hello: None,
                })
                .await?;
            return Err("Pairing declined or expired".into());
        }
        if s.stop.load(Ordering::SeqCst) {
            return Err("Listener stopped".into());
        }
        {
            let mut peers = s.trust.lock().map_err(|_| "Trust unavailable")?;
            peers.retain(|t| t.device != trust.device);
            peers.push(trust.clone());
        }
        repo(s)?.reset_peer_progress(&trust.device)?;
        channel
            .send(&PairReply {
                accepted: true,
                hello: Some(hello(s, selected, 0)),
            })
            .await?;
        Ok(0)
    } else if prelude["mode"] == "sync" {
        let trust = s
            .trust
            .lock()
            .map_err(|_| "Trust unavailable")?
            .iter()
            .find(|t| {
                B64.decode(&t.public_key).is_ok_and(|k| {
                    Some(format!("{:x}", Sha256::digest(k)).as_str()) == prelude["id"].as_str()
                })
            })
            .cloned()
            .ok_or("Unknown peer key")?;
        let remote = B64.decode(&trust.public_key).map_err(err)?;
        let noise = snow::Builder::new("Noise_KK_25519_ChaChaPoly_SHA256".parse().map_err(err)?)
            .local_private_key(&s.key[..32])
            .map_err(err)?
            .remote_public_key(&remote)
            .map_err(err)?
            .prologue(b"PCC peer protocol 1 sync")
            .map_err(err)?
            .build_responder()
            .map_err(err)?;
        let mut channel = sync_wire::handshake(stream, noise, false).await?;
        if channel.remote_key != remote {
            return Err("Pinned key mismatch".into());
        }
        let request: Hello = channel.receive().await?;
        if request.device != trust.device || request.scopes != trust.scopes || request.received < 0
        {
            return Err("Identity or scope mismatch".into());
        }
        live(s, &trust)?;
        let mut r = repo(s)?;
        let mut cursor = request.received;
        channel
            .send(&hello(
                s,
                trust.scopes.clone(),
                r.sync_progress(&trust.device)?.1,
            ))
            .await?;
        let mut changed = 0;
        for _ in 0..32 {
            let batch: Batch = channel.receive().await?;
            live(s, &trust)?;
            changed += r.apply_sync_batch(&trust.device, &batch, &trust.scopes)?;
            let outgoing = r.sync_batch(cursor, &trust.scopes)?;
            let done = !batch.more && !outgoing.more;
            let through = outgoing.through;
            channel
                .send(&Round {
                    ack: batch.through,
                    batch: outgoing,
                })
                .await?;
            let ack: Value = channel.receive().await?;
            if ack["through"].as_i64() != Some(through) {
                return Err("Invalid acknowledgement".into());
            }
            live(s, &trust)?;
            r.sync_ack(&trust.device, through)?;
            cursor = through;
            if done {
                break;
            }
        }
        Ok(changed)
    } else {
        Err("Unsupported peer operation".into())
    }
}

pub fn refresh(request: &Value) -> Result<Value> {
    let Ok(s) = active() else {
        return Ok(json!({"running":false}));
    };
    let peers: Vec<Trust> = serde_json::from_value(request["trusts"].clone()).map_err(err)?;
    if peers.len() > 8 {
        return Err("At most eight peers".into());
    }
    let mut ids = std::collections::BTreeSet::new();
    for peer in &peers {
        uuid::Uuid::parse_str(&peer.device).map_err(err)?;
        scopes(&json!(peer.scopes))?;
        if peer.device == s.device
            || !ids.insert(peer.device.clone())
            || B64.decode(&peer.public_key).map_err(err)?.len() != 32
        {
            return Err("Invalid trust identity".into());
        }
    }
    *s.trust.lock().map_err(|_| "Trust unavailable")? = peers;
    Ok(json!({"updated":true}))
}
