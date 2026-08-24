//! MCP Client（§4.8）— stdio 传输 + JSON-RPC 2.0
//!
//! 生命周期：
//! 1. connect：spawn 子进程 → initialize 握手 → notifications/initialized → tools/list
//! 2. 每个连接一个 reader 任务：逐行解析 stdout，把 JSON-RPC 消息推入 rx 通道
//! 3. call：`tools/call` 请求，按 id 匹配响应；非当前 id 的消息丢弃
//!
//! 设计说明：std 互斥锁不能跨 await 持有，因此 call 时先把 handle 从池中
//! 取出、await 调用后再放回（同一服务器串行调用，符合 MCP 调用节奏）。

use crate::models::{McpServerInfo, McpServerInput, McpTool};
use crate::state::{AppState, McpHandle};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

const MCP_PROTOCOL_VERSION: &str = "2024-11-05";
static RPC_ID: AtomicU64 = AtomicU64::new(1);

fn next_id() -> u64 {
    RPC_ID.fetch_add(1, Ordering::Relaxed)
}

// ---------- 持久化 ----------

pub(crate) fn load_server(conn: &rusqlite::Connection, id: &str) -> Result<McpServerInfo, String> {
    conn.query_row(
        "SELECT id, name, transport, command, args, env, url, status, tools_json, error_message
         FROM mcp_servers WHERE id = ?1",
        rusqlite::params![id],
        row_to_server,
    )
    .map_err(|_| format!("MCP 服务器不存在: {id}"))
}

pub(crate) fn row_to_server(row: &rusqlite::Row) -> rusqlite::Result<McpServerInfo> {
    let args: Option<String> = row.get(4)?;
    let tools_json: Option<String> = row.get(8)?;
    let tools: Vec<McpTool> = tools_json
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    Ok(McpServerInfo {
        id: row.get(0)?,
        name: row.get(1)?,
        transport: row.get(2)?,
        command: row.get(3)?,
        args: args
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default(),
        env: None,
        url: row.get(6)?,
        status: row.get(7)?,
        tools,
        error_message: row.get(9)?,
    })
}

fn persist(
    conn: &rusqlite::Connection,
    id: &str,
    status: &str,
    tools: Option<&[McpTool]>,
    error_message: Option<&str>,
) -> Result<(), String> {
    let tools_json = match tools {
        Some(t) => Some(serde_json::to_string(t).map_err(|e| e.to_string())?),
        None => None,
    };
    conn.execute(
        "UPDATE mcp_servers SET status = ?2, tools_json = ?3, error_message = ?4 WHERE id = ?1",
        rusqlite::params![id, status, tools_json, error_message],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

// ---------- 进程与消息 ----------

fn spawn_stdio(
    server: &McpServerInfo,
) -> Result<(tokio::process::Child, tokio::process::ChildStdin, tokio::process::ChildStdout), String>
{
    let Some(command) = &server.command else {
        return Err("stdio 传输需要配置 command".to_string());
    };
    let mut cmd = tokio::process::Command::new(command);
    cmd.args(&server.args);
    if let Some(env) = &server.env {
        if let Ok(map) =
            serde_json::from_value::<std::collections::HashMap<String, String>>(env.clone())
        {
            for (k, v) in map {
                cmd.env(k, v);
            }
        }
    }
    cmd.stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    let mut child = cmd.spawn().map_err(|e| format!("启动 MCP 进程失败: {e}"))?;
    let stdin = child.stdin.take().ok_or("MCP stdin 不可用")?;
    let stdout = child.stdout.take().ok_or("MCP stdout 不可用")?;
    Ok((child, stdin, stdout))
}

/// 发送一行 JSON-RPC（请求或通知）
async fn write_line(
    writer: &tokio::sync::Mutex<tokio::process::ChildStdin>,
    value: &Value,
) -> Result<(), String> {
    let mut stdin = writer.lock().await;
    let line = serde_json::to_string(value).map_err(|e| e.to_string())?;
    stdin
        .write_all(line.as_bytes())
        .await
        .map_err(|e| format!("写入 MCP stdin 失败: {e}"))?;
    stdin
        .write_all(b"\n")
        .await
        .map_err(|e| format!("写入 MCP stdin 失败: {e}"))?;
    stdin
        .flush()
        .await
        .map_err(|e| format!("写入 MCP stdin 失败: {e}"))?;
    Ok(())
}

/// 请求-响应调用：发请求行，在 rx 中等待匹配 id 的响应
async fn rpc_call(
    writer: &tokio::sync::Mutex<tokio::process::ChildStdin>,
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<Value>,
    method: &str,
    params: Value,
) -> Result<Value, String> {
    let id = next_id();
    let req = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
    write_line(writer, &req).await?;

    loop {
        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Some(v) => {
                        // 只处理与本次请求匹配的响应；通知/其他请求丢弃
                        if let Some(resp_id) = v.get("id").and_then(Value::as_u64) {
                            if resp_id == id {
                                if let Some(err) = v.get("error") {
                                    return Err(format!("MCP 错误: {err}"));
                                }
                                return Ok(v.get("result").cloned().unwrap_or(Value::Null));
                            }
                        }
                    }
                    None => return Err("MCP 连接已关闭".to_string()),
                }
            }
            _ = tokio::time::sleep(std::time::Duration::from_secs(120)) => {
                return Err("MCP 调用超时".to_string());
            }
        }
    }
}

