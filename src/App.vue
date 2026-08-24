<script setup lang="ts">
import { RouterView } from 'vue-router'
import { Toaster } from '@/components/ui/sonner'
import { useThemeWatcher } from '@/composables/useTheme'
import CommandPalette from '@/components/CommandPalette.vue'
import CloseAskDialog from '@/components/CloseAskDialog.vue'

// 顶层负责：主题副作用、全局 Toast 挂载、命令面板（Cmd+K）、关闭询问框
useThemeWatcher()
</script>

<template>
  <RouterView />
  <CommandPalette />
  <CloseAskDialog />
  <!-- 全局 Toast：右上角、3s 自动消失、不阻塞交互（sonner 非模态） -->
  <Toaster position="top-right" :duration="3000" :offset="16" />
</template>

