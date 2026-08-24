import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import { messagesApi, sessionsApi, toApiError } from '@/lib/api'
import type { Message, SearchResults, Session, SessionInput, ThinkingLevel } from '@/types'

/**
 * 会话 Store：Workspace → Folder → Session 中的 Session/Message 层
 * Folder 层级在 Phase 2 接入
 */
export const useSessionStore = defineStore('session', () => {
  const sessions = ref<Session[]>([])
  const currentId = ref<string | null>(null)
  const messages = ref<Message[]>([])
  const loaded = ref(false)
  /** 侧边栏「已归档」区块显隐 */
  const showArchived = ref(false)
  /** 搜索跳转目标消息：ChatView 渲染后滚动定位并高亮 */
  const scrollToMessageId = ref<string | null>(null)

  const current = computed<Session | null>(
    () => sessions.value.find((s) => s.id === currentId.value) ?? null,
  )

  /**
   * 当前会话 token 统计：
   * - total：全部消息 tokensUsed 累计（本会话累计用量）
   * - lastContext：最近一条 assistant 消息的 tokensUsed
   *   （≈ 最近一轮请求的 prompt+completion，作为当前上下文大小的近似）
   */
  const tokenStats = computed(() => {
    let total = 0
    let lastContext = 0
    for (const m of messages.value) {
      if (m.tokensUsed && m.tokensUsed > 0) {
        total += m.tokensUsed
        if (m.role === 'assistant') lastContext = m.tokensUsed
      }
    }
    return { total, lastContext }
  })

  async function load(force = false) {
    if (loaded.value && !force) return
    try {
      sessions.value = await sessionsApi.list()
      loaded.value = true
    } catch (e) {
      throw toApiError(e)
    }
  }

  async function open(id: string) {
    try {
      messages.value = await messagesApi.list(id)
      currentId.value = id
    } catch (e) {
      throw toApiError(e)
    }
  }

  async function create(
    modelId: string | null,
    systemPrompt?: string | null,
    workspaceId?: string | null,
    thinkingLevel?: ThinkingLevel | null,
  ): Promise<Session> {
    try {
      const s = await sessionsApi.save({
        modelId,
        systemPrompt: systemPrompt ?? null,
        workspaceId: workspaceId ?? null,
        thinkingLevel: thinkingLevel ?? null,
      })
      await load(true)
      await open(s.id)
      return s
    } catch (e) {
      throw toApiError(e)
    }
  }

  /** 差量更新：只提交需要变更的字段 */
  async function update(id: string, patch: Omit<SessionInput, 'id'>) {
    try {
      const saved = await sessionsApi.save({ id, ...patch })
      const idx = sessions.value.findIndex((s) => s.id === id)
      if (idx >= 0) sessions.value[idx] = saved
      return saved
    } catch (e) {
      throw toApiError(e)
    }
  }

  async function remove(id: string) {
    try {
      await sessionsApi.remove(id)
      if (currentId.value === id) {
        currentId.value = null
        messages.value = []
      }
      await load(true)
    } catch (e) {
      throw toApiError(e)
    }
  }

  function pushMessage(m: Message) {
    messages.value.push(m)
  }

  /** 「新建对话」：清空当前会话，真实创建推迟到首次发送（ensureSession） */
  function clearCurrent() {
    currentId.value = null
    messages.value = []
  }

  /** 分支会话（§4.5.4）：从指定消息处复制上下文到新会话并打开 */
  async function branch(sourceId: string, upToMessageId: string, title: string) {
    try {
      const s = await sessionsApi.branch(sourceId, upToMessageId, title)
      await load(true)
      await open(s.id)
      return s
    } catch (e) {
      throw toApiError(e)
    }
  }

  async function toggleArchive(id: string, archived: boolean) {
    try {
      await update(id, { isArchived: archived })
      await load(true)
    } catch (e) {
      throw toApiError(e)
    }
  }

  async function search(query: string): Promise<SearchResults> {
    try {
      return await sessionsApi.search(query)
    } catch (e) {
      throw toApiError(e)
    }
  }

  return {
    sessions,
    currentId,
    messages,
    loaded,
    current,
    tokenStats,
    showArchived,
    scrollToMessageId,
    load,
    open,
    create,
    update,
    remove,
    pushMessage,
    clearCurrent,
    branch,
    toggleArchive,
    search,
  }
})
