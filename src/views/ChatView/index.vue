<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue-sonner'
import { ArrowUp, Boxes, Brain, ChevronDown, Command, FileText, Gauge, Slash, SquareStop, Wrench } from 'lucide-vue-next'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import MessageBubble from '@/components/chat/MessageBubble.vue'
import MaxThinking from '@/components/chat/MaxThinking.vue'
import ThinkingLevelSelector from '@/components/chat/ThinkingLevel.vue'
import ArtifactsPreview from '@/components/chat/ArtifactsPreview.vue'
import MarkdownView from '@/components/markdown/MarkdownView.vue'
import WorkspacePicker from '@/components/WorkspacePicker.vue'
import WorkspaceChip from '@/components/WorkspaceChip.vue'
import ModelPicker from './components/ModelPicker.vue'
import BuiltinCommands, { type BuiltinCommandDef } from './components/BuiltinCommands.vue'
import { useChat } from '@/composables/useChat'
import { useCompact } from '@/composables/useCompact'
import { useDraftBus } from '@/composables/useDraftBus'
import { toApiError, messagesApi, workspacesApi } from '@/lib/api'
import { useModelStore } from '@/stores/model'
import { usePromptStore } from '@/stores/prompts'
import { useProviderStore } from '@/stores/provider'
import { useSessionStore } from '@/stores/session'
import { useSettingsStore } from '@/stores/settings'
import { useWorkspaceStore } from '@/stores/workspace'
import { cn } from '@/lib/utils'
import type { Model, Prompt, ThinkingLevel } from '@/types'

const { t } = useI18n()
const sessions = useSessionStore()
const models = useModelStore()
const providers = useProviderStore()
const settings = useSettingsStore()
const prompts = usePromptStore()
const { sending, streaming, streamingSessionId, thinkingSeconds, toolCalls, send, stop } = useChat()
const { maybeAutoCompact } = useCompact()

// 输入框草稿来自共享总线（侧边栏文件树可追加 @引用）
const { draftText: draft } = useDraftBus()
const scroller = ref<HTMLElement | null>(null)
const textareaEl = ref<HTMLTextAreaElement | null>(null)

// ---------- 工作区 ----------
const workspaces = useWorkspaceStore()

/**
 * 工作区门禁（对齐 dsh 的 inert composer）：
 * 未绑定项目路径的工作区无法开始 Vibe Coding——必须先选择一个工作区（§4.5）
 */
const workspacePickerOpen = ref(false)
const needsWorkspace = computed(() => !workspaces.current?.path)

/** 输入框点击/聚焦时若未选择工作区，则弹出工作区选择器（dsh onRequestWorkspace） */
function onComposerClick() {
  if (needsWorkspace.value) workspacePickerOpen.value = true
}

// ---------- 模型切换（底部 chip + /model） ----------
const modelPickerOpen = ref(false)

const currentModel = computed(() => {
  // 会话内模型优先，无会话时回退到响应式的「最近使用」模型（选择后立即刷新）
  const id = sessions.current?.modelId ?? models.preferredModelId ?? null
  return id ? models.get(id) : null
})

const currentModelLabel = computed(() => {
  const m = currentModel.value
  if (!m) return null
  const p = providers.get(m.providerId)
  return `${p?.name ?? '?'} / ${m.displayName}`
})

// ---------- 内置命令 ----------
const builtinCommandsRef = ref<InstanceType<typeof BuiltinCommands> | null>(null)
const cmdActive = ref<'init' | 'review' | 'help' | null>(null)

// ---------- Artifacts 预览（§4.10） ----------

const artifactOpen = ref(false)
const artifactCode = ref('')
const artifactLang = ref('html')

function onPreview(p: { lang: string; code: string }) {
  artifactLang.value = p.lang
  artifactCode.value = p.code
  artifactOpen.value = true
}

/** 用户贴底时才自动滚动，避免打断向上翻阅 */
let stickToBottom = true

// ---------- 发送 ----------

// ---------- 新会话待用默认值：会话未创建时在头部直接设置，首条消息发出时随会话落库 ----------

const pendingSysPrompt = ref('')
const pendingThinking = ref<ThinkingLevel>('default')

async function ensureSession(): Promise<boolean> {
  if (sessions.current) return true
  const modelId = models.lastUsed() ?? models.enabledModels[0]?.id ?? null
  // System Prompt 层级：头部设置的待用值 > 全局默认（§4.6）；思考强度一并带入
  const sp = pendingSysPrompt.value.trim() || settings.defaultSystemPrompt.trim() || null
  await sessions.create(
    modelId,
    sp,
    workspaces.currentId,
    pendingThinking.value !== 'default' ? pendingThinking.value : null,
  )
  pendingSysPrompt.value = ''
  pendingThinking.value = 'default'
  return !!sessions.current
}

