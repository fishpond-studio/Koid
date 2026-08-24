<script setup lang="ts">
import { ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRoute } from 'vue-router'
import AppearanceSettings from './components/AppearanceSettings.vue'
import BackupSettings from './components/BackupSettings.vue'
import GeneralSettings from './components/GeneralSettings.vue'
import McpSettings from './components/McpSettings.vue'
import ModelSettings from './components/ModelSettings.vue'
import NetworkSettings from './components/NetworkSettings.vue'
import PluginSettings from './components/PluginSettings.vue'
import PromptSettings from './components/PromptSettings.vue'
import { cn } from '@/lib/utils'

const { t } = useI18n()
const route = useRoute()

/** 设置页分区：按 Phase 逐步点亮 */
const sections = [
  'appearance',
  'model',
  'network',
  'prompts',
  'mcp',
  'plugins',
  'backup',
  'general',
] as const
type Section = (typeof sections)[number]
const active = ref<Section>('appearance')

// 支持 ?section=mcp 深链（内置命令 /mcp 使用）
watch(
  () => route.query.section,
  (s) => {
    if (typeof s === 'string' && (sections as readonly string[]).includes(s)) {
      active.value = s as Section
    }
  },
  { immediate: true },
)
</script>

<template>
  <div class="mx-auto flex h-full max-w-4xl gap-8 p-8">
    <!-- 左侧分区导航 -->
    <nav class="w-44 shrink-0 space-y-1 pt-2">
      <button
        v-for="s in sections"
        :key="s"
        class="w-full rounded-md px-3 py-2 text-left text-sm transition-colors"
        :class="
          cn(
            active === s
              ? 'bg-secondary font-medium text-foreground'
              : 'text-muted-foreground hover:bg-secondary/50 hover:text-foreground',
          )
        "
        @click="active = s"
      >
        {{ t(`settings.sections.${s}`) }}
      </button>
    </nav>

    <!-- 右侧内容区 -->
    <div class="scrollbar-thin min-w-0 flex-1 overflow-y-auto">
      <h1 class="mb-6 text-2xl font-semibold tracking-tight">{{ t('settings.title') }}</h1>
      <AppearanceSettings v-if="active === 'appearance'" />
      <ModelSettings v-else-if="active === 'model'" />
      <NetworkSettings v-else-if="active === 'network'" />
      <PromptSettings v-else-if="active === 'prompts'" />
      <McpSettings v-else-if="active === 'mcp'" />
      <PluginSettings v-else-if="active === 'plugins'" />
      <BackupSettings v-else-if="active === 'backup'" />
      <GeneralSettings v-else-if="active === 'general'" />
      <div v-else class="py-20 text-center text-sm text-muted-foreground">
        {{ t('common.comingSoon') }}
      </div>
    </div>
  </div>
</template>
