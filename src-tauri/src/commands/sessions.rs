//! 会话管理（§4.5）：列表/创建/更新/删除 + 分支 + 全局搜索

use crate::models::{Message, MessageHit, SearchResults, Session, SessionInput};
use crate::state::AppState;
use crate::utils;
use rusqlite::{params, Connection, Row};
use tauri::State;

const SELECT_COLS: &str = "id, title, folder_id, model_id, system_prompt, temperature,
    top_p, max_tokens, created_at, updated_at, is_pinned, is_archived, workspace_id, thinking_level";

/// 行 → Session 映射：列表/单查/搜索共用，避免多份拷贝
fn session_from_row(row: &Row) -> rusqlite::Result<Session> {
    Ok(Session {
        id: row.get(0)?,
        title: row.get(1)?,
        folder_id: row.get(2)?,
        model_id: row.get(3)?,
        system_prompt: row.get(4)?,
        temperature: row.get(5)?,
        top_p: row.get(6)?,
        max_tokens: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
        is_pinned: row.get(10)?,
        is_archived: row.get(11)?,
        workspace_id: row.get(12)?,
        thinking_level: row.get(13)?,
    })
}

pub(crate) fn load_sessions(conn: &Connection) -> Result<Vec<Session>, String> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {SELECT_COLS} FROM sessions
             ORDER BY is_pinned DESC, updated_at DESC"
        ))
        .map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], session_from_row).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

pub(crate) fn load_session(conn: &Connection, id: &str) -> Result<Session, String> {
    conn.query_row(
        &format!("SELECT {SELECT_COLS} FROM sessions WHERE id = ?1"),
        params![id],
        session_from_row,
    )
    .map_err(|_| format!("会话不存在: {id}"))
}

#[tauri::command]
pub fn list_sessions(state: State<'_, AppState>) -> Result<Vec<Session>, String> {
    let conn = state.db()?;
    load_sessions(&conn)
}

/// 无 id = 新建（时间戳自动生成）；有 id = 差量更新（未携带字段保留原值）
#[tauri::command]
pub fn save_session(
    input: SessionInput,
    state: State<'_, AppState>,
) -> Result<Session, String> {
    let conn = state.db()?;
    let now = utils::now_ms();

    let id = match input.id.clone() {
        Some(id) => {
            let cur = load_session(&conn, &id)?;
            conn.execute(
                "UPDATE sessions SET title=?2, folder_id=?3, model_id=?4, system_prompt=?5,
                 temperature=?6, top_p=?7, max_tokens=?8, updated_at=?9,
                 is_pinned=?10, is_archived=?11, workspace_id=?12, thinking_level=?13 WHERE id=?1",
                params![
                    id,
                    input.title.unwrap_or(cur.title),
                    // 差量更新：未提供的字段保留原值，避免把 model_id/system_prompt 清空
                    input.folder_id.or(cur.folder_id),
                    input.model_id.or(cur.model_id),
                    input.system_prompt.or(cur.system_prompt),
                    input.temperature.or(cur.temperature),
                    input.top_p.or(cur.top_p),
                    input.max_tokens.or(cur.max_tokens),
                    now,
                    input.is_pinned.unwrap_or(cur.is_pinned),
                    input.is_archived.unwrap_or(cur.is_archived),
                    input.workspace_id.or(cur.workspace_id),
                    input.thinking_level.or(cur.thinking_level),
                ],
            )
            .map_err(|e| format!("更新会话失败: {e}"))?;
            id
        }
        None => {
            let id = utils::new_id();
            conn.execute(
                "INSERT INTO sessions
                 (id, title, folder_id, model_id, system_prompt, temperature, top_p,
                  max_tokens, created_at, updated_at, is_pinned, is_archived, workspace_id, thinking_level)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                params![
                    id,
                    input.title.unwrap_or_else(|| "新对话".to_string()),
                    input.folder_id,
                    input.model_id,
                    input.system_prompt,
                    input.temperature,
                    input.top_p,
                    input.max_tokens,
                    now,
                    now,
                    input.is_pinned.unwrap_or(false),
                    input.is_archived.unwrap_or(false),
                    // 未指定工作区时归入「默认工作区」
                    input.workspace_id.or_else(|| Some("default".to_string())),
                    input.thinking_level,
                ],
            )
            .map_err(|e| format!("创建会话失败: {e}"))?;
            id
        }
    };

    load_session(&conn, &id)
}

#[tauri::command]
pub fn delete_session(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let conn = state.db()?;
    // messages 通过 ON DELETE CASCADE 一并清理
    conn.execute("DELETE FROM sessions WHERE id = ?1", params![id])
        .map_err(|e| format!("删除会话失败: {e}"))?;
    Ok(())
}

