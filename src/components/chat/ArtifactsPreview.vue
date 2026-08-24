<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import { toast } from 'vue-sonner'
import { Copy } from 'lucide-vue-next'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'

/**
 * Artifacts 预览（§4.10）：沙箱 iframe 渲染 HTML/SVG
 * sandbox 只给 allow-scripts：无 allow-same-origin，预览代码无法触达
 * 应用 DOM / localStorage / Tauri API，崩溃也被隔离在本 iframe
 */
const props = defineProps<{ open: boolean; code: string; lang: string }>()
const emit = defineEmits<{ 'update:open': [v: boolean] }>()

const { t } = useI18n()

async function copyCode() {
  try {
    await navigator.clipboard.writeText(props.code)
    toast.success(t('common.copied'))
  } catch {
    /* 剪贴板不可用时静默 */
  }
}
</script>

<template>
  <Dialog :open="open" @update:open="(v: boolean) => emit('update:open', v)">
    <DialogContent
      class="flex h-[85vh] max-w-4xl flex-col gap-0 overflow-hidden p-0"
    >
      <DialogHeader class="flex-row items-center justify-between space-y-0 border-b px-4 py-2.5">
        <DialogTitle class="text-sm">
          {{ t('chat.artifacts') }}
          <span class="ml-2 font-mono text-xs font-normal text-muted-foreground">
            {{ lang }}
          </span>
        </DialogTitle>
        <Button variant="ghost" size="icon" class="size-7" @click="() => void copyCode()">
          <Copy class="size-3.5" />
        </Button>
      </DialogHeader>

      <!-- 沙箱预览区：白底保证浅色 HTML 可读性 -->
      <iframe
        :srcdoc="code"
        sandbox="allow-scripts"
        class="w-full flex-1 border-0 bg-white"
        :title="t('chat.artifacts')"
      />
    </DialogContent>
  </Dialog>
</template>