async function submit() {
  if (needsWorkspace.value) {
    workspacePickerOpen.value = true
    return
  }
  const text = draft.value
  if (!text.trim() || sending.value) return
  draft.value = ''
  resetHeight()
  try {
    await ensureSession()
    // 自动总结上下文：开关开启且占用超阈值时先压缩再发送
    await maybeAutoCompact()
    // 仅解析显式的 @文件引用（用户主动指定）；项目探索由 agent 工具完成
    const expanded = await expandFileRefs(text)
    await send(expanded)
  } catch {
    // 失败时把内容还给输入框，避免丢字；错误 Toast 由 useChat 负责
    draft.value = text
  }
}

/**
 * 解析输入中的显式 `@相对路径` 文件引用：
 * 命中工作区文件则读取内容注入。项目结构不自动注入——
 * 模型自主探索由 agent 工具（list_dir/read_file/grep/glob）完成（对齐 opencode）
 */
async function expandFileRefs(text: string): Promise<string> {
  const ws = workspaces.current
  if (!ws?.path) return text

  // 确保文件树已加载（发送前兜底，防止冷启动/路径变更后为空）
  if (workspaces.files.length === 0) {
    await workspaces.loadFiles()
  }
  const files = workspaces.files
  if (files.length === 0) return text

  // 解析 @ 引用的文件
  const fileSet = new Set(files.filter((f) => !f.isDir).map((f) => f.path))
  const re = /@([\w./\\-]+)/g
  const found = new Set<string>()
  let m: RegExpExecArray | null
  while ((m = re.exec(text))) {
    const candidate = m[1].replaceAll('\\', '/')
    if (fileSet.has(candidate)) found.add(candidate)
  }
  if (found.size === 0) return text

  const blocks: string[] = []
  for (const path of found) {
    try {
      const content = await workspacesApi.readFile(ws.id, path)
      blocks.push(`[文件: ${path}]\n${content}`)
    } catch (e) {
      toast.error(`${t('chat.fileReadFailed', { path })}: ${toApiError(e).message}`)
    }
  }

  // 保留原始文本，被引用文件内容追加其后
  return blocks.length > 0 ? text + '\n\n---\n' + blocks.join('\n\n---\n') : text
}

// ---------- 输入框触发菜单（§4.10：/ 命令 + Snippets，@ 提及） ----------

type MenuItem =
  | { kind: 'builtin'; command: BuiltinCommandDef }
  | { kind: 'snippet'; prompt: Prompt }
  | { kind: 'model'; model: Model }
  | { kind: 'prompt'; prompt: Prompt }
  | { kind: 'file'; path: string; name: string }

const menuKind = ref<'slash' | 'at' | null>(null)
const menuQuery = ref('')
const menuIndex = ref(0)

/** 检测输入末尾的触发符：/ 或 @ 后跟零或多个词字符 */
function detectMenu(text: string) {
  const slash = text.match(/(^|[\s\n])\/([\w-]*)$/)
  if (slash) {
    menuKind.value = 'slash'
    menuQuery.value = slash[2]
    menuIndex.value = 0
    return
  }
  const at = text.match(/(^|[\s\n])@([\w-]*)$/)
  if (at) {
    menuKind.value = 'at'
    menuQuery.value = at[2]
    menuIndex.value = 0
    return
  }
  menuKind.value = null
}

const menuItems = computed<MenuItem[]>(() => {
  const q = menuQuery.value.toLowerCase()
  if (menuKind.value === 'slash') {
    // 内置命令（§对齐 OpenCode）+ 用户 Snippets
    const builtins: MenuItem[] = (builtinCommandsRef.value?.commands ?? [])
      .filter((c) => c.title.toLowerCase().includes(q) || c.id.includes(q))
      .map((c) => ({ kind: 'builtin' as const, command: c }))
    const snips: MenuItem[] = prompts.snippets
      .filter(
        (p) => p.title.toLowerCase().includes(q) || p.content.toLowerCase().startsWith(q),
      )
      .slice(0, 8)
      .map((p) => ({ kind: 'snippet' as const, prompt: p }))
    return [...builtins, ...snips]
  }
  if (menuKind.value === 'at') {
    const ms: MenuItem[] = models.enabledModels
      .filter((m) => `${m.displayName} ${m.modelId}`.toLowerCase().includes(q))
      .slice(0, 6)
      .map((m) => ({ kind: 'model' as const, model: m }))
    const ps: MenuItem[] = prompts.prompts
      .filter((p) => p.title.toLowerCase().includes(q))
      .slice(0, 6)
      .map((p) => ({ kind: 'prompt' as const, prompt: p }))
    // 工作区文件（vibe coding：@路径 引用）
    const fs: MenuItem[] = workspaces.files
      .filter((f) => !f.isDir && (f.path.toLowerCase().includes(q) || f.name.toLowerCase().includes(q)))
      .slice(0, 6)
      .map((f) => ({ kind: 'file' as const, path: f.path, name: f.name }))
    return [...fs, ...ms, ...ps]
  }
  return []
})

