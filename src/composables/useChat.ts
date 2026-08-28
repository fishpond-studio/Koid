import { ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { toast } from 'vue-sonner'
import { chatApi, errI18nKey, messagesApi, toApiError } from '@/lib/api'
import { useModelStore } from '@/stores/model'
import { useProviderStore } from '@/stores/provider'
import { useSessionStore } from '@/stores/session'
import type { ChatChunk, ChatMessage, ChatRequest, FailoverEvent, ToolCallEvent } from '@/types'

/** 流式中的消息状态（尚未落库） */
export interface StreamingState {
  content: string
  reasoning: string
}

// ---------- 模块级共享状态（应用生命周期单例） ----------
// 必须放在模块作用域而非 useChat() 函数体内：流式期间用户可能切换页面，
// ChatView 卸载重建后仍需接续显示同一份流式状态（否则回复"消失"、停止失效）。

const sending = ref(false)
const streaming = ref<StreamingState | null>(null)
/** 正在流式的会话 ID：气泡只在该会话视图内渲染，防止切换会话时串显 */
const streamingSessionId = ref<string | null>(null)
/** 思考已持续秒数（§4.10：「已思考 {n} 秒」） */
const thinkingSeconds = ref(0)
/** 本次生成中的工具调用记录（agent 循环，chat:tool_call） */
const toolCalls = ref<ToolCallEvent[]>([])

let requestId: string | null = null
let unlisten: UnlistenFn | null = null
let unlistenFailover: UnlistenFn | null = null
let unlistenTools: UnlistenFn | null = null
let reasoningStartedAt: number | null = null
let thinkingTimer: number | undefined
// 流式缓冲：chunk 只写缓冲，rAF 合并后再刷入 streaming ref，
// 避免每个 SSE token 都触发整棵 ChatView 重渲染（性能热点 #2）
let bufContent = ''
let bufReasoning = ''
let flushRaf = 0

/** 把缓冲刷入响应式状态（每帧最多一次） */
function flushBuffer() {
  flushRaf = 0
  if (!streaming.value) return
  if (bufContent) {
    streaming.value.content += bufContent
    bufContent = ''
  }
  if (bufReasoning) {
    streaming.value.reasoning += bufReasoning
    bufReasoning = ''
  }
}

function scheduleFlush() {
  if (!flushRaf) flushRaf = requestAnimationFrame(flushBuffer)
}

/** request id 生成：crypto.randomUUID 兜底（file:// 协议极端情况下可能不可用） */
function newRequestId(): string {
  if (typeof crypto !== 'undefined' && 'randomUUID' in crypto) {
    return crypto.randomUUID()
  }
  return `${Date.now()}-${Math.random().toString(16).slice(2)}`
}

/** 会话自动标题：取首行非空文本，去掉 Markdown 修饰符后截 30 字（避免截半句/带符号） */
function deriveTitle(text: string): string {
  const firstLine =
    text
      .split('\n')
      .map((l) => l.trim())
      .find((l) => l.length > 0) ?? ''
  const cleaned = firstLine
    .replace(/^[#>*\-\s]+/, '')
    .replaceAll(/[`*[\]]/g, '')
    .trim()
  return (cleaned || firstLine).slice(0, 30)
}

/**
 * 对话闭环：发送 → 持久化用户消息 → chat_stream → 监听 chat:chunk →
 * 增量渲染 → done 时持久化 assistant 消息。
 * 中断通过 chat_abort 置位 Rust 侧旗标实现（§6.1 可取消要求）。
 */
export function useChat() {
  const { t } = useI18n()
  const sessions = useSessionStore()
  const models = useModelStore()
  const providers = useProviderStore()

  function cleanup() {
    sending.value = false
    streaming.value = null
    streamingSessionId.value = null
    requestId = null
    toolCalls.value = []
    unlisten?.()
    unlisten = null
    unlistenFailover?.()
    unlistenFailover = null
    unlistenTools?.()
    unlistenTools = null
    window.clearInterval(thinkingTimer)
    thinkingTimer = undefined
    if (flushRaf) {
      cancelAnimationFrame(flushRaf)
      flushRaf = 0
    }
    bufContent = ''
    bufReasoning = ''
    reasoningStartedAt = null
    thinkingSeconds.value = 0
  }

  async function send(text: string) {
    const content = text.trim()
    if (!content || sending.value) return

    const session = sessions.current
    if (!session) return

    const model = session.modelId ? models.get(session.modelId) : null
    if (!model) {
      toast.error(t('chat.noModel'))
      return
    }
    const provider = providers.get(model.providerId)
    if (!provider?.enabled) {
      toast.error(t('chat.providerDisabled'))
      return
    }

    // 1) 用户消息即时落库并渲染
    try {
      const userMsg = await messagesApi.append({
        sessionId: session.id,
        role: 'user',
        content,
      })
      sessions.pushMessage(userMsg)
    } catch (e) {
      toast.error(toApiError(e).message)
      return
    }

    // 首次发言自动生成标题（截取前 30 字符，失败不影响主流程）
    if (!session.title || session.title === '新对话') {
      void sessions.update(session.id, { title: deriveTitle(content) }).catch(() => {})
    }

    // 2) 组装请求：内部统一 OpenAI 消息格式，system 单独字段（§6.1）
    const rid = newRequestId()
    requestId = rid
    sending.value = true
    streaming.value = { content: '', reasoning: '' }
    streamingSessionId.value = session.id

    /**
     * 历史回放（跨轮次记忆的关键）：
     * 含工具调用的 assistant 消息拆成 assistant(tool_calls) → tool 结果 →
     * assistant(最终回答) 三段，模型才能"记得"自己读过/改过什么。
     * 超长工具结果截断，防止上下文爆炸。
     */
    const TOOL_RESULT_MAX = 6000
    const clip = (s: string) =>
      s.length > TOOL_RESULT_MAX
        ? s.slice(0, TOOL_RESULT_MAX) + `\n…[结果过长已截断，原长 ${s.length} 字符]`
        : s

    const history: ChatMessage[] = []
    for (const m of sessions.messages) {
      if (m.role === 'user') {
        history.push({ role: 'user', content: m.content })
        continue
      }
      if (m.role !== 'assistant') continue
      const calls = m.toolCalls ?? []
      if (calls.length === 0) {
        history.push({ role: 'assistant', content: m.content })
        continue
      }
      // assistant 工具调用消息（content 置空：正文是工具后的最终回答）
      history.push({
        role: 'assistant',
        content: '',
        toolCalls: calls.map((c) => ({
          id: c.id,
          type: 'function',
          function: { name: c.name, arguments: c.arguments },
        })),
      } as ChatMessage)
      // 每个调用对应一条 tool 结果
      const results = m.toolResults ?? []
      for (const c of calls) {
        const r = results.find((x) => x.toolCallId === c.id)
        history.push({
          role: 'tool',
          content: clip(r?.content ?? '（无执行记录）'),
          toolCallId: c.id,
          toolName: c.name,
        } as ChatMessage)
      }
      // 最终回答
      history.push({ role: 'assistant', content: m.content })
    }

    const request: ChatRequest = {
      requestId: rid,
      providerId: model.providerId,
      modelId: model.modelId,
      messages: history,
      system: session.systemPrompt ?? null,
      temperature: session.temperature ?? null,
      topP: session.topP ?? null,
      maxTokens: session.maxTokens ?? null,
      stream: true,
      // 会话 ID 供 Rust 侧写故障转移日志（§4.3）
      sessionId: session.id,
      // 思考强度（会话级，§4.2）：default/None 不下发参数
      thinkingLevel: session.thinkingLevel ?? null,
    }

    // 3) 完成处理：错误提示 / assistant 消息落库 / 刷新侧边栏排序
    const finish = async (chunk: ChatChunk) => {
      const text2 = streaming.value?.content ?? ''
      const reasoning = streaming.value?.reasoning ?? ''

      if (chunk.error && chunk.error !== 'ABORTED') {
        toast.error(t(errI18nKey(chunk.error)))
      }

      if (text2 || reasoning) {
        try {
          const saved = await messagesApi.append({
            sessionId: session.id,
            role: 'assistant',
            content: text2,
            reasoning: reasoning || null,
            toolCalls: chunk.result?.toolCalls ?? null,
            toolResults: chunk.result?.toolResults ?? null,
            tokensUsed: chunk.result?.usage?.totalTokens ?? null,
            latencyMs: chunk.result?.latencyMs ?? null,
          })
          // 流式期间用户可能已切走会话：仅当前仍在本会话时追加到视图
          if (sessions.currentId === session.id) {
            sessions.pushMessage(saved)
          }
        } catch (e) {
          toast.error(toApiError(e).message)
        }
      }

      // 会话 updatedAt 变化，侧边栏重排
      void sessions.load(true).catch(() => {})
      cleanup()
    }

    // 4) 监听增量事件（以 requestId 过滤，防止串流）
    unlisten = await listen<ChatChunk>('chat:chunk', (event) => {
      const c = event.payload
      if (c.requestId !== rid) return
      if (!c.done) {
        if (streaming.value) {
          bufContent += c.delta
          if (c.reasoningDelta) {
            bufReasoning += c.reasoningDelta
            // 首个思考增量到达时启动计时
            if (reasoningStartedAt === null) {
              reasoningStartedAt = Date.now()
              thinkingTimer = window.setInterval(() => {
                if (reasoningStartedAt !== null) {
                  thinkingSeconds.value = Math.floor(
                    (Date.now() - reasoningStartedAt) / 1000,
                  )
                }
              }, 500)
            }
          }
          scheduleFlush()
        }
      } else {
        // 完成前先同步刷掉残留缓冲，保证落库内容完整
        flushBuffer()
        void finish(c)
      }
    })

    // 故障转移通知：非侵入 Toast（§4.3 bottom-right，由全局 Toaster 统一呈现）
    unlistenFailover = await listen<FailoverEvent>('chat:failover', (event) => {
      const f = event.payload
      if (f.requestId !== rid) return
      toast.info(t('chat.failoverToast', { provider: f.toProvider }))
    })

    // 工具调用（agent 循环，chat:tool_call）：更新/追加到 toolCalls
    unlistenTools = await listen<ToolCallEvent>('chat:tool_call', (event) => {
      const tc = event.payload
      if (tc.requestId !== rid) return
      const idx = toolCalls.value.findIndex(
        (x) => x.round === tc.round && x.tool === tc.tool && x.arguments === tc.arguments,
      )
      if (idx >= 0) {
        toolCalls.value[idx] = tc
      } else {
        toolCalls.value.push(tc)
      }
    })

    // 5) 启动流式任务；命令层失败（如未配置 Key）在此捕获
    try {
      await chatApi.stream(request)
    } catch (e) {
      const apiErr = toApiError(e)
      if (apiErr.code !== 'ABORTED') {
        toast.error(apiErr.code === 'UNKNOWN' ? apiErr.message : t(errI18nKey(apiErr.code)))
      }
      cleanup()
    }
  }

  function stop() {
    if (requestId) void chatApi.abort(requestId)
  }

  return { sending, streaming, streamingSessionId, thinkingSeconds, toolCalls, send, stop }
}
