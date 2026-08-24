//! 提示词库命令（§4.6）：Snippets / Templates / System Prompt + 版本历史
//!
//! 约定：
//! - 变量从 content 的 {{var}} 自动解析（与前端保持一致的提取规则）
//! - builtin: 前缀为内置模板，禁止删除
//! - 每次更新前把旧内容快照写入 prompt_versions，每模板仅保留最近 10 份

use crate::models::{Prompt, PromptInput, PromptVersion};
use crate::state::AppState;
use crate::utils;
use rusqlite::{params, Connection};
use tauri::State;

const MAX_VERSIONS: i64 = 10;

/// 提取 {{var}} 变量名：保持出现顺序并去重
fn parse_variables(content: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut rest = content;
    while let Some(start) = rest.find("{{") {
        let after = &rest[start + 2..];
        if let Some(end) = after.find("}}") {
            let name = after[..end].trim();
            // 仅接受合法标识符，避免把 {{ a ? b : c }} 之类表达式当变量
            if !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
                && !out.iter().any(|v| v == name)
            {
                out.push(name.to_string());
            }
            rest = &after[end + 2..];
        } else {
            break;
        }
    }
    out
}

fn json_vec(v: &[String]) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "[]".to_string())
}

fn parse_json_vec(s: Option<String>) -> Vec<String> {
    s.and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub(crate) fn load_prompts(conn: &Connection) -> Result<Vec<Prompt>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, title, content, variables, type, tags, usage_count, created_at
             FROM prompts ORDER BY usage_count DESC, created_at DESC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            let vars: Option<String> = row.get(3)?;
            let tags: Option<String> = row.get(5)?;
            Ok(Prompt {
                id: row.get(0)?,
                title: row.get(1)?,
                content: row.get(2)?,
                variables: parse_json_vec(vars),
                r#type: row.get(4)?,
                tags: parse_json_vec(tags),
                usage_count: row.get(6)?,
                created_at: row.get(7)?,
            })
        })
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

fn snapshot_version(conn: &Connection, prompt_id: &str, content: &str) -> Result<(), String> {
    conn.execute(
        "INSERT INTO prompt_versions (id, prompt_id, content, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![utils::new_id(), prompt_id, content, utils::now_ms()],
    )
    .map_err(|e| format!("写入版本历史失败: {e}"))?;

    // 仅保留最近 MAX_VERSIONS 份：删除更旧的快照
    conn.execute(
        &format!(
            "DELETE FROM prompt_versions WHERE prompt_id = ?1 AND id NOT IN (
               SELECT id FROM prompt_versions WHERE prompt_id = ?1
               ORDER BY created_at DESC LIMIT {MAX_VERSIONS}
             )"
        ),
        params![prompt_id],
    )
    .map_err(|e| format!("清理版本历史失败: {e}"))?;
    Ok(())
}

#[tauri::command]
pub fn list_prompts(state: State<'_, AppState>) -> Result<Vec<Prompt>, String> {
    let conn = state.db()?;
    load_prompts(&conn)
}

#[tauri::command]
pub fn save_prompt(input: PromptInput, state: State<'_, AppState>) -> Result<Prompt, String> {
    if !matches!(input.prompt_type.as_str(), "system" | "snippet" | "template") {
        return Err(format!("未知的提示词类型: {}", input.prompt_type));
    }
    if input.title.trim().is_empty() {
        return Err("标题不能为空".to_string());
    }

    let conn = state.db()?;
    let vars = json_vec(&parse_variables(&input.content));
    let tags = json_vec(&input.tags.clone().unwrap_or_default());

    let id = match input.id.clone() {
        Some(id) => {
            // 更新前先快照旧内容（版本历史，§4.6）
            let old: String = conn
                .query_row("SELECT content FROM prompts WHERE id = ?1", params![id], |r| {
                    r.get(0)
                })
                .map_err(|_| format!("提示词不存在: {id}"))?;
            snapshot_version(&conn, &id, &old)?;

            conn.execute(
                "UPDATE prompts SET title=?2, content=?3, variables=?4, tags=?5 WHERE id=?1",
                params![id, input.title.trim(), input.content, vars, tags],
            )
            .map_err(|e| format!("更新提示词失败: {e}"))?;
            id
        }
        None => {
            let id = utils::new_id();
            conn.execute(
                "INSERT INTO prompts (id, title, content, variables, type, tags, usage_count, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7)",
                params![
                    id,
                    input.title.trim(),
                    input.content,
                    vars,
                    input.prompt_type,
                    tags,
                    utils::now_ms(),
                ],
            )
            .map_err(|e| format!("创建提示词失败: {e}"))?;
            id
        }
    };

    conn.query_row(
        "SELECT id, title, content, variables, type, tags, usage_count, created_at
         FROM prompts WHERE id = ?1",
        params![id],
        |row| {
            let vars: Option<String> = row.get(3)?;
            let tags: Option<String> = row.get(5)?;
            Ok(Prompt {
                id: row.get(0)?,
                title: row.get(1)?,
                content: row.get(2)?,
                variables: parse_json_vec(vars),
                r#type: row.get(4)?,
                tags: parse_json_vec(tags),
                usage_count: row.get(6)?,
                created_at: row.get(7)?,
            })
        },
    )
    .map_err(|e| format!("读取提示词失败: {e}"))
}

#[tauri::command]
pub fn delete_prompt(id: String, state: State<'_, AppState>) -> Result<(), String> {
    if id.starts_with("builtin:") {
        return Err("FORBIDDEN:内置模板不可删除".to_string());
    }
    let conn = state.db()?;
    conn.execute("DELETE FROM prompts WHERE id = ?1", params![id])
        .map_err(|e| format!("删除提示词失败: {e}"))?;
    Ok(())
}

/// 使用计数自增（Snippet 插入 / 模板使用 / 设为 System Prompt 时调用）
#[tauri::command]
pub fn bump_prompt_usage(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let conn = state.db()?;
    conn.execute(
        "UPDATE prompts SET usage_count = usage_count + 1 WHERE id = ?1",
        params![id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// 版本历史：按时间倒序（最新在前）
#[tauri::command]
pub fn list_prompt_versions(
    prompt_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<PromptVersion>, String> {
    let conn = state.db()?;
    let mut stmt = conn
        .prepare(
            "SELECT id, prompt_id, content, created_at FROM prompt_versions
             WHERE prompt_id = ?1 ORDER BY created_at DESC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![prompt_id], |row| {
            Ok(PromptVersion {
                id: row.get(0)?,
                prompt_id: row.get(1)?,
                content: row.get(2)?,
                created_at: row.get(3)?,
            })
        })
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variables_extracted_in_order_and_deduped() {
        let v = parse_variables("用 {{language}} 解释 {{code}}，再看 {{code}} 和 {{bad x}}");
        // 重复去重、非法标识符被忽略、顺序保持
        assert_eq!(v, vec!["language", "code"]);
    }

    #[test]
    fn variables_handles_empty_and_unterminated() {
        assert!(parse_variables("").is_empty());
        assert!(parse_variables("无变量文本").is_empty());
        // 未闭合的 {{ 不产生变量
        assert!(parse_variables("半截 {{var").is_empty());
    }
}
