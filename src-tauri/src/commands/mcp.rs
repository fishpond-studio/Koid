//! MCP 命令（§4.8）：服务器 CRUD + 连接/断开 + 工具调用

use crate::models::{McpServerInfo, McpServerInput, McpTool};
use crate::services::mcp;
use crate::state::AppState;
use tauri::State;

fn load_all(conn: &rusqlite::Connection) -> Result<Vec<McpServerInfo>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, name, transport, command, args, env, url, status, tools_json, error_message
             FROM mcp_servers ORDER BY name",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], mcp::row_to_server)
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

#[tauri::command]
pub fn list_mcp_servers(state: State<'_, AppState>) -> Result<Vec<McpServerInfo>, String> {
    let conn = state.db()?;
    load_all(&conn)
}

#[tauri::command]
pub fn save_mcp_server(
    input: McpServerInput,
    state: State<'_, AppState>,
) -> Result<McpServerInfo, String> {
    let conn = state.db()?;
    mcp::upsert_server(&conn, input)
}

#[tauri::command]
pub fn delete_mcp_server(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let conn = state.db()?;
    mcp::delete_server(&conn, &id)?;
    Ok(())
}

/// 连接并发现工具（stdio 握手）
#[tauri::command]
pub async fn connect_mcp_server(
    id: String,
    state: State<'_, AppState>,
) -> Result<Vec<McpTool>, String> {
    mcp::connect(&state, &id).await
}

#[tauri::command]
pub async fn disconnect_mcp_server(
    id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    mcp::disconnect(&state, &id).await
}

/// 手动测试工具调用（MCP 管理界面 / Skill tool 步骤共用）
#[tauri::command]
pub async fn call_mcp_tool(
    server_id: String,
    tool: String,
    args: Option<serde_json::Value>,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let result = mcp::call_tool(&state, &server_id, &tool, args.unwrap_or_default()).await?;
    Ok(mcp::tool_output_to_string(&result))
}
