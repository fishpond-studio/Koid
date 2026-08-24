//! 消息持久化（§4.5 自动保存由前端 3s 节流后调用 append_message）

use crate::models::{Message, MessageInput};
use crate::state::AppState;
use crate::utils;
use rusqlite::params;
use tauri::State;

#[tauri::command]
pub fn list_messages(
    session_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<Message>, String> {
    let conn = state.db()?;
    let mut stmt = conn
        .prepare(
            "SELECT id, session_id, role, content, reasoning, tool_calls, tool_results,
                    tokens_used, latency_ms, created_at, parent_id
             FROM messages WHERE session_id = ?1 ORDER BY created_at, rowid",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![session_id], |row| {
            let tool_calls: Option<String> = row.get(5)?;
            let tool_results: Option<String> = row.get(6)?;
            Ok(Message {
                id: row.get(0)?,
                session_id: row.get(1)?,
                role: row.get(2)?,
                content: row.get(3)?,
                reasoning: row.get(4)?,
                tool_calls: tool_calls.and_then(|s| serde_json::from_str(&s).ok()),
                tool_results: tool_results.and_then(|s| serde_json::from_str(&s).ok()),
                tokens_used: row.get(7)?,
                latency_ms: row.get(8)?,
                created_at: row.get(9)?,
                parent_id: row.get(10)?,
            })
        })
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

/// 追加消息并同步刷新会话的 updated_at（保证侧边栏排序正确）
#[tauri::command]
pub fn append_message(
    input: MessageInput,
    state: State<'_, AppState>,
) -> Result<Message, String> {
    let conn = state.db()?;
    let id = utils::new_id();
    let now = utils::now_ms();

    let tool_calls = match &input.tool_calls {
        Some(v) => Some(serde_json::to_string(v).map_err(|e| e.to_string())?),
        None => None,
    };
    let tool_results = match &input.tool_results {
        Some(v) => Some(serde_json::to_string(v).map_err(|e| e.to_string())?),
        None => None,
    };

    conn.execute(
        "INSERT INTO messages
         (id, session_id, role, content, reasoning, tool_calls, tool_results,
          tokens_used, latency_ms, created_at, parent_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            id,
            input.session_id,
            input.role,
            input.content,
            input.reasoning,
            tool_calls,
            tool_results,
            input.tokens_used,
            input.latency_ms,
            now,
            input.parent_id,
        ],
    )
    .map_err(|e| format!("写入消息失败: {e}"))?;

    conn.execute(
        "UPDATE sessions SET updated_at = ?2 WHERE id = ?1",
        params![input.session_id, now],
    )
    .map_err(|e| e.to_string())?;

    Ok(Message {
        id,
        session_id: input.session_id,
        role: input.role,
        content: input.content,
        reasoning: input.reasoning,
        tool_calls: input.tool_calls,
        tool_results: input.tool_results,
        tokens_used: input.tokens_used,
        latency_ms: input.latency_ms,
        created_at: now,
        parent_id: input.parent_id,
    })
}

#[tauri::command]
pub fn delete_message(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let conn = state.db()?;
    conn.execute("DELETE FROM messages WHERE id = ?1", params![id])
        .map_err(|e| format!("删除消息失败: {e}"))?;
    Ok(())
}

/// 删除某条消息及其之后的全部消息（撤回 / 编辑重发的截断语义）。
/// 排序口径与 list_messages 一致（created_at, rowid）。
/// 返回删除条数。
#[tauri::command]
pub fn delete_messages_from(
    session_id: String,
    message_id: String,
    state: State<'_, AppState>,
) -> Result<u32, String> {
    let conn = state.db()?;
    let mut stmt = conn
        .prepare("SELECT id FROM messages WHERE session_id = ?1 ORDER BY created_at, rowid")
        .map_err(|e| e.to_string())?;
    let ids: Vec<String> = stmt
        .query_map(params![session_id], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    let Some(idx) = ids.iter().position(|id| id == &message_id) else {
        return Err(format!("消息不存在: {message_id}"));
    };

    let mut deleted: u32 = 0;
    for id in &ids[idx..] {
        deleted += conn
            .execute("DELETE FROM messages WHERE id = ?1", params![id])
            .map_err(|e| format!("删除消息失败: {e}"))? as u32;
    }
    Ok(deleted)
}
