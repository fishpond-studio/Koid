/**
 * Koid 全局类型定义（前端视角）
 * 与 src-tauri/src/models 中的 Rust 结构体一一对应（serde 序列化契约）
 */

// ---------- 模型管理（§4.2） ----------

export type ProviderType =
  | 'openai-compatible'
  | 'anthropic'
  | 'openai-response'
  | 'ollama'
  | 'custom'

export type ProxyType = 'direct' | 'http' | 'socks5'

export interface Provider {
  id: string
  name: string
  type: ProviderType
  baseUrl: string
  /** 仅存于系统 keyring；此字段前端永远只读到掩码值 */
  apiKeyMasked: string | null
  proxyType: ProxyType
  proxyUrl: string | null
  /** 秒 */
  timeout: number
  retries: number
  orderIndex: number
  enabled: boolean
}

export type ModelCapability = 'chat' | 'vision' | 'tools' | 'reasoning'

export interface Model {
  id: string
  providerId: string
  /** 实际传给 API 的 ID，如 gpt-4o */
  modelId: string
  displayName: string
  contextWindow: number
  capabilities: ModelCapability[]
  enabled: boolean
}

// ---------- 会话（§4.5） ----------

/** 思考强度档位（default = 不下发参数走模型默认） */
export type ThinkingLevel = 'default' | 'low' | 'medium' | 'high' | 'max'

export interface Session {
  id: string
  title: string
  folderId: string | null
  modelId: string
  systemPrompt: string | null
  temperature: number | null
  topP: number | null
  maxTokens: number | null
  createdAt: number
  updatedAt: number
  isPinned: boolean
  isArchived: boolean
  /** 所属工作区（§4.5 自动分组） */
  workspaceId: string | null
  /** 思考强度（见 ThinkingLevel） */
  thinkingLevel: ThinkingLevel | null
}

export interface Folder {
  id: string
  name: string
  parentId: string | null
  orderIndex: number
  createdAt: number
}

/** 顶层工作区（§4.5 Workspace → Folder → Session 第一层）
 *  path 为本地项目目录（vibe coding 的文件读写根目录） */
export interface Workspace {
  id: string
  name: string
  orderIndex: number
  createdAt: number
  path: string | null
}

export interface WorkspaceInput {
  id?: string | null
  name: string
  orderIndex?: number
  path?: string | null
}

/** 工作区文件树条目（vibe coding：模型读取工作区文件） */
export interface WorkspaceFileEntry {
  path: string
  name: string
  isDir: boolean
}

export type MessageRole = 'user' | 'assistant' | 'system' | 'tool'

export interface ToolCall {
  id: string
  name: string
  arguments: string
}

export interface ToolResult {
  toolCallId: string
  name?: string
  content: string
  isError: boolean
}

export interface Message {
  id: string
  sessionId: string
  role: MessageRole
  content: string
  reasoning?: string | null
  toolCalls?: ToolCall[] | null
  toolResults?: ToolResult[] | null
  tokensUsed?: number | null
  latencyMs?: number | null
  createdAt: number
  parentId?: string | null
}

// ---------- LLM 请求/响应（§6.1） ----------

export interface ChatMessage {
  role: MessageRole
  content: string
  /** assistant 消息的工具调用（OpenAI 原生格式） */
  toolCalls?: unknown
  /** tool 消息关联的调用 id */
  toolCallId?: string
  toolName?: string
}

export interface ChatRequest {
  /** 请求唯一 ID，用于中断（chat_abort） */
  requestId: string
  providerId: string
  modelId: string
  messages: ChatMessage[]
  temperature?: number | null
  topP?: number | null
  maxTokens?: number | null
  system?: string | null
  stream: boolean
  /** 所属会话：故障转移日志定位用 */
  sessionId?: string | null
  /** 思考强度：default / low / medium / high（default = 不下发参数） */
  thinkingLevel?: string | null
}

export interface TokenUsage {
  promptTokens: number
  completionTokens: number
  totalTokens: number
}

export interface ChatResponse {
  content: string
  reasoning?: string | null
  toolCalls?: ToolCall[] | null
  toolResults?: ToolResult[] | null
  usage?: TokenUsage | null
  latencyMs: number
  providerUsed: string
}

/** chat_stream 增量事件载荷 */
export interface ChatChunk {
  requestId: string
  /** 增量文本 */
  delta: string
  /** 增量思考文本 */
  reasoningDelta?: string | null
  done: boolean
  result?: ChatResponse | null
  error?: string | null
}

/** 工具调用事件载荷（chat:tool_call，agent 循环） */
export interface ToolCallEvent {
  requestId: string
  round: number
  tool: string
  arguments: string
  /** running / done / error */
  status: 'running' | 'done' | 'error'
  result?: string
  isError?: boolean
}

// ---------- 提示词（§4.6） ----------

export type PromptType = 'system' | 'snippet' | 'template'

export interface Prompt {
  id: string
  title: string
  content: string
  variables: string[]
  type: PromptType
  tags: string[]
  usageCount: number
  createdAt: number
}

export interface PromptInput {
  id?: string | null
  title: string
  content: string
  type: PromptType
  tags?: string[] | null
}

/** 提示词版本快照（§4.6 版本历史，最近 10 份） */
export interface PromptVersion {
  id: string
  promptId: string
  content: string
  createdAt: number
}

// ---------- MCP（§4.8） ----------

export interface McpToolSchema {
  type: string
  properties?: Record<string, unknown>
  required?: string[]
}

export interface McpTool {
  name: string
  description?: string | null
  inputSchema?: McpToolSchema | null
}

