//! 供应商 CRUD（§4.2）
//! Key 只写 keyring，DB 中 api_key 列恒为空串（§7.4）

use crate::models::{Provider, ProviderInput, ProviderType, ProxyType};
use crate::services::keyring;
use crate::state::AppState;
use crate::utils;
use rusqlite::{params, Connection};
use tauri::State;

type RawRow = (
    String,          // id
    String,          // name
    String,          // type
    String,          // base_url
    String,          // proxy_type
    Option<String>,  // proxy_url
    i64,             // timeout
    i64,             // retries
    i64,             // order_index
    bool,            // enabled
);

pub(crate) fn type_to_db(t: ProviderType) -> &'static str {
    match t {
        ProviderType::OpenAiCompatible => "openai-compatible",
        ProviderType::Anthropic => "anthropic",
        ProviderType::OpenAiResponse => "openai-response",
        ProviderType::Ollama => "ollama",
        ProviderType::Custom => "custom",
    }
}

pub(crate) fn type_from_db(s: &str) -> Result<ProviderType, String> {
    match s {
        "openai-compatible" => Ok(ProviderType::OpenAiCompatible),
        "anthropic" => Ok(ProviderType::Anthropic),
        "openai-response" => Ok(ProviderType::OpenAiResponse),
        "ollama" => Ok(ProviderType::Ollama),
        "custom" => Ok(ProviderType::Custom),
        other => Err(format!("未知的供应商类型: {other}")),
    }
}

pub(crate) fn proxy_to_db(p: ProxyType) -> &'static str {
    match p {
        ProxyType::Direct => "direct",
        ProxyType::Http => "http",
        ProxyType::Socks5 => "socks5",
    }
}

pub(crate) fn proxy_from_db(s: &str) -> Result<ProxyType, String> {
    match s {
        "direct" => Ok(ProxyType::Direct),
        "http" => Ok(ProxyType::Http),
        "socks5" => Ok(ProxyType::Socks5),
        other => Err(format!("未知的代理类型: {other}")),
    }
}

fn raw_to_provider(raw: RawRow) -> Result<Provider, String> {
    Ok(Provider {
        id: raw.0.clone(),
        name: raw.1,
        provider_type: type_from_db(&raw.2)?,
        base_url: raw.3,
        // Key 不进 DB：从 keyring 读出来做掩码展示
        api_key_masked: keyring::get_api_key(&raw.0).map(|k| keyring::mask_key(&k)),
        proxy_type: proxy_from_db(&raw.4)?,
        proxy_url: raw.5,
        timeout: raw.6,
        retries: raw.7,
        order_index: raw.8,
        enabled: raw.9,
    })
}

const SELECT_COLS: &str =
    "id, name, type, base_url, proxy_type, proxy_url, timeout, retries, order_index, enabled";

pub(crate) fn load_providers(conn: &Connection) -> Result<Vec<Provider>, String> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {SELECT_COLS} FROM providers ORDER BY order_index, name"
        ))
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, i64>(7)?,
                row.get::<_, i64>(8)?,
                row.get::<_, bool>(9)?,
            ))
        })
        .map_err(|e| e.to_string())?;

    let mut out = Vec::new();
    for r in rows {
        out.push(raw_to_provider(r.map_err(|e| e.to_string())?)?);
    }
    Ok(out)
}

pub(crate) fn load_provider(conn: &Connection, id: &str) -> Result<Provider, String> {
    let raw: RawRow = conn
        .query_row(
            &format!("SELECT {SELECT_COLS} FROM providers WHERE id = ?1"),
            params![id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                    row.get(9)?,
                ))
            },
        )
        .map_err(|_| format!("供应商不存在: {id}"))?;
    raw_to_provider(raw)
}

#[tauri::command]
pub fn list_providers(state: State<'_, AppState>) -> Result<Vec<Provider>, String> {
    let conn = state.db()?;
    load_providers(&conn)
}

/// 新建或更新供应商；api_key 非空时写入系统 keyring
#[tauri::command]
pub fn save_provider(
    input: ProviderInput,
    state: State<'_, AppState>,
) -> Result<Provider, String> {
    let conn = state.db()?;

    // 先克隆 id，避免后续字段移动与借用冲突
    let id = match input.id.clone() {
        Some(id) => {
            // 读-改-写：未携带的字段保留原值，前端只需提交变更项
            let cur = load_provider(&conn, &id)?;
            conn.execute(
                "UPDATE providers SET name=?2, type=?3, base_url=?4, proxy_type=?5,
                 proxy_url=?6, timeout=?7, retries=?8, order_index=?9, enabled=?10
                 WHERE id=?1",
                params![
                    id,
                    input.name,
                    type_to_db(input.provider_type),
                    input.base_url,
                    proxy_to_db(input.proxy_type.unwrap_or(cur.proxy_type)),
                    input.proxy_url,
                    input.timeout.unwrap_or(cur.timeout),
                    input.retries.unwrap_or(cur.retries),
                    input.order_index.unwrap_or(cur.order_index),
                    input.enabled.unwrap_or(cur.enabled),
                ],
            )
            .map_err(|e| format!("更新供应商失败: {e}"))?;
            id.clone()
        }
        None => {
            let id = utils::new_id();
            conn.execute(
                "INSERT INTO providers
                 (id, name, type, base_url, proxy_type, proxy_url, timeout, retries, order_index, enabled)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    id,
                    input.name,
                    type_to_db(input.provider_type),
                    input.base_url,
                    proxy_to_db(input.proxy_type.unwrap_or(ProxyType::Direct)),
                    input.proxy_url,
                    input.timeout.unwrap_or(60),
                    input.retries.unwrap_or(2),
                    input.order_index.unwrap_or(0),
                    input.enabled.unwrap_or(true),
                ],
            )
            .map_err(|e| format!("创建供应商失败: {e}"))?;
            id
        }
    };

    if let Some(key) = input.api_key.filter(|k| !k.trim().is_empty()) {
        keyring::set_api_key(&id, key.trim())?;
    }

    load_provider(&conn, &id)
}

#[tauri::command]
pub fn delete_provider(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let conn = state.db()?;
    conn.execute("DELETE FROM providers WHERE id = ?1", params![id])
        .map_err(|e| format!("删除供应商失败: {e}"))?;
    keyring::delete_api_key(&id);
    Ok(())
}
