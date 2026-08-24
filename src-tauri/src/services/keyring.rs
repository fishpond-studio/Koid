//! API Key 存储（§7.4 安全要求）
//!
//! 只使用系统凭据存储（Windows Credential Manager / macOS Keychain /
//! Linux Secret Service），SQLite 中 providers.api_key 恒为空串。

const SERVICE: &str = "studio.fishpond.koid";

pub fn set_api_key(provider_id: &str, key: &str) -> Result<(), String> {
    let entry = keyring::Entry::new(SERVICE, provider_id)
        .map_err(|e| format!("keyring 初始化失败: {e}"))?;
    entry
        .set_password(key)
        .map_err(|e| format!("写入 API Key 失败: {e}"))
}

/// 不存在时返回 None（NoEntry 视为正常情况而非错误）
pub fn get_api_key(provider_id: &str) -> Option<String> {
    let entry = keyring::Entry::new(SERVICE, provider_id).ok()?;
    match entry.get_password() {
        Ok(key) => Some(key),
        Err(keyring::Error::NoEntry) => None,
        Err(_) => None,
    }
}

pub fn delete_api_key(provider_id: &str) {
    if let Ok(entry) = keyring::Entry::new(SERVICE, provider_id) {
        // 删除失败（如本就不存在）可安全忽略
        let _ = entry.delete_credential();
    }
}

/// 掩码展示：sk-abcdef...wxyz；过短 Key 全部遮盖
pub fn mask_key(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    if chars.len() <= 8 {
        return "********".to_string();
    }
    let head: String = chars.iter().take(4).collect();
    let tail: String = chars.iter().skip(chars.len() - 4).collect();
    format!("{head}...{tail}")
}