/** 用选中项替换输入末尾的触发词（/xxx 或 @xxx / @路径） */
function selectItem(item: MenuItem) {
  if (item.kind === 'file') {
    // @文件：保留引用文本（发送时 expandFileRefs 解析注入内容）
    const re = /(^|[\s\n])@[\w./\\-]*$/
    draft.value = draft.value.replace(re, (_m, lead: string) => lead + '@' + item.path)
    menuKind.value = null
    textareaEl.value?.focus()
    return
  }
  const re = /(^|[\s\n])[@/][\w-]*$/
  if (item.kind === 'builtin') {
    // 清除 /xxx，执行内置命令
    draft.value = draft.value.replace(re, (_m, lead: string) => lead)
    menuKind.value = null
    void builtinCommandsRef.value?.runBuiltin(item.command.id)
    return
  }
  if (item.kind === 'snippet' || item.kind === 'prompt') {
    prompts.bumpUsage(item.prompt.id)
    draft.value = draft.value.replace(re, (_m, lead: string) => lead + item.prompt.content)
  } else {
    // @模型 = 切换当前会话模型
    models.remember(item.model.id)
    if (sessions.current) {
      void sessions.update(sessions.current.id, { modelId: item.model.id })
    }
    draft.value = draft.value.replace(re, (_m, lead: string) => lead)
  }
  menuKind.value = null
  textareaEl.value?.focus()
}

/** @ 菜单分组标题（文件区 / 模型区 / 提示词区） */
function groupLabel(item: MenuItem, idx: number): string | null {
  if (menuKind.value !== 'at') return null
  const prev = idx > 0 ? menuItems.value[idx - 1] : null
  if (item.kind === 'file' && prev?.kind !== 'file') return t('chat.mentionFiles')
  if (item.kind === 'model' && prev?.kind !== 'model') return t('chat.mentionModels')
  if (item.kind === 'prompt' && prev?.kind !== 'prompt') return t('chat.mentionPrompts')
  return null
}

function itemIcon(item: MenuItem) {
  if (item.kind === 'builtin') return Command
  if (item.kind === 'snippet') return Slash
  if (item.kind === 'model') return Boxes
  if (item.kind === 'file') return FileText
  return FileText
}

function onKeydown(e: KeyboardEvent) {
  // 菜单打开时优先处理导航键
  if (menuKind.value && menuItems.value.length > 0) {
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      menuIndex.value = (menuIndex.value + 1) % menuItems.value.length
      return
    }
    if (e.key === 'ArrowUp') {
      e.preventDefault()
      menuIndex.value =
        (menuIndex.value - 1 + menuItems.value.length) % menuItems.value.length
      return
    }
    if (e.key === 'Enter' || e.key === 'Tab') {
      e.preventDefault()
      selectItem(menuItems.value[menuIndex.value])
      return
    }
    if (e.key === 'Escape') {
      e.preventDefault()
      menuKind.value = null
      return
    }
  }
  // Enter 发送 / Shift+Enter 换行（§4.10）
  if (e.key === 'Enter' && !e.shiftKey) {
    e.preventDefault()
    void submit()
  }
}

// ---------- 输入框自动增高（最大 8 行，rAF 合并避免输入时抖动） ----------

const MAX_INPUT_ROWS = 8
/** 上一帧记录的输入框高度：只有真实变化才写入，避免反复重排导致晃动 */
let lastInputHeight = ''

function autoGrow() {
  const el = textareaEl.value
  if (!el) return
  el.style.height = 'auto'
  // 行高由 CSS leading-5 控制（20px）；max = 8 行
  const maxHeight = 20 * MAX_INPUT_ROWS
  const target = Math.min(el.scrollHeight, maxHeight) + 'px'
  if (target !== lastInputHeight) {
    lastInputHeight = target
    el.style.height = target
  }
}

let growRaf = 0
function scheduleAutoGrow() {
  cancelAnimationFrame(growRaf)
  growRaf = requestAnimationFrame(autoGrow)
}

function resetHeight() {
  const el = textareaEl.value
  if (!el) return
  el.style.height = 'auto'
  lastInputHeight = ''
}

watch(draft, (v) => {
  // rAF 合并：连打不重复强制同步布局
  scheduleAutoGrow()
  detectMenu(v)
})

// ---------- 滚动跟随 ----------

