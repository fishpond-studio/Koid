//! Rust 数据结构（计划 §三：models/）
//!
//! 所有结构体均与前端 src/types/index.ts 一一对应，serde 统一 camelCase。
//! 数据库行 ↔ 结构体的转换放在各 command 内的 row mapper，保持结构体纯净。

use serde::{Deserialize, Serialize};

// ---------- 供应商 ----------

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ProviderType {
    #[serde(rename = "openai-compatible")]
    OpenAiCompatible,
    #[serde(rename = "anthropic")]
    Anthropic,
    #[serde(rename = "openai-response")]
    OpenAiResponse,
    #[serde(rename = "ollama")]
    Ollama,
    #[serde(rename = "custom")]
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ProxyType {
    #[serde(rename = "direct")]
    Direct,
    #[serde(rename = "http")]
    Http,
    #[serde(rename = "socks5")]
    Socks5,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Provider {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub provider_type: ProviderType,
    pub base_url: String,
    /// 前端永远只拿到掩码（如 sk-a...wxyz），真实 Key 在系统 keyring
    pub api_key_masked: Option<String>,
    pub proxy_type: ProxyType,
    pub proxy_url: Option<String>,
    pub timeout: i64,
    pub retries: i64,
    pub order_index: i64,
    pub enabled: bool,
}

/// 保存供应商的入参（apiKey 为真实值，仅写 keyring 不落库）
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInput {
    /// None = 新建，Some = 更新
    pub id: Option<String>,
    pub name: String,
    #[serde(rename = "type")]
    pub provider_type: ProviderType,
    pub base_url: String,
    /// 未提供或空串表示不修改已有 Key
    pub api_key: Option<String>,
    #[serde(default)]
    pub proxy_type: Option<ProxyType>,
    #[serde(default)]
    pub proxy_url: Option<String>,
    #[serde(default)]
    pub timeout: Option<i64>,
    #[serde(default)]
    pub retries: Option<i64>,
    #[serde(default)]
    pub order_index: Option<i64>,
    #[serde(default)]
    pub enabled: Option<bool>,
}

// ---------- 模型 ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Model {
    pub id: String,
    pub provider_id: String,
    pub model_id: String,
    pub display_name: String,
    pub context_window: Option<i64>,
    pub capabilities: Vec<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInput {
    pub id: Option<String>,
    pub provider_id: String,
    pub model_id: String,
    pub display_name: Option<String>,
    #[serde(default)]
    pub context_window: Option<i64>,
    #[serde(default)]
    pub capabilities: Option<Vec<String>>,
    #[serde(default)]
    pub enabled: Option<bool>,
}

/// `/v1/models` 发现结果：解析供应商返回的模型列表，合并本地启用状态
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredModel {
    /// 传给 API 的真实 ID
    pub model_id: String,
    /// 展示名（供应商未提供时回退为 model_id）
    pub display_name: String,
    /// 当前是否已在本地库中启用（勾选预置）
    pub enabled: bool,
}

// ---------- 会话 / 文件夹 ----------

// 文件夹 CRUD 在 Phase 2 模块接入，结构先行定义
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Folder {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub order_index: i64,
    pub created_at: i64,
}

/// 顶层工作区（§4.5：Workspace → Folder → Session 层级第一层）
/// path 为本地项目目录（vibe coding 的文件读写根目录）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub order_index: i64,
    pub created_at: i64,
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceInput {
    pub id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub order_index: Option<i64>,
    #[serde(default)]
    pub path: Option<String>,
}

/// 工作区文件树条目（vibe coding：模型读取工作区文件）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceFileEntry {
    /// 相对工作区根目录的路径（/ 分隔）
    pub path: String,
    pub name: String,
    pub is_dir: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub title: String,
    pub folder_id: Option<String>,
    pub model_id: Option<String>,
    pub system_prompt: Option<String>,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub max_tokens: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
    pub is_pinned: bool,
    pub is_archived: bool,
    /// 所属工作区（§4.5 自动分组）
    #[serde(default)]
    pub workspace_id: Option<String>,
    /// 思考强度：default / low / medium / high（default = 不发送参数）
    #[serde(default)]
    pub thinking_level: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInput {
    pub id: Option<String>,
    pub title: Option<String>,
    #[serde(default)]
    pub folder_id: Option<String>,
    #[serde(default)]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub model_id: Option<String>,
    #[serde(default)]
    pub system_prompt: Option<String>,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub top_p: Option<f64>,
    #[serde(default)]
    pub max_tokens: Option<i64>,
    #[serde(default)]
    pub is_pinned: Option<bool>,
    #[serde(default)]
    pub is_archived: Option<bool>,
    /// 思考强度：default / low / medium / high（default = 不发送参数）
    #[serde(default)]
    pub thinking_level: Option<String>,
}

/// 全局搜索结果：命中的会话 + 命中的消息（带所属会话标题，§4.5.3）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageHit {
    pub session_title: String,
    pub message: Message,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResults {
    pub sessions: Vec<Session>,
    pub messages: Vec<MessageHit>,
}

// ---------- 消息 ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolResult {
    pub tool_call_id: String,
    pub content: String,
    pub is_error: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    pub session_id: String,
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_results: Option<Vec<ToolResult>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens_used: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<i64>,
    pub created_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageInput {
    pub session_id: String,
    pub role: String,
    pub content: String,
    #[serde(default)]
    pub reasoning: Option<String>,
    #[serde(default)]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(default)]
    pub tool_results: Option<Vec<ToolResult>>,
    #[serde(default)]
    pub tokens_used: Option<i64>,
    #[serde(default)]
    pub latency_ms: Option<i64>,
    #[serde(default)]
    pub parent_id: Option<String>,
}

// ---------- 提示词（§4.6） ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Prompt {
    pub id: String,
    pub title: String,
    pub content: String,
    /// 从 content 中解析 {{var}} 自动提取
    pub variables: Vec<String>,
    /// system / snippet / template
    pub r#type: String,
    pub tags: Vec<String>,
    pub usage_count: i64,
    pub created_at: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptInput {
    pub id: Option<String>,
    pub title: String,
    pub content: String,
    #[serde(rename = "type")]
    pub prompt_type: String,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptVersion {
    pub id: String,
    pub prompt_id: String,
    pub content: String,
    pub created_at: i64,
}

// ---------- Skills（§4.7） ----------

/// Skill 步骤：llm / condition / tool / message / input
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillStep {
    pub id: String,
    #[serde(rename = "type")]
    pub step_type: String,
    /// llm 步骤提示词
    #[serde(default)]
    pub prompt: Option<String>,
    /// message/input 步骤文案（input 为用户提示语）
    #[serde(default)]
    pub content: Option<String>,
    /// condition 表达式：contains( {{step.output}}, 'text' )
    #[serde(default)]
    pub condition: Option<String>,
    /// condition 为真跳转的步骤 id（then 是 Rust 保留字，序列化保持 then）
    #[serde(rename = "then", default)]
    pub then_step: Option<String>,
    #[serde(rename = "else", default)]
    pub else_step: Option<String>,
    /// tool 步骤：MCP 工具名与参数（Phase 3 MCP 模块对接）
    #[serde(default)]
    pub tool: Option<String>,
    #[serde(default)]
    pub server: Option<String>,
    #[serde(default)]
    pub args: Option<String>,
}

/// Skill 定义（YAML 序列化，§4.7）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillDef {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub icon: Option<String>,
    /// 可按 model_id / displayName 匹配；缺省取首个可用模型
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub system_prompt: Option<String>,
    #[serde(default)]
    pub steps: Vec<SkillStep>,
    /// 运行期元信息：builtin / user（不入 YAML）
    #[serde(default)]
    pub source: String,
    /// skill tool 步骤所需的 MCP 工具（引擎解析）
    #[serde(default)]
    pub enabled: bool,
}

/// Skill 执行事件载荷（事件名 skill:event）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillEvent {
    pub request_id: String,
    pub skill_id: String,
    /// started / step / output / input-required / message / done / error / cancelled
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// 已完成 / 总步骤数
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<f64>,
}

// ---------- MCP（§4.8） ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpTool {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_schema: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerInfo {
    pub id: String,
    pub name: String,
    pub transport: String,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub env: Option<serde_json::Value>,
    pub url: Option<String>,
    /// connected / disconnected / error
    pub status: String,
    pub tools: Vec<McpTool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerInput {
    pub id: Option<String>,
    pub name: String,
    pub transport: String,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Option<Vec<String>>,
    #[serde(default)]
    pub env: Option<serde_json::Value>,
    #[serde(default)]
    pub url: Option<String>,
}

// ---------- LLM 请求 / 响应（§6.1） ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    /// assistant 消息的工具调用（OpenAI 原生格式）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<serde_json::Value>,
    /// tool 消息关联的调用 id
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// 工具调用名（Anthropic 转换用）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
}

