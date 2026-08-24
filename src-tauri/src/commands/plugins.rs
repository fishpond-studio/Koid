//! 插件命令（§4.9）：列表 / 读取入口 / 文件 / 网络 / 安装 / 删除

use crate::services::plugin;
use crate::state::AppState;
use tauri::{AppHandle, State};

#[tauri::command]
pub fn list_plugins(app: AppHandle) -> Result<Vec<plugin::PluginInfo>, String> {
    plugin::list_plugins(&app)
}

#[tauri::command]
pub fn plugin_html(id: String, app: AppHandle) -> Result<String, String> {
    plugin::plugin_html(&app, &id)
}

#[tauri::command]
pub fn delete_plugin(id: String, app: AppHandle, _state: State<'_, AppState>) -> Result<(), String> {
    plugin::delete_plugin(&app, &id)
}

// ---------- koid.file.* ----------

#[tauri::command]
pub fn plugin_file_read(
    plugin_id: String,
    path: String,
    app: AppHandle,
) -> Result<String, String> {
    plugin::workspace_read(&app, &plugin_id, &path)
}

#[tauri::command]
pub fn plugin_file_write(
    plugin_id: String,
    path: String,
    content: String,
    app: AppHandle,
) -> Result<(), String> {
    plugin::workspace_write(&app, &plugin_id, &path, &content)
}

// ---------- koid.network.fetch ----------

#[tauri::command]
pub async fn plugin_fetch(
    url: String,
    method: String,
    headers: Option<serde_json::Value>,
    body: Option<String>,
    app: AppHandle,
) -> Result<plugin::FetchResult, String> {
    plugin::network_fetch(&app, &url, &method, headers.as_ref(), body.as_deref()).await
}

// ---------- 安装 ----------

#[tauri::command]
pub fn install_plugin_from_path(
    zip_path: String,
    app: AppHandle,
) -> Result<plugin::PluginInfo, String> {
    plugin::install_from_zip(&app, &zip_path)
}

#[tauri::command]
pub async fn install_plugin_from_url(
    url: String,
    app: AppHandle,
) -> Result<plugin::PluginInfo, String> {
    plugin::install_from_url(&app, &url).await
}
