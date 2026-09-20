//! LLM 统一封装服务（§6.1）
//!
//! 职责：格式转换（OpenAI / Anthropic）、代理注入、超时控制、SSE 流式解析、可中断。
//! 内部统一使用 OpenAI Chat Completions 消息格式（§4.2.3），
//! Anthropic 由本层自动完成请求/响应转换。
//!
//! 错误约定：Err 字符串格式为 `CODE:可读信息`，CODE ∈
//! NETWORK / TIMEOUT / UNAUTHORIZED / RATE_LIMITED / SERVER / BAD_REQUEST /
//! EMPTY / ABORTED / UNSUPPORTED，前端按 CODE 映射 i18n 提示（§7.2）

use crate::models::{ChatRequest, ChatResponse, GlobalProxy, Provider, TokenUsage};
use crate::services::proxy::{build_client, resolve_proxy};
use futures_util::StreamExt;
use reqwest::StatusCode;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// 增量回调：(&delta, reasoning_delta)
pub type DeltaSink = Box<dyn Fn(&str, Option<&str>) + Send + Sync>;

/// 一次调用的上下文：供应商配置 + keyring 中的真实 Key + 中断旗标
pub struct LlmContext {
    pub provider: Provider,
    pub api_key: Option<String>,
    pub abort: Arc<AtomicBool>,
    /// 全局代理（供应商级代理优先，§4.4）
    pub global_proxy: Option<GlobalProxy>,
}

fn err(code: &str, msg: impl std::fmt::Display) -> String {
    format!("{code}:{msg}")
}

fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

// ---------- URL 约定 ----------
// OpenAI 兼容：base 已含 /v1 则不重复拼接；Anthropic 同理指向 /messages

fn openai_chat_url(base: &str) -> String {
    let b = base.trim_end_matches('/');
    if b.ends_with("/v1") {
        format!("{b}/chat/completions")
    } else {
        format!("{b}/v1/chat/completions")
    }
}

fn openai_responses_url(base: &str) -> String {
    let b = base.trim_end_matches('/');
    if b.ends_with("/v1") {
        format!("{b}/responses")
    } else {
        format!("{b}/v1/responses")
    }
}

fn anthropic_url(base: &str) -> String {
    let b = base.trim_end_matches('/');
    if b.ends_with("/v1") {
        format!("{b}/messages")
    } else {
        format!("{b}/v1/messages")
    }
}

// ---------- 错误分类 ----------

fn classify_reqwest(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        err("TIMEOUT", "请求超时")
    } else if e.is_connect() {
        err("NETWORK", "无法连接到服务器，请检查网络或代理设置")
    } else {
        err("NETWORK", e)
    }
}

fn classify_status(status: StatusCode, body: &str) -> String {
    match status.as_u16() {
        401 | 403 => err("UNAUTHORIZED", format!("HTTP {status}：API Key 无效或权限不足")),
        429 => err("RATE_LIMITED", format!("HTTP 429：请求过于频繁 {}", truncate(body, 200))),
        500..=599 => err("SERVER", format!("HTTP {status}：服务器错误 {}", truncate(body, 200))),
        _ => err("BAD_REQUEST", format!("HTTP {status}: {}", truncate(body, 300))),
    }
}

// ---------- 请求构造 ----------

/// 是否 OpenAI 官方端点。
///
/// 推理模型的那套特殊参数形态（developer 角色、禁 temperature/top_p、
/// max_completion_tokens）只有 OpenAI 官方认。DeepSeek、Ollama、vLLM 与各类中转
/// 走的是同一个 build_openai_request，却大多只接受 system + max_tokens，
/// 按模型名一刀切反而会替它们造出 400，所以必须按端点门控。
fn is_openai_official(base_url: &str) -> bool {
    let lowered = base_url.to_lowercase();
    let after_scheme = lowered.split("://").nth(1).unwrap_or(&lowered);
    let authority = after_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("")
        // 去掉 user:pass@ 前缀
        .rsplit('@')
        .next()
        .unwrap_or("");
    // 去掉 :port（IPv6 字面量在此会解析失败，但那种情况本就不是官方端点，返回 false 即回退到安全行为）
    let host = match authority.split_once(':') {
        Some((h, _)) => h,
        None => authority,
    };
    host == "api.openai.com"
}

/// 是否推理模型。仅用于 OpenAI 官方端点的参数形态判定，见 [`is_openai_official`]。
fn is_reasoning_model(model: &str) -> bool {
    let lowered = model.to_lowercase();
    // 剥掉 "openai/" 之类的供应商前缀与 ":latest" 之类的 tag，只看模型名本体，
    // 否则带前缀的 id（openai/o1-mini）会漏判
    let after_slash = lowered.rsplit('/').next().unwrap_or(&lowered);
    let name = after_slash.split(':').next().unwrap_or(after_slash);
    // 切词后整词比对：裸 contains("r1") 会命中大量无关名字
    let words: Vec<&str> = name
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|s| !s.is_empty())
        .collect();
    let word = |w: &str| words.iter().any(|x| *x == w);
    name.starts_with("o1")
        || name.starts_with("o3")
        || name.starts_with("o4")
        || name.starts_with("gpt-5")
        || word("r1")
        || word("reasoner")
        || word("reasoning")
        || word("thinking")
        || word("qwq")
}