export type McpTransport = 'stdio' | 'sse'
export type McpStatus = 'connected' | 'disconnected' | 'error'

export interface McpServer {
  id: string
  name: string
  transport: McpTransport
  command?: string | null
  args?: string[] | null
  env?: Record<string, string> | null
  url?: string | null
  status: McpStatus
  tools: McpTool[]
  errorMessage?: string | null
}

export interface McpServerInput {
  id?: string | null
  name: string
  transport: McpTransport
  command?: string | null
  args?: string[] | null
  env?: Record<string, string> | null
  url?: string | null
}

// ---------- Skills（§4.7） ----------

export type SkillStepType = 'llm' | 'condition' | 'tool' | 'message' | 'input'

export interface SkillStep {
  id: string
  type: SkillStepType
  prompt?: string | null
  content?: string | null
  condition?: string | null
  then?: string | null
  else?: string | null
  tool?: string | null
  server?: string | null
  args?: string | null
}

export interface SkillDef {
  id: string
  name: string
  description: string
  icon?: string | null
  /** 按 model_id / displayName 匹配；缺省取首个可用模型 */
  model?: string | null
  systemPrompt?: string | null
  steps: SkillStep[]
  /** builtin / user */
  source: 'builtin' | 'user'
  enabled: boolean
}

export type SkillEventKind =
  | 'started'
  | 'step'
  | 'output'
  | 'input-required'
  | 'message'
  | 'done'
  | 'error'
  | 'cancelled'

/** Skill 执行事件载荷（skill:event） */
export interface SkillEvent {
  requestId: string
  skillId: string
  kind: SkillEventKind
  stepId?: string | null
  label?: string | null
  content?: string | null
  error?: string | null
  progress?: number | null
}

// ---------- 故障转移（§4.3） ----------

export type FailoverStrategy = 'sequential' | 'round-robin' | 'random'

export type FailoverTrigger =
  | 'timeout'
  | '5xx'
  | 'empty-response'
  | 'content-filter'

export interface FailoverConfig {
  enabled: boolean
  strategy: FailoverStrategy
  triggerConditions: FailoverTrigger[]
  /** 默认 [401, 403]：这些错误代表凭证问题，重试无意义 */
  excludedStatusCodes: number[]
  backoffMultiplier: number
  maxBackoffSeconds: number
  /** 用户自选备选模型链（有序，可跨/同供应商多模型）；空 = 自动发现候选 */
  fallbackChain: string[]
}

/** 故障转移通知事件载荷（chat:failover） */
export interface FailoverEvent {
  requestId: string
  fromProvider: string
  toProvider: string
  reason: string
}

// ---------- 代理（§4.4） ----------

export interface GlobalProxySettings {
  proxyType: ProxyType
  proxyUrl: string | null
}

export interface ProxyTestInput {
  url: string
  proxyType: ProxyType
  proxyUrl?: string | null
  timeout?: number
}

export interface ProxyTestResult {
  success: boolean
  latencyMs: number | null
  statusCode: number | null
  error: string | null
}

// ---------- Rust Command 入参（与 src-tauri models 的 Input 结构体对应） ----------

export interface ProviderInput {
  id?: string | null
  name: string
  type: ProviderType
  baseUrl: string
  /** 编辑时留空表示不修改 Key */
  apiKey?: string | null
  proxyType?: ProxyType
  proxyUrl?: string | null
  timeout?: number
  retries?: number
  orderIndex?: number
  enabled?: boolean
}

export interface ModelInput {
  id?: string | null
  providerId: string
  modelId: string
  displayName?: string | null
  contextWindow?: number | null
  capabilities?: ModelCapability[] | null
  enabled?: boolean
}

/** `/v1/models` 发现结果（§4.2 模型自动发现） */
export interface DiscoveredModel {
  modelId: string
  displayName: string
  /** 是否已在本地库启用（勾选预置） */
  enabled: boolean
}

export interface SessionInput {
  id?: string | null
  title?: string | null
  folderId?: string | null
  workspaceId?: string | null
  modelId?: string | null
  systemPrompt?: string | null
  temperature?: number | null
  topP?: number | null
  maxTokens?: number | null
  isPinned?: boolean
  isArchived?: boolean
  /** 思考强度（见 ThinkingLevel） */
  thinkingLevel?: ThinkingLevel | null
}

/** 全局搜索结果（§4.5.3） */
export interface MessageHit {
  sessionTitle: string
  message: Message
}

export interface SearchResults {
  sessions: Session[]
  messages: MessageHit[]
}

export interface MessageInput {
  sessionId: string
  role: MessageRole
  content: string
  reasoning?: string | null
  toolCalls?: ToolCall[] | null
  toolResults?: ToolResult[] | null
  tokensUsed?: number | null
  latencyMs?: number | null
  parentId?: string | null
}

// ---------- 插件（§4.9） ----------

export interface PluginInfo {
  id: string
  name: string
  version: string
  entry: string
  permissions: string[]
}

// ---------- 设置 ----------

export type ThemeMode = 'light' | 'dark' | 'system'
export type ThemeColor =
  | 'indigo'
  | 'emerald'
  | 'rose'
  | 'amber'
  | 'violet'
  | 'sky'
  | 'custom'
export type UiDensity = 'compact' | 'default' | 'comfortable'

export interface ThemeSettings {
  mode: ThemeMode
  color: ThemeColor
  /** 自定义主题色的 HEX 值（color === 'custom' 时生效） */
  customPrimary: string | null
  density: UiDensity
  codeFont: string
}
