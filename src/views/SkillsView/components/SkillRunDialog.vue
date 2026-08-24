<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { toast } from 'vue-sonner'
import { Brain, Check, FileUp, Send, Square, X } from 'lucide-vue-next'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import MarkdownView from '@/components/markdown/MarkdownView.vue'
import { skillsApi } from '@/lib/api'
import type { SkillDef, SkillEvent, SkillEventKind } from '@/types'

/**
 * Skill 运行对话框（§4.7）：
 * 1. 变量收集：启动入参（selection/file/clipboard）
 * 2. 执行日志：监听 skill:event 渲染步骤时间线
 * 3. input 步骤：内联表单提交（skill_respond）
 */

interface LogEntry {
  kind: SkillEventKind
  stepId: string | null
  content: string | null
  label: string | null
  error: string | null
  progress: number | null
  ts: number
}

const props = defineProps<{ open: boolean; skill: SkillDef | null }>()
const emit = defineEmits<{ 'update:open': [v: boolean] }>()

const { t } = useI18n()

// ---------- 变量收集 ----------

/** 收集启动入参：{{name}} 且不含 .output 后缀的 token */
function collectPreVars(skill: SkillDef): string[] {
  const text = skill.steps
    .flatMap((s) => [s.prompt, s.content, s.condition])
    .filter((x): x is string => !!x)
    .join('\n')
  const re = /\{\{\s*([A-Za-z0-9_-]+)\s*\}\}/g
  const out = new Set<string>()
  let m: RegExpExecArray | null
  while ((m = re.exec(text))) {
    if (!m[1].includes('.')) out.add(m[1])
  }
  return [...out]
}

const preVars = ref<string[]>([])
const varValues = ref<Record<string, string>>({})

/** 文件读取：<input type="file"> 原生对话框 + JS 读取（§7.3 超 1MB 拒绝） */
async function pickFile(varName: string) {
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
    varValues.value[varName] = await file.text()
  }
  input.click()
}

async function readClipboard(varName: string) {
  try {
    varValues.value[varName] = await navigator.clipboard.readText()
  } catch {
    toast.error(t('skills.clipboardDenied'))
  }
}

// ---------- 执行 ----------

const phase = ref<'setup' | 'running' | 'done' | 'error'>('setup')
const log = ref<LogEntry[]>([])
const pendingInput = ref<{ stepId: string; label: string } | null>(null)
const inputValue = ref('')
let requestId = ''
let unlisten: UnlistenFn | null = null

watch(
  () => props.open,
  (open) => {
    if (!open) return
    if (props.skill) {
      preVars.value = collectPreVars(props.skill)
      varValues.value = {}
      for (const v of preVars.value) varValues.value[v] = ''
    }
    phase.value = 'setup'
    log.value = []
    pendingInput.value = null
  },
)

async function start() {
  if (!props.skill) return
  requestId = `skill-${Date.now()}-${Math.random().toString(16).slice(2)}`
  phase.value = 'running'

  unlisten = await listen<SkillEvent>('skill:event', (event) => {
    const ev = event.payload
    if (ev.requestId !== requestId) return
    appendLog(ev)
  })

  try {
    await skillsApi.run(requestId, props.skill.id, varValues.value)
  } catch (e) {
    phase.value = 'error'
    toast.error(String(e))
  }
}

function appendLog(ev: SkillEvent) {
  const entry: LogEntry = {
    kind: ev.kind,
    stepId: ev.stepId ?? null,
    content: ev.content ?? null,
    label: ev.label ?? null,
    error: ev.error ?? null,
    progress: ev.progress ?? null,
    ts: Date.now(),
  }
  log.value.push(entry)

  if (ev.kind === 'input-required') {
    pendingInput.value = { stepId: ev.stepId ?? '', label: ev.label ?? '' }
    inputValue.value = ''
  } else if (ev.kind === 'message') {
    pendingInput.value = null
    phase.value = 'done'
  } else if (ev.kind === 'done') {
    pendingInput.value = null
    phase.value = 'done'
  } else if (ev.kind === 'error') {
    pendingInput.value = null
    phase.value = 'error'
  } else if (ev.kind === 'cancelled') {
    pendingInput.value = null
    phase.value = 'error'
  }
}

async function submitInput() {
  if (!pendingInput.value) return
  const ok = await skillsApi.respond(requestId, inputValue.value)
  if (ok) {
    pendingInput.value = null
    inputValue.value = ''
  }
}

async function cancel() {
  await skillsApi.cancel(requestId)
}

onBeforeUnmount(() => {
  unlisten?.()
})

const running = computed(() => phase.value === 'running')

// ---------- 渲染辅助 ----------

function fmtTime(ts: number): string {
  return new Date(ts).toLocaleTimeString('zh-CN', { hour12: false })
}

// 模板内不能直接写字面 {{，用函数包裹展示变量标记
function fmtVar(v: string): string {
  return '{{' + v + '}}'
}
</script>

