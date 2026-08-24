//! 代理连通性测试命令（§4.4）

use crate::models::{ProxyTestInput, ProxyTestResult};
use crate::services::proxy;
use crate::state::AppState;
use tauri::State;

#[tauri::command]
pub async fn test_proxy(
    input: ProxyTestInput,
    state: State<'_, AppState>,
) -> Result<ProxyTestResult, String> {
    // 目标 URL 基础校验，避免空目标产生误导性结果
    let url = input.url.trim().to_string();
    if url.is_empty() || !(url.starts_with("http://") || url.starts_with("https://")) {
        return Ok(ProxyTestResult {
            success: false,
            latency_ms: None,
            status_code: None,
            error: Some("目标 URL 需以 http:// 或 https:// 开头".to_string()),
        });
    }
    let _ = &state;
    Ok(proxy::test_connectivity(
        &url,
        input.proxy_type,
        input.proxy_url.as_deref(),
        input.timeout.unwrap_or(10),
    )
    .await)
}
