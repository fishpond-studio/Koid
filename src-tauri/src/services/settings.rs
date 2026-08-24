//! 键值设置读写（settings 表）+ 常用配置解析

use crate::models::{FailoverConfig, GlobalProxy};
use rusqlite::{params, Connection};

pub const KEY_GLOBAL_PROXY: &str = "global_proxy";
pub const KEY_FAILOVER_CONFIG: &str = "failover_config";
/// 关闭行为：hide（隐藏到托盘）/ quit（退出）/ ask（每次询问，默认）
pub const KEY_CLOSE_MODE: &str = "close_mode";

pub fn get(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row(
        "SELECT value FROM settings WHERE key = ?1",
        params![key],
        |row| row.get(0),
    )
    .ok()
}

pub fn set(conn: &Connection, key: &str, value: &str) -> Result<(), String> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )
    .map_err(|e| format!("保存设置失败: {e}"))?;
    Ok(())
}

/// 全局代理：未配置或 JSON 损坏时返回 None（等价 direct）
pub fn get_global_proxy(conn: &Connection) -> Option<GlobalProxy> {
    get(conn, KEY_GLOBAL_PROXY).and_then(|v| serde_json::from_str(&v).ok())
}

/// 故障转移配置：未配置或 JSON 损坏时返回保守默认值（关闭）
pub fn get_failover_config(conn: &Connection) -> FailoverConfig {
    get(conn, KEY_FAILOVER_CONFIG)
        .and_then(|v| serde_json::from_str(&v).ok())
        .unwrap_or_default()
}
