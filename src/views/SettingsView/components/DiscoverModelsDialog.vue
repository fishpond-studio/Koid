<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue-sonner'
import { Check, Loader2, RefreshCw } from 'lucide-vue-next'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { modelsApi, toApiError } from '@/lib/api'
import { useModelStore } from '@/stores/model'
import type { DiscoveredModel, Provider } from '@/types'

/**
 * 模型发现对话框（§4.2）：请求 /v1/models → 勾选启用
 * 保存语义：勾选的入库并启用；已存在但取消勾选的禁用；列表外的本地模型不动
 */
const props = defineProps<{ open: boolean; provider: Provider | null }>()
const emit = defineEmits<{ 'update:open': [v: boolean] }>()

const { t } = useI18n()
const models = useModelStore()

const loading = ref(false)
const saving = ref(false)
const discovered = ref<DiscoveredModel[]>([])
const checked = ref<Record<string, boolean>>({})
const error = ref<string | null>(null)

watch(
  () => props.open,
  async (open) => {
    if (!open || !props.provider) return
    error.value = null
    await load()
  },
)

async function load() {
  if (!props.provider) return
  loading.value = true
  error.value = null
  try {
    discovered.value = await modelsApi.discover(props.provider.id)
    checked.value = {}
    for (const m of discovered.value) checked.value[m.modelId] = m.enabled
  } catch (e) {
    error.value = toApiError(e).message
    discovered.value = []
  } finally {
    loading.value = false
  }
}

const allChecked = computed(
  () => discovered.value.length > 0 && discovered.value.every((m) => checked.value[m.modelId]),
)

function toggleAll() {
  const target = !allChecked.value
  for (const m of discovered.value) checked.value[m.modelId] = target
}

const checkedCount = computed(
  () => discovered.value.filter((m) => checked.value[m.modelId]).length,
)

async function save() {
  if (!props.provider || saving.value) return
  saving.value = true
  try {
    // 现有本地模型按 modelId 定位（用于更新/禁用）
    const existing = models.byProvider(props.provider.id)
    for (const m of discovered.value) {
      const want = checked.value[m.modelId]
      const local = existing.find((x) => x.modelId === m.modelId)
      if (want) {
        // 勾选：不存在则创建，存在但禁用则启用
        if (!local) {
          await modelsApi.save({
            providerId: props.provider.id,
            modelId: m.modelId,
            displayName: m.displayName === m.modelId ? null : m.displayName,
            enabled: true,
          })
        } else if (!local.enabled) {
          await modelsApi.save({
            id: local.id,
            providerId: props.provider.id,
            modelId: m.modelId,
            displayName: m.displayName === m.modelId ? null : m.displayName,
            contextWindow: local.contextWindow,
            capabilities: local.capabilities,
            enabled: true,
          })
        }
      } else if (local?.enabled) {
        // 取消勾选：仅禁用本地已启用的
        await modelsApi.save({
          id: local.id,
          providerId: props.provider.id,
          modelId: m.modelId,
          displayName: m.displayName === m.modelId ? null : m.displayName,
          contextWindow: local.contextWindow,
          capabilities: local.capabilities,
          enabled: false,
        })
      }
    }
    await models.load(true)
    toast.success(t('settings.model.discovered', { count: checkedCount.value }))
    emit('update:open', false)
  } catch (e) {
    toast.error(toApiError(e).message)
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <Dialog :open="open" @update:open="(v: boolean) => emit('update:open', v)">
    <DialogContent class="flex max-h-[80vh] max-w-lg flex-col overflow-hidden">
      <DialogHeader class="flex-row items-center justify-between space-y-0">
        <DialogTitle>
          {{ t('settings.model.discover') }}
          <span v-if="provider" class="ml-2 font-mono text-xs font-normal text-muted-foreground">
            {{ provider.name }}
          </span>
        </DialogTitle>
        <Button variant="ghost" size="icon" class="size-7" @click="() => void load()">
          <RefreshCw class="size-3.5" />
        </Button>
      </DialogHeader>

      <!-- 加载 / 错误 -->
      <div v-if="loading" class="flex items-center gap-2 py-8 text-sm text-muted-foreground">
        <Loader2 class="size-4 animate-spin" />
        {{ t('common.loading') }}
      </div>
      <p v-else-if="error" class="py-8 text-center text-sm text-destructive">{{ error }}</p>

      <!-- 列表 -->
      <template v-else>
        <div
          v-if="discovered.length"
          class="flex items-center justify-between border-b px-1 py-2 text-xs text-muted-foreground"
        >
          <button class="flex items-center gap-1" @click="toggleAll">
            <span
              class="flex size-4 items-center justify-center rounded border"
              :class="allChecked ? 'border-primary bg-primary' : 'border-input'"
            >
              <Check v-if="allChecked" class="size-3 text-primary-foreground" />
            </span>
            {{ t('settings.model.selectAll') }}
          </button>
          <span>
            {{ t('settings.model.selected', { n: checkedCount, total: discovered.length }) }}
          </span>
        </div>

        <div class="scrollbar-thin min-h-0 flex-1 space-y-0.5 overflow-y-auto py-2">
          <p v-if="discovered.length === 0" class="py-8 text-center text-sm text-muted-foreground">
            {{ t('settings.model.noneDiscovered') }}
          </p>
          <button
            v-for="m in discovered"
            :key="m.modelId"
            class="flex w-full items-center gap-2.5 rounded-md px-2 py-1.5 text-left hover:bg-secondary/60"
            @click="checked[m.modelId] = !checked[m.modelId]"
          >
            <span
              class="flex size-4 shrink-0 items-center justify-center rounded border transition-colors"
              :class="checked[m.modelId] ? 'border-primary bg-primary' : 'border-input'"
            >
              <Check
                v-if="checked[m.modelId]"
                class="size-3 text-primary-foreground"
              />
            </span>
            <span class="min-w-0 flex-1">
              <span class="block truncate text-sm">{{ m.displayName }}</span>
              <span class="block truncate font-mono text-[10px] text-muted-foreground">
                {{ m.modelId }}
              </span>
            </span>
          </button>
        </div>
      </template>

      <DialogFooter class="border-t pt-3">
        <Button variant="outline" @click="emit('update:open', false)">
          {{ t('common.cancel') }}
        </Button>
        <Button :disabled="saving || loading || discovered.length === 0" @click="() => void save()">
          {{ t('common.save') }}
        </Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>
