<script setup lang="ts">
import { reactive, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue-sonner'
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
import { Switch } from '@/components/ui/switch'
import { useModelStore } from '@/stores/model'
import { toApiError } from '@/lib/api'
import type { Model, Provider } from '@/types'

/** 模型新增/编辑对话框：挂靠在某个供应商下 */
const props = defineProps<{ open: boolean; provider: Provider | null; model: Model | null }>()
const emit = defineEmits<{ 'update:open': [v: boolean] }>()

const { t } = useI18n()
const store = useModelStore()

const saving = ref(false)

const form = reactive({
  modelId: '',
  displayName: '',
  contextWindow: null as number | null,
  enabled: true,
})

watch(
  () => props.open,
  (open) => {
    if (!open) return
    if (props.model) {
      form.modelId = props.model.modelId
      form.displayName = props.model.displayName
      form.contextWindow = props.model.contextWindow
      form.enabled = props.model.enabled
    } else {
      form.modelId = ''
      form.displayName = ''
      form.contextWindow = null
      form.enabled = true
    }
  },
)

async function save() {
  if (!props.provider) return
  if (!form.modelId.trim()) {
    toast.error(t('settings.model.validationRequired'))
    return
  }
  saving.value = true
  try {
    await store.save({
      id: props.model?.id ?? null,
      providerId: props.provider.id,
      modelId: form.modelId.trim(),
      // 展示名缺省用模型 ID（Rust 侧同样兜底）
      displayName: form.displayName.trim() || null,
      contextWindow: form.contextWindow,
      capabilities: props.model?.capabilities ?? ['chat', 'tools'],
      enabled: form.enabled,
    })
    toast.success(t('common.saved'))
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
    <DialogContent class="sm:max-w-md">
      <DialogHeader>
        <DialogTitle>
          {{ model ? t('settings.model.edit') : t('settings.model.add') }}
          <span v-if="provider" class="ml-2 text-sm font-normal text-muted-foreground">
            · {{ provider.name }}
          </span>
        </DialogTitle>
      </DialogHeader>

      <div class="space-y-4 py-2">
        <div class="space-y-1.5">
          <Label>{{ t('settings.model.modelId') }}</Label>
          <Input
            v-model="form.modelId"
            :placeholder="t('settings.model.modelIdPlaceholder')"
            class="font-mono text-xs"
          />
        </div>

        <div class="space-y-1.5">
          <Label>{{ t('settings.model.displayName') }}</Label>
          <Input v-model="form.displayName" :placeholder="t('settings.model.displayNamePlaceholder')" />
        </div>

        <div class="space-y-1.5">
          <Label>{{ t('settings.model.contextWindow') }}</Label>
          <Input
            :model-value="form.contextWindow ?? undefined"
            type="number"
            min="0"
            placeholder="128000"
            @update:model-value="(v: string | number) => (form.contextWindow = v === '' ? null : Number(v))"
          />
        </div>

        <div class="flex items-center justify-between rounded-lg border px-3 py-2">
          <Label>{{ t('common.enabled') }}</Label>
          <Switch v-model:checked="form.enabled" />
        </div>
      </div>

      <DialogFooter>
        <Button variant="outline" @click="emit('update:open', false)">
          {{ t('common.cancel') }}
        </Button>
        <Button :disabled="saving" @click="() => void save()">
          {{ t('common.save') }}
        </Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>