fn build_openai_request(
    ctx: &LlmContext,
    req: &ChatRequest,
    client: &reqwest::Client,
) -> reqwest::RequestBuilder {
    let reasoning = is_openai_official(&ctx.provider.base_url) && is_reasoning_model(&req.model_id);
    let system_role = if reasoning { "developer" } else { "system" };

    // system 提示词置顶为 system/developer 消息
    let mut messages: Vec<Value> = Vec::new();
    if let Some(sys) = &req.system {
        if !sys.trim().is_empty() {
            messages.push(json!({ "role": system_role, "content": sys }));
        }
    }
    for m in &req.messages {
        let role = if reasoning && m.role == "system" { "developer" } else { m.role.as_str() };
        let mut msg = json!({ "role": role, "content": m.content });
        if let Some(tc) = &m.tool_calls {
            msg["tool_calls"] = tc.clone();
        }
        if let Some(id) = &m.tool_call_id {
            msg["tool_call_id"] = json!(id);
        }
        messages.push(msg);
    }

    let mut body = json!({
        "model": req.model_id,
        "messages": messages,
        "stream": req.stream,
    });

    // OpenAI 官方推理模型禁止自定义 temperature 与 top_p，否则 400；
    // 其余端点（含 DeepSeek-R1，其官方推荐 temperature=0.6）照常下发
    if !reasoning {
        if let Some(t) = req.temperature {
            body["temperature"] = json!(t);
        }
        if let Some(p) = req.top_p {
            body["top_p"] = json!(p);
        }
    }
    if let Some(m) = req.max_tokens {
        // OpenAI 推理模型同样拒绝 max_tokens，要求 max_completion_tokens；
        // 只剥 temperature/top_p 而不改名，o 系仍会 400
        let key = if reasoning { "max_completion_tokens" } else { "max_tokens" };
        body[key] = json!(m);
    }
    // 思考强度（OpenAI o 系/gpt-5 reasoning_effort；"default" 与 None 均不下发）。
    // "max" 映射为 xhigh——仅部分新模型支持，不支持者返回 BAD_REQUEST 时用户可降档。
    let effort = match req.thinking_level.as_deref() {
        Some("low") => Some("low"),
        Some("medium") => Some("medium"),
        Some("high") => Some("high"),
        Some("max") => Some("xhigh"),
        _ => None,
    };
    if let Some(effort) = effort {
        body["reasoning_effort"] = json!(effort);
    }
    // 工具定义（agent 循环：模型可调用工作区工具）
    if let Some(tools) = &req.tools {
        let tool_defs: Vec<Value> = tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters,
                    }
                })
            })
            .collect();
        body["tools"] = json!(tool_defs);
    }
    // 流式下要求携带用量统计，便于前端显示 token 消耗
    if req.stream {
        body["stream_options"] = json!({ "include_usage": true });
    }

    let mut builder = client
        .post(openai_chat_url(&ctx.provider.base_url))
        .json(&body);
    if let Some(key) = &ctx.api_key {
        builder = builder.bearer_auth(key);
    }
    builder
}

/// Anthropic 请求：提取 system、转换工具消息（tool_use / tool_result 块格式）
fn build_anthropic_request(
    ctx: &LlmContext,
    req: &ChatRequest,
    client: &reqwest::Client,
) -> reqwest::RequestBuilder {
    let mut system = req.system.clone().unwrap_or_default();
    let mut messages: Vec<Value> = Vec::new();
    for m in &req.messages {
        match m.role.as_str() {
            "system" => {
                system = format!("{}\n{}", system.trim(), m.content.trim())
                    .trim()
                    .to_string();
            }
            "assistant" if m.tool_calls.is_some() => {
                // assistant 带工具调用 → 多个 tool_use 内容块
                let mut blocks: Vec<Value> = Vec::new();
                if !m.content.is_empty() {
                    blocks.push(json!({ "type": "text", "text": m.content }));
                }
                if let Some(Value::Array(calls)) = &m.tool_calls {
                    for c in calls {
                        let name = c
                            .pointer("/function/name")
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        let id = c.get("id").and_then(Value::as_str).unwrap_or("");
                        let args_str = c
                            .pointer("/function/arguments")
                            .and_then(Value::as_str)
                            .unwrap_or("{}");
                        let input: Value = serde_json::from_str(args_str).unwrap_or(Value::Null);
                        blocks.push(json!({
                            "type": "tool_use",
                            "id": id,
                            "name": name,
                            "input": input,
                        }));
                    }
                }
                messages.push(json!({ "role": "assistant", "content": blocks }));
            }
            "tool" => {
                // tool 结果 → user 角色的 tool_result 块
                messages.push(json!({
                    "role": "user",
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": m.tool_call_id.as_deref().unwrap_or(""),
                        "content": m.content,
                    }]
                }));
            }
            "user" | "assistant" => {
                messages.push(json!({ "role": m.role, "content": m.content }));
            }
            _ => {}
        }
    }

    let mut body = json!({
        "model": req.model_id,
        // anthropic 必填字段，用户未配置时给合理默认
        "max_tokens": req.max_tokens.unwrap_or(4096),
        "messages": messages,
        "stream": req.stream,
    });
    if !system.trim().is_empty() {
        body["system"] = json!(system);
    }
    if let Some(t) = req.temperature {
        // Anthropic 上限为 1.0
        body["temperature"] = json!(t.min(1.0));
    }
    if let Some(p) = req.top_p {
        body["top_p"] = json!(p);
    }
    // 思考强度 → Anthropic extended thinking：档位映射 budget_tokens。
    // 约束：max_tokens 必须 > budget_tokens；思考模式下 temperature 仅允许 1，直接移除。
    let thinking_budget = match req.thinking_level.as_deref() {
        Some("low") => Some(4096),
        Some("medium") => Some(10240),
        Some("high") => Some(20480),
        // 「最大」≈32k 思考预算（现代 Claude 200k 上下文模型的输出窗口内）
        Some("max") => Some(32768),
        _ => None,
    };
    if let Some(budget) = thinking_budget {
        body["max_tokens"] =
            json!(req.max_tokens.map_or(budget + 4096, |m| m.max(budget + 1024)));
        body["thinking"] = json!({ "type": "enabled", "budget_tokens": budget });
        if let Some(obj) = body.as_object_mut() {
            obj.remove("temperature");
        }
    }
    // 工具定义转换（Anthropic tools 格式）
    if let Some(tools) = &req.tools {
        let tool_defs: Vec<Value> = tools
            .iter()
            .map(|t| {
                json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.parameters,
                })
            })
            .collect();
        body["tools"] = json!(tool_defs);
    }

    let mut builder = client
        .post(anthropic_url(&ctx.provider.base_url))
        .header("anthropic-version", "2023-06-01")
        .json(&body);
    if let Some(key) = &ctx.api_key {
        builder = builder.header("x-api-key", key);
    }
    builder
}

