<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRouter } from 'vue-router'
import { toast } from 'vue-sonner'
import { Command, FileCode2, HelpCircle, Loader2, Send } from 'lucide-vue-next'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Label } from '@/components/ui/label'
import { chatApi, messagesApi, toApiError } from '@/lib/api'
import { useModelStore } from '@/stores/model'
import { useSessionStore } from '@/stores/session'
import { useCompact } from '@/composables/useCompact'

/**
 * 内置命令（对齐 OpenCode：/model /init /review /undo /compact /mcp /help）
 * - /model  切换模型（emit model-pick → ChatView 打开 ModelPicker）
 * - /init   基于描述生成并应用会话 System Prompt
 * - /review 对粘贴代码发起审查（复用会话内对话，send 由 ChatView 注入）
 * - /undo   撤销最近一条用户消息及其回复
 * - /compact 压缩早期对话为摘要，保留最近一轮
 * - /mcp    跳转 MCP 设置
 * - /help   命令说明
 */
const props = defineProps<{
  active: 'init' | 'review' | 'help' | null
  /** ChatView 的 useChat.send，保证流式渲染在会话内可见 */
  sendText: (text: string) => Promise<void>
}>()
const emit = defineEmits<{
  'update:active': [v: 'init' | 'review' | 'help' | null]
  'model-pick': []
}>()

const { t } = useI18n()
const router = useRouter()
const sessions = useSessionStore()
const models = useModelStore()
const { compactSession } = useCompact()

// ---------- /init ----------
const initDesc = ref('')
const initBusy = ref(false)

watch(
  () => props.active,
  (a) => {
    if (a === 'init') {
      initDesc.value = sessions.current?.systemPrompt ?? ''
      initBusy.value = false
    }
  },
)

async function generateInit() {
  const desc = initDesc.value.trim()
  if (!desc || initBusy.value) return
  const model = models.enabledModels[0]
  if (!model) {
    toast.error(t('chat.noModel'))
    return
  }
  initBusy.value = true
  try {
    const resp = await chatApi.chat({
      requestId: `init-${Date.now()}`,
      providerId: model.providerId,
      modelId: model.modelId,
      messages: [
        {
          role: 'user',
          content: `请基于下面的项目/任务描述，生成一段简洁的 System Prompt（中文，20 行以内），说明 AI 助手应遵循的角色、约束与工作方式：\n\n${desc}`,
        },
      ],
      system: null,
      stream: false,
    })
    initDesc.value = resp.content.trim()
  } catch (e) {
    toast.error(toApiError(e).message)
  } finally {
    initBusy.value = false
  }
}

async function applyInit() {
  if (!sessions.current) {
    toast.error(t('chat.noSession'))
    return
  }
  try {
    await sessions.update(sessions.current.id, { systemPrompt: initDesc.value.trim() || null })
    toast.success(t('chat.initDone'))
    emit('update:active', null)
  } catch (e) {
    toast.error(toApiError(e).message)
  }
}

// ---------- /review ----------
const reviewCode = ref('')
const reviewBusy = ref(false)

async function doReview() {
  const code = reviewCode.value.trim()
  if (!code || reviewBusy.value) return
  if (!sessions.current) {
    toast.error(t('chat.noSession'))
    return
  }
  reviewBusy.value = true
  try {
    await props.sendText(
      `请审查以下代码，分三部分输出：\n1) 潜在 Bug 与边界问题\n2) 安全隐患\n3) 风格与最佳实践建议\n\n\`\`\`\n${code}\n\`\`\``,
    )
    reviewCode.value = ''
    emit('update:active', null)
  } catch (e) {
    toast.error(toApiError(e).message)
  } finally {
    reviewBusy.value = false
  }
}

function pickReviewFile() {
  const input = document.createElement('input')
  input.type = 'file'
  input.accept = '*'
  input.onchange = async () => {
    const file = input.files?.[0]
    if (!file) return
    if (file.size > 1024 * 1024) {
      toast.error(t('skills.fileTooLarge'))
      return
    }
    reviewCode.value = await file.text()
  }
  input.click()
}

// ---------- /undo ----------
async function doUndo() {
  const session = sessions.current
  if (!session || sessions.messages.length === 0) {
    toast.info(t('chat.undoNothing'))
    return
  }
  const last = sessions.messages[sessions.messages.length - 1]
  const toRemove: string[] = [last.id]
  if (last.role === 'assistant' && sessions.messages.length > 1) {
    const prev = sessions.messages[sessions.messages.length - 2]
    if (prev.role === 'user') toRemove.push(prev.id)
  }
  try {
    for (const id of toRemove) await messagesApi.remove(id)
    await sessions.open(session.id)
    toast.success(t('chat.undoDone'))
  } catch (e) {
    toast.error(toApiError(e).message)
  }
}

// ---------- /compact ----------
async function doCompact() {
  const session = sessions.current
  if (!session || sessions.messages.length < 4) {
    toast.info(t('chat.compactTooShort'))
    return
  }
  const ok = await compactSession()
  if (ok) toast.success(t('chat.compactDone'))
}

// ---------- /mcp ----------
function gotoMcp() {
  void router.push({ path: '/settings', query: { section: 'mcp' } })
}