function scrollToBottom(force = false) {
  const el = scroller.value
  if (!el) return
  if (force || stickToBottom) {
    void nextTick(() => {
      el.scrollTop = el.scrollHeight
    })
  }
}

function onScroll() {
  const el = scroller.value
  if (!el) return
  stickToBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 80
}

watch(() => sessions.messages.length, () => scrollToBottom())
watch(() => sessions.currentId, () => {
  stickToBottom = true
  scrollToBottom(true)
})
watch(
  () => streaming.value?.content.length ?? 0,
  () => scrollToBottom(),
)

/** 搜索跳转：滚动到目标消息并闪烁提示（§4.5.3） */
watch(
  () => sessions.scrollToMessageId,
  async (mid) => {
    if (!mid) return
    await nextTick()
    // 等待 Markdown 渲染影响布局后再定位
    window.setTimeout(() => {
      const el = scroller.value?.querySelector(`[data-mid="${mid}"]`)
      if (el) {
        el.scrollIntoView({ behavior: 'smooth', block: 'center' })
        el.classList.remove('flash-highlight')
        // 强制 reflow 以重启动画
        void (el as HTMLElement).offsetWidth
        el.classList.add('flash-highlight')
      }
      sessions.scrollToMessageId = null
    }, 60)
  },
)

/** 从此处分支（§4.5.4）：复制截止该消息的上下文到新会话 */
async function onBranch(messageId: string) {
  const session = sessions.current
  if (!session) return
  try {
    await sessions.branch(
      session.id,
      messageId,
      t('sidebar.branchTitle', { title: session.title }),
    )
    toast.success(t('chat.branchDone'))
  } catch (e) {
    toast.error(toApiError(e).message)
  }
}

// ---------- 会话 System Prompt（§4.6 层级 2：会话级覆盖全局默认；无会话时存待用值） ----------

const sysPromptOpen = ref(false)
const sysPromptDraft = ref('')

function openSysPrompt() {
  const cur = sessions.current?.systemPrompt
  sysPromptDraft.value = cur ?? (pendingSysPrompt.value || settings.defaultSystemPrompt)
  sysPromptOpen.value = true
}

async function saveSysPrompt() {
  const val = sysPromptDraft.value.trim()
  const s = sessions.current
  if (!s) {
    pendingSysPrompt.value = val
    sysPromptOpen.value = false
    toast.success(t('chat.sysPromptSaved'))
    return
  }
  try {
    await sessions.update(s.id, { systemPrompt: val || null })
    sysPromptOpen.value = false
    toast.success(t('chat.sysPromptSaved'))
  } catch (e) {
    toast.error(toApiError(e).message)
  }
}

// ---------- 撤回 / 编辑重发（用户消息 hover 操作，§4.5.4） ----------

/** 撤回：删除该消息及其后的全部消息 */
async function onRetractMessage(messageId: string) {
  const s = sessions.current
  if (!s || sending.value) return
  try {
    await messagesApi.deleteFrom(s.id, messageId)
    await sessions.open(s.id)
    toast.success(t('chat.retractDone'))
  } catch (e) {
    toast.error(toApiError(e).message)
  }
}

/** 编辑重发：截断该消息及其后内容，以原文填入草稿重新走发送流程 */
async function onEditMessage(messageId: string) {
  const s = sessions.current
  if (!s || sending.value) return
  const content = sessions.messages.find((m) => m.id === messageId)?.content
  if (content === undefined) return
  try {
    await messagesApi.deleteFrom(s.id, messageId)
    await sessions.open(s.id)
    draft.value = content
    await submit()
  } catch (e) {
    toast.error(toApiError(e).message)
  }
}

/** 最后一条 assistant 消息 id：仅它显示「重新生成」 */
const lastAssistantId = computed(() => {
  for (let i = sessions.messages.length - 1; i >= 0; i--) {
    if (sessions.messages[i].role === 'assistant') return sessions.messages[i].id
  }
  return null
})

/**
 * 重新生成：删除该回答及之后内容，回退到其触发的用户消息原文重发。
 * 先校验模型可用再删库，避免删完发不出去导致消息丢失。
 */
async function onRegenerate(messageId: string) {
  const s = sessions.current
  if (!s || sending.value) return
  if (!models.get(s.modelId ?? '')) {
    toast.error(t('chat.noModel'))
    return
  }
  const idx = sessions.messages.findIndex((m) => m.id === messageId)
  if (idx < 0) return
  let userIdx = -1
  for (let i = idx - 1; i >= 0; i--) {
    if (sessions.messages[i].role === 'user') {
      userIdx = i
      break
    }
  }
  if (userIdx < 0) return
  const userMsg = sessions.messages[userIdx]
  if (!userMsg.content) return
  try {
    await messagesApi.deleteFrom(s.id, userMsg.id)
    await sessions.open(s.id)
    await send(userMsg.content)
  } catch (e) {
    toast.error(toApiError(e).message)
  }
}

