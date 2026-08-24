//! 备份命令（§4.11 云端同步基础：E2E 加密备份导出/导入）

use crate::services::backup;
use tauri::AppHandle;

/// 导出加密备份：返回文件路径
#[tauri::command]
pub fn export_backup(
    passphrase: String,
    dest_path: Option<String>,
    app: AppHandle,
) -> Result<String, String> {
    backup::export(&app, &passphrase, dest_path.as_deref())
}

/// 从加密备份导入（合并进当前数据库）
#[tauri::command]
pub fn import_backup(
    passphrase: String,
    path: String,
    app: AppHandle,
) -> Result<(), String> {
    backup::import(&app, &passphrase, &path)
}
