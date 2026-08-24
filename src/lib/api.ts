/**
 * Tauri Command 统一访问层
 * 所有 Rust 命令调用从此处经过，集中处理：
 * - 类型收敛（invoke 泛型）
 * - 错误解析：Rust 约定 `CODE:可读信息`，拆为 ApiError(code, message)
 */
import { invoke } from '@tauri-apps/api/core'
import type {
  ChatRequest,
  ChatResponse,
  DiscoveredModel,
  McpServer,
  McpServerInput,
  McpTool,
  Message,
  MessageInput,
  Model,
  ModelInput,
  PluginInfo,
  Prompt,
  PromptInput,
  PromptVersion,
  Provider,
  ProviderInput,
  ProxyTestInput,
  ProxyTestResult,
  SearchResults,
  Session,
  SessionInput,
  SkillDef,
  Workspace,
  WorkspaceFileEntry,
  WorkspaceInput,
} from '@/types'

export class ApiError extends Error {
  constructor(
    public code: string,
    message: string,
  ) {
    super(message)
    this.name = 'ApiError'
  }
}

/** Rust 返回的错误字符串 → ApiError；未知结构兜底 UNKNOWN */
export function toApiError(e: unknown): ApiError {
  const s = typeof e === 'string' ? e : e instanceof Error ? e.message : String(e)
  const idx = s.indexOf(':')
  if (idx > 0 && idx < 20) {
    const code = s.slice(0, idx)
    if (/^[A-Z_]+$/.test(code)) return new ApiError(code, s.slice(idx + 1).trim())
  }
  return new ApiError('UNKNOWN', s)
}

/**
 * 错误码 → i18n key（§7.2 网络错误分类）
 * EMPTY/ABORTED 不属于需要 Toast 的错误（空响应由容灾处理，中断是用户行为）
 */
export function errI18nKey(code: string): string {
  switch (code) {
    case 'NETWORK':
    case 'ECONNREFUSED':
      return 'error.network'
    case 'TIMEOUT':
      return 'error.timeout'
    case 'UNAUTHORIZED':
      return 'error.unauthorized'
    case 'RATE_LIMITED':
      return 'error.rateLimited'
    case 'SERVER':
    case 'BAD_REQUEST':
      return 'error.server'
    default:
      return 'error.title'
  }
}

export const providersApi = {
  list: () => invoke<Provider[]>('list_providers'),
  save: (input: ProviderInput) => invoke<Provider>('save_provider', { input }),
  remove: (id: string) => invoke<void>('delete_provider', { id }),
}

export const modelsApi = {
  list: (providerId?: string) =>
    invoke<Model[]>('list_models', { providerId: providerId ?? null }),
  save: (input: ModelInput) => invoke<Model>('save_model', { input }),
  remove: (id: string) => invoke<void>('delete_model', { id }),
  discover: (providerId: string) => invoke<DiscoveredModel[]>('discover_models', { providerId }),
}

export const sessionsApi = {
  list: () => invoke<Session[]>('list_sessions'),
  save: (input: SessionInput) => invoke<Session>('save_session', { input }),
  remove: (id: string) => invoke<void>('delete_session', { id }),
  branch: (sessionId: string, upToMessageId: string, title: string) =>
    invoke<Session>('branch_session', { sessionId, upToMessageId, title }),
  search: (query: string) => invoke<SearchResults>('search_sessions', { query }),
}

export const workspacesApi = {
  list: () => invoke<Workspace[]>('list_workspaces'),
  save: (input: WorkspaceInput) => invoke<Workspace>('save_workspace', { input }),
  remove: (id: string) => invoke<void>('delete_workspace', { id }),
  listFiles: (workspaceId: string) =>
    invoke<WorkspaceFileEntry[]>('list_workspace_files', { workspaceId }),
  readFile: (workspaceId: string, relPath: string) =>
    invoke<string>('read_workspace_file', { workspaceId, relPath }),
}