fn parse_usage(v: &Value) -> Option<TokenUsage> {
    let u = v.get("usage")?;
    let prompt = u.get("prompt_tokens").and_then(Value::as_i64);
    let completion = u.get("completion_tokens").and_then(Value::as_i64);
    let total = u.get("total_tokens").and_then(Value::as_i64);
    if prompt.is_none() && completion.is_none() {
        return None;
    }
    let (p, c) = (prompt.unwrap_or(0), completion.unwrap_or(0));
    Some(TokenUsage {
        prompt_tokens: p,
        completion_tokens: c,
        total_tokens: total.unwrap_or(p + c),
    })
}

// ---------- 主入口 ----------

pub async fn execute(
    ctx: LlmContext,
    req: ChatRequest,
    sink: &DeltaSink,
) -> Result<ChatResponse, String> {
    use crate::models::ProviderType;

    let timeout_secs = ctx.provider.timeout.max(1) as u64;
    // 代理解析：供应商独立代理 > 全局代理 > 环境变量回退
    let (ptype, purl) = resolve_proxy(&ctx.provider, ctx.global_proxy.as_ref());
    let client = build_client(ptype, purl.as_deref(), timeout_secs, req.stream)?;

/// OpenAI Response API (/v1/responses) 请求构造
fn build_openai_response_request(
    ctx: &LlmContext,
    req: &ChatRequest,
    client: &reqwest::Client,
) -> reqwest::RequestBuilder {
    // 构造 input (支持 system / user / assistant / tool 块)
    let mut input: Vec<Value> = Vec::new();
    if let Some(sys) = &req.system {
        if !sys.trim().is_empty() {
            input.push(json!({ "role": "system", "content": sys }));
        }
    }
    for m in &req.messages {
        let mut msg = json!({ "role": m.role, "content": m.content });
        if let Some(tc) = &m.tool_calls {
            msg["tool_calls"] = tc.clone();
        }
        if let Some(id) = &m.tool_call_id {
            msg["tool_call_id"] = json!(id);
        }
        input.push(msg);
    }

    let mut body = json!({
        "model": req.model_id,
        "input": input,
        "stream": req.stream,
    });

    let is_reasoning = is_reasoning_model(&req.model_id);
    if !is_reasoning {
        if let Some(t) = req.temperature {
            body["temperature"] = json!(t);
        }
        if let Some(p) = req.top_p {
            body["top_p"] = json!(p);
        }
    }

    if let Some(m) = req.max_tokens {
        body["max_output_tokens"] = json!(m);
    }

    // reasoning effort
    let effort = match req.thinking_level.as_deref() {
        Some("low") => Some("low"),
        Some("medium") => Some("medium"),
        Some("high") => Some("high"),
        Some("max") => Some("xhigh"),
        _ => None,
    };
    if let Some(effort) = effort {
        body["reasoning"] = json!({ "effort": effort });
    }

    // 工具定义转换为 Response API 格式
    if let Some(tools) = &req.tools {
        let tool_defs: Vec<Value> = tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.parameters,
                })
            })
            .collect();
        body["tools"] = json!(tool_defs);
    }

    let mut builder = client
        .post(openai_responses_url(&ctx.provider.base_url))
        .json(&body);
    if let Some(key) = &ctx.api_key {
        builder = builder.bearer_auth(key);
    }
    builder
}
    let builder = match ctx.provider.provider_type {
        ProviderType::OpenAiCompatible
        | ProviderType::Ollama
        | ProviderType::Custom => build_openai_request(&ctx, &req, &client),
        ProviderType::Anthropic => build_anthropic_request(&ctx, &req, &client),
        ProviderType::OpenAiResponse => build_openai_response_request(&ctx, &req, &client),
    };

    let request = builder
        .build()
        .map_err(|e| err("NETWORK", format!("构建请求失败: {e}")))?;

    // 外层超时：覆盖「建连 + 收到响应头」阶段。
    // 流式请求的响应体读取不在此超时内——由 stream_body 的空闲超时接管
    // （provider.timeout 语义对流式 = 两次数据之间的最大间隔）
    let start = Instant::now();
    let resp = tokio::time::timeout(
        Duration::from_secs(timeout_secs + 5),
        client.execute(request),
    )
    .await
    .map_err(|_| err("TIMEOUT", format!("连接/响应头超时（>{timeout_secs}s）")))?
    .map_err(|e| classify_reqwest(&e))?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(classify_status(status, &body));
    }

    let p_type = ctx.provider.provider_type;
    let provider_name = ctx.provider.name.clone();

    let (content, reasoning, usage, tool_calls) = if req.stream {
        stream_body(&ctx, resp, p_type, timeout_secs, sink).await?
    } else {
        full_body(&ctx, resp, p_type).await?
    };

    // 空响应作为可转移故障类型（§4.3 trigger: empty-response）
    // 有 tool_calls 时不算空响应（模型可能只返回工具调用）
    if content.is_empty()
        && reasoning.as_deref().unwrap_or_default().is_empty()
        && tool_calls.is_empty()
    {
        return Err(err("EMPTY", "模型返回空响应"));
    }

    Ok(ChatResponse {
        content,
        reasoning,
        tool_calls: if tool_calls.is_empty() {
            None
        } else {
            Some(tool_calls)
        },
        tool_results: None,
        usage,
        latency_ms: start.elapsed().as_millis() as u64,
        provider_used: provider_name,
    })
}

