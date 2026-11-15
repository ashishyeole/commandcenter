//! Shared mobile storage and Noise initiator. Platform wrappers keep keys and trust in secure storage.
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
use command_center_core::{
    sync::{valid_scope, Batch},
    workspace::{PortableWorkspace, Record},
    Item, Repository,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    ffi::{c_char, CStr, CString},
    net::{IpAddr, SocketAddr},
    time::Duration,
};
use zeroize::{Zeroize, Zeroizing};
type Result<T> = std::result::Result<T, String>;
mod listener;
mod mobile_vault;
#[path = "../../../apps/desktop/src-tauri/src/sync_wire.rs"]
mod sync_wire;
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Trust {
    device: String,
    label: String,
    public_key: String,
    scopes: Vec<String>,
    endpoint: String,
    paired_at: String,
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
struct PairCode {
    version: u32,
    invitation: String,
    secret: String,
    public_key: String,
    port: u16,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Round {
    ack: i64,
    batch: Batch,
}
fn address(s: &str, allow_mesh: bool) -> Result<SocketAddr> {
    let a: SocketAddr = s
        .parse()
        .map_err(|_| "Enter a numeric private IPv4 address and port")?;
    if a.port() == 0
        || !matches!(a.ip(),IpAddr::V4(ip)if ip.is_private()||ip.is_loopback()||ip.is_link_local()||(allow_mesh&&(u32::from(ip)&0xffc00000)==0x64400000))
    {
        return Err("Only private IPv4 LAN addresses are enabled".into());
    }
    Ok(a)
}
fn scopes(v: &Value) -> Result<Vec<String>> {
    let s: Vec<String> = serde_json::from_value(v.clone()).map_err(err)?;
    if s.is_empty() || s.len() > 32 || s.iter().any(|s| !valid_scope(s)) {
        return Err("Invalid ordinary-data scope".into());
    }
    Ok(s)
}
fn snapshot(repo: &mut Repository) -> Result<Value> {
    repo.activate_conditions()?;
    repo.prepare_sync()?;
    Ok(
        json!({"peer_status":repo.peer_status()?,"items":repo.list("",false)?,"records":repo.records("",false)?,"conflicts":repo.sync_conflicts()?,"feeds":repo.feeds()?,"verification":repo.setting("pair_verification")?}),
    )
}
fn strarg<'a>(request: &'a Value, key: &str) -> Result<&'a str> {
    request[key]
        .as_str()
        .ok_or_else(|| format!("Missing {key}"))
}
async fn pair(repo: &mut Repository, request: &Value) -> Result<Value> {
    let a = address(
        strarg(request, "address")?,
        request["allow_mesh"].as_bool().unwrap_or(false),
    )?;
    let code = strarg(request, "code")?;
    if code.len() > 2048 {
        return Err("Invalid pairing code".into());
    }
    let bytes = Zeroizing::new(
        B64.decode(
            code.strip_prefix("pcc1.")
                .ok_or("Use a PCC pairing invitation")?,
        )
        .map_err(err)?,
    );
    let mut invite: PairCode = serde_json::from_slice(&bytes).map_err(err)?;
    if invite.version != 1 || invite.port != a.port() {
        return Err("Invitation port or protocol mismatch".into());
    }
    let secret = Zeroizing::new(B64.decode(&invite.secret).map_err(err)?);
    invite.secret.zeroize();
    let psk: &[u8; 32] = secret
        .as_slice()
        .try_into()
        .map_err(|_| "Invalid invitation secret")?;
    let key = Zeroizing::new(B64.decode(strarg(request, "identity")?).map_err(err)?);
    if key.len() != 64 {
        return Err("Invalid local identity")?;
    }
    let remote = B64.decode(&invite.public_key).map_err(err)?;
    if remote.len() != 32 {
        return Err("Invalid remote identity".into());
    }
    let selected = scopes(&request["scopes"])?;
    let label = strarg(request, "label")?;
    if label.is_empty() || label.len() > 80 {
        return Err("Use a short device name".into());
    }
    let mut stream =
        tokio::time::timeout(Duration::from_secs(10), tokio::net::TcpStream::connect(a))
            .await
            .map_err(|_| "Peer unreachable")?
            .map_err(err)?;
    sync_wire::write_prelude(
        &mut stream,
        &json!({"version":1,"mode":"pair","id":invite.invitation}),
    )
    .await?;
    let noise = snow::Builder::new(
        "Noise_XXpsk3_25519_ChaChaPoly_SHA256"
            .parse()
            .map_err(err)?,
    )
    .local_private_key(&key[..32])
    .map_err(err)?
    .psk(3, psk)
    .map_err(err)?
    .prologue(b"PCC peer protocol 1 pairing")
    .map_err(err)?
    .build_initiator()
    .map_err(err)?;
    let mut channel = sync_wire::handshake(stream, noise, true).await?;
    if channel.remote_key != remote {
        return Err("Inviting device key mismatch".into());
    }
    repo.set_setting("pair_verification", &channel.verification)?;
    // Advertise the optional foreground receiver; port 1 means no listener is active.
    channel
        .send(&Hello {
            mobile: true,
            deliveries: vec![],
            device: repo.device_id.clone(),
            label: label.into(),
            scopes: selected.clone(),
            received: 0,
            port: listener::port(),
        })
        .await?;
    let reply: PairReply = channel.receive().await?;
    if !reply.accepted {
        return Err("Pairing was declined".into());
    }
    let peer = reply.hello.ok_or("Missing peer greeting")?;
    uuid::Uuid::parse_str(&peer.device).map_err(err)?;
    if peer.device == repo.device_id
        || peer.label.len() > 80
        || peer.scopes.is_empty()
        || peer.scopes.iter().any(|s| !selected.contains(s))
    {
        return Err("Invalid peer identity or expanded scope".into());
    }
    repo.reset_peer_progress(&peer.device)?;
    Ok(
        json!({"trust":Trust{device:peer.device,label:peer.label,public_key:B64.encode(&remote),scopes:peer.scopes,endpoint:a.to_string(),paired_at:command_center_core::now()},"verification":channel.verification}),
    )
}
async fn sync(repo: &mut Repository, request: &Value) -> Result<Value> {
    let trust: Trust = serde_json::from_value(request["trust"].clone()).map_err(err)?;
    uuid::Uuid::parse_str(&trust.device).map_err(err)?;
    scopes(&json!(trust.scopes))?;
    let a = address(
        strarg(request, "address")?,
        request["allow_mesh"].as_bool().unwrap_or(false),
    )?;
    let key = Zeroizing::new(B64.decode(strarg(request, "identity")?).map_err(err)?);
    let remote = B64.decode(&trust.public_key).map_err(err)?;
    if key.len() != 64 || remote.len() != 32 {
        return Err("Invalid secure-store identity".into());
    }
    let mut stream =
        tokio::time::timeout(Duration::from_secs(10), tokio::net::TcpStream::connect(a))
            .await
            .map_err(|_| "Peer connection timed out")?
            .map_err(err)?;
    sync_wire::write_prelude(
        &mut stream,
        &json!({"version":1,"mode":"sync","id":format!("{:x}",Sha256::digest(&key[32..]))}),
    )
    .await?;
    let noise = snow::Builder::new("Noise_KK_25519_ChaChaPoly_SHA256".parse().map_err(err)?)
        .local_private_key(&key[..32])
        .map_err(err)?
        .remote_public_key(&remote)
        .map_err(err)?
        .prologue(b"PCC peer protocol 1 sync")
        .map_err(err)?
        .build_initiator()
        .map_err(err)?;
    let mut channel = sync_wire::handshake(stream, noise, true).await?;
    if channel.remote_key != remote {
        return Err("Pinned peer key mismatch".into());
    }
    channel
        .send(&Hello {
            mobile: true,
            deliveries: vec![],
            device: repo.device_id.clone(),
            label: strarg(request, "label")?.into(),
            scopes: trust.scopes.clone(),
            received: repo.sync_progress(&trust.device)?.1,
            port: listener::port(),
        })
        .await?;
    let hello: Hello = channel.receive().await?;
    if hello.device != trust.device || hello.scopes != trust.scopes {
        return Err("Peer identity or scope changed; re-pair explicitly".into());
    }
    let mut after = hello.received;
    let mut changed = 0;
    for _ in 0..32 {
        let batch = repo.sync_batch(after, &trust.scopes)?;
        let more = batch.more;
        let sent = batch.through;
        channel.send(&batch).await?;
        let response: Round = channel.receive().await?;
        if response.ack != sent {
            return Err("Invalid peer acknowledgement".into());
        }
        changed += repo.apply_sync_batch(&trust.device, &response.batch, &trust.scopes)?;
        repo.sync_ack(&trust.device, sent)?;
        channel
            .send(&json!({"through":response.batch.through}))
            .await?;
        after = sent;
        if !more && !response.batch.more {
            break;
        }
    }
    Ok(json!({"changed":changed,"snapshot":snapshot(repo)?}))
}
fn dispatch(request: Value) -> Result<Value> {
    let operation = strarg(&request, "operation")?;
    if operation == "keygen" {
        let mut pair = snow::Builder::new("Noise_KK_25519_ChaChaPoly_SHA256".parse().map_err(err)?)
            .generate_keypair()
            .map_err(err)?;
        let mut raw = Zeroizing::new(pair.private.clone());
        raw.extend(&pair.public);
        pair.private.zeroize();
        return Ok(json!({"identity":B64.encode(raw.as_slice())}));
    }
    match operation {
        "listener_stop" => return listener::stop(),
        "listener_status" => return listener::status(),
        "listener_approve" => return listener::approve(&request),
        "listener_refresh" => return listener::refresh(&request),
        _ => {}
    }
    let path = strarg(&request, "path")?;
    let device = strarg(&request, "device")?;
    uuid::Uuid::parse_str(device).map_err(err)?;
    let mut repo = Repository::open(path)?;
    if let Some(owner) = repo.setting("mobile_owner")? {
        if owner != device {
            return Err("Device identity changed; restore into a fresh local database".into());
        }
    } else {
        repo.set_setting("mobile_owner", device)?;
        repo.set_setting("device_id", device)?;
    }
    repo.device_id = device.into();
    if operation.starts_with("vault_") {
        return mobile_vault::dispatch(&mut repo, operation, &request);
    }
    match operation {
        "listener_start" => listener::start(&request),
        "initialize" => {
            if repo.setting("legacy_migrated")?.is_none() {
                let items: Vec<Item> =
                    serde_json::from_value(request["legacy_items"].clone()).map_err(err)?;
                repo.import_workspace(&items, &PortableWorkspace::default())?;
                repo.set_setting("legacy_migrated", "true")?;
            }
            snapshot(&mut repo)
        }
        "snapshot" => snapshot(&mut repo),
        "attachments" => Ok(
            json!({"rows":repo.attachment_summaries(None,request["query"].as_str().unwrap_or(""),false)?}),
        ),
        "attachment" => {
            let a = repo.attachment(strarg(&request, "id")?)?;
            if a.deleted_at.is_some() {
                return Err("Attachment is in Trash".into());
            }
            a.validate()?;
            serde_json::to_value(a).map_err(err)
        }
        "save" => {
            let item: Item = serde_json::from_value(request["item"].clone()).map_err(err)?;
            repo.save(item)?;
            snapshot(&mut repo)
        }
        "complete" => {
            repo.complete(strarg(&request, "id")?)?;
            snapshot(&mut repo)
        }
        "save_record" => {
            let record: Record = serde_json::from_value(request["record"].clone()).map_err(err)?;
            repo.save_record(record)?;
            snapshot(&mut repo)
        }
        "cache" => {
            let source = strarg(&request, "source")?;
            if !["github", "news"].contains(&source) {
                return Err("Unsupported mobile feed".into());
            }
            if serde_json::to_vec(&request["items"]).map_err(err)?.len() > 2 * 1024 * 1024 {
                return Err("Feed too large".into());
            }
            repo.cache_feed(source, &request["items"])?;
            snapshot(&mut repo)
        }
        "export" => Ok(
            json!({"format":"personal-command-center-mobile","schema":1,"peer_status":repo.peer_status()?,"items":repo.list("",false)?}),
        ),
        "import" => {
            let envelope = &request["exchange"];
            if envelope["format"] != "personal-command-center-mobile" || envelope["schema"] != 1 {
                return Err("Unsupported exchange")?;
            }
            let items: Vec<Item> =
                serde_json::from_value(envelope["items"].clone()).map_err(err)?;
            if items.len() > 10000 {
                return Err("Too many imported records".into());
            }
            let mut ids = std::collections::HashSet::new();
            for i in &items {
                if !ids.insert(&i.id) {
                    return Err("Duplicate import ID".into());
                }
            }
            let n = repo.import_workspace(&items, &PortableWorkspace::default())?;
            Ok(json!({"imported":n,"snapshot":snapshot(&mut repo)?}))
        }
        "resolve" => {
            repo.resolve_sync_conflict(
                strarg(&request, "id")?,
                strarg(&request, "choice")?,
                request["revision"].as_i64().ok_or("Missing revision")?,
                request.get("edited").filter(|v| !v.is_null()).cloned(),
            )?;
            snapshot(&mut repo)
        }
        "forget" => {
            repo.forget_peer_progress(strarg(&request, "peer")?)?;
            Ok(json!({"revoked":true}))
        }
        "pair" | "sync" => {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(err)?;
            runtime.block_on(async {
                if operation == "pair" {
                    pair(&mut repo, &request).await
                } else {
                    sync(&mut repo, &request).await
                }
            })
        }
        _ => Err("Unsupported native operation".into()),
    }
}
fn request_json(raw: &str) -> String {
    let result = std::panic::catch_unwind(|| {
        if raw.len() > 40 * 1024 * 1024 {
            return Err("Native request exceeds limit".into());
        }
        dispatch(serde_json::from_str(raw).map_err(err)?)
    })
    .unwrap_or_else(|_| Err("Native operation failed safely".into()));
    match result {
        Ok(value) => json!({"ok":true,"value":value}).to_string(),
        Err(error) => json!({"ok":false,"error":error}).to_string(),
    }
}
/// Input and output are UTF-8 JSON. Caller owns the returned allocation until pcc_mobile_free.
#[no_mangle]
pub unsafe extern "C" fn pcc_mobile_request(input: *const c_char) -> *mut c_char {
    if input.is_null() {
        return std::ptr::null_mut();
    }
    let raw = match CStr::from_ptr(input).to_str() {
        Ok(v) => v,
        Err(_) => return std::ptr::null_mut(),
    };
    CString::new(request_json(raw))
        .map(CString::into_raw)
        .unwrap_or(std::ptr::null_mut())
}
#[no_mangle]
pub unsafe extern "C" fn pcc_mobile_free(output: *mut c_char) {
    if !output.is_null() {
        drop(CString::from_raw(output));
    }
}
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_com_personal_commandcenter_MobilePeerNative_request(
    mut env: jni::JNIEnv,
    _object: jni::objects::JObject,
    input: jni::objects::JString,
) -> jni::sys::jstring {
    let raw: std::result::Result<String, _> = env.get_string(&input).map(|s| s.into());
    let output = raw
        .map(|s| request_json(&s))
        .unwrap_or_else(|_| json!({"ok":false,"error":"Invalid UTF-8 request"}).to_string());
    env.new_string(output)
        .map(|s| s.into_raw())
        .unwrap_or(std::ptr::null_mut())
}
