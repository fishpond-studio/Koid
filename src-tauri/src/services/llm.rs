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

fn build_openai_request(
    ctx: &LlmContext,
    req: &ChatRequest,
    client: &reqwest::Client,
) -> reqwest::RequestBuilder {
    // system 提示词置顶为 system 消息（内部格式统一为 OpenAI 风格）
    let mut messages: Vec<Value> = Vec::new();
    if let Some(sys) = &req.system {
        if !sys.trim().is_empty() {
            messages.push(json!({ "role": "system", "content": sys }));
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
        messages.push(msg);
    }

    let mut body = json!({
        "model": req.model_id,
        "messages": messages,
        "stream": req.stream,
    });
    if let Some(t) = req.temperature {
        body["temperature"] = json!(t);
    }
    if let Some(p) = req.top_p {
        body["top_p"] = json!(p);
    }
    if let Some(m) = req.max_tokens {
        body["max_tokens"] = json!(m);
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
    let client = build_client(ptype, purl.as_deref(), timeout_secs)?;

    let builder = match ctx.provider.provider_type {
        ProviderType::OpenAiCompatible
        | ProviderType::Ollama
        | ProviderType::Custom => build_openai_request(&ctx, &req, &client),
        ProviderType::Anthropic => build_anthropic_request(&ctx, &req, &client),
        // OpenAI Response API 转换在 Phase 2 引入
        ProviderType::OpenAiResponse => {
            return Err(err("UNSUPPORTED", "openai-response 格式将在 Phase 2 支持"));
        }
    };

    let request = builder
        .build()
        .map_err(|e| err("NETWORK", format!("构建请求失败: {e}")))?;

    // 外层超时兜底（内层 reqwest timeout 通常先触发）
    let start = Instant::now();
    let resp = tokio::time::timeout(
        Duration::from_secs(timeout_secs + 5),
        client.execute(request),
    )
    .await
    .map_err(|_| err("TIMEOUT", format!("请求超时（>{timeout_secs}s）")))?
    .map_err(|e| classify_reqwest(&e))?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(classify_status(status, &body));
    }

    let is_anthropic = ctx.provider.provider_type == ProviderType::Anthropic;
    let provider_name = ctx.provider.name.clone();

    let (content, reasoning, usage, tool_calls) = if req.stream {
        stream_body(&ctx, resp, is_anthropic, sink).await?
    } else {
        full_body(&ctx, resp, is_anthropic).await?
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
    is_anthropic: bool,
    sink: &DeltaSink,
) -> Result<(String, Option<String>, Option<TokenUsage>, Vec<crate::models::ToolCall>), String> {
    let mut stream = resp.bytes_stream();
    let mut buf: Vec<u8> = Vec::new();
    let mut content = String::new();
    let mut reasoning = String::new();
    let mut usage: Option<TokenUsage> = None;
    let mut tools = ToolAccumulator::default();

    while let Some(chunk) = stream.next().await {
        // 每次收到新数据先检查中断旗标，做到快速停止
        if ctx.abort.load(Ordering::Relaxed) {
            return Err(err("ABORTED", "用户停止了生成"));
        }
        let chunk = chunk.map_err(|e| classify_reqwest(&e))?;
        buf.extend_from_slice(&chunk);

        // SSE 按行处理：先在缓冲上切片处理所有完整行（零中间分配），
        // 循环结束后一次性 drain 掉已消费部分（原实现每行 drain 一次，O(行数×缓冲)）
        let mut start = 0usize;
        while let Some(rel) = buf[start..].iter().position(|&b| b == b'\n') {
            let end = start + rel;
            let line = String::from_utf8_lossy(&buf[start..end]);
            let line = line.trim();
            if !line.is_empty() {
                if is_anthropic {
                    handle_anthropic_line(line, &mut content, &mut reasoning, &mut usage, &mut tools, sink);
                } else {
                    handle_openai_line(line, &mut content, &mut reasoning, &mut usage, &mut tools, sink);
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
        // 部分供应商以 reasoning_content 返回思考过程（§4.10）
        if let Some(r) = delta.get("reasoning_content").and_then(Value::as_str) {
            if !r.is_empty() {
                reasoning.push_str(r);
                sink("", Some(r));
            }
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

async fn full_body(
    ctx: &LlmContext,
    resp: reqwest::Response,
    is_anthropic: bool,
) -> Result<(String, Option<String>, Option<TokenUsage>, Vec<crate::models::ToolCall>), String> {
    let _ = ctx;
    let v: Value = resp
        .json()
        .await
        .map_err(|e| err("SERVER", format!("解析响应失败: {e}")))?;

    if is_anthropic {
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
}