// ---------- 对外接口 ----------

/// 连接并发现工具
pub async fn connect(app: &AppState, server_id: &str) -> Result<Vec<McpTool>, String> {
    let server = {
        let conn = app.db()?;
        load_server(&conn, server_id)?
    };
    if server.transport != "stdio" {
        return Err("当前仅支持 stdio 传输".to_string());
    }

    // 已有连接则先断开（防重复 spawn）
    let _ = disconnect(app, server_id).await;

    let (child, stdin, stdout) = spawn_stdio(&server)?;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

    // reader 任务：stdout 逐行解析为 JSON 消息
    {
        let tx = tx.clone();
        tauri::async_runtime::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            loop {
                match lines.next_line().await {
                    Ok(Some(line)) => {
                        if line.trim().is_empty() {
                            continue;
                        }
                        if let Ok(v) = serde_json::from_str::<Value>(&line) {
                            if tx.send(v).is_err() {
                                break;
                            }
                        }
                    }
                    Ok(None) | Err(_) => break,
                }
            }
        });
    }

    let writer = tokio::sync::Mutex::new(stdin);

    // initialize 握手
    let init_result = rpc_call(
        &writer,
        &mut rx,
        "initialize",
        json!({
            "protocolVersion": MCP_PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": { "name": "koid", "version": "0.1.0" }
        }),
    )
    .await?;
    if let Some(v) = init_result.get("protocolVersion") {
        // 服务器可能返回不兼容版本，宽容处理
        let _ = v;
    }

    // initialized 通知（无响应）
    let _ = write_line(
        &writer,
        &json!({ "jsonrpc": "2.0", "method": "notifications/initialized", "params": {} }),
    )
    .await;

    // 工具发现
    let tools_result = rpc_call(&writer, &mut rx, "tools/list", json!({})).await?;
    let tools: Vec<McpTool> = tools_result
        .get("tools")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|t| serde_json::from_value::<McpTool>(t.clone()).ok())
                .collect()
        })
        .unwrap_or_default();

    // 存入连接池
    let handle = McpHandle {
        process: child,
        writer,
        rx,
    };
    {
        let mut map = app.mcp.lock().map_err(|e| e.to_string())?;
        map.insert(server_id.to_string(), handle);
    }

    let conn = app.db()?;
    persist(&conn, server_id, "connected", Some(&tools), None)?;
    Ok(tools)
}

/// 断开：杀死进程并从池中移除
pub async fn disconnect(app: &AppState, server_id: &str) -> Result<(), String> {
    let handle = {
        let mut map = app.mcp.lock().map_err(|e| e.to_string())?;
        map.remove(server_id)
    };
    if let Some(mut h) = handle {
        let _ = h.process.kill().await;
    }
    if let Ok(conn) = app.db() {
        let _ = persist(&conn, server_id, "disconnected", None, None);
    }
    Ok(())
}

