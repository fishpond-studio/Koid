<script setup lang="ts">
import { watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { storeToRefs } from 'pinia'
import { Check } from 'lucide-vue-next'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Separator } from '@/components/ui/separator'
import { useThemeStore, CODE_FONTS } from '@/stores/theme'
import type { ThemeColor } from '@/types'

const { t } = useI18n()
const store = useThemeStore()
const { mode, color, customPrimary, density, codeFont } = storeToRefs(store)

/** 6 套预设（§4.1），preview 用于在界面上绘制色卡 */
const presets: { id: ThemeColor; preview: string }[] = [
  { id: 'indigo', preview: '#6366f1' },
  { id: 'emerald', preview: '#10b981' },
  { id: 'rose', preview: '#e11d48' },
  { id: 'amber', preview: '#d97706' },
  { id: 'violet', preview: '#8b5cf6' },
  { id: 'sky', preview: '#0ea5e9' },
]

// 自定义颜色输入实时预览：输入合法 HEX 立即生效
watch(customPrimary, (hex) => {
  if (color.value === 'custom' && hex && /^#[0-9a-fA-F]{6}$/.test(hex)) {
    store.setColor('custom', hex)
  }
})
</script>

<template>
  <div class="space-y-8 pb-16">
    <!-- 模式 -->
    <section class="space-y-3">
      <Label>{{ t('settings.appearance.mode') }}</Label>
      <div class="flex gap-2">
        <Button
          v-for="m in (['light', 'dark', 'system'] as const)"
          :key="m"
          :variant="mode === m ? 'default' : 'outline'"
          @click="store.setMode(m)"
        >
          {{ t(`settings.appearance.modes.${m}`) }}
        </Button>
      </div>
    </section>

    <Separator />

    <!-- 主题色预设 -->
    <section class="space-y-3">
      <Label>{{ t('settings.appearance.themeColor') }}</Label>
      <div class="flex flex-wrap items-center gap-3">
        <button
          v-for="p in presets"
          :key="p.id"
          class="flex size-9 items-center justify-center rounded-full border-2 transition-transform hover:scale-110"
          :class="color === p.id ? 'border-foreground' : 'border-transparent'"
          :style="{ backgroundColor: p.preview }"
          :title="p.id"
          @click="store.setColor(p.id)"
        >
          <Check v-if="color === p.id" class="size-4 text-white" />
        </button>

        <!-- 自定义 HEX -->
        <div class="flex items-center gap-2">
          <input
            type="color"
            :value="customPrimary ?? '#6366f1'"
            class="size-9 cursor-pointer rounded-full border bg-transparent"
            @input="
              (e) => {
                customPrimary = (e.target as HTMLInputElement).value
                store.setColor('custom', customPrimary ?? undefined)
              }
            "
          />
          <Input
            :model-value="customPrimary ?? ''"
            placeholder="#RRGGBB"
            class="w-28 font-mono text-xs"
            @update:model-value="(v) => (customPrimary = String(v))"
          />
        </div>
      </div>
    </section>

    <Separator />

    <!-- UI 密度 -->
    <section class="space-y-3">
      <Label>{{ t('settings.appearance.density') }}</Label>
      <div class="flex gap-2">
        <Button
          v-for="d in (['compact', 'default', 'comfortable'] as const)"
          :key="d"
          :variant="density === d ? 'default' : 'outline'"
          @click="store.setDensity(d)"
        >
          {{ t(`settings.appearance.densities.${d}`) }}
        </Button>
      </div>
    </section>

    <Separator />

    <!-- 代码字体 -->
    <section class="space-y-3">
      <Label>{{ t('settings.appearance.codeFont') }}</Label>
      <select
        :value="codeFont"
        class="h-9 w-64 rounded-md border border-input bg-background px-3 text-sm focus:outline-none focus:ring-2 focus:ring-ring"
        @change="store.setCodeFont(($event.target as HTMLSelectElement).value)"
      >
        <option v-for="f in CODE_FONTS" :key="f" :value="f">
          {{ f.split("'")[1] }}
        </option>
      </select>
      <pre class="rounded-lg bg-muted p-3 font-mono text-sm">const koid = await chat("hello")</pre>
    </section>
  </div>
</template>

