<script setup lang="ts">
import { onMounted, onUnmounted, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { useI18n } from 'vue-i18n'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'

/**
 * 关闭询问框：关闭行为为 `ask`（每次询问）时，Rust 拦截关闭并发 `koid://close-ask`，
 * 由本组件弹出「隐藏到托盘 / 退出 / 下次再问」三个选项。
 */
const { t } = useI18n()
const open = ref(false)
let unlisten: UnlistenFn | null = null

onMounted(async () => {
  unlisten = await listen('koid://close-ask', () => {
    open.value = true
  })
})
onUnmounted(() => {
  unlisten?.()
})

function choose(choice: 'hide' | 'quit' | 'ask') {
  open.value = false
  void invoke('resolve_close', { choice })
}
</script>

<template>
  <Dialog v-model:open="open">
    <DialogContent class="sm:max-w-sm">
      <DialogHeader>
        <DialogTitle>{{ t('closeAsk.title') }}</DialogTitle>
        <DialogDescription>{{ t('closeAsk.message') }}</DialogDescription>
      </DialogHeader>
      <DialogFooter class="flex-col gap-2 sm:flex-col">
        <Button class="w-full" @click="choose('hide')">
          {{ t('closeAsk.hide') }}
        </Button>
        <div class="flex w-full gap-2">
          <Button variant="outline" class="flex-1" @click="choose('ask')">
            {{ t('closeAsk.ask') }}
          </Button>
          <Button variant="outline" class="flex-1" @click="choose('quit')">
            {{ t('closeAsk.quit') }}
          </Button>
        </div>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>