// 思考过程过长自动收缩：超过阈值字符自动折叠一次，之后尊重用户手动展开/折叠
const REASONING_EXPAND_LIMIT = 500
const reasoningExpanded = ref(true)
let reasoningAutoCollapsed = false
watch(
  () => streaming.value?.reasoning,
  (r, old) => {
    if (!r) {
      reasoningAutoCollapsed = false
      reasoningExpanded.value = true
      return
    }
    if (old === undefined || old === '') {
      reasoningAutoCollapsed = false
      reasoningExpanded.value = true
    }
    if (!reasoningAutoCollapsed && r.length > REASONING_EXPAND_LIMIT) {
      reasoningExpanded.value = false
      reasoningAutoCollapsed = true
    }
  },
)

// 工具调用折叠：执行中自动展开，全部完成后自动收起；手动切换被尊重
const toolsBusy = computed(() =>
  toolCalls.value.some((tc) => tc.status === 'running' || tc.status === undefined),
)
const toolsExpanded = ref(true)
let prevBusy = false
watch(toolsBusy, (busy) => {
  if (busy) {
    // 新一轮执行开始：重新展开
    toolsExpanded.value = true
  } else if (prevBusy) {
    // 从运行中转为全部完成：自动收起一次
    toolsExpanded.value = false
  }
  prevBusy = busy
})
// 流式结束后重置（下一轮默认展开）
watch(sending, (s) => {
  if (!s) {
    toolsExpanded.value = true
    prevBusy = false
  }
})

// ---------- 上下文占用 / Token 用量（头部常驻，自侧边栏迁入） ----------

/** 上下文窗口来源：当前会话模型 > 最近使用模型（无会话时也可见，§4.2） */
const contextWindow = computed(() => {
  const mid = sessions.current?.modelId ?? models.preferredModelId
  return mid ? (models.get(mid)?.contextWindow ?? null) : null
})

const ctxUsed = computed(() => sessions.tokenStats.lastContext)

/** 占用百分比：按最近一轮请求近似（与 auto-compact 同口径，§4.6） */
const contextPct = computed(() => {
  if (!contextWindow.value || !ctxUsed.value) return null
  return Math.min(100, Math.round((ctxUsed.value / contextWindow.value) * 100))
})

const ctxBarClass = computed(() =>
  contextPct.value !== null && contextPct.value > 80 ? 'bg-destructive' : 'bg-primary',
)

