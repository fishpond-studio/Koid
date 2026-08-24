//! 关闭询问框回调：用户选择后记忆并执行本次行为
//!
//! - `hide`：记住「隐藏到托盘」，本次隐藏
//! - `quit`：记住「退出」，本次退出
//! - `ask`（下次再问）：保持「每次询问」，本次先隐藏（应用继续运行）

use crate::services::settings::{self, KEY_CLOSE_MODE};
use crate::state::AppState;
use tauri::{AppHandle, State, Window};

#[tauri::command]
pub fn resolve_close(
    choice: String,
    app: AppHandle,
    window: Window,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let conn = state.db()?;
    match choice.as_str() {
        "hide" => {
            settings::set(&conn, KEY_CLOSE_MODE, "hide")?;
            window.hide().map_err(|e| format!("隐藏窗口失败: {e}"))
        }
        "quit" => {
            settings::set(&conn, KEY_CLOSE_MODE, "quit")?;
            app.exit(0);
            Ok(())
        }
        _ => {
            // 下次再问：保留 ask 模式，本次隐藏到托盘
            settings::set(&conn, KEY_CLOSE_MODE, "ask")?;
            window.hide().map_err(|e| format!("隐藏窗口失败: {e}"))
        }
    }
}