<template>
  <Dialog
    :open="open"
    @update:open="(v: boolean) => emit('update:open', v)"
  >
    <DialogContent class="max-h-[80vh] flex max-w-2xl flex-col overflow-hidden">
      <DialogHeader>
        <DialogTitle>
          {{ skill?.name }}
          <span v-if="skill" class="ml-2 font-mono text-xs font-normal text-muted-foreground">
            {{ skill.id }}
          </span>
        </DialogTitle>
      </DialogHeader>

      <!-- 变量收集阶段 -->
      <div v-if="phase === 'setup'" class="min-h-0 flex-1 space-y-4 overflow-y-auto py-2">
        <p v-if="preVars.length === 0" class="text-sm text-muted-foreground">
          {{ t('skills.noVarsNeeded') }}
        </p>
        <div v-for="v in preVars" :key="v" class="space-y-1.5">
          <Label class="font-mono text-xs">{{ fmtVar(v) }}</Label>
          <div class="flex items-center gap-2">
            <Input
              v-model="varValues[v]"
              :placeholder="t('skills.varPlaceholder')"
              class="flex-1 font-mono text-xs"
            />
            <Button
              v-if="v === 'file'"
              variant="outline"
              size="sm"
              class="h-8 gap-1 text-xs"
              @click="() => void pickFile(v)"
            >
              <FileUp class="size-3.5" />
              {{ t('skills.chooseFile') }}
            </Button>
            <Button
              v-if="v === 'clipboard'"
              variant="outline"
              size="sm"
              class="h-8 gap-1 text-xs"
              @click="() => void readClipboard(v)"
            >
              <Check class="size-3.5" />
              {{ t('skills.pasteClipboard') }}
            </Button>
          </div>
        </div>

        <DialogFooter>
          <Button variant="outline" @click="emit('update:open', false)">
            {{ t('common.cancel') }}
          </Button>
          <Button class="gap-1" @click="() => void start()">
            <Send class="size-3.5" />
            {{ t('skills.run') }}
          </Button>
        </DialogFooter>
      </div>

      <!-- 执行日志阶段 -->
      <div v-else class="scrollbar-thin min-h-0 flex-1 space-y-3 overflow-y-auto py-2 pr-1">
        <!-- 进行中的 input 请求 -->
        <div v-if="pendingInput" class="rounded-lg border border-primary/40 bg-primary/5 p-3">
          <p class="mb-2 text-sm font-medium">
            {{ pendingInput.label || t('skills.needInput') }}
          </p>
          <div class="flex items-center gap-2">
            <textarea
              v-model="inputValue"
              rows="3"
              class="min-w-0 flex-1 resize-y rounded-md border border-input bg-background px-3 py-2 font-mono text-xs outline-none focus:ring-1 focus:ring-ring"
            />
            <Button size="sm" class="gap-1" @click="() => void submitInput()">
              <Send class="size-3.5" />
              {{ t('chat.send') }}
            </Button>
          </div>
        </div>

        <template v-for="(entry, idx) in log" :key="idx">
          <!-- 步骤切换 -->
          <div
            v-if="entry.kind === 'step'"
            class="flex items-center gap-2 pt-1 text-xs text-muted-foreground"
          >
            <span class="size-1.5 rounded-full bg-primary/50" />
            <span class="font-mono">{{ entry.stepId }}</span>
            <span class="text-[10px]">{{ fmtTime(entry.ts) }}</span>
          </div>

          <!-- llm 输出 -->
          <div
            v-else-if="entry.kind === 'output'"
            class="ml-3 rounded-lg border bg-muted/30 px-3 py-2"
          >
            <div class="mb-1 text-[10px] font-medium uppercase text-muted-foreground">
              {{ entry.stepId }} · output
            </div>
            <MarkdownView :content="entry.content ?? ''" />
          </div>

          <!-- message 结果 -->
          <div v-else-if="entry.kind === 'message'" class="rounded-lg border px-3 py-2">
            <MarkdownView :content="entry.content ?? ''" />
          </div>

          <!-- 错误 -->
          <div
            v-else-if="entry.kind === 'error'"
            class="flex items-center gap-2 rounded-lg border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive"
          >
            <X class="size-4 shrink-0" />
            {{ entry.error || t('skills.runFailed') }}
          </div>

          <!-- 取消 -->
          <div
            v-else-if="entry.kind === 'cancelled'"
            class="text-sm text-muted-foreground"
          >
            {{ t('skills.cancelled') }}
          </div>
        </template>

        <!-- 思考中 -->
        <div
          v-if="running"
          class="flex items-center gap-2 pt-2 text-xs text-muted-foreground"
        >
          <Brain class="size-3.5 animate-pulse" />
          {{ t('skills.executing') }}
        </div>
      </div>

      <!-- 底部：取消/完成 -->
      <DialogFooter v-if="phase !== 'setup'" class="border-t pt-3">
        <Button v-if="running" variant="outline" size="sm" class="gap-1" @click="() => void cancel()">
          <Square class="size-3.5" />
          {{ t('chat.stop') }}
        </Button>
        <Button v-else size="sm" @click="emit('update:open', false)">
          <Check class="mr-1 size-3.5" />
          {{ t('common.confirm') }}
        </Button>
        <span v-if="phase === 'done'" class="mr-auto flex items-center gap-1 text-xs text-emerald-600">
          <Check class="size-3.5" />
          {{ t('skills.done') }}
        </span>
        <span v-else-if="phase === 'error'" class="mr-auto flex items-center gap-1 text-xs text-destructive">
          <X class="size-3.5" />
          {{ t('skills.failed') }}
        </span>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>
