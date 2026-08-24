//! 模型自动故障转移（§4.3）
//!
//! 核心逻辑：
//! 1. is_retryable 判定错误是否属于可转移故障（对照 triggerConditions）
//! 2. pick_candidates 按 strategy 生成候选供应商列表
//! 3. backoff_delay 指数退避：delay = min(base * multiplier^attempt, maxBackoff)
//!
//! 设计说明：计划 triggerConditions 枚举不含 connect 失败，但「断网自动切换」
//! 验收场景下 NETWORK（ECONNREFUSED）必须可转移，故将其并入 timeout 类处理。

use crate::models::{FailoverConfig, FailoverStrategy, Model, Provider};
use crate::utils;
use rusqlite::{params, Connection};
use std::time::Duration;

/// 错误码 → 触发条件映射；返回 None 表示该错误不可转移
fn trigger_of(code: &str) -> Option<&'static str> {
    match code {
        "TIMEOUT" => Some("timeout"),
        // NETWORK（连接失败）并入 timeout 类：覆盖断网/服务宕机场景
        "NETWORK" => Some("timeout"),
        "SERVER" => Some("5xx"),
        "EMPTY" => Some("empty-response"),
        "CONTENT_FILTER" => Some("content-filter"),
        // UNAUTHORIZED / RATE_LIMITED / ABORTED / BAD_REQUEST / UNSUPPORTED 不转移
        _ => None,
    }
}