/** token 数缩写：1234 → 1.2k */
function fmtTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`
  if (n >= 1000) return `${(n / 1000).toFixed(1)}k`
  return String(n)
}

const ctxTip = computed(() =>
  t('chat.ctxTip', {
    sum: fmtTokens(sessions.tokenStats.total),
    used: fmtTokens(ctxUsed.value),
    total: contextWindow.value ? fmtTokens(contextWindow.value) : '—',
    pct: contextPct.value ?? '—',
  }),
)

// ---------- 思考强度（§4.2）：无会话时存待用值，首条消息创建会话时落库 ----------

const thinkingLevel = computed<ThinkingLevel>(() => {
  const l = sessions.current?.thinkingLevel
  if (l === 'low' || l === 'medium' || l === 'high' || l === 'max') return l
  return pendingThinking.value
})

function setThinkingLevel(level: ThinkingLevel) {
  const s = sessions.current
  if (!s) {
    pendingThinking.value = level
    return
  }
  void sessions.update(s.id, { thinkingLevel: level }).catch((e) => {
    toast.error(toApiError(e).message)
  })
}

/** 最大档思考 → Codex 同款动画 + 呼吸光晕 */
const isMaxThinking = computed(() => thinkingLevel.value === 'max')

const showWelcome = computed(
  () => sessions.messages.length === 0 && !sending.value,
)
</script>

<template>
  <div class="flex h-full flex-col">
    <!-- 头部：会话标题 + 当前模型 -->
    <header class="flex h-12 shrink-0 items-center justify-between gap-4 border-b px-4">
      <h2 class="truncate text-sm font-medium">
        {{ sessions.current?.title ?? t('chat.newSession') }}
      </h2>
      <div class="flex items-center gap-1">
        <!-- 会话 System Prompt（无会话时编辑的是新会话待用值） -->
        <button
          class="flex size-8 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-secondary hover:text-foreground"
          :title="t('chat.sysPromptTitle')"
          @click="openSysPrompt"
        >
          <FileText class="size-3.5" />
        </button>
        <!-- 思考强度滑块（无会话时为新会话待用值） -->
        <ThinkingLevelSelector :level="thinkingLevel" @change="setThinkingLevel" />
        <!-- Token 用量 / 上下文占用：占用 >80% 红色预警（与 auto-compact 阈值联动） -->
        <div
          v-if="contextWindow || (sessions.current && sessions.tokenStats.total > 0)"
          class="flex items-center gap-1.5 rounded-md px-1.5 py-1 text-xs text-muted-foreground"
          :title="ctxTip"
        >
          <Gauge class="size-3.5 shrink-0" />
          <span v-if="contextWindow" class="font-mono">
            {{ fmtTokens(ctxUsed ?? 0) }}/{{ fmtTokens(contextWindow) }}
          </span>
          <span v-else class="font-mono">{{ fmtTokens(sessions.tokenStats.total) }}</span>
          <div
            v-if="contextPct !== null"
            class="h-1 w-10 shrink-0 overflow-hidden rounded-full bg-muted"
          >
            <div
              class="h-full rounded-full transition-all"
              :class="ctxBarClass"
              :style="{ width: `${contextPct}%` }"
            />
          </div>
        </div>
        <button
          class="flex max-w-56 items-center gap-1.5 rounded-md px-2 py-1 text-xs text-muted-foreground transition-colors hover:bg-secondary hover:text-foreground"
          @click="modelPickerOpen = true"
        >
          <Boxes class="size-3.5 shrink-0" />
          <span class="truncate">{{ currentModelLabel ?? t('chat.selectModel') }}</span>
          <ChevronDown class="size-3.5 shrink-0" />
        </button>
      </div>
    </header>

    <!-- 消息区 -->
    <div ref="scroller" class="scrollbar-thin flex-1 overflow-y-auto" @scroll="onScroll">
      <div class="mx-auto flex max-w-3xl flex-col gap-4 p-4">
        <!-- 空状态欢迎页（未选工作区时即为工作区门禁，对齐 dsh hero） -->
        <div v-if="showWelcome" class="relative flex flex-col items-center gap-3 py-16">
          <!-- 光晕背景（对齐 dsh HeroGlow） -->
          <div
            class="pointer-events-none absolute left-1/2 top-1/2 -z-10 h-72 w-[44rem] -translate-x-1/2 -translate-y-1/2 rounded-full bg-primary/10 blur-3xl"
            aria-hidden="true"
          />
          <img
            src="@/assets/icon.png"
            alt="Koid"
            class="size-14 rounded-2xl object-cover"
          />
          <h1 class="text-xl font-semibold">
            {{ needsWorkspace ? t('chat.workspaceGateTitle') : t('chat.empty') }}
          </h1>
          <p class="max-w-md text-center text-sm text-muted-foreground">
            {{ needsWorkspace ? t('chat.workspaceGateHint') : t('chat.emptyHint') }}
          </p>

          <!-- 工作区 chip：常驻 hero、始终可切换（对齐 dsh WorkspaceChip） -->
          <WorkspacePicker class="mt-2">
            <WorkspaceChip size="lg" />
          </WorkspacePicker>
        </div>

        <!-- 历史消息 -->
        <MessageBubble
          v-for="m in sessions.messages"
          :key="m.id"
          :message="m"
          :can-regenerate="m.id === lastAssistantId && !sending"
          @branch="(id: string) => void onBranch(id)"
          @preview="onPreview"
          @edit="(id: string) => void onEditMessage(id)"
          @retract="(id: string) => void onRetractMessage(id)"
          @regenerate="(id: string) => void onRegenerate(id)"
        />

        <!-- 工具调用（agent 循环：模型自主探索项目；执行中展开、完成后自动收起） -->
        <details
          v-if="sending && toolCalls.length && streamingSessionId === sessions.currentId"
          :open="toolsExpanded"
          class="group/tools rounded-lg border bg-muted/30"
          @toggle="toolsExpanded = ($event.target as HTMLDetailsElement).open"
        >
          <summary
            class="flex cursor-pointer select-none items-center gap-2 px-3 py-2 text-xs text-muted-foreground [&::-webkit-details-marker]:hidden"
          >
            <Wrench class="size-3.5 shrink-0" />
            <span class="font-medium">
              {{ t('chat.toolCalls', { n: toolCalls.length }) }}
            </span>
            <span class="text-[10px]">
              {{
                toolsBusy
                  ? t('chat.toolRunning')
                  : t('chat.toolsAllDone', {
                      done: toolCalls.filter((tc) => tc.status === 'done').length,
                      total: toolCalls.length,
                    })
              }}
            </span>
            <ChevronDown
              class="ml-auto size-3.5 shrink-0 transition-transform group-open/tools:rotate-180"
            />
          </summary>
          <div class="space-y-1.5 border-t p-2">
            <div
              v-for="(tc, i) in toolCalls"
              :key="i"
              class="flex flex-col rounded-md bg-background/60 px-2.5 py-1.5"
            >
              <div class="flex items-center gap-2 text-xs">
                <span class="font-mono font-medium">{{ tc.tool }}</span>
                <span class="truncate font-mono text-[10px] text-muted-foreground">
                  {{ tc.arguments }}
                </span>
                <span
                  class="ml-auto shrink-0 rounded-full px-2 py-0.5 text-[10px]"
                  :class="
                    tc.status === 'done'
                      ? 'bg-emerald-500/15 text-emerald-600'
                      : tc.status === 'error'
                        ? 'bg-destructive/15 text-destructive'
                        : 'bg-primary/10 text-primary'
                  "
                >
                  {{
                    tc.status === 'done'
                      ? t('chat.toolDone')
                      : tc.status === 'error'
                        ? t('chat.toolError')
                        : t('chat.toolRunning')
                  }}
                </span>
              </div>
              <pre
                v-if="tc.result"
                class="scrollbar-thin mt-1 max-h-32 overflow-auto whitespace-pre-wrap rounded bg-background/80 p-2 font-mono text-[11px] text-muted-foreground"
              >{{ tc.result }}</pre>
            </div>
          </div>
        </details>

        <!-- 流式消息（未落库）；最大档思考时外层带呼吸光晕 -->
        <div
          v-if="sending && streaming && streamingSessionId === sessions.currentId"
          class="flex flex-col"
          :class="{ 'aurora-wrap': isMaxThinking }"
        >
          <div class="max-w-full rounded-2xl bg-muted px-4 py-3">
            <details
              v-if="streaming.reasoning"
              :open="reasoningExpanded"
              class="mb-2"
              @toggle="reasoningExpanded = ($event.target as HTMLDetailsElement).open"
            >
              <summary
                class="flex w-fit cursor-pointer select-none items-center gap-1.5 text-xs text-muted-foreground"
              >
                <template v-if="isMaxThinking">
                  <MaxThinking :seconds="thinkingSeconds" />
                </template>
                <template v-else>
                  <Brain class="size-3.5 animate-spin" />
                  {{
                    thinkingSeconds > 0
                      ? t('chat.message.thoughtFor', { n: thinkingSeconds })
                      : t('chat.message.thinking')
                  }}
                </template>
              </summary>
              <div
                class="mt-2 whitespace-pre-wrap border-l-2 border-border pl-3 text-sm italic text-muted-foreground"
              >
                {{ streaming.reasoning }}
              </div>
            </details>
            <MarkdownView :content="streaming.content" :streaming="true" />
            <span class="streaming-cursor" />
          </div>
        </div>
      </div>
    </div>

    <!-- 输入区 -->
    <div class="mx-auto w-full max-w-3xl shrink-0 p-4 pt-2">
      <div class="glass relative rounded-xl p-2">
        <!-- 触发菜单（/ Snippets、@ 提及，§4.10） -->
        <div
          v-if="menuKind"
          class="glass absolute bottom-full left-0 right-0 mb-2 max-h-64 overflow-y-auto rounded-lg border p-1 shadow-lg"
        >
          <p
            v-if="menuItems.length === 0"
            class="px-3 py-2 text-xs text-muted-foreground"
          >
            {{ menuKind === 'slash' ? t('chat.slashEmpty') : t('sidebar.noResults') }}
          </p>
          <template v-for="(item, idx) in menuItems" :key="idx">
            <p
              v-if="groupLabel(item, idx)"
              class="px-2 pb-0.5 pt-1.5 text-[10px] font-medium uppercase text-muted-foreground"
            >
              {{ groupLabel(item, idx) }}
            </p>
            <button
              class="flex w-full items-start gap-2 rounded-md px-2 py-1.5 text-left text-sm transition-colors"
              :class="cn(idx === menuIndex ? 'bg-secondary' : 'hover:bg-secondary/60')"
              @mousedown.prevent="selectItem(item)"
              @mouseenter="menuIndex = idx"
            >
              <component
                :is="itemIcon(item)"
                class="mt-0.5 size-3.5 shrink-0 text-muted-foreground"
              />
              <span class="min-w-0 flex-1">
                <span class="block truncate">
                  {{
                    item.kind === 'model'
                      ? item.model.displayName
                      : item.kind === 'builtin'
                        ? `/${item.command.id}`
                        : item.kind === 'file'
                          ? item.name
                          : item.prompt.title
                  }}
                </span>
                <span class="block truncate font-mono text-[10px] text-muted-foreground">
                  {{
                    item.kind === 'model'
                      ? item.model.modelId
                      : item.kind === 'builtin'
                        ? item.command.desc
                        : item.kind === 'file'
                          ? item.path
                          : item.prompt.content.split('\n')[0].slice(0, 80)
                  }}
                </span>
              </span>
            </button>
          </template>
        </div>

        <textarea
          ref="textareaEl"
          v-model="draft"
          :readonly="needsWorkspace"
          :placeholder="
            needsWorkspace ? t('chat.placeholderWorkspace') : t('chat.placeholder')
          "
          rows="1"
          class="min-h-[1.75rem] w-full resize-none rounded-md bg-transparent px-2 py-1.5 text-sm leading-5 outline-none placeholder:text-muted-foreground"
          @keydown="onKeydown"
          @click="onComposerClick"
          @focus="onComposerClick"
        />
        <div class="flex items-center justify-between gap-2 p-1">
          <div class="flex min-w-0 items-center gap-2">
            <!-- 工作区 chip（工作区切换常驻入口） -->
            <WorkspacePicker v-model:open="workspacePickerOpen">
              <WorkspaceChip size="sm" />
            </WorkspacePicker>

            <!-- 底部模型切换 chip（§4.2：跨供应商切换） -->
            <button
              class="flex min-w-0 items-center gap-1.5 rounded-lg border border-border/60 px-2.5 py-1.5 text-xs text-muted-foreground transition-colors hover:bg-secondary hover:text-foreground"
              :title="t('chat.selectModel')"
              @click="modelPickerOpen = true"
            >
              <Boxes class="size-3.5 shrink-0" />
              <span class="truncate">{{ currentModelLabel ?? t('chat.selectModel') }}</span>
              <ChevronDown class="size-3.5 shrink-0" />
            </button>
          </div>

          <div class="flex items-center gap-2">
            <Button v-if="sending" size="icon" variant="outline" class="rounded-lg" @click="stop">
              <SquareStop class="size-4" />
            </Button>
            <Button
              v-else
              size="icon"
              class="rounded-lg"
              :disabled="
                needsWorkspace || !draft.trim() || models.enabledModels.length === 0
              "
              @click="() => void submit()"
            >
              <ArrowUp class="size-4" />
            </Button>
          </div>
        </div>
      </div>
    </div>

    <!-- Artifacts 预览弹窗 -->
    <ArtifactsPreview
      v-model:open="artifactOpen"
      :code="artifactCode"
      :lang="artifactLang"
    />

    <!-- 模型选择器（底部 chip / /model / 头部共用） -->
    <ModelPicker v-model:open="modelPickerOpen" />

    <!-- 内置命令（/init /review /help 对话框） -->
    <BuiltinCommands
      ref="builtinCommandsRef"
      v-model:active="cmdActive"
      :send-text="send"
      @model-pick="modelPickerOpen = true"
    />

    <!-- 会话 System Prompt 编辑（头部 FileText 入口） -->
    <Dialog v-model:open="sysPromptOpen">
      <DialogContent class="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{{ t('chat.sysPromptTitle') }}</DialogTitle>
          <DialogDescription>{{ t('chat.sysPromptHint') }}</DialogDescription>
        </DialogHeader>
        <textarea
          v-model="sysPromptDraft"
          rows="8"
          class="scrollbar-thin w-full resize-y rounded-md border border-input bg-background/60 p-3 text-sm outline-none focus:ring-1 focus:ring-ring"
        />
        <DialogFooter>
          <Button variant="outline" @click="sysPromptOpen = false">
            {{ t('common.cancel') }}
          </Button>
          <Button @click="() => void saveSysPrompt()">
            {{ t('common.save') }}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  </div>
</template>

<style scoped>
.streaming-cursor {
  display: inline-block;
  width: 0.5rem;
  height: 1rem;
  margin-left: 2px;
  vertical-align: text-bottom;
  border-radius: 1px;
  background: hsl(var(--primary));
  /* ease-out 闪烁被禁止（§二），用透明度呼吸动画替代 */
  animation: koid-cursor 1.2s ease-in-out infinite;
}

@keyframes koid-cursor {
  0%,
  100% {
    opacity: 1;
  }
  50% {
    opacity: 0.25;
  }
}

/* 最大档思考：气泡呼吸光晕（透明度/阴影呼吸，非闪烁） */
.aurora-wrap > div {
  animation: koid-think-glow 3.2s ease-in-out infinite;
}

@keyframes koid-think-glow {
  0%,
  100% {
    box-shadow:
      0 0 0 1px hsl(var(--primary) / 14%),
      0 0 28px -10px hsl(var(--primary) / 35%);
  }
  50% {
    box-shadow:
      0 0 0 1px hsl(var(--primary) / 28%),
      0 0 42px -8px hsl(var(--primary) / 55%);
  }
}

@media (prefers-reduced-motion: reduce) {
  .aurora-wrap > div {
    animation: none;
  }
}
</style>
