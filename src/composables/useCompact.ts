import { useI18n } from 'vue-i18n'
import { toast } from 'vue-sonner'
import { chatApi, messagesApi, toApiError } from '@/lib/api'
import { useModelStore } from '@/stores/model'
import { useSessionStore } from '@/stores/session'
import { useSettingsStore } from '@/stores/settings'

/** 上下文占用达到模型窗口该比例时自动压缩 */
const AUTO_COMPACT_RATIO = 0.7
/** 压缩保留最近 N 条消息不参与摘要（保留最近一轮问答） */
const KEEP_RECENT = 2

/**
 * 对话压缩：把早期消息压缩为一条 system 摘要（§4.10 /compact）。
 * 手动 /compact 与「自动总结上下文」开关共用。
 */
export function useCompact() {
  const { t } = useI18n()
  const sessions = useSessionStore()
  const models = useModelStore()
  const settings = useSettingsStore()

  /**
   * 压缩用模型：设置里指定的优先（可用便宜/快的模型做总结），
   * 未指定则跟随会话当前模型，再退回首个启用模型
   */
  function resolveModel() {
    const session = sessions.current
    const specified = settings.compactModelId
      ? models.get(settings.compactModelId)
      : null
    if (specified?.enabled) return specified
    if (!session) return models.enabledModels[0] ?? null
    return (
      (session.modelId ? models.get(session.modelId) : null) ??
      models.enabledModels[0] ??
      null
    )
  }

  /**
   * 压缩当前会话：早期消息 → system 摘要，删除原消息。
   * @returns 是否执行了压缩（会话太短/无模型时返回 false）
   */
  async function compactSession(): Promise<boolean> {
    const session = sessions.current
    if (!session || sessions.messages.length < KEEP_RECENT * 2) return false
    const model = resolveModel()
    if (!model) return false

    const summaryTargets = sessions.messages.slice(0, -KEEP_RECENT)
    try {
      const summary = await chatApi.chat({
        requestId: `compact-${Date.now()}`,
        providerId: model.providerId,
        modelId: model.modelId,
        messages: [
          {
            role: 'system',
            content:
              '你是对话压缩器。用中文把下面的对话压缩成简洁的摘要，保留关键事实、用户意图和结论。',
          },
          {
            role: 'user',
            content: summaryTargets.map((m) => `${m.role}: ${m.content}`).join('\n\n'),
          },
        ],
        system: null,
        stream: false,
      })
      for (const m of summaryTargets) await messagesApi.remove(m.id)
      await messagesApi.append({
        sessionId: session.id,
        role: 'system',
        content: `以下为之前对话的摘要，请在此基础上继续：\n\n${summary.content}`,
      })
      await sessions.open(session.id)
      return true
    } catch (e) {
      toast.error(toApiError(e).message)
      return false
    }
  }

  /**
   * 自动压缩检查：开关开启且上下文占用 ≥ 70% 时先压缩。
   * 在发送消息前调用；返回是否发生了自动压缩（供提示）。
   */
  async function maybeAutoCompact(): Promise<boolean> {
    if (!settings.autoCompact) return false
    const session = sessions.current
    if (!session || sessions.messages.length < KEEP_RECENT * 2 + 2) return false

    const contextWindow =
      (session.modelId ? models.get(session.modelId)?.contextWindow : null) ?? null
    if (!contextWindow || contextWindow <= 0) return false

    // 上下文近似 = 最近一条 assistant 消息 tokensUsed（与侧边栏同口径）
    let lastContext = 0
    for (let i = sessions.messages.length - 1; i >= 0; i--) {
      const m = sessions.messages[i]
      if (m.role === 'assistant' && m.tokensUsed && m.tokensUsed > 0) {
        lastContext = m.tokensUsed
        break
      }
    }
    if (lastContext / contextWindow < AUTO_COMPACT_RATIO) return false

    const done = await compactSession()
    if (done) toast.success(t('chat.autoCompactDone'))
    return done
  }

  return {
    compactSession,
    maybeAutoCompact,
  }
}