// ---------- 命令注册表 ----------

export interface BuiltinCommandDef {
  id: string
  title: string
  desc: string
}

const commands = computed<BuiltinCommandDef[]>(() => [
  { id: 'model', title: t('chat.cmd.model.title'), desc: t('chat.cmd.model.desc') },
  { id: 'init', title: t('chat.cmd.init.title'), desc: t('chat.cmd.init.desc') },
  { id: 'review', title: t('chat.cmd.review.title'), desc: t('chat.cmd.review.desc') },
  { id: 'undo', title: t('chat.cmd.undo.title'), desc: t('chat.cmd.undo.desc') },
  { id: 'compact', title: t('chat.cmd.compact.title'), desc: t('chat.cmd.compact.desc') },
  { id: 'mcp', title: t('chat.cmd.mcp.title'), desc: t('chat.cmd.mcp.desc') },
  { id: 'help', title: t('chat.cmd.help.title'), desc: t('chat.cmd.help.desc') },
])

/** ChatView 调用入口：执行指定命令 */
async function runBuiltin(id: string) {
  switch (id) {
    case 'model':
      emit('model-pick')
      break
    case 'init':
      emit('update:active', 'init')
      break
    case 'review':
      emit('update:active', 'review')
      break
    case 'undo':
      await doUndo()
      break
    case 'compact':
      await doCompact()
      break
    case 'mcp':
      gotoMcp()
      break
    case 'help':
      emit('update:active', 'help')
      break
  }
}

defineExpose({ commands, runBuiltin })
</script>

<template>
  <!-- /init 对话框 -->
  <Dialog :open="active === 'init'" @update:open="(v: boolean) => v || emit('update:active', null)">
    <DialogContent class="sm:max-w-lg">
      <DialogHeader>
        <DialogTitle>{{ t('chat.cmd.init.title') }}</DialogTitle>
      </DialogHeader>
      <div class="space-y-3 py-2">
        <div class="space-y-1.5">
          <Label>{{ t('chat.cmd.init.desc') }}</Label>
          <textarea
            v-model="initDesc"
            rows="6"
            class="w-full resize-none rounded-md border border-input bg-background px-3 py-2 font-mono text-xs outline-none focus:ring-2 focus:ring-ring"
            :placeholder="t('chat.cmd.init.placeholder')"
          />
        </div>
      </div>
      <DialogFooter>
        <Button
          variant="outline"
          :disabled="initBusy || !initDesc.trim()"
          @click="() => void generateInit()"
        >
          <Loader2 v-if="initBusy" class="mr-1 size-3.5 animate-spin" />
          {{ t('chat.cmd.init.generate') }}
        </Button>
        <Button variant="outline" @click="emit('update:active', null)">
          {{ t('common.cancel') }}
        </Button>
        <Button @click="() => void applyInit()">{{ t('chat.cmd.init.apply') }}</Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>

  <!-- /review 对话框 -->
  <Dialog :open="active === 'review'" @update:open="(v: boolean) => v || emit('update:active', null)">
    <DialogContent class="sm:max-w-lg">
      <DialogHeader>
        <DialogTitle>{{ t('chat.cmd.review.title') }}</DialogTitle>
      </DialogHeader>
      <div class="space-y-2 py-2">
        <textarea
          v-model="reviewCode"
          rows="8"
          class="w-full resize-none rounded-md border border-input bg-background px-3 py-2 font-mono text-xs outline-none focus:ring-2 focus:ring-ring"
          :placeholder="t('chat.cmd.review.placeholder')"
        />
        <div class="flex gap-2">
          <Button variant="outline" size="sm" class="gap-1" @click="pickReviewFile">
            <FileCode2 class="size-3.5" />
            {{ t('skills.chooseFile') }}
          </Button>
        </div>
      </div>
      <DialogFooter>
        <Button variant="outline" @click="emit('update:active', null)">
          {{ t('common.cancel') }}
        </Button>
        <Button :disabled="!reviewCode.trim() || reviewBusy" @click="() => void doReview()">
          <Send v-if="!reviewBusy" class="mr-1 size-3.5" />
          {{ t('chat.cmd.review.send') }}
        </Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>

  <!-- /help 对话框 -->
  <Dialog :open="active === 'help'" @update:open="(v: boolean) => v || emit('update:active', null)">
    <DialogContent class="sm:max-w-md">
      <DialogHeader>
        <DialogTitle class="flex items-center gap-1.5">
          <HelpCircle class="size-4" />
          {{ t('chat.cmd.help.title') }}
        </DialogTitle>
      </DialogHeader>
      <div class="space-y-2 py-1">
        <button
          v-for="c in commands"
          :key="c.id"
          class="flex w-full items-start gap-3 rounded-md px-2 py-2 text-left hover:bg-secondary/60"
          @click="c.id !== 'help' && runBuiltin(c.id)"
        >
          <Command class="mt-0.5 size-4 shrink-0 text-muted-foreground" />
          <span class="min-w-0 flex-1">
            <span class="block font-mono text-xs font-medium">/{{ c.id }}</span>
            <span class="block text-xs text-muted-foreground">{{ c.desc }}</span>
          </span>
        </button>
      </div>
    </DialogContent>
  </Dialog>
</template>