/// 从错误信息中提取 HTTP 状态码（分类错误形如 "SERVER:HTTP 502：..."）
fn extract_status(msg: &str) -> Option<u16> {
    let idx = msg.find("HTTP ")?;
    let rest = &msg[idx + 5..];
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

/// 拆分 `CODE:message` 错误格式
pub fn split_error(e: &str) -> (&str, &str) {
    match e.split_once(':') {
        Some((c, m)) => (c, m),
        None => ("UNKNOWN", e),
    }
}

/// 判定一次失败是否应触发故障转移
pub fn is_retryable(error: &str, cfg: &FailoverConfig) -> bool {
    if !cfg.enabled {
        return false;
    }
    let (code, msg) = split_error(error);
    let Some(trigger) = trigger_of(code) else {
        return false;
    };
    // excludedStatusCodes 优先（§4.3：默认 401/403 不重试）
    if let Some(status) = extract_status(msg) {
        if cfg.excluded_status_codes.contains(&status) {
            return false;
        }
    }
    cfg.trigger_conditions.iter().any(|t| t == trigger)
}

/// 指数退避等待时长（base = 0.5s，保证首次切换用户感知 <3s 的验收要求）
pub fn backoff_delay(attempt: u32, cfg: &FailoverConfig) -> Duration {
    let base = 0.5_f64;
    let mult = if cfg.backoff_multiplier > 0.0 {
        cfg.backoff_multiplier
    } else {
        2.0
    };
    let secs = (base * mult.powi(attempt as i32)).min(cfg.max_backoff_seconds.max(0.5));
    Duration::from_secs_f64(secs)
}

/// 用户自选备选链：按 fallbackChain 顺序解析模型 id → (Provider, Model)。
/// 过滤：不存在的/禁用的模型、禁用供应商；不去重当前失败的组合（由调用方过滤 Key 后统一处理）。
pub fn pick_from_chain(
    chain: &[String],
    providers: &[Provider],
    models: &[Model],
) -> Vec<(Provider, Model)> {
    let mut out = Vec::new();
    for model_id in chain {
        let Some(m) = models.iter().find(|m| &m.id == model_id && m.enabled) else {
            continue;
        };
        let Some(p) = providers.iter().find(|p| p.id == m.provider_id && p.enabled) else {
            continue;
        };
        // 同一项重复出现只保留第一次
        if out.iter().any(|(ep, em): &(Provider, Model)| ep.id == p.id && em.id == m.id) {
            continue;
        }
        out.push((p.clone(), m.clone()));
    }
    out
}

/// 候选供应商：启用的、非当前失败的、且至少有一个启用模型的供应商
/// strategy 决定顺序：sequential（order_index）/ round-robin（轮转）/ random（洗牌）
pub fn pick_candidates(
    providers: &[Provider],
    models_by_provider: &[Model],
    failed_provider_id: &str,
    strategy: FailoverStrategy,
) -> Vec<(Provider, Model)> {
    let mut base: Vec<(Provider, Model)> = Vec::new();
    for p in providers {
        if !p.enabled || p.id == failed_provider_id {
            continue;
        }
        // 每个候选取首个启用模型作为接管模型
        if let Some(m) = models_by_provider
            .iter()
            .find(|m| m.provider_id == p.id && m.enabled)
        {
            base.push((p.clone(), m.clone()));
        }
    }

    match strategy {
        FailoverStrategy::Sequential => base,
        FailoverStrategy::RoundRobin => {
            // 以失败供应商在启用列表中的位置做轮转偏移，避免总是从同一候选开始
            let offset = providers
                .iter()
                .position(|p| p.id == failed_provider_id)
                .unwrap_or(0);
            let n = base.len();
            if n > 0 {
                base.rotate_left(offset % n);
            }
            base
        }
        FailoverStrategy::Random => {
            // Fisher-Yates：rand 依赖不引入，用时间+地址熵做伪随机足够（候选少）
            let mut seed = utils::now_ms() as usize;
            for i in (1..base.len()).rev() {
                seed = seed
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                let j = seed % (i + 1);
                base.swap(i, j);
            }
            base
        }
    }
}

/// 故障日志落库（failover_logs 表，§4.3）
pub fn log_failover(
    conn: &Connection,
    session_id: Option<&str>,
    from_provider: &str,
    to_provider: &str,
    reason: &str,
) {
    let _ = conn.execute(
        "INSERT INTO failover_logs (session_id, message_id, from_provider, to_provider, reason, created_at)
         VALUES (?1, NULL, ?2, ?3, ?4, ?5)",
        params![session_id, from_provider, to_provider, reason, utils::now_ms()],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ProviderType, ProxyType};

    fn provider(id: &str, enabled: bool, order: i64) -> Provider {
        Provider {
            id: id.to_string(),
            name: id.to_string(),
            provider_type: ProviderType::OpenAiCompatible,
            base_url: "http://x".to_string(),
            api_key_masked: None,
            proxy_type: ProxyType::Direct,
            proxy_url: None,
            timeout: 60,
            retries: 2,
            order_index: order,
            enabled,
        }
    }

    fn model(id: &str, provider_id: &str) -> Model {
        Model {
            id: id.to_string(),
            provider_id: provider_id.to_string(),
            model_id: id.to_string(),
            display_name: id.to_string(),
            context_window: None,
            capabilities: vec![],
            enabled: true,
        }
    }

    #[test]
    fn retryable_follows_triggers_and_exclusions() {
        let cfg = FailoverConfig::default();
        let mut cfg = cfg;
        cfg.enabled = true;
        assert!(is_retryable("TIMEOUT:请求超时", &cfg));
        assert!(is_retryable("NETWORK:无法连接", &cfg));
        assert!(is_retryable("SERVER:HTTP 502：网关错误", &cfg));
        assert!(is_retryable("EMPTY:模型返回空响应", &cfg));
        // 排除项：401/403 不重试
        assert!(!is_retryable("UNAUTHORIZED:HTTP 401：Key 无效", &cfg));
        assert!(!is_retryable("RATE_LIMITED:HTTP 429：频繁", &cfg));
        assert!(!is_retryable("ABORTED:用户停止了生成", &cfg));
        assert!(!is_retryable("BAD_REQUEST:HTTP 400: bad", &cfg));
        // 配置关闭时一律不转移
        cfg.enabled = false;
        assert!(!is_retryable("TIMEOUT:x", &cfg));
    }

    #[test]
    fn backoff_is_exponential_and_capped() {
        let cfg = FailoverConfig::default();
        assert_eq!(backoff_delay(0, &cfg), Duration::from_secs_f64(0.5));
        assert_eq!(backoff_delay(1, &cfg), Duration::from_secs_f64(1.0));
        assert_eq!(backoff_delay(2, &cfg), Duration::from_secs_f64(2.0));
        // 上限封顶 16s
        assert_eq!(backoff_delay(20, &cfg), Duration::from_secs_f64(16.0));
    }

    #[test]
    fn candidates_skip_failed_disabled_and_modelless() {
        let providers = vec![
            provider("a", true, 0),
            provider("b", true, 1),
            provider("c", false, 2),
            provider("d", true, 3),
        ];
        let models = vec![model("m-a", "a"), model("m-b", "b")]; // d 无模型
        let out = pick_candidates(
            &providers,
            &models,
            "a",
            FailoverStrategy::Sequential,
        );
        let ids: Vec<&str> = out.iter().map(|(p, _)| p.id.as_str()).collect();
        // a 是失败者，c 被禁用，d 无模型 → 只剩 b
        assert_eq!(ids, vec!["b"]);
    }

    #[test]
    fn chain_keeps_user_order_and_supports_same_provider() {
        let providers = vec![provider("p1", true, 0), provider("p2", true, 1)];
        let models = vec![
            model("m-1", "p1"),
            model("m-2", "p1"), // 同供应商第二个模型
            model("m-3", "p2"),
        ];
        // 用户顺序：p2 的模型 → p1 的 m-2 → p1 的 m-1（同供应商多模型依次排序）
        let chain = vec![
            "m-3".to_string(),
            "m-2".to_string(),
            "m-1".to_string(),
            "m-9".to_string(), // 不存在的模型被跳过
        ];
        let out = pick_from_chain(&chain, &providers, &models);
        let ids: Vec<&str> = out.iter().map(|(_, m)| m.id.as_str()).collect();
        assert_eq!(ids, vec!["m-3", "m-2", "m-1"]);
    }

    #[test]
    fn chain_skips_disabled_models_and_providers() {
        let providers = vec![provider("p1", true, 0), provider("p2", false, 1)];
        let mut disabled = model("m-off", "p1");
        disabled.enabled = false;
        let models = vec![disabled, model("m-p2", "p2")];
        let out = pick_from_chain(
            &["m-off".to_string(), "m-p2".to_string()],
            &providers,
            &models,
        );
        // 禁用模型跳过；禁用供应商的模型也跳过 → 空
        assert!(out.is_empty());
    }
}
