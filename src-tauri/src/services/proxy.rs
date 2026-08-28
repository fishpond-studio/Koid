//! HTTP 客户端构建与代理解析（§4.4 代理系统 / §6.2）
//!
//! 优先级：供应商独立代理 > 全局代理 > 环境变量
//! HTTPS_PROXY / ALL_PROXY / HTTP_PROXY（大小写均尝试）

use crate::models::{GlobalProxy, Provider, ProxyTestResult, ProxyType};
use reqwest::{Client, Proxy};
use std::time::{Duration, Instant};

pub fn proxy_type_str(t: ProxyType) -> &'static str {
    match t {
        ProxyType::Direct => "direct",
        ProxyType::Http => "http",
        ProxyType::Socks5 => "socks5",
    }
}

/// 解析某次请求实际生效的代理（供应商覆盖全局，§4.4 配置粒度）
pub fn resolve_proxy(provider: &Provider, global: Option<&GlobalProxy>) -> (&'static str, Option<String>) {
    if provider.proxy_type != ProxyType::Direct {
        if let Some(url) = provider.proxy_url.as_deref().filter(|u| !u.trim().is_empty()) {
            return (proxy_type_str(provider.proxy_type), Some(url.to_string()));
        }
    }
    if let Some(g) = global {
        if g.proxy_type != ProxyType::Direct {
            if let Some(url) = g.proxy_url.as_deref().filter(|u| !u.trim().is_empty()) {
                return (proxy_type_str(g.proxy_type), Some(url.to_string()));
            }
        }
    }
    ("direct", None)
}

/// 按代理配置构建 reqwest Client
///
/// - http / socks5：`Proxy::all` 同时覆盖 http/https 目标；socks5:// URL 由
///   reqwest 的 socks feature 处理
/// - 连接超时固定 10s，避免代理不通时整体超时被长 timeout 拖住
pub fn build_client(
    _proxy_type: &str,
    proxy_url: Option<&str>,
    timeout_secs: u64,
    streaming: bool,
) -> Result<Client, String> {
    let mut builder = Client::builder()
        .connect_timeout(Duration::from_secs(10));
    // 总超时仅用于非流式请求。流式绝不能设：Client::timeout 覆盖「发起→读完整个
    // 响应体」，长回答/长思考必然撞墙导致流被中途掐断（表现为对话老是断）。
    // 流式的正确语义是「空闲超时」，由 llm.rs 在读流循环中逐 chunk 判定。
    if !streaming {
        builder = builder.timeout(Duration::from_secs(timeout_secs.max(1)));
    }

    let user_url = proxy_url.map(str::trim).filter(|s| !s.is_empty());

    let effective: Option<String> = match user_url {
        Some(url) => Some(url.to_string()),
        // 用户未配置代理时，环境变量作为 fallback（计划 §4.4）
        None => env_proxy_fallback(),
    };

    if let Some(url) = effective {
        let proxy = Proxy::all(&url).map_err(|e| format!("代理配置无效 {url}: {e}"))?;
        builder = builder.proxy(proxy);
    }

    builder
        .build()
        .map_err(|e| format!("HTTP 客户端初始化失败: {e}"))
}

fn env_proxy_fallback() -> Option<String> {
    ["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy", "HTTP_PROXY", "http_proxy"]
        .iter()
        .find_map(|key| std::env::var(key).ok().filter(|v| !v.trim().is_empty()))
}

/// 代理连通性测试（§4.4）：通过指定代理 GET 目标 URL，返回延迟/状态码
///
/// 先 HEAD 后 GET 回退：部分服务拒绝 HEAD，回退保证测试可用
pub async fn test_connectivity(
    url: &str,
    proxy_type: ProxyType,
    proxy_url: Option<&str>,
    timeout_secs: u64,
) -> ProxyTestResult {
    let start = Instant::now();
    let client = match build_client(proxy_type_str(proxy_type), proxy_url, timeout_secs, false) {
        Ok(c) => c,
        Err(e) => {
            return ProxyTestResult {
                success: false,
                latency_ms: None,
                status_code: None,
                error: Some(e),
            }
        }
    };

    let request = async {
        let resp = client.head(url).send().await?;
        // 405 Method Not Allowed 说明服务端拒绝 HEAD，改用 GET
        if resp.status() == reqwest::StatusCode::METHOD_NOT_ALLOWED {
            return client.get(url).send().await;
        }
        Ok::<_, reqwest::Error>(resp)
    };

    match request.await {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let latency = start.elapsed().as_millis() as u64;
            // 拿到任何 HTTP 应答都说明链路通；4xx/5xx 是目标服务自身状态
            ProxyTestResult {
                success: true,
                latency_ms: Some(latency),
                status_code: Some(status),
                error: None,
            }
        }
        Err(e) => ProxyTestResult {
            success: false,
            latency_ms: Some(start.elapsed().as_millis() as u64),
            status_code: None,
            error: Some(if e.is_timeout() {
                "连接超时".to_string()
            } else if e.is_connect() {
                "无法连接，请检查代理地址与网络".to_string()
            } else {
                e.to_string()
            }),
        },
    }
}