/// 分支会话（§4.5.4）：从源会话复制配置 + 截止指定消息（含）的全部消息
///
/// parent_id 不继承：跨会话引用无意义，分支后自成一条链
#[tauri::command]
pub fn branch_session(
    session_id: String,
    up_to_message_id: String,
    title: String,
    state: State<'_, AppState>,
) -> Result<Session, String> {
    let conn = state.db()?;

    let src = load_session(&conn, &session_id)?;

    // 分支点必须属于源会话，防止跨会话伪造
    let cut: i64 = conn
        .query_row(
            "SELECT created_at FROM messages WHERE id = ?1 AND session_id = ?2",
            params![up_to_message_id, session_id],
            |r| r.get(0),
        )
        .map_err(|_| "分支点消息不存在".to_string())?;

    let now = utils::now_ms();
    let new_id = utils::new_id();
    conn.execute(
        "INSERT INTO sessions
         (id, title, folder_id, model_id, system_prompt, temperature, top_p,
          max_tokens, created_at, updated_at, is_pinned, is_archived, workspace_id, thinking_level)
         VALUES (?1, ?2, NULL, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0, 0, ?10, ?11)",
        params![
            new_id,
            title,
            src.model_id,
            src.system_prompt,
            src.temperature,
            src.top_p,
            src.max_tokens,
            now,
            now,
            src.workspace_id,
            src.thinking_level,
        ],
    )
    .map_err(|e| format!("创建分支会话失败: {e}"))?;

    // 复制截止分支点（含）之前的消息，顺序不变
    let mut stmt = conn
        .prepare(
            "SELECT role, content, reasoning, tool_calls, tool_results, tokens_used, latency_ms, created_at
             FROM messages WHERE session_id = ?1 AND created_at <= ?2
             ORDER BY created_at, rowid",
        )
        .map_err(|e| e.to_string())?;
    let rows: Vec<(String, String, Option<String>, Option<String>, Option<String>, Option<i64>, Option<i64>, i64)> = stmt
        .query_map(params![session_id, cut], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                r.get(7)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;

    for (role, content, reasoning, tool_calls, tool_results, tokens, latency, created) in rows {
        conn.execute(
            "INSERT INTO messages
             (id, session_id, role, content, reasoning, tool_calls, tool_results,
              tokens_used, latency_ms, created_at, parent_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, NULL)",
            params![
                utils::new_id(),
                new_id,
                role,
                content,
                reasoning,
                tool_calls,
                tool_results,
                tokens,
                latency,
                created,
            ],
        )
        .map_err(|e| format!("复制消息失败: {e}"))?;
    }

    load_session(&conn, &new_id)
}

/// 全局搜索（§4.5.3）：标题 + 消息内容，归档会话不参与
#[tauri::command]
pub fn search_sessions(query: String, state: State<'_, AppState>) -> Result<SearchResults, String> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(SearchResults {
            sessions: vec![],
            messages: vec![],
        });
    }
    let conn = state.db()?;
    let pattern = format!("%{q}%");

    // 标题命中（最近优先，上限 20）
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {SELECT_COLS} FROM sessions
             WHERE title LIKE ?1 AND is_archived = 0
             ORDER BY updated_at DESC LIMIT 20"
        ))
        .map_err(|e| e.to_string())?;
    let sessions: Vec<Session> = stmt
        .query_map(params![pattern], session_from_row)
        .map_err(|e| e.to_string())?
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;

    // 消息内容命中（JOIN 取会话标题，上限 20）
    let mut stmt = conn
        .prepare(
            "SELECT s.title, msg.id, msg.session_id, msg.role, msg.content, msg.reasoning,
                    msg.tokens_used, msg.latency_ms, msg.created_at
             FROM messages msg
             JOIN sessions s ON s.id = msg.session_id
             WHERE msg.content LIKE ?1 AND s.is_archived = 0
             ORDER BY msg.created_at DESC LIMIT 20",
        )
        .map_err(|e| e.to_string())?;
    let messages: Vec<MessageHit> = stmt
        .query_map(params![pattern], |r| {
            Ok(MessageHit {
                session_title: r.get(0)?,
                message: Message {
                    id: r.get(1)?,
                    session_id: r.get(2)?,
                    role: r.get(3)?,
                    content: r.get(4)?,
                    reasoning: r.get(5)?,
                    tool_calls: None,
                    tool_results: None,
                    tokens_used: r.get(6)?,
                    latency_ms: r.get(7)?,
                    created_at: r.get(8)?,
                    parent_id: None,
                },
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;

    Ok(SearchResults { sessions, messages })
}
