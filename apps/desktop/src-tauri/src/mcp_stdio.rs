//! MCP JSON-RPC over bounded stdin/stdout of an already sandbox-configured container.
use crate::Result;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
async fn send(writer: &mut tokio::process::ChildStdin, message: Value) -> Result<()> {
    let mut bytes = serde_json::to_vec(&message).map_err(|_| "Cannot encode MCP request")?;
    bytes.push(b'\n');
    writer
        .write_all(&bytes)
        .await
        .map_err(|_| "MCP stdin closed")?;
    writer
        .flush()
        .await
        .map_err(|_| "MCP stdin flush failed".into())
}
async fn response(
    reader: &mut BufReader<tokio::process::ChildStdout>,
    writer: &mut tokio::process::ChildStdin,
    id: u32,
    total: &mut usize,
) -> Result<Value> {
    for _ in 0..32 {
        let mut line = vec![];
        loop {
            let b = reader
                .read_u8()
                .await
                .map_err(|_| "MCP server closed before completing its response")?;
            *total += 1;
            if *total > 262144 || line.len() >= 65536 {
                return Err("MCP stdio output exceeded 64 KiB/message or 256 KiB/session".into());
            }
            if b == b'\n' {
                break;
            }
            line.push(b);
        }
        let value: Value = serde_json::from_slice(&line)
            .map_err(|_| "Server stdout must contain newline-delimited JSON-RPC only")?;
        if value["jsonrpc"] != "2.0" {
            return Err("Invalid MCP JSON-RPC version".into());
        }
        if value.get("method").is_some() {
            if let Some(request_id) = value.get("id") {
                send(writer,json!({"jsonrpc":"2.0","id":request_id,"error":{"code":-32601,"message":"Client-side requests, roots, sampling and elicitation are unavailable"}})).await?;
            }
            continue;
        }
        if value["id"] != json!(id) {
            return Err("Unexpected MCP response identifier".into());
        }
        if value.get("error").is_some() {
            return Err(format!(
                "MCP server returned error code {} (message omitted from durable audit)",
                value["error"]["code"]
                    .as_i64()
                    .map(|v| v.to_string())
                    .unwrap_or("unknown".into())
            ));
        }
        return value
            .get("result")
            .cloned()
            .ok_or("MCP response omitted result".into());
    }
    Err("Too many MCP messages before requested response".into())
}
pub async fn exchange(
    mut command: tokio::process::Command,
    input: &Value,
    allowed: &[String],
) -> Result<Value> {
    let mut child = command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| "Cannot start isolated MCP container")?;
    let mut writer = child.stdin.take().ok_or("MCP stdin missing")?;
    let mut reader = BufReader::new(child.stdout.take().ok_or("MCP stdout missing")?);
    let mut bytes = 0;
    send(&mut writer,json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"PersonalCommandCenter","version":"0.5.0"}}})).await?;
    let initialization = response(&mut reader, &mut writer, 1, &mut bytes).await?;
    if initialization["protocolVersion"] != "2025-06-18"
        || !initialization["capabilities"]["tools"].is_object()
    {
        return Err("Server must negotiate MCP 2025-06-18 and tool capabilities".into());
    }
    send(
        &mut writer,
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    )
    .await?;
    let page = input["page"].as_u64().unwrap_or(1);
    let mut catalog = json!({});
    let mut seen = std::collections::HashSet::new();
    for current in 1..=page {
        let params = if current == 1 {
            json!({})
        } else {
            let cursor = catalog["nextCursor"]
                .as_str()
                .filter(|s| !s.is_empty() && s.len() <= 2000)
                .ok_or("Requested catalog page is unavailable")?;
            if !seen.insert(cursor.to_owned()) {
                return Err("Server repeated a pagination cursor".into());
            }
            json!({"cursor":cursor})
        };
        let request_id = 1 + current as u32;
        send(
            &mut writer,
            json!({"jsonrpc":"2.0","id":request_id,"method":"tools/list","params":params}),
        )
        .await?;
        catalog = response(&mut reader, &mut writer, request_id, &mut bytes).await?;
    }
    let tools = catalog["tools"]
        .as_array()
        .ok_or("Server tool catalog is invalid")?;
    let result = match input["operation"].as_str() {
        Some("list") => {
            json!({"tools":tools,"nextCursor":catalog["nextCursor"],"allowed_tools":allowed,"page":page,"notice":"Only the requested catalog page is shown; no tool was called."})
        }
        Some("call") => {
            let name = input["name"].as_str().ok_or("Missing tool name")?;
            if !allowed.iter().any(|t| t == name)
                || !tools.iter().any(|t| t["name"].as_str() == Some(name))
            {
                return Err(
                    "Tool is not explicitly allowed and present on the requested server catalog page"
                        .into(),
                );
            }
            send(&mut writer,json!({"jsonrpc":"2.0","id":10,"method":"tools/call","params":{"name":name,"arguments":input["arguments"]}})).await?;
            response(&mut reader, &mut writer, 10, &mut bytes).await?
        }
        _ => return Err("Invalid MCP operation".into()),
    };
    drop(writer);
    // No persistent sessions: terminate this isolated server after the single reviewed operation.
    let _ = child.start_kill();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(3), child.wait()).await;
    Ok(
        json!({"isError":result["isError"]==true,"untrusted_result":true,"protocol":"2025-06-18","result":result,"permissions":"No container network, host mounts, credentials, sampling, roots or server-initiated client actions. Outer runner removes the named container."}),
    )
}
