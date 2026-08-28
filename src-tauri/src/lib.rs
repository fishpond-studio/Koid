//! Koid 应用入口：状态初始化 + 命令注册

mod commands;
mod db;
mod models;
mod services;
mod state;
mod utils;

use state::AppState;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // 单实例锁必须最先注册：二次启动时聚焦已有主窗口后退出
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // 数据目录：%APPDATA%/studio.fishpond.koid（Windows）
            let data_dir = app
                .path()
                .app_data_dir()
                .map_err(|e| format!("无法定位数据目录: {e}"))?;
            std::fs::create_dir_all(&data_dir)
                .map_err(|e| format!("创建数据目录失败: {e}"))?;

            let conn = db::init(&data_dir.join("koid.db"))?;
            app.manage(AppState::new(conn));

            // 系统托盘 + 关闭行为拦截（close_mode: hide / quit / ask）
            crate::services::tray::setup(app)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::providers::list_providers,
            commands::providers::save_provider,
            commands::providers::delete_provider,
            commands::models::list_models,
            commands::models::save_model,
            commands::models::delete_model,
            commands::models::discover_models,
            commands::sessions::list_sessions,
            commands::sessions::save_session,
            commands::sessions::delete_session,
            commands::sessions::branch_session,
            commands::sessions::search_sessions,
            commands::workspaces::list_workspaces,
            commands::workspaces::save_workspace,
            commands::workspaces::delete_workspace,
            commands::workspaces::list_workspace_files,
            commands::workspaces::read_workspace_file,
            commands::workspaces::write_workspace_file,
            commands::workspaces::edit_workspace_file,
            commands::workspaces::delete_workspace_file,
            commands::messages::list_messages,
            commands::messages::append_message,
            commands::messages::delete_message,
            commands::messages::delete_messages_from,
            commands::chat::chat,
            commands::chat::chat_stream,
            commands::chat::chat_abort,
            commands::settings::get_setting,
            commands::settings::set_setting,
            commands::tray::resolve_close,
            commands::proxy::test_proxy,
            commands::prompts::list_prompts,
            commands::prompts::save_prompt,
            commands::prompts::delete_prompt,
            commands::prompts::bump_prompt_usage,
            commands::prompts::list_prompt_versions,
            commands::skills::list_skills,
            commands::skills::save_skill,
            commands::skills::delete_skill,
            commands::skills::run_skill,
            commands::skills::skill_respond,
            commands::skills::skill_cancel,
            commands::mcp::list_mcp_servers,
            commands::mcp::save_mcp_server,
            commands::mcp::delete_mcp_server,
            commands::mcp::connect_mcp_server,
            commands::mcp::disconnect_mcp_server,
            commands::mcp::call_mcp_tool,
            commands::plugins::list_plugins,
            commands::plugins::plugin_html,
            commands::plugins::delete_plugin,
            commands::plugins::plugin_file_read,
            commands::plugins::plugin_file_write,
            commands::plugins::plugin_fetch,
            commands::plugins::install_plugin_from_path,
            commands::plugins::install_plugin_from_url,
            commands::backup::export_backup,
            commands::backup::import_backup,
        ])
        .run(tauri::generate_context!())
        .expect("启动 Koid 失败");
}