/// 工具定义（OpenAI tools 格式，Anthropic 由 llm.rs 转换）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDef {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// JSON Schema 参数定义
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatRequest {
    /// 前端生成的请求唯一 ID，用于流式事件关联与中断
    pub request_id: String,
    pub provider_id: String,
    pub model_id: String,
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub top_p: Option<f32>,
    #[serde(default)]
    pub max_tokens: Option<i64>,
    #[serde(default)]
    pub system: Option<String>,
    #[serde(default)]
    pub stream: bool,
    /// 所属会话：故障转移日志定位用（可选）
    #[serde(default)]
    pub session_id: Option<String>,
    /// 可供模型调用的工具（agent 循环注入，§agent）
    #[serde(default)]
    pub tools: Option<Vec<ToolDef>>,
    /// 思考强度：default / low / medium / high / max；None 或 "default" 不下发任何参数
    #[serde(default)]
    pub thinking_level: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub total_tokens: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatResponse {
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<TokenUsage>,
    pub latency_ms: u64,
    pub provider_used: String,
}

/// chat_stream 增量事件载荷（事件名 chat:chunk）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatChunk {
    pub request_id: String,
    /// 增量文本
    pub delta: String,
    /// 增量思考文本（thinking / reasoning_content）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_delta: Option<String>,
    pub done: bool,
    /// 完成时携带完整结果
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<ChatResponse>,
    /// 失败时的错误码：NETWORK / TIMEOUT / UNAUTHORIZED / RATE_LIMITED / SERVER / ABORTED / EMPTY / UNKNOWN
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
}

