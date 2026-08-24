<script setup lang="ts">
import { onMounted, onUnmounted } from 'vue'
import { RouterView } from 'vue-router'
import { toast } from 'vue-sonner'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import Sidebar from '@/components/Sidebar.vue'
import { toApiError } from '@/lib/api'
import { useModelStore } from '@/stores/model'
import { usePromptStore } from '@/stores/prompts'
import { useProviderStore } from '@/stores/provider'
import { useSessionStore } from '@/stores/session'
import { useSettingsStore } from '@/stores/settings'
import { useWorkspaceStore } from '@/stores/workspace'

/**
 * 主布局：左侧会话侧边栏 + 右侧路由内容
 * 布局层负责全局数据冷启动加载，保证任意路由直达时数据可用
 */
const providers = useProviderStore()
const models = useModelStore()
const sessions = useSessionStore()
const settings = useSettingsStore()
const prompts = usePromptStore()
const workspaces = useWorkspaceStore()

let unlistenWs: UnlistenFn | null = null

onMounted(async () => {
  try {
    await Promise.all([
      providers.load(),
      models.load(),
      sessions.load(),
      settings.load(),
      prompts.load(),
      workspaces.load(),
    ])
    // 冷启动自动选择工作区（对齐 dsh startInitialSelection）：
    // 当前工作区未绑定项目路径时，优先切到「最近会话所属工作区」，
    // 否则取最早可用且已绑定路径的工作区
    if (!workspaces.current?.path) {
      const recent = sessions.sessions[0]?.workspaceId
      const target =
        (recent && workspaces.workspaces.some((w) => w.id === recent && w.path) && recent) ||
        workspaces.workspaces.find((w) => w.path)?.id ||
        null
      if (target) workspaces.persistCurrent(target)
    }
    // 冷启动默认打开最近会话（列表已按 pinned/updatedAt 排序）
    const first = sessions.sessions[0]
    if (!sessions.currentId && first) {
      await sessions.open(first.id)
      if (first.workspaceId) workspaces.persistCurrent(first.workspaceId)
    }
  } catch (e) {
    toast.error(toApiError(e).message)
  }

  // Agent 写操作后刷新工作区文件树
  unlistenWs = await listen('workspace:changed', () => {
    void workspaces.loadFiles()
  })
})

onUnmounted(() => {
  unlistenWs?.()
})
</script>

<template>
  <div class="flex h-screen overflow-hidden">
    <Sidebar />
    <main class="min-w-0 flex-1">
      <RouterView />
    </main>
  </div>
</template>
