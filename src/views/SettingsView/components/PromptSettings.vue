<script setup lang="ts">
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue-sonner'
import { History, Pencil, Pin, Plus, Trash2 } from 'lucide-vue-next'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import PromptDialog from './PromptDialog.vue'
import VersionsDialog from './VersionsDialog.vue'
import { usePromptStore } from '@/stores/prompts'
import { useSessionStore } from '@/stores/session'
import { toApiError } from '@/lib/api'
import { cn } from '@/lib/utils'
import type { Prompt, PromptType } from '@/types'

const { t } = useI18n()
const prompts = usePromptStore()
const sessions = useSessionStore()

type Filter = 'all' | PromptType
const filter = ref<Filter>('all')
const filters: Filter[] = ['all', 'template', 'snippet', 'system']

const dialogOpen = ref(false)
const editingPrompt = ref<Prompt | null>(null)
const versionsOpen = ref(false)
const versionsPrompt = ref<Prompt | null>(null)

const list = computed(() =>
  filter.value === 'all'
    ? prompts.prompts
    : prompts.prompts.filter((p) => p.type === filter.value),
)

function openAdd() {
  editingPrompt.value = null
  dialogOpen.value = true
}

// 模板内不能直接写字面 {{，用函数包裹展示变量标记
function fmtVar(v: string): string {
  return '{{' + v + '}}'
}

function openEdit(p: Prompt) {
  editingPrompt.value = p
  dialogOpen.value = true
}

function openVersions(p: Prompt) {
  versionsPrompt.value = p
  versionsOpen.value = true
}

async function remove(p: Prompt) {
  if (!window.confirm(t('settings.prompts.deleteConfirm', { name: p.title }))) return
  try {
    await prompts.remove(p.id)
    toast.success(t('common.deleted'))
  } catch (e) {
    toast.error(toApiError(e).message)
  }
}

/** 一键设为当前会话 System Prompt（§4.6） */
async function setAsSystemPrompt(p: Prompt) {
  if (!sessions.current) {
    toast.error(t('settings.prompts.noSession'))
    return
  }
  try {
    await sessions.update(sessions.current.id, { systemPrompt: p.content })
    prompts.bumpUsage(p.id)
    toast.success(t('settings.prompts.setDone'))
  } catch (e) {
    toast.error(toApiError(e).message)
  }
}
</script>

<template>
  <div class="space-y-6 pb-16">
    <div class="flex items-center justify-between">
      <p class="text-sm text-muted-foreground">{{ t('settings.prompts.subtitle') }}</p>
      <Button size="sm" class="gap-1" @click="openAdd">
        <Plus class="size-4" />
        {{ t('settings.prompts.add') }}
      </Button>
    </div>

    <!-- 类型过滤 -->
    <div class="flex gap-2">
      <button
        v-for="f in filters"
        :key="f"
        class="rounded-full border px-3 py-1 text-xs transition-colors"
        :class="
          cn(
            filter === f
              ? 'border-primary bg-primary/10 text-primary'
              : 'text-muted-foreground hover:bg-secondary',
          )
        "
        @click="filter = f"
      >
        {{ t(`settings.prompts.filters.${f}`) }}
      </button>
    </div>

    <!-- 列表 -->
    <p v-if="list.length === 0" class="py-16 text-center text-sm text-muted-foreground">
      {{ t('settings.prompts.empty') }}
    </p>
    <div v-for="p in list" :key="p.id" class="rounded-lg border px-4 py-3">
      <div class="flex items-center gap-2">
        <span class="font-medium">{{ p.title }}</span>
        <Badge variant="secondary" class="text-[10px]">
          {{ t(`settings.prompts.types.${p.type}`) }}
        </Badge>
        <Badge v-if="prompts.isBuiltin(p.id)" variant="outline" class="text-[10px]">
          {{ t('settings.prompts.builtin') }}
        </Badge>
        <span class="ml-auto text-xs text-muted-foreground">
          {{ t('settings.prompts.used') }} {{ p.usageCount }}
        </span>
      </div>

      <p class="mt-1.5 line-clamp-2 whitespace-pre-wrap font-mono text-xs text-muted-foreground">
        {{ p.content }}
      </p>

      <div class="mt-2 flex items-center gap-1">
        <Badge
          v-for="v in p.variables"
          :key="v"
          variant="outline"
          class="font-mono text-[10px]"
        >
          {{ fmtVar(v) }}
        </Badge>
        <span class="flex-1" />
        <Button
          variant="ghost"
          size="sm"
          class="h-7 gap-1 text-xs"
          :title="t('settings.prompts.setAsSystemPrompt')"
          @click="() => void setAsSystemPrompt(p)"
        >
          <Pin class="size-3.5" />
          {{ t('settings.prompts.setAsSystemPrompt') }}
        </Button>
        <Button variant="ghost" size="icon" class="size-7" @click="openVersions(p)">
          <History class="size-3.5" />
        </Button>
        <Button variant="ghost" size="icon" class="size-7" @click="openEdit(p)">
          <Pencil class="size-3.5" />
        </Button>
        <Button
          variant="ghost"
          size="icon"
          class="size-7 hover:text-destructive"
          :disabled="prompts.isBuiltin(p.id)"
          :title="
            prompts.isBuiltin(p.id) ? t('settings.prompts.builtinProtected') : t('common.delete')
          "
          @click="() => void remove(p)"
        >
          <Trash2 class="size-3.5" />
        </Button>
      </div>
    </div>

    <PromptDialog v-model:open="dialogOpen" :prompt="editingPrompt" />
    <VersionsDialog v-model:open="versionsOpen" :prompt="versionsPrompt" />
  </div>
</template>