// ---------- 工具调用累积 ----------

/// 累积流式 tool_calls（OpenAI delta.tool_calls 与 Anthropic input_json_delta 都累积进这里）
#[derive(Default)]
pub(crate) struct ToolAccumulator {
    /// 按 tool_call index 累积的临时数据
    items: Vec<ToolAccumItem>,
}

#[derive(Default)]
struct ToolAccumItem {
    id: String,
    name: String,
    arguments: String,
}

impl ToolAccumulator {
    /// 最终产物：过滤掉不完整（缺 id/name）的调用
    fn finish(&self) -> Vec<crate::models::ToolCall> {
        self.items
            .iter()
            .filter(|it| !it.name.is_empty())
            .map(|it| crate::models::ToolCall {
                id: it.id.clone(),
                name: it.name.clone(),
                arguments: it.arguments.clone(),
            })
            .collect()
    }

    fn ensure(&mut self, idx: usize) -> &mut ToolAccumItem {
        while self.items.len() <= idx {
            self.items.push(ToolAccumItem::default());
        }
        &mut self.items[idx]
    }

    /// OpenAI 流式 delta：{ index, id?, function:{name?, arguments?} }
    fn apply_openai_delta(&mut self, delta: &Value) {
        let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) else {
            return;
        };
        for call in calls {
            let idx = call.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
            let item = self.ensure(idx);
            if let Some(id) = call.get("id").and_then(Value::as_str) {
                item.id.push_str(id);
            }
            if let Some(name) = call.get("function").and_then(|f| f.get("name")).and_then(Value::as_str) {
                item.name.push_str(name);
            }
            if let Some(args) = call
                .get("function")
                .and_then(|f| f.get("arguments"))
                .and_then(Value::as_str)
            {
                item.arguments.push_str(args);
            }
        }
    }

    /// Anthropic 流式：content_block_start(tool_use) + content_block_delta(input_json_delta)
    fn apply_anthropic_event(&mut self, v: &Value, block_idx: usize) {
        match v.get("type").and_then(Value::as_str) {
            Some("content_block_start") => {
                let block = v.get("content_block");
                let is_tool = block.and_then(|b| b.get("type")).and_then(Value::as_str) == Some("tool_use");
                if is_tool {
                    let item = self.ensure(block_idx);
                    item.id = block.and_then(|b| b.get("id")).and_then(Value::as_str).unwrap_or("").to_string();
                    item.name = block.and_then(|b| b.get("name")).and_then(Value::as_str).unwrap_or("").to_string();
                }
            }
            Some("content_block_delta") => {
                let delta = v.get("delta");
                let is_input = delta.and_then(|d| d.get("type")).and_then(Value::as_str) == Some("input_json_delta");
                if is_input {
                    if let Some(args) = delta.and_then(|d| d.get("partial_json")).and_then(Value::as_str) {
                        self.ensure(block_idx).arguments.push_str(args);
                    }
                }
            }
            _ => {}
        }
    }
}

// ---------- 流式解析 ----------