export const messagesApi = {
  list: (sessionId: string) => invoke<Message[]>('list_messages', { sessionId }),
  append: (input: MessageInput) => invoke<Message>('append_message', { input }),
  remove: (id: string) => invoke<void>('delete_message', { id }),
  /** 删除某条消息及其后的全部消息（撤回 / 编辑重发），返回删除条数 */
  deleteFrom: (sessionId: string, messageId: string) =>
    invoke<number>('delete_messages_from', { sessionId, messageId }),
}

export const chatApi = {
  /** 返回即代表任务已启动，增量经 chat:chunk 事件推送 */
  stream: (request: ChatRequest) => invoke<void>('chat_stream', { request }),
  abort: (requestId: string) => invoke<boolean>('chat_abort', { requestId }),
  /** 非流式：插件系统 koid.llm.chat 复用 */
  chat: (request: ChatRequest) => invoke<ChatResponse>('chat', { request }),
}

export const settingsApi = {
  get: (key: string) => invoke<string | null>('get_setting', { key }),
  set: (key: string, value: string) => invoke<void>('set_setting', { key, value }),
}

export const proxyApi = {
  test: (input: ProxyTestInput) => invoke<ProxyTestResult>('test_proxy', { input }),
}

export const promptsApi = {
  list: () => invoke<Prompt[]>('list_prompts'),
  save: (input: PromptInput) => invoke<Prompt>('save_prompt', { input }),
  remove: (id: string) => invoke<void>('delete_prompt', { id }),
  bumpUsage: (id: string) => invoke<void>('bump_prompt_usage', { id }),
  versions: (promptId: string) => invoke<PromptVersion[]>('list_prompt_versions', { promptId }),
}

export const skillsApi = {
  list: () => invoke<SkillDef[]>('list_skills'),
  save: (content: string) => invoke<SkillDef>('save_skill', { content }),
  remove: (id: string) => invoke<void>('delete_skill', { id }),
  run: (requestId: string, skillId: string, vars: Record<string, string>) =>
    invoke<void>('run_skill', { requestId, skillId, vars }),
  respond: (requestId: string, value: string) =>
    invoke<boolean>('skill_respond', { requestId, value }),
  cancel: (requestId: string) => invoke<void>('skill_cancel', { requestId }),
}

export const mcpApi = {
  list: () => invoke<McpServer[]>('list_mcp_servers'),
  save: (input: McpServerInput) => invoke<McpServer>('save_mcp_server', { input }),
  remove: (id: string) => invoke<void>('delete_mcp_server', { id }),
  connect: (id: string) => invoke<McpTool[]>('connect_mcp_server', { id }),
  disconnect: (id: string) => invoke<void>('disconnect_mcp_server', { id }),
  callTool: (serverId: string, tool: string, args: unknown) =>
    invoke<string>('call_mcp_tool', { serverId, tool, args }),
}

export const pluginsApi = {
  list: () => invoke<PluginInfo[]>('list_plugins'),
  html: (id: string) => invoke<string>('plugin_html', { id }),
  remove: (id: string) => invoke<void>('delete_plugin', { id }),
  fileRead: (pluginId: string, path: string) =>
    invoke<string>('plugin_file_read', { pluginId, path }),
  fileWrite: (pluginId: string, path: string, content: string) =>
    invoke<void>('plugin_file_write', { pluginId, path, content }),
  fetch: (url: string, method: string, headers?: Record<string, string>, body?: string) =>
    invoke<PluginFetchResult>('plugin_fetch', { url, method, headers, body }),
  installFromPath: (zipPath: string) =>
    invoke<PluginInfo>('install_plugin_from_path', { zipPath }),
  installFromUrl: (url: string) => invoke<PluginInfo>('install_plugin_from_url', { url }),
}

export interface PluginFetchResult {
  ok: boolean
  status: number
  body: string
}

export const backupApi = {
  export: (passphrase: string, destPath: string | null) =>
    invoke<string>('export_backup', { passphrase, destPath }),
  import: (passphrase: string, path: string) =>
    invoke<void>('import_backup', { passphrase, path }),
}

