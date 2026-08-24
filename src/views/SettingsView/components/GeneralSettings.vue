<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue-sonner'
import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { toApiError } from '@/lib/api'
import { useSettingsStore } from '@/stores/settings'
import { useModelStore } from '@/stores/model'
import { useProviderStore } from '@/stores/provider'

/** 通用设置：默认 System Prompt（§4.6 层级 1）+ 关闭行为 + 自动总结上下文 */
const { t } = useI18n()
const settings = useSettingsStore()
const models = useModelStore()
const providers = useProviderStore()

const closeModes = ['hide', 'quit', 'ask'] as const

/** 总结上下文模型选项：默认 + 全部启用模型（显示「供应商 / 模型名」） */
const compactModelOptions = computed(() =>
  models.enabledModels.map((m) => ({
    id: m.id,
    label: `${providers.get(m.providerId)?.name ?? '?'} / ${m.displayName}`,
  })),
)

async function save() {
  try {
    await Promise.all([
      settings.saveDefaultSystemPrompt(),
      settings.saveCloseMode(),
      settings.saveAutoCompact(),
      settings.saveCompactModel(),
    ])
    toast.success(t('common.saved'))
  } catch (e) {
    toast.error(toApiError(e).message)
  }
}
</script>

<template>
  <div class="space-y-4 pb-16">
    <div>
      <Label>{{ t('settings.general.defaultSystemPrompt') }}</Label>
      <p class="mt-0.5 text-xs text-muted-foreground">
        {{ t('settings.general.defaultSystemPromptHint') }}
      </p>
    </div>

    <textarea
      v-model="settings.defaultSystemPrompt"
      rows="6"
      class="w-full resize-none rounded-md border border-input bg-background px-3 py-2 font-mono text-xs outline-none focus:ring-2 focus:ring-ring"
      :placeholder="t('settings.general.defaultSystemPromptPlaceholder')"
    />

    <!-- 自动总结上下文 -->
    <div class="flex items-center justify-between gap-4 pt-2">
      <div>
        <Label>{{ t('settings.general.autoCompact') }}</Label>
        <p class="mt-0.5 text-xs text-muted-foreground">
          {{ t('settings.general.autoCompactHint') }}
        </p>
      </div>
      <Switch v-model:checked="settings.autoCompact" />
    </div>

    <!-- 总结上下文使用的模型 -->
    <div>
      <Label>{{ t('settings.general.compactModel') }}</Label>
      <p class="mt-0.5 text-xs text-muted-foreground">
        {{ t('settings.general.compactModelHint') }}
      </p>
      <Select v-model="settings.compactModelId">
        <SelectTrigger class="mt-2 w-full">
          <SelectValue :placeholder="t('settings.general.compactModelDefault')" />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="default">
            {{ t('settings.general.compactModelDefault') }}
          </SelectItem>
          <SelectItem v-for="m in compactModelOptions" :key="m.id" :value="m.id">
            {{ m.label }}
          </SelectItem>
        </SelectContent>
      </Select>
    </div>

    <!-- 关闭行为 -->
    <div class="pt-2">
      <Label>{{ t('settings.general.closeBehaviour') }}</Label>
      <p class="mt-0.5 text-xs text-muted-foreground">
        {{ t('settings.general.closeBehaviourHint') }}
      </p>
      <Select v-model="settings.closeMode" class="mt-2">
        <SelectTrigger class="mt-2 w-full">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem
            v-for="m in closeModes"
            :key="m"
            :value="m"
          >
            {{ t(`settings.general.closeModes.${m}`) }}
          </SelectItem>
        </SelectContent>
      </Select>
    </div>

    <div class="flex justify-end">
      <Button @click="() => void save()">{{ t('common.save') }}</Button>
    </div>
  </div>
</template>
