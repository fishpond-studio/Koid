//! 模型 CRUD + 模型发现（§4.2：/v1/models 自动拉取）

use crate::models::{DiscoveredModel, Model, ModelInput};
use crate::state::AppState;
use crate::utils;
use rusqlite::{params, Connection};
use tauri::State;

const SELECT_COLS: &str =
    "id, provider_id, model_id, display_name, context_window, capabilities, enabled";

fn parse_caps(s: Option<String>) -> Vec<String> {
    s.and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub(crate) fn load_models(conn: &Connection, provider_id: Option<&str>) -> Result<Vec<Model>, String> {
    let (sql, use_param) = match provider_id {
        Some(_) => (
            format!("SELECT {SELECT_COLS} FROM models WHERE provider_id = ?1 ORDER BY display_name"),
            true,
        ),
        None => (format!("SELECT {SELECT_COLS} FROM models ORDER BY display_name"), false),
    };
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let mapper = |row: &rusqlite::Row| -> rusqlite::Result<Model> {
        let caps: Option<String> = row.get(5)?;
        let ctx: Option<i64> = row.get(4)?;
        Ok(Model {
            id: row.get(0)?,
            provider_id: row.get(1)?,
            model_id: row.get(2)?,
            display_name: row.get(3)?,
            context_window: ctx,
            capabilities: parse_caps(caps),
            enabled: row.get(6)?,
        })
    };
    let rows = if use_param {
        stmt.query_map(params![provider_id.unwrap_or_default()], mapper)
            .map_err(|e| e.to_string())?
    } else {
        stmt.query_map([], mapper).map_err(|e| e.to_string())?
    };
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

#[tauri::command]
pub fn list_models(
    provider_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<Model>, String> {
    let conn = state.db()?;
    load_models(&conn, provider_id.as_deref())
}

#[tauri::command]
pub fn save_model(input: ModelInput, state: State<'_, AppState>) -> Result<Model, String> {
    let conn = state.db()?;
    // display_name 缺省直接用 model_id，减少用户输入负担
    let display = input
        .display_name
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| input.model_id.clone());
    let caps = serde_json::to_string(&input.capabilities.clone().unwrap_or_default())
        .map_err(|e| e.to_string())?;

    let id = match input.id.clone() {
        Some(id) => {
            conn.execute(
                "UPDATE models SET provider_id=?2, model_id=?3, display_name=?4,
                 context_window=?5, capabilities=?6, enabled=?7 WHERE id=?1",
                params![
                    id,
                    input.provider_id,
                    input.model_id,
                    display,
                    input.context_window,
                    caps,
                    input.enabled.unwrap_or(true),
                ],
            )
            .map_err(|e| format!("更新模型失败: {e}"))?;
            id
        }
        None => {
            let id = utils::new_id();
            conn.execute(
                "INSERT INTO models
                 (id, provider_id, model_id, display_name, context_window, capabilities, enabled)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    id,
                    input.provider_id,
                    input.model_id,
                    display,
                    input.context_window,
                    caps,
                    input.enabled.unwrap_or(true),
                ],
            )
            .map_err(|e| format!("创建模型失败: {e}"))?;
            id
        }
    };

    conn.query_row(
        &format!("SELECT {SELECT_COLS} FROM models WHERE id = ?1"),
        params![id],
        |row| {
            let caps: Option<String> = row.get(5)?;
            let ctx: Option<i64> = row.get(4)?;
            Ok(Model {
                id: row.get(0)?,
                provider_id: row.get(1)?,
                model_id: row.get(2)?,
                display_name: row.get(3)?,
                context_window: ctx,
                capabilities: parse_caps(caps),
                enabled: row.get(6)?,
            })
        },
    )
    .map_err(|e| format!("读取模型失败: {e}"))
}

#[tauri::command]
pub fn delete_model(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let conn = state.db()?;
    conn.execute("DELETE FROM models WHERE id = ?1", params![id])
        .map_err(|e| format!("删除模型失败: {e}"))?;
    Ok(())
}

/// 模型发现 URL 候选（§4.2）：base 已含 /v1 直接用，否则拼接 /v1/models
/// 返回两个候选，前者优先；兼容部分第三方只暴露 /models 的服务
fn models_url_candidates(base: &str) -> Vec<String> {
    let b = base.trim_end_matches('/');
    if b.ends_with("/v1") {
        vec![format!("{b}/models"), format!("{b}/models")]
    } else {
        vec![format!("{b}/v1/models"), format!("{b}/models")]
    }
}

/// 模型发现（§4.2）：请求供应商 `/v1/models`，解析模型列表并合并本地启用状态
///
/// - OpenAI 兼容：`Authorization: Bearer`；Anthropic：`x-api-key`
/// - URL 依次尝试 `/v1/models` → `/models`（部分服务没有 /v1 前缀）
/// - 返回值 `enabled` 表示该模型是否已在本地库启用（勾选预置）
#[tauri::command]
pub async fn discover_models(
    provider_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<DiscoveredModel>, String> {
    use crate::models::ProviderType;
    use crate::services::{keyring, proxy, settings};

    // 1. 读取供应商 + 全局代理
    let (provider, global_proxy) = {
        let conn = state.db()?;
        (
            crate::commands::providers::load_provider(&conn, &provider_id)?,
            settings::get_global_proxy(&conn),
        )
    };

    // 2. Key：Ollama 本地无需 Key，其余缺 Key 直接失败
    let api_key = keyring::get_api_key(&provider_id);
    if provider.provider_type != ProviderType::Ollama && api_key.is_none() {
        return Err(format!(
            "UNAUTHORIZED:供应商「{}」未配置 API Key",
            provider.name
        ));
    }

    // 3. 代理解析（供应商 > 全局 > 环境变量）并构建客户端
    let (ptype, purl) = proxy::resolve_proxy(&provider, global_proxy.as_ref());
    let client = proxy::build_client(ptype, purl.as_deref(), provider.timeout.max(1) as u64, false)?;

    // 4. 依次尝试 URL 候选，取第一个成功的响应
    let mut last_status: Option<u16> = None;
    let mut last_body = String::new();
    let mut last_err: Option<String> = None;
    for url in models_url_candidates(&provider.base_url) {
        let mut req = client.get(&url);
        req = match provider.provider_type {
            ProviderType::Anthropic => req.header("x-api-key", api_key.clone().unwrap_or_default()),
            _ => req.bearer_auth(api_key.clone().unwrap_or_default()),
        };

        let resp = match req.send().await {
            Ok(r) => r,
            Err(e) => {
                last_err = Some(if e.is_timeout() {
                    "TIMEOUT:请求超时".to_string()
                } else {
                    format!("NETWORK:{e}")
                });
                continue;
            }
        };
        let status = resp.status();
        if status.is_success() {
            let json = resp
                .json::<serde_json::Value>()
                .await
                .map_err(|e| format!("解析模型列表失败: {e}"))?;
            // 直接解析，成功即用
            if let Some(parsed) = parse_models_json(&json) {
                return Ok(merge_with_local(&state, &provider_id, parsed)?);
            }
            // 解析为空视为该候选无效，继续下一个
            last_body = format!("服务器返回的 data 为空: {status}");
            continue;
        }
        last_status = Some(status.as_u16());
        last_body = resp.text().await.unwrap_or_default();
    }
    if let Some(code) = last_status {
        let code_str = match code {
            401 | 403 => "UNAUTHORIZED",
            429 => "RATE_LIMITED",
            500..=599 => "SERVER",
            _ => "BAD_REQUEST",
        };
        return Err(format!(
            "{code_str}:HTTP {code} {}",
            last_body.chars().take(300).collect::<String>()
        ));
    }
    if let Some(e) = last_err {
        return Err(e);
    }
    Err("服务器未返回可用模型列表".to_string())
}

/// 解析 `/v1/models` 响应体 data[].id（兼容 Anthropic display_name）
fn parse_models_json(v: &serde_json::Value) -> Option<Vec<(String, String)>> {
    let arr = v.get("data")?.as_array()?;
    let mut raw = Vec::new();
    for item in arr {
        let mid = item.get("id")?.as_str()?;
        let name = item
            .get("display_name")
            .and_then(|n| n.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(mid);
        raw.push((mid.to_string(), name.to_string()));
    }
    if raw.is_empty() {
        None
    } else {
        Some(raw)
    }
}

/// 合并本地启用状态
fn merge_with_local(
    state: &State<'_, AppState>,
    provider_id: &str,
    raw: Vec<(String, String)>,
) -> Result<Vec<DiscoveredModel>, String> {
    let existing = {
        let conn = state.db()?;
        load_models(&conn, Some(provider_id))?
    };
    let enabled_map: std::collections::HashMap<&str, bool> = existing
        .iter()
        .map(|m| (m.model_id.as_str(), m.enabled))
        .collect();
    Ok(raw
        .into_iter()
        .map(|(model_id, display_name)| {
            let enabled = enabled_map.get(model_id.as_str()).copied().unwrap_or(false);
            DiscoveredModel {
                model_id,
                display_name,
                enabled,
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn models_url_candidates_handle_v1_and_bare() {
        assert_eq!(
            models_url_candidates("https://api.openai.com/v1"),
            vec![
                "https://api.openai.com/v1/models".to_string(),
                "https://api.openai.com/v1/models".to_string()
            ]
        );
        assert_eq!(
            models_url_candidates("https://gateway.example.com"),
            vec![
                "https://gateway.example.com/v1/models".to_string(),
                "https://gateway.example.com/models".to_string()
            ]
        );
        // 结尾斜杠被去除
        assert_eq!(
            models_url_candidates("https://api.anthropic.com/")[0],
            "https://api.anthropic.com/v1/models".to_string()
        );
    }

    #[test]
    fn parse_models_json_openai_and_anthropic() {
        let openai = serde_json::json!({
            "object": "list",
            "data": [
                { "id": "gpt-4o", "object": "model" },
                { "id": "gpt-4o-mini", "object": "model" }
            ]
        });
        let parsed = parse_models_json(&openai).unwrap();
        assert_eq!(parsed[0].0, "gpt-4o");
        assert_eq!(parsed[0].1, "gpt-4o"); // 无 display_name 回退 id

        let anthropic = serde_json::json!({
            "data": [
                { "type": "model", "id": "claude-3-5-sonnet-20241022", "display_name": "Claude 3.5 Sonnet" }
            ]
        });
        let parsed = parse_models_json(&anthropic).unwrap();
        assert_eq!(parsed[0].0, "claude-3-5-sonnet-20241022");
        assert_eq!(parsed[0].1, "Claude 3.5 Sonnet");
    }

    #[test]
    fn parse_models_json_rejects_empty_or_malformed() {
        assert!(parse_models_json(&serde_json::json!({ "data": [] })).is_none());
        assert!(parse_models_json(&serde_json::json!({ "models": [] })).is_none());
    }
}
