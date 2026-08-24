<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue-sonner'
import { Check, Boxes } from 'lucide-vue-next'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { useModelStore } from '@/stores/model'
import { useProviderStore } from '@/stores/provider'
import { useSessionStore } from '@/stores/session'
import { cn } from '@/lib/utils'

/**
 * 模型选择器（§4.2）：按供应商分组的启用模型列表
 * 底部模型切换 chip 与 /model 命令共用
 */
const props = defineProps<{ open: boolean }>()
const emit = defineEmits<{ 'update:open': [v: boolean] }>()

const { t } = useI18n()
const models = useModelStore()
const providers = useProviderStore()
const sessions = useSessionStore()

/** 按供应商分组：{ providerId, providerName, models[] } */
const groups = computed(() => {
  const map = new Map<string, { providerId: string; providerName: string; models: typeof models.enabledModels }>()
  for (const m of models.enabledModels) {
    const p = providers.get(m.providerId)
    if (!p) continue
    if (!map.has(m.providerId)) {
      map.set(m.providerId, { providerId: m.providerId, providerName: p.name, models: [] })
    }
    map.get(m.providerId)!.models.push(m)
  }
  return [...map.values()]
})

const currentModelId = computed(() => sessions.current?.modelId ?? null)

function pick(modelId: string) {
  // 无会话时也允许选择：记住模型，新建会话时自动采用（§模型选择修复）
  models.remember(modelId)
  if (sessions.current) {
    void sessions.update(sessions.current.id, { modelId })
  }
  emit('update:open', false)
  toast.success(t('chat.modelSwitched'))
}
</script>

<template>
  <Dialog :open="open" @update:open="(v: boolean) => emit('update:open', v)">
    <DialogContent class="flex max-h-[75vh] max-w-md flex-col overflow-hidden">
      <DialogHeader>
        <DialogTitle class="flex items-center gap-1.5">
          <Boxes class="size-4" />
          {{ t('chat.selectModel') }}
        </DialogTitle>
      </DialogHeader>

      <div class="scrollbar-thin min-h-0 flex-1 space-y-4 overflow-y-auto py-1">
        <p v-if="groups.length === 0" class="py-10 text-center text-sm text-muted-foreground">
          {{ t('chat.noModels') }}
        </p>
        <section v-for="g in groups" :key="g.providerId">
          <p class="px-2 pb-1 text-[10px] font-medium uppercase text-muted-foreground">
            {{ g.providerName }}
          </p>
          <button
            v-for="m in g.models"
            :key="m.id"
            class="flex w-full items-center gap-2.5 rounded-md px-2.5 py-2 text-left text-sm transition-colors"
            :class="cn(currentModelId === m.id ? 'bg-secondary' : 'hover:bg-secondary/60')"
            @click="pick(m.id)"
          >
            <span
              class="flex size-4 shrink-0 items-center justify-center rounded-full border"
              :class="currentModelId === m.id ? 'border-primary' : 'border-input'"
            >
              <span
                v-if="currentModelId === m.id"
                class="size-2 rounded-full bg-primary"
              />
            </span>
            <span class="min-w-0 flex-1">
              <span class="block truncate">{{ m.displayName }}</span>
              <span class="block truncate font-mono text-[10px] text-muted-foreground">
                {{ m.modelId }}
              </span>
            </span>
            <Check v-if="currentModelId === m.id" class="size-4 shrink-0 text-primary" />
          </button>
        </section>
      </div>
    </DialogContent>
  </Dialog>
</template>