/// 调用指定服务器的工具
pub async fn call_tool(
    app: &AppState,
    server_id: &str,
    tool: &str,
    args: Value,
) -> Result<Value, String> {
    let mut handle = {
        let mut map = app.mcp.lock().map_err(|e| e.to_string())?;
        map.remove(server_id).ok_or("MCP 服务器未连接，请先连接")?
    };

    let result = rpc_call(
        &handle.writer,
        &mut handle.rx,
        "tools/call",
        json!({ "name": tool, "arguments": args }),
    )
    .await;

    // 放回连接池
    {
        let mut map = app.mcp.lock().map_err(|e| e.to_string())?;
        map.insert(server_id.to_string(), handle);
    }
    result
}

/// 工具返回内容提取：content 数组中的 text / json 字段拼接
pub fn tool_output_to_string(result: &Value) -> String {
    let contents = result.get("content").and_then(Value::as_array);
    let mut text = String::new();
    if let Some(contents) = contents {
        for c in contents {
            if let Some(t) = c.get("text").and_then(Value::as_str) {
                text.push_str(t);
            }
        }
    }
    if !text.is_empty() {
        return text;
    }
    result.to_string()
}

/// 供 Skills 引擎调用的便捷入口：按名称/任意已连接服务器定位
pub async fn call_tool_by_name(
    app: &AppState,
    server_hint: Option<&str>,
    tool: &str,
    args_json: &str,
) -> Result<String, String> {
    let args: Value = if args_json.trim().is_empty() {
        json!({})
    } else {
        serde_json::from_str(args_json).map_err(|e| format!("工具参数不是合法 JSON: {e}"))?
    };

    let candidates: Vec<String> = {
        let map = app.mcp.lock().map_err(|e| e.to_string())?;
        map.keys().cloned().collect()
    };
    if candidates.is_empty() {
        return Err("没有已连接的 MCP 服务器".to_string());
    }

    // 优先按 hint 精确匹配（id 或 name），否则取第一个已连接服务器
    let server_id = match server_hint {
        Some(hint) => candidates
            .iter()
            .find(|id| **id == hint)
            .cloned()
            .or_else(|| {
                candidates.iter().find(|id| {
                    app.db()
                        .ok()
                        .and_then(|conn| load_server(&conn, id).ok())
                        .map(|s| s.name == hint)
                        .unwrap_or(false)
                }).cloned()
            })
            .ok_or_else(|| format!("MCP 服务器 {hint} 未连接"))?,
        None => candidates[0].clone(),
    };

    let result = call_tool(app, &server_id, tool, args).await?;
    Ok(tool_output_to_string(&result))
}

/// 通用 MCP 服务器 CRUD（供 commands/mcp.rs 复用）
pub(crate) fn upsert_server(
    conn: &rusqlite::Connection,
    input: McpServerInput,
) -> Result<McpServerInfo, String> {
    use crate::utils;
    let args_json = serde_json::to_string(input.args.as_deref().unwrap_or(&[])).map_err(|e| e.to_string())?;
    let env_json = input
        .env
        .as_ref()
        .map(|v| serde_json::to_string(v).map_err(|e| e.to_string()))
        .transpose()?;

    let id = match input.id.clone() {
        Some(id) => {
            conn.execute(
                "UPDATE mcp_servers SET name=?2, transport=?3, command=?4, args=?5, env=?6, url=?7 WHERE id=?1",
                rusqlite::params![
                    id,
                    input.name,
                    input.transport,
                    input.command,
                    args_json,
                    env_json,
                    input.url,
                ],
            )
            .map_err(|e| format!("更新 MCP 服务器失败: {e}"))?;
            id
        }
        None => {
            let id = utils::new_id();
            conn.execute(
                "INSERT INTO mcp_servers (id, name, transport, command, args, env, url, status, tools_json, error_message)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'disconnected', '[]', NULL)",
                rusqlite::params![
                    id,
                    input.name,
                    input.transport,
                    input.command,
                    args_json,
                    env_json,
                    input.url,
                ],
            )
            .map_err(|e| format!("创建 MCP 服务器失败: {e}"))?;
            id
        }
    };
    load_server(conn, &id)
}

pub(crate) fn delete_server(conn: &rusqlite::Connection, id: &str) -> Result<(), String> {
    conn.execute("DELETE FROM mcp_servers WHERE id = ?1", rusqlite::params![id])
        .map_err(|e| format!("删除 MCP 服务器失败: {e}"))?;
    Ok(())
}