async fn stream_body(
    ctx: &LlmContext,
    resp: reqwest::Response,
    provider_type: ProviderType,
    idle_secs: u64,
    sink: &DeltaSink,
) -> Result<(String, Option<String>, Option<TokenUsage>, Vec<crate::models::ToolCall>), String> {
    let mut stream = resp.bytes_stream();
    let mut buf: Vec<u8> = Vec::new();
    let mut content = String::new();
    let mut reasoning = String::new();
    let mut usage: Option<TokenUsage> = None;
    let mut tools = ToolAccumulator::default();

    // 空闲超时：只要数据持续到达就不限制总时长（流式长回答/长思考不再被掐断）
    let idle = Duration::from_secs(idle_secs.max(1));
    loop {
        // 每次等待新数据前先检查中断旗标，做到快速停止
        if ctx.abort.load(Ordering::Relaxed) {
            return Err(err("ABORTED", "用户停止了生成"));
        }
        let next = match tokio::time::timeout(idle, stream.next()).await {
            Ok(Some(chunk)) => chunk,
            Ok(None) => break, // 流正常结束
            Err(_) => {
                return Err(err(
                    "TIMEOUT",
                    format!("连接空闲超过 {idle_secs}s，服务器无数据返回"),
                ))
            }
        };
        let chunk = next.map_err(|e| classify_reqwest(&e))?;
        buf.extend_from_slice(&chunk);

        // SSE 按行处理：先在缓冲上切片处理所有完整行（零中间分配），
        // 循环结束后一次性 drain 掉已消费部分（原实现每行 drain 一次，O(行数×缓冲)）
        let mut start = 0usize;
        while let Some(rel) = buf[start..].iter().position(|&b| b == b'\n') {
            let end = start + rel;
            let line = String::from_utf8_lossy(&buf[start..end]);
            let line = line.trim();
            if !line.is_empty() {
                match provider_type {
                    ProviderType::Anthropic => {
                        handle_anthropic_line(line, &mut content, &mut reasoning, &mut usage, &mut tools, sink);
                    }
                    ProviderType::OpenAiResponse => {
                        handle_openai_response_line(line, &mut content, &mut reasoning, &mut usage, &mut tools, sink);
                    }
                    _ => {
                        handle_openai_line(line, &mut content, &mut reasoning, &mut usage, &mut tools, sink);
                    }
                }
            }
            start = end + 1;
        }
        if start > 0 {
            buf.drain(..start);
        }
    }

    Ok((content, non_empty(reasoning), usage, tools.finish()))
}

fn non_empty(s: String) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// OpenAI 兼容 SSE：`data: {choices:[{delta:{content, reasoning_content, tool_calls}}]}` / `data: [DONE]`
pub(crate) fn handle_openai_line(
    line: &str,
    content: &mut String,
    reasoning: &mut String,
    usage: &mut Option<TokenUsage>,
    tools: &mut ToolAccumulator,
    sink: &DeltaSink,
) {
    let Some(data) = line.strip_prefix("data:") else {
        return;
    };
    let data = data.trim();
    if data == "[DONE]" {
        return;
    }
    let Ok(v) = serde_json::from_str::<Value>(data) else {
        return;
    };
    if let Some(u) = parse_usage(&v) {
        *usage = Some(u);
    }
    if let Some(delta) = v.pointer("/choices/0/delta") {
        if let Some(c) = delta.get("content").and_then(Value::as_str) {
            if !c.is_empty() {
                content.push_str(c);
                sink(c, None);
            }
        }
        // 兼容多供应商思考过程字段：reasoning_content / reasoning / thought
        // 不能串 get().or_else(get())：某字段存在但值为 null 时 or_else 不再回退，思考内容会被丢弃
        let reasoning_val = ["reasoning_content", "reasoning", "thought"]
            .into_iter()
            .find_map(|k| delta.get(k).and_then(Value::as_str).filter(|s| !s.is_empty()));

        if let Some(r) = reasoning_val {
            reasoning.push_str(r);
            sink("", Some(r));
        }
        // 工具调用增量（agent 循环）
        tools.apply_openai_delta(delta);
    }
}

/// Anthropic SSE：按 data.type 分发（content_block_delta / message_delta / message_start）
pub(crate) fn handle_anthropic_line(
    line: &str,
    content: &mut String,
    reasoning: &mut String,
    usage: &mut Option<TokenUsage>,
    tools: &mut ToolAccumulator,
    sink: &DeltaSink,
) {
    let Some(data) = line.strip_prefix("data:") else {
        return;
    };
    let Ok(v) = serde_json::from_str::<Value>(data.trim()) else {
        return;
    };
    // content block index：Anthropic 用 index 区分 text/thinking/tool_use 块
    let block_idx = v.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
    // 工具调用（tool_use start + input_json_delta）
    tools.apply_anthropic_event(&v, block_idx);

    match v.get("type").and_then(Value::as_str) {
        Some("content_block_delta") => {
            let delta = v.get("delta");
            match delta.and_then(|d| d.get("type")).and_then(Value::as_str) {
                Some("text_delta") => {
                    if let Some(t) = delta.and_then(|d| d.get("text")).and_then(Value::as_str) {
                        if !t.is_empty() {
                            content.push_str(t);
                            sink(t, None);
                        }
                    }
                }
                Some("thinking_delta") => {
                    if let Some(t) = delta
                        .and_then(|d| d.get("thinking"))
                        .and_then(Value::as_str)
                    {
                        if !t.is_empty() {
                            reasoning.push_str(t);
                            sink("", Some(t));
                        }
                    }
                }
                _ => {}
            }
        }
        Some("message_start") => {
            // 输入 token 在 message_start 中给出，先落一半
            if let Some(u) = v.pointer("/message/usage") {
                let p = u.get("input_tokens").and_then(Value::as_i64).unwrap_or(0);
                *usage = Some(TokenUsage {
                    prompt_tokens: p,
                    completion_tokens: 0,
                    total_tokens: p,
                });
            }
        }
        Some("message_delta") => {
            if let Some(c) = v.pointer("/usage/output_tokens").and_then(Value::as_i64) {
                match usage.as_mut() {
                    Some(u) => {
                        u.completion_tokens = c;
                        u.total_tokens = u.prompt_tokens + c;
                    }
                    None => {
                        *usage = Some(TokenUsage {
                            prompt_tokens: 0,
                            completion_tokens: c,
                            total_tokens: c,
                        });
                    }
                }
            }
        }
        _ => {}
    }
}

// ---------- 非流式解析 ----------

