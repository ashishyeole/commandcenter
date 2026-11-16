//! Declarative extension packages. No renderer scripts, host shell or automatic execution.
use crate::{AppState, Result};
use command_center_core::workspace::Record;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tauri::State;
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: u32,
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub publisher: String,
    pub license: String,
    pub kind: String,
    pub endpoint: String,
    pub method: String,
    pub image: String,
    pub command: Vec<String>,
    #[serde(default)]
    pub tools: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Installed {
    pub manifest: Manifest,
    pub sha256: String,
    pub publisher_fingerprint: Option<String>,
    pub enabled: bool,
}
fn hash(v: &Value) -> Result<String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(v).map_err(|e| e.to_string())?)
    ))
}
fn validate(m: &Manifest) -> Result<()> {
    if m.schema != 1
        || m.id.is_empty()
        || m.id.len() > 100
        || !m
            .id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        || m.name.trim().is_empty()
        || m.name.len() > 100
        || m.version.is_empty()
        || m.version.len() > 80
        || m.publisher.len() > 200
        || m.license.len() > 100
        || m.description.len() > 2000
    {
        return Err("Invalid extension metadata".into());
    }
    if m.tools.len() > 50
        || m.tools.iter().any(|s| {
            s.is_empty()
                || s.len() > 128
                || !s
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        })
        || (m.kind != "mcp-container" && !m.tools.is_empty())
    {
        return Err("Only MCP containers may declare up to 50 explicit tool names".into());
    }
    match m.kind.as_str(){
 "rest"=>{let u=reqwest::Url::parse(&m.endpoint).map_err(|_|"Invalid extension endpoint")?;if u.scheme()!="https"||u.host_str().is_none()||!u.username().is_empty()||u.password().is_some()||u.query().is_some()||u.fragment().is_some()||m.endpoint.len()>2000||!["GET","POST"].contains(&m.method.as_str())||!m.image.is_empty()||!m.command.is_empty(){return Err("REST extensions require a fixed public HTTPS endpoint and GET or POST, without URL credentials/query".into());}},
 "container"|"mcp-container"=>{let parts=m.image.split("@sha256:").collect::<Vec<_>>();if parts.len()!=2||parts[0].is_empty()||parts[0].starts_with('-')||!parts[0].bytes().all(|b|b.is_ascii_alphanumeric()||b"./:_-".contains(&b))||parts[1].len()!=64||!parts[1].bytes().all(|b|b.is_ascii_hexdigit())||m.image.len()>300||m.command.is_empty()||m.command.len()>20||m.command.iter().any(|s|s.is_empty()||s.len()>2000||s.contains('\0'))||!m.endpoint.is_empty()||!m.method.is_empty(){return Err("Container extensions require an immutable image@sha256 digest and a bounded command array".into());}},
 _=>return Err("Supported extension kinds: rest, container, mcp-container. Remote MCP has its dedicated approved connection registry.".into())};
    Ok(())
}
fn list(state: &AppState) -> Result<Vec<Installed>> {
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    serde_json::from_str(&repo.setting("installed_extensions")?.unwrap_or("[]".into()))
        .map_err(|_| "Invalid extension registry".into())
}
#[tauri::command]
pub fn list_extensions(state: State<AppState>) -> Result<Vec<Installed>> {
    list(&state)
}
#[tauri::command]
pub fn install_extension(
    state: State<AppState>,
    manifest_json: String,
    public_key: String,
    signature: String,
    confirmation: String,
    allow_unverified: bool,
) -> Result<Value> {
    if manifest_json.len() > 16000 {
        return Err("Extension manifest exceeds 16 KB".into());
    }
    let m: Manifest =
        serde_json::from_str(&manifest_json).map_err(|e| format!("Invalid manifest: {e}"))?;
    validate(&m)?;
    if confirmation != m.name {
        return Err("Type the extension name to approve installation. Installation never enables execution.".into());
    }
    let fingerprint = if public_key.is_empty() && signature.is_empty() {
        if !allow_unverified {
            return Err("Unverified community packages require explicit approval".into());
        }
        None
    } else {
        let key = minisign_verify::PublicKey::decode(&public_key)
            .map_err(|_| "Invalid Minisign public key")?;
        let sig = minisign_verify::Signature::decode(&signature)
            .map_err(|_| "Invalid Minisign signature")?;
        key.verify(manifest_json.as_bytes(), &sig, false)
            .map_err(|_| "Publisher signature verification failed")?;
        Some(format!("{:x}", Sha256::digest(public_key.as_bytes())))
    };
    let sha256 = hash(&serde_json::to_value(&m).map_err(|e| e.to_string())?)?;
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    let mut installed: Vec<Installed> =
        serde_json::from_str(&repo.setting("installed_extensions")?.unwrap_or("[]".into()))
            .map_err(|_| "Invalid extension registry")?;
    if let Some(previous) = installed.iter().find(|e| e.manifest.id == m.id) {
        if previous.publisher_fingerprint.is_some() && previous.publisher_fingerprint != fingerprint
        {
            return Err("Publisher key changed. Remove the old package explicitly before trusting a different publisher.".into());
        }
    }
    installed.retain(|e| e.manifest.id != m.id);
    if installed.len() >= 30 {
        return Err("At most 30 installed extensions".into());
    }
    let id = m.id.clone();
    installed.push(Installed {
        manifest: m,
        sha256: sha256.clone(),
        publisher_fingerprint: fingerprint,
        enabled: false,
    });
    repo.set_setting(
        "installed_extensions",
        &serde_json::to_string(&installed).map_err(|e| e.to_string())?,
    )?;
    drop(repo);
    {
        let jobs = state.jobs.lock().map_err(|_| "Job state unavailable")?;
        for job in [format!("extension:{id}"), format!("extension-image:{id}")] {
            if let Some(token) = jobs.get(&job) {
                token.cancel();
            }
        }
    }
    Ok(
        json!({"installed":true,"enabled":false,"sha256":sha256,"notice":"Publisher verification proves signing-key possession, not safety or endorsement. No image, code or dependencies were downloaded."}),
    )
}
#[tauri::command]
pub fn configure_extension(
    state: State<AppState>,
    id: String,
    action: String,
    confirmation: String,
) -> Result<()> {
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    let mut all: Vec<Installed> =
        serde_json::from_str(&repo.setting("installed_extensions")?.unwrap_or("[]".into()))
            .map_err(|_| "Invalid extension registry")?;
    let extension = all
        .iter_mut()
        .find(|e| e.manifest.id == id)
        .ok_or("Extension not found")?;
    if confirmation != extension.manifest.name {
        return Err("Type the extension name to confirm permission changes".into());
    }
    match action.as_str() {
        "enable" => extension.enabled = true,
        "disable" => extension.enabled = false,
        "remove" => all.retain(|e| e.manifest.id != id),
        _ => return Err("Invalid extension action".into()),
    };
    repo.set_setting(
        "installed_extensions",
        &serde_json::to_string(&all).map_err(|e| e.to_string())?,
    )?;
    drop(repo);
    {
        let jobs = state.jobs.lock().map_err(|_| "Job state unavailable")?;
        for job in [format!("extension:{id}"), format!("extension-image:{id}")] {
            if let Some(token) = jobs.get(&job) {
                token.cancel();
            }
        }
    }
    Ok(())
}
fn prepared(state: &AppState, id: &str, input: Value) -> Result<(Installed, Value, String)> {
    let e = list(state)?
        .into_iter()
        .find(|e| e.manifest.id == id && e.enabled)
        .ok_or("Enable an installed extension first")?;
    validate(&e.manifest)?;
    if serde_json::to_vec(&input).map_err(|e| e.to_string())?.len() > 16000 {
        return Err("Input exceeds 16 KB".into());
    }
    if e.manifest.kind == "container" {
        let args = input
            .as_array()
            .ok_or("Container input must be a JSON array of argument strings")?;
        if args.len() > 30
            || args.iter().any(|v| {
                v.as_str()
                    .is_none_or(|s| s.len() > 2000 || s.contains('\0'))
            })
        {
            return Err("Use up to 30 bounded string arguments".into());
        }
    } else if e.manifest.kind == "mcp-container" {
        let object = input.as_object().ok_or("MCP input must be an object")?;
        let page = input
            .get("page")
            .map(|v| {
                v.as_u64()
                    .filter(|p| (1..=5).contains(p))
                    .ok_or("MCP page must be an integer from 1 to 5")
            })
            .transpose()?;
        let extra = usize::from(page.is_some());
        match input["operation"].as_str() {
            Some("list") if object.len()==1+extra=>(),
            Some("call") if object.len()==3+extra && input["arguments"].is_object() && input["name"].as_str().is_some_and(|name|e.manifest.tools.iter().any(|t|t==name))=>(),
            _=>return Err("Use operation=list or operation=call with name and arguments; optional page is 1–5".into())
        }
    } else if !input.is_object() || (e.manifest.method == "GET" && input != json!({})) {
        return Err("REST input must be an object; GET requires an empty object".into());
    }
    let payload = json!({"extension":e,"input":input,"permissions":if e.manifest.kind!="rest"{"Docker container: no network, no host mounts, read-only root filesystem, unprivileged UID, default seccomp, dropped capabilities, resource/time/output limits. Image must already be installed; no pulls."}else{"One explicit HTTPS request. POST may cause external changes. No automatic retries, credentials, model context or follow-up requests."}});
    let digest = hash(&payload)?;
    Ok((e, payload, digest))
}
#[tauri::command]
pub fn preview_extension(state: State<AppState>, id: String, input: Value) -> Result<Value> {
    let (_, payload, sha256) = prepared(&state, &id, input)?;
    Ok(json!({"payload":payload,"sha256":sha256}))
}
fn docker() -> Result<std::path::PathBuf> {
    #[cfg(target_os = "windows")]
    let paths = vec![std::path::PathBuf::from(
        std::env::var_os("ProgramFiles").ok_or("Windows program directory unavailable")?,
    )
    .join("Docker/Docker/resources/bin/docker.exe")];
    #[cfg(not(target_os = "windows"))]
    let paths = vec![
        "/Applications/Docker.app/Contents/Resources/bin/docker",
        "/usr/local/bin/docker",
        "/usr/bin/docker",
    ]
    .into_iter()
    .map(std::path::PathBuf::from)
    .collect::<Vec<_>>();
    paths.into_iter().find(|p|p.is_file()).ok_or("Install and start Docker separately. The app never installs a daemon or pulls tool images automatically.".into())
}
fn local_docker_host() -> Result<String> {
    #[cfg(target_os = "windows")]
    {
        return Ok("npipe:////./pipe/docker_engine".into());
    }
    #[cfg(not(target_os = "windows"))]
    {
        if let Some(home) = std::env::var_os("HOME") {
            let socket = std::path::PathBuf::from(home).join(".docker/run/docker.sock");
            if socket.exists() {
                return Ok(format!("unix://{}", socket.display()));
            }
        }
        if std::path::Path::new("/var/run/docker.sock").exists() {
            return Ok("unix:///var/run/docker.sock".into());
        }
        Err(
            "No supported local Docker socket found. Remote Docker contexts are not permitted."
                .into(),
        )
    }
}
async fn container(
    exe: &std::path::Path,
    name: &str,
    m: &Manifest,
    input: &Value,
) -> Result<Value> {
    use std::process::Stdio;
    use tokio::io::AsyncReadExt;
    let mut cmd = tokio::process::Command::new(exe);
    cmd.args(["--host", &local_docker_host()?]);
    cmd.args([
        "run",
        "--rm",
        "--pull=never",
        "--network=none",
        "--read-only",
        "--cap-drop=ALL",
        "--security-opt=no-new-privileges",
        "--pids-limit=64",
        "--memory=256m",
        "--cpus=1",
        "--user=65534:65534",
        "--tmpfs=/tmp:rw,noexec,nosuid,size=16777216",
        "--name",
        name,
    ]);
    if m.kind == "mcp-container" {
        cmd.arg("--interactive");
    }
    cmd.arg(&m.image).args(&m.command);
    if m.kind == "mcp-container" {
        return crate::mcp_stdio::exchange(cmd, input, &m.tools).await;
    }
    for arg in input.as_array().ok_or("Invalid arguments")? {
        cmd.arg(arg.as_str().ok_or("Invalid argument")?);
    }
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| "Cannot start container engine")?;
    let stdout = child.stdout.take().ok_or("Missing stdout")?;
    let stderr = child.stderr.take().ok_or("Missing stderr")?;
    async fn bounded<R: tokio::io::AsyncRead + Unpin>(reader: R) -> Result<Vec<u8>> {
        let mut v = vec![];
        reader
            .take(65537)
            .read_to_end(&mut v)
            .await
            .map_err(|_| "Tool output interrupted")?;
        if v.len() > 65536 {
            return Err("Tool output exceeded 64 KiB per stream".into());
        }
        Ok(v)
    }
    let (out, err, status) = tokio::try_join!(bounded(stdout), bounded(stderr), async {
        child
            .wait()
            .await
            .map_err(|_| "Container process interrupted".to_string())
    })?;
    Ok(
        json!({"exit_code":status.code(),"stdout":String::from_utf8_lossy(&out),"stderr":String::from_utf8_lossy(&err),"isError":!status.success(),"untrusted_result":true}),
    )
}
struct Guard<'a> {
    state: &'a AppState,
    id: String,
}
impl Drop for Guard<'_> {
    fn drop(&mut self) {
        if let Ok(mut jobs) = self.state.jobs.lock() {
            jobs.remove(&self.id);
        }
    }
}
#[tauri::command]
pub async fn run_extension(
    state: State<'_, AppState>,
    id: String,
    input: Value,
    reviewed_sha256: String,
    confirmation: String,
    consent: bool,
) -> Result<Value> {
    let (extension, payload, digest) = prepared(&state, &id, input.clone())?;
    if !consent || digest != reviewed_sha256 || confirmation != extension.manifest.name {
        return Err("Review the exact operation and type the extension name to approve it".into());
    }
    let epoch = state.data_epoch.load(std::sync::atomic::Ordering::SeqCst);
    let job_id = format!("extension:{id}");
    let cancel = tokio_util::sync::CancellationToken::new();
    {
        let mut jobs = state.jobs.lock().map_err(|_| "Job state unavailable")?;
        if jobs.contains_key(&job_id) {
            return Err("This extension is already running".into());
        }
        jobs.insert(job_id.clone(), cancel.clone());
    }
    let _guard = Guard {
        state: &state,
        id: job_id,
    };
    let (_, _, still_approved) = prepared(&state, &id, input.clone())?;
    if still_approved != reviewed_sha256 {
        return Err("Extension permissions changed; review again".into());
    }
    let mut audit = {
        let mut repo = state.repo.lock().map_err(|_| "Database unavailable")?;
        if epoch != state.data_epoch.load(std::sync::atomic::Ordering::SeqCst) {
            return Err("Workspace reset before tool execution".into());
        }
        let mut r = Record::new(
            "missions",
            &format!("Tool: {}", extension.manifest.name),
            &repo.device_id,
        );
        r.status = "running".into();
        r.summary="Individually approved extension call. Arguments and results are excluded from durable history.".into();
        r.fields.insert("approval_sha256".into(), digest);
        r.fields.insert("extension_id".into(), id.clone());
        repo.save_record(r)?
    };
    let image_name = format!("pcc-tool-{}", uuid::Uuid::new_v4());
    let executable = if extension.manifest.kind != "rest" {
        Some(docker())
    } else {
        None
    };
    let work = async {
        if let Some(exe) = &executable {
            return container(
                exe.as_ref().map_err(Clone::clone)?,
                &image_name,
                &extension.manifest,
                &input,
            )
            .await;
        }
        let url =
            reqwest::Url::parse(&extension.manifest.endpoint).map_err(|_| "Invalid endpoint")?;
        let client = crate::research::public_client(&url).await?;
        let request = if extension.manifest.method == "GET" {
            client.get(url)
        } else {
            client.post(url).json(&payload["input"])
        };
        let mut response = request
            .send()
            .await
            .map_err(|_| "REST extension request failed; remote effects may have occurred")?;
        let status = response.status().as_u16();
        let mut bytes = vec![];
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| "Extension response interrupted")?
        {
            if bytes.len() + chunk.len() > 65536 {
                return Err(
                    "Extension response exceeds 64 KiB; remote effects may have occurred".into(),
                );
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(
            json!({"status":status,"body":String::from_utf8_lossy(&bytes),"isError":status>=400,"untrusted_result":true}),
        )
    };
    let result: Result<Value> = tokio::select! {biased; _=cancel.cancelled()=>Err("Extension cancelled locally; external effects may already have occurred".into()),r=tokio::time::timeout(std::time::Duration::from_secs(90),work)=>r.map_err(|_|"Extension timed out; remote effects may already have occurred".to_string()).and_then(|r|r)};
    let mut cleanup_warning = None;
    if let Some(Ok(exe)) = executable {
        let cleanup = tokio::process::Command::new(exe)
            .args(["--host", &local_docker_host()?])
            .args(["rm", "--force", &image_name])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .status();
        let cleaned = tokio::time::timeout(std::time::Duration::from_secs(5), cleanup).await;
        if !matches!(cleaned,Ok(Ok(status)) if status.success())
            && (result.is_err() || extension.manifest.kind == "mcp-container")
        {
            cleanup_warning = Some(format!(
                "Could not confirm cleanup. Inspect only container {image_name} in Docker."
            ));
        }
    }
    if epoch == state.data_epoch.load(std::sync::atomic::Ordering::SeqCst) {
        audit.status = if result.as_ref().is_ok_and(|v| v["isError"] != true) {
            "completed"
        } else {
            "failed"
        }
        .into();
        audit.summary=match &result{Ok(_)=>"Tool returned. Result is untrusted and was not stored in this audit or sent to a model.".into(),Err(e)=>e.clone()};
        if let Some(w) = &cleanup_warning {
            audit.summary.push_str(w);
        }
        state
            .repo
            .lock()
            .map_err(|_| "Database unavailable")?
            .save_record(audit)?;
    }
    result
        .map_err(|e| {
            format!(
                "{e}{}",
                cleanup_warning
                    .as_ref()
                    .map(|w| format!(" {w}"))
                    .unwrap_or_default()
            )
        })
        .map(|mut v| {
            v["cleanup_warning"] = json!(cleanup_warning);
            v
        })
}

#[tauri::command]
pub async fn pull_extension_image(
    state: State<'_, AppState>,
    id: String,
    confirmation: String,
    consent: bool,
) -> Result<Value> {
    let extension = list(&state)?
        .into_iter()
        .find(|e| e.manifest.id == id)
        .ok_or("Install and review the extension manifest first")?;
    validate(&extension.manifest)?;
    if extension.manifest.kind == "rest" || !consent || confirmation != extension.manifest.image {
        return Err("Approve the download and type the complete immutable image reference".into());
    }
    let exe = docker()?;
    let host = local_docker_host()?;
    let image = extension.manifest.image.clone();
    let epoch = state.data_epoch.load(std::sync::atomic::Ordering::SeqCst);
    let job_id = format!("extension-image:{id}");
    let token = tokio_util::sync::CancellationToken::new();
    {
        let mut jobs = state.jobs.lock().map_err(|_| "Job state unavailable")?;
        if jobs.contains_key(&job_id) || jobs.len() >= 4 {
            return Err("Finish or cancel an active image download first".into());
        }
        jobs.insert(job_id.clone(), token.clone());
    }
    let _guard = Guard {
        state: &state,
        id: job_id,
    };
    if !list(&state)?
        .iter()
        .any(|e| e.manifest.id == id && e.sha256 == extension.sha256)
    {
        return Err("Extension changed after download review".into());
    }
    let work = async {
        use tokio::io::AsyncReadExt;
        let mut child = tokio::process::Command::new(exe)
            .args(["--host", &host, "pull", &image])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| "Cannot start local Docker image download")?;
        async fn drain<R: tokio::io::AsyncRead + Unpin>(mut reader: R) -> Result<()> {
            let mut chunk = [0u8; 8192];
            let mut total = 0usize;
            loop {
                let n = reader
                    .read(&mut chunk)
                    .await
                    .map_err(|_| "Image download output interrupted")?;
                if n == 0 {
                    return Ok(());
                }
                total += n;
                if total > 2 * 1024 * 1024 {
                    return Err(
                        "Image download status exceeds 2 MiB; inspect Docker before retrying"
                            .into(),
                    );
                }
            }
        }
        let stdout = child.stdout.take().ok_or("Download stdout missing")?;
        let stderr = child.stderr.take().ok_or("Download stderr missing")?;
        let (_, _, status) = tokio::try_join!(drain(stdout), drain(stderr), async {
            child
                .wait()
                .await
                .map_err(|_| "Image download interrupted".to_string())
        })?;
        if !status.success() {
            return Err("Docker did not confirm the image download. Review its local registry/authentication/storage status before retrying.".into());
        }
        Ok(
            json!({"downloaded":true,"image":image,"executed":false,"notice":"Docker verified the requested content-addressed image. Nothing in the image was executed. Publisher signature/trust and later execution approval remain separate."}),
        )
    };
    let result: Result<Value> = tokio::select! {biased;_=token.cancelled()=>Err("Stopped waiting for the image pull. Docker may retain layers or continue an in-flight download; inspect Docker before retrying.".into()),r=tokio::time::timeout(std::time::Duration::from_secs(600),work)=>r.map_err(|_|"Image download exceeded ten minutes; inspect Docker before retrying".to_string()).and_then(|v|v)};
    let mut repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    if epoch == state.data_epoch.load(std::sync::atomic::Ordering::SeqCst) {
        let mut audit = Record::new(
            "missions",
            &format!("Extension image setup: {}", extension.manifest.name),
            &repo.device_id,
        );
        audit.status = if result.is_ok() {
            "completed"
        } else {
            "failed"
        }
        .into();
        audit.fields.insert("extension_id".into(), id);
        audit.summary = if result.is_ok() {
            "Explicit immutable image download completed. No container was executed. Docker manages image storage independently of workspace reset.".into()
        } else {
            "Image download did not complete with confirmed success; inspect Docker. No container execution was requested.".into()
        };
        repo.save_record(audit)?;
    }
    result
}