// ---------- 代理与故障转移（§4.3 / §4.4） ----------

/// 全局代理配置（settings 表 global_proxy 键）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalProxy {
    pub proxy_type: ProxyType,
    pub proxy_url: Option<String>,
}

/// 代理连通性测试结果（§4.4）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyTestResult {
    pub success: bool,
    pub latency_ms: Option<u64>,
    pub status_code: Option<u16>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyTestInput {
    /// 测试目标：通常为供应商 Base URL
    pub url: String,
    #[serde(rename = "proxyType")]
    pub proxy_type: ProxyType,
    #[serde(default)]
    pub proxy_url: Option<String>,
    #[serde(default)]
    pub timeout: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum FailoverStrategy {
    #[serde(rename = "sequential")]
    Sequential,
    #[serde(rename = "round-robin")]
    RoundRobin,
    #[serde(rename = "random")]
    Random,
}

/// 故障转移配置（settings 表 failover_config 键；字段同前端 FailoverConfig）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FailoverConfig {
    pub enabled: bool,
    pub strategy: FailoverStrategy,
    /// timeout / 5xx / empty-response / content-filter
    pub trigger_conditions: Vec<String>,
    /// 这些 HTTP 状态码不触发转移（凭证类错误重试无意义）
    pub excluded_status_codes: Vec<u16>,
    pub backoff_multiplier: f64,
    pub max_backoff_seconds: f64,
    /// 用户自选的备选模型链（按顺序依次接管，模型 id；可跨供应商、可同供应商多模型）。
    /// 空 = 自动发现候选（旧行为）。serde default 兼容旧配置 JSON。
    #[serde(default)]
    pub fallback_chain: Vec<String>,
}

impl Default for FailoverConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            strategy: FailoverStrategy::Sequential,
            trigger_conditions: vec![
                "timeout".to_string(),
                "5xx".to_string(),
                "empty-response".to_string(),
                "content-filter".to_string(),
            ],
            excluded_status_codes: vec![401, 403],
            backoff_multiplier: 2.0,
            max_backoff_seconds: 16.0,
            fallback_chain: vec![],
        }
    }
}

/// 故障转移通知事件载荷（事件名 chat:failover）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FailoverEvent {
    pub request_id: String,
    pub from_provider: String,
    pub to_provider: String,
    pub reason: String,
}
