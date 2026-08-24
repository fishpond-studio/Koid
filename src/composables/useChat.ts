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

/** request id 生成：crypto.randomUUID 兜底（file:// 协议极端情况下可能不可用） */
function newRequestId(): string {
  if (typeof crypto !== 'undefined' && 'randomUUID' in crypto) {
    return crypto.randomUUID()
  }
  return `${Date.now()}-${Math.random().toString(16).slice(2)}`
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
      void sessions.update(session.id, { title: content.slice(0, 30) }).catch(() => {})
    }

    // 2) 组装请求：内部统一 OpenAI 消息格式，system 单独字段（§6.1）
    const rid = newRequestId()
    requestId = rid
    sending.value = true
    streaming.value = { content: '', reasoning: '' }
    streamingSessionId.value = session.id

    const history: ChatMessage[] = sessions.messages
      .filter((m) => m.role === 'user' || m.role === 'assistant')
      .map((m) => ({ role: m.role, content: m.content }))

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
          streaming.value.content += c.delta
          if (c.reasoningDelta) {
            streaming.value.reasoning += c.reasoningDelta
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
        }
      } else {
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
