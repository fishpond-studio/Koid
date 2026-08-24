//! 键值设置命令：前端按 JSON 自解析（全局代理 / 故障转移配置等）

use crate::services::settings;
use crate::state::AppState;
use tauri::State;

#[tauri::command]
pub fn get_setting(key: String, state: State<'_, AppState>) -> Result<Option<String>, String> {
    let conn = state.db()?;
    Ok(settings::get(&conn, &key))
}

#[tauri::command]
pub fn set_setting(key: String, value: String, state: State<'_, AppState>) -> Result<(), String> {
    let conn = state.db()?;
    settings::set(&conn, &key, &value)
}