/// OpenAI Response API SSE 行处理
pub(crate) fn handle_openai_response_line(
    line: &str,
    content: &mut String,
    reasoning: &mut String,
    usage: &mut Option<TokenUsage>,
    tools: &mut ToolAccumulator,
    sink: &DeltaSink,
) {
    let Some(data) = line.strip_prefix("data:") else {
        return;
    };
    let data = data.trim();
    if data == "[DONE]" {
        return;
    }
    let Ok(v) = serde_json::from_str::<Value>(data) else {
        return;
    };

    if let Some(u) = parse_usage(&v) {
        *usage = Some(u);
    }

    let event_type = v.get("type").and_then(Value::as_str).unwrap_or("");
    match event_type {
        // 文本增量
        "response.output_item.delta" => {
            if let Some(delta) = v.get("delta") {
                if let Some(t) = delta.get("text").and_then(Value::as_str) {
                    if !t.is_empty() {
                        content.push_str(t);
                        sink(t, None);
                    }
                }
                // 工具调用参数增量 (function_call)
                if let Some(args) = delta.get("arguments").and_then(Value::as_str) {
                    let item_idx = v.get("output_index").and_then(Value::as_u64).unwrap_or(0) as usize;
                    let fake_delta = json!({
                        "tool_calls": [{
                            "index": item_idx,
                            "function": { "arguments": args }
                        }]
                    });
                    tools.apply_openai_delta(&fake_delta);
                }
            }
        }
        // 思考过程增量
        "response.reasoning.delta" => {
            if let Some(t) = v.get("delta").and_then(|d| d.get("text")).and_then(Value::as_str) {
                if !t.is_empty() {
                    reasoning.push_str(t);
                    sink("", Some(t));
                }
            }
        }
        // 工具项开始 (声明函数名和 ID)
        "response.output_item.added" => {
            if let Some(item) = v.get("item") {
                if item.get("type").and_then(Value::as_str) == Some("function_call") {
                    let item_idx = v.get("output_index").and_then(Value::as_u64).unwrap_or(0) as usize;
                    let call_id = item.get("call_id").or_else(|| item.get("id")).and_then(Value::as_str).unwrap_or("");
                    let name = item.get("name").and_then(Value::as_str).unwrap_or("");
                    let fake_delta = json!({
                        "tool_calls": [{
                            "index": item_idx,
                            "id": call_id,
                            "function": { "name": name, "arguments": "" }
                        }]
                    });
                    tools.apply_openai_delta(&fake_delta);
                }
            }
        }
        // 兜底通用 choices/delta (有些兼容端点即便路由在 /responses 也会回普通 chunk)
        _ => {
            if let Some(delta) = v.pointer("/choices/0/delta") {
                if let Some(c) = delta.get("content").and_then(Value::as_str) {
                    if !c.is_empty() {
                        content.push_str(c);
                        sink(c, None);
                    }
                }
                let r_val = delta
                    .get("reasoning_content")
                    .or_else(|| delta.get("reasoning"))
                    .or_else(|| delta.get("thought"))
                    .and_then(Value::as_str);
                if let Some(r) = r_val {
                    if !r.is_empty() {
                        reasoning.push_str(r);
                        sink("", Some(r));
                    }
                }
                tools.apply_openai_delta(delta);
            }
        }
    }
}
async fn full_body(
    ctx: &LlmContext,
    resp: reqwest::Response,
    provider_type: ProviderType,
) -> Result<(String, Option<String>, Option<TokenUsage>, Vec<crate::models::ToolCall>), String> {
    let _ = ctx;
    let v: Value = resp
        .json()
        .await
        .map_err(|e| err("SERVER", format!("解析响应失败: {e}")))?;

    if provider_type == ProviderType::Anthropic {
        // content 为块数组：拼接 text 块、thinking 块归入 reasoning、tool_use 收集
        let mut content = String::new();
        let mut reasoning = String::new();
        let mut tools: Vec<crate::models::ToolCall> = Vec::new();
        if let Some(blocks) = v.get("content").and_then(Value::as_array) {
            for b in blocks {
                match b.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        if let Some(t) = b.get("text").and_then(Value::as_str) {
                            content.push_str(t);
                        }
                    }
                    Some("thinking") => {
                        if let Some(t) = b.get("thinking").and_then(Value::as_str) {
                            reasoning.push_str(t);
                        }
                    }
                    Some("tool_use") => {
                        tools.push(crate::models::ToolCall {
                            id: b.get("id").and_then(Value::as_str).unwrap_or("").to_string(),
                            name: b.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
                            arguments: b.get("input").map(|x| x.to_string()).unwrap_or_default(),
                        });
                    }
                    _ => {}
                }
            }
        }
        let usage = v
            .get("usage")
            .map(|u| TokenUsage {
                prompt_tokens: u.get("input_tokens").and_then(Value::as_i64).unwrap_or(0),
                completion_tokens: u.get("output_tokens").and_then(Value::as_i64).unwrap_or(0),
                total_tokens: 0,
            })
            .map(|mut u| {
                u.total_tokens = u.prompt_tokens + u.completion_tokens;
                u
            });
        Ok((content, non_empty(reasoning), usage, tools))
    } else if provider_type == ProviderType::OpenAiResponse {
        let mut content = String::new();
        let mut reasoning = String::new();
        let mut tools: Vec<crate::models::ToolCall> = Vec::new();

        if let Some(output) = v.get("output").and_then(Value::as_array) {
            for item in output {
                let item_type = item.get("type").and_then(Value::as_str).unwrap_or("");
                if item_type == "message" {
                    if let Some(parts) = item.pointer("/content").and_then(Value::as_array) {
                        for p in parts {
                            if p.get("type").and_then(Value::as_str) == Some("text") {
                                if let Some(txt) = p.get("text").and_then(Value::as_str) {
                                    content.push_str(txt);
                                }
                            }
                        }
                    }
                } else if item_type == "function_call" {
                    tools.push(crate::models::ToolCall {
                        id: item.get("call_id").or_else(|| item.get("id")).and_then(Value::as_str).unwrap_or("").to_string(),
                        name: item.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
                        arguments: item.get("arguments").and_then(Value::as_str).unwrap_or("{}").to_string(),
                    });
                }
            }
        }
        // 如果 output 为空，尝试常规解析
        if content.is_empty() {
            if let Some(txt) = v.pointer("/choices/0/message/content").and_then(Value::as_str) {
                content = txt.to_string();
            }
        }

        Ok((content, non_empty(reasoning), parse_usage(&v), tools))
    } else {
        let content = v
            .pointer("/choices/0/message/content")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let reasoning = v
            .pointer("/choices/0/message/reasoning_content")
            .and_then(Value::as_str)
            .map(str::to_string);
        let tools = v
            .pointer("/choices/0/message/tool_calls")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|tc| {
                        let name = tc
                            .pointer("/function/name")
                            .and_then(Value::as_str)?;
                        Some(crate::models::ToolCall {
                            id: tc.get("id").and_then(Value::as_str).unwrap_or("").to_string(),
                            name: name.to_string(),
                            arguments: tc
                                .pointer("/function/arguments")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok((content, reasoning.filter(|s| !s.is_empty()), parse_usage(&v), tools))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sink_capture() -> (DeltaSink, std::sync::Arc<std::sync::Mutex<String>>) {
        let buf = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let b = buf.clone();
        let sink: DeltaSink = Box::new(move |d, r| {
            let mut g = b.lock().unwrap();
            g.push_str(d);
            if let Some(r) = r {
                g.push_str("[R]");
                g.push_str(r);
            }
        });
        (sink, buf)
    }

    #[test]
    fn openai_sse_parses_content_and_usage() {
        let (sink, buf) = sink_capture();
        let mut content = String::new();
        let mut reasoning = String::new();
        let mut usage: Option<TokenUsage> = None;
        let mut tools = ToolAccumulator::default();

        handle_openai_line(
            r#"data: {"choices":[{"delta":{"content":"Hello"}}]}"#,
            &mut content, &mut reasoning, &mut usage, &mut tools, &sink,
        );
        handle_openai_line(
            r#"data: {"choices":[{"delta":{"reasoning_content":"think"}}]}"#,
            &mut content, &mut reasoning, &mut usage, &mut tools, &sink,
        );
        handle_openai_line(
            r#"data: {"usage":{"prompt_tokens":3,"completion_tokens":5,"total_tokens":8}}"#,
            &mut content, &mut reasoning, &mut usage, &mut tools, &sink,
        );
        // [DONE] 与非 data 行应被安全忽略
        handle_openai_line("data: [DONE]", &mut content, &mut reasoning, &mut usage, &mut tools, &sink);
        handle_openai_line(": keep-alive", &mut content, &mut reasoning, &mut usage, &mut tools, &sink);

        assert_eq!(content, "Hello");
        assert_eq!(reasoning, "think");
        assert_eq!(usage.as_ref().map(|u| u.total_tokens), Some(8));
        assert!(buf.lock().unwrap().contains("[R]think"));
    }

    #[test]
    fn anthropic_sse_parses_text_thinking_and_usage() {
        let (sink, _buf) = sink_capture();
        let mut content = String::new();
        let mut reasoning = String::new();
        let mut usage: Option<TokenUsage> = None;
        let mut tools = ToolAccumulator::default();

        handle_anthropic_line(
            r#"data: {"type":"message_start","message":{"usage":{"input_tokens":10}}}"#,
            &mut content, &mut reasoning, &mut usage, &mut tools, &sink,
        );
        handle_anthropic_line(
            r#"data: {"type":"content_block_delta","delta":{"type":"thinking_delta","thinking":"let me see"}}"#,
            &mut content, &mut reasoning, &mut usage, &mut tools, &sink,
        );
        handle_anthropic_line(
            r#"data: {"type":"content_block_delta","delta":{"type":"text_delta","text":"Hi there"}}"#,
            &mut content, &mut reasoning, &mut usage, &mut tools, &sink,
        );
        handle_anthropic_line(
            r#"data: {"type":"message_delta","usage":{"output_tokens":7}}"#,
            &mut content, &mut reasoning, &mut usage, &mut tools, &sink,
        );

        assert_eq!(content, "Hi there");
        assert_eq!(reasoning, "let me see");
        let u = usage.unwrap();
        assert_eq!(u.prompt_tokens, 10);
        assert_eq!(u.completion_tokens, 7);
        assert_eq!(u.total_tokens, 17);
    }

    #[test]
    fn openai_sse_accumulates_tool_calls() {
        let (sink, _buf) = sink_capture();
        let mut content = String::new();
        let mut reasoning = String::new();
        let mut usage: Option<TokenUsage> = None;
        let mut tools = ToolAccumulator::default();

        // 工具调用增量：id + name + 分段 arguments
        handle_openai_line(
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"read_file","arguments":"{\"path\":"}}]}}]}"#,
            &mut content, &mut reasoning, &mut usage, &mut tools, &sink,
        );
        handle_openai_line(
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"src/main.ts\"}"}}]}}]}"#,
            &mut content, &mut reasoning, &mut usage, &mut tools, &sink,
        );

        let calls = tools.finish();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "read_file");
        assert_eq!(calls[0].arguments, r#"{"path":"src/main.ts"}"#);
    }

    #[test]
    fn anthropic_sse_accumulates_tool_use() {
        let (sink, _buf) = sink_capture();
        let mut content = String::new();
        let mut reasoning = String::new();
        let mut usage: Option<TokenUsage> = None;
        let mut tools = ToolAccumulator::default();

        // content_block_start(tool_use) + input_json_delta
        handle_anthropic_line(
            r#"data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_1","name":"list_dir","input":{}}}"#,
            &mut content, &mut reasoning, &mut usage, &mut tools, &sink,
        );
        handle_anthropic_line(
            r#"data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"path\":\"src\"}"}}"#,
            &mut content, &mut reasoning, &mut usage, &mut tools, &sink,
        );

        let calls = tools.finish();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "list_dir");
        assert_eq!(calls[0].arguments, r#"{"path":"src"}"#);
    }

    #[test]
    fn openai_official_detection_only_matches_api_openai_com_host() {
        // 官方端点：带不带 /v1、带不带端口、带不带 scheme 都要认出来
        assert!(is_openai_official("https://api.openai.com/v1"));
        assert!(is_openai_official("https://api.openai.com"));
        assert!(is_openai_official("https://api.openai.com:443/v1"));
        assert!(is_openai_official("api.openai.com/v1"));
        // 兼容端点只认 system + max_tokens，误判成官方会替它们造出 400
        assert!(!is_openai_official("https://api.deepseek.com/v1"));
        assert!(!is_openai_official("http://localhost:11434/v1"));
        assert!(!is_openai_official("https://openrouter.ai/api/v1"));
        assert!(!is_openai_official("https://xxx.openai.azure.com"));
        // 子域名与形近域名不能命中
        assert!(!is_openai_official("https://not-api.openai.com/v1"));
        assert!(!is_openai_official("https://api.openai.com.evil.example/v1"));
    }

    #[test]
    fn reasoning_model_detection_strips_prefix_and_matches_whole_words() {
        // 带供应商前缀或 Ollama tag 时不能漏判
        assert!(is_reasoning_model("o1"));
        assert!(is_reasoning_model("openai/o1-mini"));
        assert!(is_reasoning_model("o3-mini"));
        assert!(is_reasoning_model("o4-mini"));
        assert!(is_reasoning_model("gpt-5"));
        assert!(is_reasoning_model("deepseek-ai/DeepSeek-R1-0528:latest"));
        assert!(is_reasoning_model("deepseek-reasoner"));
        assert!(is_reasoning_model("qwq-32b"));
        assert!(is_reasoning_model("phi-4-reasoning"));
        // 非推理模型
        assert!(!is_reasoning_model("gpt-4o"));
        assert!(!is_reasoning_model("gpt-4.1-mini"));
        assert!(!is_reasoning_model("deepseek-chat"));
        assert!(!is_reasoning_model("deepseek-v3.2-exp"));
        assert!(!is_reasoning_model("qwen3-coder-plus"));
        assert!(!is_reasoning_model("llama3.1:8b"));
        assert!(!is_reasoning_model("command-r-plus"));
    }

    #[test]
    fn reasoning_delta_falls_back_when_earlier_key_is_null() {
        let (sink, _buf) = sink_capture();
        let mut content = String::new();
        let mut reasoning = String::new();
        let mut usage: Option<TokenUsage> = None;
        let mut tools = ToolAccumulator::default();

        // 供应商同时下发 reasoning_content: null 与 reasoning 时，
        // get().or_else(get()) 串接会在第一个键上短路，整段思考被丢弃
        handle_openai_line(
            r#"data: {"choices":[{"delta":{"reasoning_content":null,"reasoning":"深层思考"}}]}"#,
            &mut content, &mut reasoning, &mut usage, &mut tools, &sink,
        );
        handle_openai_line(
            r#"data: {"choices":[{"delta":{"reasoning_content":null,"reasoning":null,"thought":"再想想"}}]}"#,
            &mut content, &mut reasoning, &mut usage, &mut tools, &sink,
        );

        assert_eq!(reasoning, "深层思考再想想");
        assert_eq!(content, "");
    }

    #[test]
    fn openai_response_api_sse_parses_delta_and_reasoning() {
        let (sink, buf) = sink_capture();
        let mut content = String::new();
        let mut reasoning = String::new();
        let mut usage: Option<TokenUsage> = None;
        let mut tools = ToolAccumulator::default();

        handle_openai_response_line(
            r#"data: {"type":"response.reasoning.delta","delta":{"text":"思考中..."}}"#,
            &mut content, &mut reasoning, &mut usage, &mut tools, &sink,
        );
        handle_openai_response_line(
            r#"data: {"type":"response.output_item.delta","delta":{"text":"你好，世界！"}}"#,
            &mut content, &mut reasoning, &mut usage, &mut tools, &sink,
        );
        handle_openai_response_line("data: [DONE]", &mut content, &mut reasoning, &mut usage, &mut tools, &sink);

        assert_eq!(reasoning, "思考中...");
        assert_eq!(content, "你好，世界！");
        assert!(buf.lock().unwrap().contains("你好，世界！"));
    }
}