#[tauri::command]
pub fn validate_extension_manifest(manifest_json: String) -> Result<Value> {
    if manifest_json.len() > 16000 {
        return Err("Manifest exceeds 16 KB".into());
    }
    let manifest: Manifest =
        serde_json::from_str(&manifest_json).map_err(|_| "Invalid declarative manifest")?;
    validate(&manifest)?;
    Ok(
        json!({"valid":true,"sha256":format!("{:x}",Sha256::digest(manifest_json.as_bytes())),"kind":manifest.kind,"setup":if manifest.kind=="rest"{"Fixed public HTTPS endpoint; no local dependency installation"}else{"Preinstalled Docker and the declared immutable image. Image retrieval and every execution need separate approval."},"credential_policy":"Credentials are never embedded in this manifest; no renderer scripts, host shell, filesystem roots or automatic grants are accepted."}),
    )
}
#[tauri::command]
pub async fn export_extension_manifest(manifest_json: String) -> Result<Option<String>> {
    validate_extension_manifest(manifest_json.clone())?;
    let Some(file) = rfd::AsyncFileDialog::new()
        .set_title("Export authored extension manifest")
        .set_file_name("command-center-extension.json")
        .save_file()
        .await
    else {
        return Ok(None);
    };
    crate::atomic_write(file.path(), manifest_json.as_bytes())?;
    Ok(Some(
        "Manifest exported. Nothing was installed, enabled or executed.".into(),
    ))
}
