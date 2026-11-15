//! Remote-only MCP boundary: no local process, filesystem, sampling, elicitation or automatic calls.
use crate::{AppState, Result};
use command_center_core::workspace::Record;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::atomic::Ordering;
use tauri::State;
use zeroize::Zeroizing;
const VERSION: &str = "2025-06-18";
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Connection {
    pub id: String,
    pub name: String,
    pub endpoint: String,
    pub allowed_tools: Vec<String>,
    pub enabled: bool,
}
fn connections(state: &AppState) -> Result<Vec<Connection>> {
    let raw = state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .setting("mcp_connections")?
        .unwrap_or("[]".into());
    serde_json::from_str(&raw).map_err(|_| "Invalid saved MCP connections".into())
}
fn credential(state: &AppState, id: &str) -> Result<keyring::Entry> {
    let device = state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .device_id
        .clone();
    keyring::Entry::new(&format!("com.personal.commandcenter.mcp.{id}"), &device)
        .map_err(|e| e.to_string())
}
fn validate(c: &Connection) -> Result<()> {
    uuid::Uuid::parse_str(&c.id).map_err(|_| "Invalid connection ID")?;
    let u = reqwest::Url::parse(&c.endpoint).map_err(|_| "Invalid endpoint")?;
    if c.name.trim().is_empty()
        || c.name.len() > 200
        || c.endpoint.len() > 2000
        || u.scheme() != "https"
        || !u.username().is_empty()
        || u.password().is_some()
        || u.query().is_some()
        || u.fragment().is_some()
        || c.allowed_tools.len() > 50
        || c.allowed_tools.iter().any(|s| {
            s.is_empty()
                || s.len() > 128
                || !s
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        })
    {
        return Err("Use a public HTTPS endpoint without credentials or query, and up to 50 exact tool names".into());
    }
    Ok(())
}
#[tauri::command]
pub fn mcp_connections(state: State<AppState>) -> Result<Vec<Connection>> {
    connections(&state)
}
#[tauri::command]
pub fn save_mcp_connection(
    state: State<AppState>,
    connection: Connection,
    token: Option<String>,
) -> Result<()> {
    validate(&connection)?;
    let mut list = connections(&state)?;
    if token.is_none()
        && list
            .iter()
            .any(|c| c.id == connection.id && c.endpoint != connection.endpoint)
    {
        return Err(
            "Changing an endpoint requires explicitly replacing or removing its credential".into(),
        );
    }
    list.retain(|c| c.id != connection.id);
    if list.len() >= 20 {
        return Err("At most 20 MCP connections".into());
    }
    if let Some(raw) = token {
        let raw = Zeroizing::new(raw);
        if raw.len() > 4096 || raw.contains(['\r', '\n']) {
            return Err("Invalid bearer credential".into());
        }
        if raw.is_empty() {
            match credential(&state, &connection.id)?.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => (),
                Err(_) => return Err("Cannot remove MCP credential".into()),
            }
        } else {
            credential(&state, &connection.id)?
                .set_password(&raw)
                .map_err(|_| "OS secure store refused MCP credential")?;
        }
    }
    if let Some(cancel) = state
        .jobs
        .lock()
        .map_err(|_| "Job state unavailable")?
        .get(&format!("mcp:{}", connection.id))
    {
        cancel.cancel();
    }
    list.push(connection);
    state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .set_setting(
            "mcp_connections",
            &serde_json::to_string(&list).map_err(|e| e.to_string())?,
        )
}
#[tauri::command]
pub fn remove_mcp_connection(state: State<AppState>, id: String) -> Result<()> {
    uuid::Uuid::parse_str(&id).map_err(|_| "Invalid connection ID")?;
    let mut list = connections(&state)?;
    list.retain(|c| c.id != id);
    state
        .repo
        .lock()
        .map_err(|_| "Database unavailable")?
        .set_setting(
            "mcp_connections",
            &serde_json::to_string(&list).map_err(|e| e.to_string())?,
        )?;
    if let Some(cancel) = state
        .jobs
        .lock()
        .map_err(|_| "Job state unavailable")?
        .get(&format!("mcp:{id}"))
    {
        cancel.cancel();
    }
    match credential(&state, &id)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(_) => Err("Connection removed; credential deletion failed".into()),
    }
}
fn selected(state: &AppState, id: &str) -> Result<Connection> {
    connections(state)?
        .into_iter()
        .find(|c| c.id == id && c.enabled)
        .ok_or("Enable this connection first".into())
}
fn packet(c: &Connection, tool: &str, args: &Value) -> Result<Value> {
    if !c.allowed_tools.iter().any(|s| s == tool)
        || !args.is_object()
        || serde_json::to_vec(args).map_err(|e| e.to_string())?.len() > 32000
    {
        return Err(
            "Tool must be explicitly allowlisted; arguments must be a JSON object under 32 KB"
                .into(),
        );
    }
    Ok(json!({"connection":c,"method":"tools/call","params":{"name":tool,"arguments":args}}))
}
fn digest(v: &Value) -> Result<String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(v).map_err(|e| e.to_string())?)
    ))
}
#[tauri::command]
pub fn preview_mcp_call(
    state: State<AppState>,
    id: String,
    tool: String,
    arguments: Value,
) -> Result<Value> {
    let payload = packet(&selected(&state, &id)?, &tool, &arguments)?;
    Ok(json!({"sha256":digest(&payload)?,"payload":payload}))
}
struct Remote {
    client: reqwest::Client,
    url: reqwest::Url,
    token: Option<Zeroizing<String>>,
    session: Option<String>,
}
impl Remote {
    async fn post(&mut self, body: Value) -> Result<Value> {
        let expected = body.get("id").cloned();
        let mut req = self
            .client
            .post(self.url.clone())
            .header("Accept", "application/json, text/event-stream")
            .header("MCP-Protocol-Version", VERSION)
            .json(&body);
        if let Some(token) = &self.token {
            req = req.bearer_auth(token.as_str());
        }
        if let Some(session) = &self.session {
            req = req.header("Mcp-Session-Id", session);
        }
        let mut res = req.send().await.map_err(|_| "MCP request failed")?;
        if !res.status().is_success() {
            return Err(format!(
                "MCP returned HTTP {}. Redirects and automatic retries are disabled.",
                res.status()
            ));
        }
        if self.session.is_none() {
            if let Some(s) = res.headers().get("Mcp-Session-Id") {
                let s = s.to_str().map_err(|_| "Invalid MCP session header")?;
                if s.len() > 256 || !s.bytes().all(|b| (33..=126).contains(&b)) {
                    return Err("Invalid MCP session header".into());
                }
                self.session = Some(s.into());
            }
        }
        if expected.is_none() {
            if res.status().as_u16() != 202 {
                return Err("Server did not acknowledge notification with HTTP 202".into());
            }
            return Ok(Value::Null);
        }
        let stream = res
            .headers()
            .get("content-type")
            .and_then(|s| s.to_str().ok())
            .is_some_and(|s| s.starts_with("text/event-stream"));
        let mut bytes = vec![];
        let mut processed = 0;
        let mut event_data = String::new();
        while let Some(chunk) = res.chunk().await.map_err(|_| "MCP stream interrupted")? {
            if bytes.len() + chunk.len() > 2 * 1024 * 1024 {
                return Err("MCP result exceeds 2 MiB".into());
            }
            bytes.extend_from_slice(&chunk);
            if stream {
                while let Some(end) = bytes[processed..].iter().position(|b| *b == b'\n') {
                    let line = std::str::from_utf8(&bytes[processed..processed + end])
                        .map_err(|_| "MCP stream must be UTF-8")?
                        .trim_end_matches('\r');
                    processed += end + 1;
                    if line.is_empty() {
                        if !event_data.is_empty() {
                            let value: Value = serde_json::from_str(event_data.trim())
                                .map_err(|_| "Malformed MCP event")?;
                            event_data.clear();
                            if let Some(value) = rpc_result(value, &expected)? {
                                return Ok(value);
                            }
                        }
                    } else if let Some(data) = line.strip_prefix("data:") {
                        event_data.push_str(data.strip_prefix(' ').unwrap_or(data));
                        event_data.push('\n');
                    }
                }
            }
        }
        if stream {
            return Err("MCP stream closed without a matching response".into());
        }
        let value = serde_json::from_slice(&bytes).map_err(|_| "Invalid MCP JSON response")?;
        rpc_result(value, &expected)?.ok_or("MCP response ID mismatch".into())
    }
}
fn rpc_result(v: Value, expected: &Option<Value>) -> Result<Option<Value>> {
    if v["jsonrpc"] != "2.0" {
        return Err("Invalid MCP JSON-RPC version".into());
    }
    if v.get("method").is_some() {
        if v.get("id").is_some() {
            return Err("Server-initiated requests are disabled; no filesystem, sampling or elicitation capability is advertised".into());
        }
        return Ok(None);
    }
    if v.get("id") != expected.as_ref() {
        return Ok(None);
    }
    if v.get("error").is_some() {
        return Err(format!(
            "MCP returned JSON-RPC error code {}. Provider error text is withheld from logs.",
            v["error"]["code"]
        ));
    }
    v.get("result")
        .cloned()
        .map(Some)
        .ok_or("MCP response has no result".into())
}
struct Job<'a> {
    state: &'a AppState,
    key: String,
}
impl Drop for Job<'_> {
    fn drop(&mut self) {
        if let Ok(mut jobs) = self.state.jobs.lock() {
            jobs.remove(&self.key);
        }
    }
}
#[tauri::command]
pub async fn run_mcp_request(
    state: State<'_, AppState>,
    id: String,
    action: String,
    tool: String,
    arguments: Value,
    reviewed_sha256: Option<String>,
    confirmation: String,
    consent: bool,
) -> Result<Value> {
    if !consent || !["list", "call"].contains(&action.as_str()) {
        return Err("Approve the selected MCP network operation".into());
    }
    let c = selected(&state, &id)?;
    validate(&c)?;
    if action == "call" {
        let reviewed = packet(&c, &tool, &arguments)?;
        if confirmation != tool || reviewed_sha256.as_deref() != Some(digest(&reviewed)?.as_str()) {
            return Err(
                "Review the current exact payload and type the tool name to authorize this call"
                    .into(),
            );
        }
    }
    let key = format!("mcp:{id}");
    let cancel = tokio_util::sync::CancellationToken::new();
    {
        let mut jobs = state.jobs.lock().map_err(|_| "Job state unavailable")?;
        if jobs.contains_key(&key) || jobs.keys().filter(|k| k.starts_with("mcp:")).count() >= 2 {
            return Err("This MCP connection is busy".into());
        }
        jobs.insert(key.clone(), cancel.clone());
    }
    let _job = Job { state: &state, key };
    let epoch = state.data_epoch.load(Ordering::SeqCst);
    let mut audit = {
        let mut repo = state.repo.lock().map_err(|_| "Database unavailable")?;
        let mut r = Record::new(
            "missions",
            if action == "call" {
                "Approved remote MCP call"
            } else {
                "Approved MCP tool discovery"
            },
            &repo.device_id,
        );
        r.status = "running".into();
        r.fields.insert("connection_id".into(), id.clone());
        r.fields.insert(
            "tool".into(),
            if action == "call" {
                tool.clone()
            } else {
                "tools/list".into()
            },
        );
        r.fields.insert(
            "provider".into(),
            "Remote MCP; explicit user approval; arguments/results omitted from audit".into(),
        );
        r.summary="Started an approved request. Outcome is unknown until a final record is saved; interruption does not imply rollback.".into();
        repo.save_record(r)?
    };
    let work = async {
        let url = reqwest::Url::parse(&c.endpoint).map_err(|_| "Invalid endpoint")?;
        let client = crate::research::public_client(&url).await?;
        let token = match credential(&state, &id)?.get_password() {
            Ok(v) => Some(Zeroizing::new(v)),
            Err(keyring::Error::NoEntry) => None,
            Err(_) => return Err("Cannot read MCP credential".into()),
        };
        let mut remote = Remote {
            client,
            url,
            token,
            session: None,
        };
        let init=remote.post(json!({"jsonrpc":"2.0","id":"initialize","method":"initialize","params":{"protocolVersion":VERSION,"capabilities":{},"clientInfo":{"name":"Personal Command Center","version":"0.5"}}})).await?;
        if init["protocolVersion"] != VERSION || !init["capabilities"]["tools"].is_object() {
            return Err("Server must support MCP 2025-06-18 and the tools capability".into());
        }
        remote
            .post(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
            .await?;
        if state.data_epoch.load(Ordering::SeqCst) != epoch
            || digest(&serde_json::to_value(selected(&state, &id)?).map_err(|e| e.to_string())?)?
                != digest(&serde_json::to_value(&c).map_err(|e| e.to_string())?)?
        {
            return Err("MCP authorization changed".into());
        }
        if action == "list" {
            let mut rows = vec![];
            let mut cursor: Option<String> = None;
            for _ in 0..5 {
                let params = cursor
                    .as_ref()
                    .map(|c| json!({"cursor":c}))
                    .unwrap_or(json!({}));
                let result=remote.post(json!({"jsonrpc":"2.0","id":uuid::Uuid::new_v4().to_string(),"method":"tools/list","params":params})).await?;
                let tools = result["tools"].as_array().ok_or("Invalid MCP tools list")?;
                if rows.len() + tools.len() > 200 {
                    return Err("Tool catalog exceeds 200-tool limit".into());
                }
                rows.extend(tools.iter().cloned());
                cursor = result["nextCursor"].as_str().map(str::to_owned);
                if cursor.is_none() {
                    return Ok(json!({"tools":rows,"untrusted_metadata":true}));
                }
            }
            Err("Tool pagination exceeds five-page limit".into())
        } else {
            remote.post(json!({"jsonrpc":"2.0","id":uuid::Uuid::new_v4().to_string(),"method":"tools/call","params":{"name":tool,"arguments":arguments}})).await
        }
    };
    let result: Result<Value> = tokio::select! {_=cancel.cancelled()=>Err("Request cancelled locally. The remote tool may still have executed; do not blindly retry.".into()),r=work=>r};
    if state.data_epoch.load(Ordering::SeqCst) == epoch {
        let mut repo = state.repo.lock().map_err(|_| "Database unavailable")?;
        audit.status = if result.as_ref().is_ok_and(|v| v["isError"] != true) {
            "completed"
        } else {
            "failed"
        }
        .into();
        audit.summary=match &result{Ok(v) if v["isError"]==true=>"Remote tool reported an error. Result displayed only in the tools view; external effects may still have occurred.".into(),Ok(_)=>"Received remote result. Arguments and result content were not stored in this audit. No follow-on action or AI context access was authorized.".into(),Err(e)=>e.clone()};
        repo.save_record(audit)?;
    }
    result
}

/// Health is derived from reviewed request history; this never pings an endpoint.
#[tauri::command]
pub fn mcp_health(state: State<AppState>) -> Result<Value> {
    let connections = connections(&state)?;
    let repo = state.repo.lock().map_err(|_| "Database unavailable")?;
    let records = repo.records("", false)?;
    Ok(json!(connections.into_iter().map(|c|{let latest=records.iter().filter(|r|r.module=="missions"&&r.fields.get("connection_id")==Some(&c.id)).max_by(|a,b|a.updated_at.cmp(&b.updated_at));json!({"id":c.id,"name":c.name,"enabled":c.enabled,"last_status":latest.map(|r|r.status.as_str()),"last_at":latest.map(|r|r.updated_at.as_str()),"last_operation":latest.and_then(|r|r.fields.get("tool")),"note":"Last observed request outcome; no connectivity or publisher trust guarantee."})}).collect::<Vec<_>>()))
}
