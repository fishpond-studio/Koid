<script setup lang="ts">
import { ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue-sonner'
import { RotateCcw } from 'lucide-vue-next'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { usePromptStore } from '@/stores/prompts'
import { toApiError } from '@/lib/api'
import { diffLines, type DiffLine } from '@/lib/diff'
import type { Prompt, PromptVersion } from '@/types'

/** 版本历史对话框（§4.6）：查看 Diff + 一键还原 */
const props = defineProps<{ open: boolean; prompt: Prompt | null }>()
const emit = defineEmits<{ 'update:open': [v: boolean] }>()

const { t } = useI18n()
const store = usePromptStore()

const versions = ref<PromptVersion[]>([])
const selected = ref<PromptVersion | null>(null)
const restoring = ref(false)

watch(
  () => props.open,
  async (open) => {
    if (!open || !props.prompt) return
    selected.value = null
    try {
      versions.value = await store.versions(props.prompt.id)
    } catch (e) {
      toast.error(toApiError(e).message)
    }
  },
)

/** Diff 方向：历史版本（旧）→ 当前内容（新） */
function diffOf(v: PromptVersion): DiffLine[] {
  if (!props.prompt) return []
  return diffLines(v.content, props.prompt.content)
}

function fmtTime(ts: number): string {
  const d = new Date(ts)
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`
}

async function restore(v: PromptVersion) {
  if (!props.prompt || restoring.value) return
  restoring.value = true
  try {
    // 还原 = 以旧内容覆盖；Rust 侧会先快照当前内容为新版本（可再还原回去）
    await store.save({
      id: props.prompt.id,
      title: props.prompt.title,
      content: v.content,
      type: props.prompt.type,
      tags: props.prompt.tags,
    })
    toast.success(t('settings.prompts.restoreDone'))
    emit('update:open', false)
  } catch (e) {
    toast.error(toApiError(e).message)
  } finally {
    restoring.value = false
  }
}
</script>

<template>
  <Dialog :open="open" @update:open="(v: boolean) => emit('update:open', v)">
    <DialogContent class="max-h-[80vh] overflow-hidden sm:max-w-2xl">
      <DialogHeader>
        <DialogTitle>
          {{ t('settings.prompts.history') }}
          <span v-if="prompt" class="ml-2 text-sm font-normal text-muted-foreground">
            · {{ prompt.title }}
          </span>
        </DialogTitle>
      </DialogHeader>

      <div class="scrollbar-thin max-h-[60vh] space-y-2 overflow-y-auto py-2">
        <p v-if="versions.length === 0" class="py-10 text-center text-sm text-muted-foreground">
          {{ t('settings.prompts.noVersions') }}
        </p>

        <div v-for="v in versions" :key="v.id" class="rounded-lg border">
          <div class="flex items-center gap-2 px-3 py-2">
            <span class="font-mono text-xs text-muted-foreground">{{ fmtTime(v.createdAt) }}</span>
            <span class="min-w-0 flex-1 truncate text-xs text-muted-foreground">
              {{ v.content.split('\n')[0].slice(0, 60) }}
            </span>
            <Button
              variant="ghost"
              size="sm"
              class="h-6 text-xs"
              @click="selected = selected?.id === v.id ? null : v"
            >
              {{ selected?.id === v.id ? t('common.collapse') : t('settings.prompts.viewDiff') }}
            </Button>
            <Button
              variant="outline"
              size="sm"
              class="h-6 gap-1 text-xs"
              :disabled="restoring"
              @click="() => void restore(v)"
            >
              <RotateCcw class="size-3" />
              {{ t('settings.prompts.restore') }}
            </Button>
          </div>

          <!-- Diff 面板：红 = 该版本有而当前没有，绿 = 当前新增 -->
          <div v-if="selected?.id === v.id" class="border-t bg-muted/30 p-3">
            <pre class="max-h-64 overflow-auto font-mono text-xs leading-5"><template v-for="(line, i) in diffOf(v)" :key="i"><span
              class="block px-1"
              :class="{
                'bg-emerald-500/15 text-emerald-700 dark:text-emerald-400': line.type === 'added',
                'bg-red-500/15 text-red-600 line-through dark:text-red-400': line.type === 'removed',
              }"
            >{{ line.text || ' ' }}</span></template></pre>
          </div>
        </div>
      </div>
    </DialogContent>
  </Dialog>
</template>
