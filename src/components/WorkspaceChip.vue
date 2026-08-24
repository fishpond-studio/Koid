<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { ChevronDown, Folder, FolderOpen } from 'lucide-vue-next'
import { useWorkspaceStore } from '@/stores/workspace'

/**
 * 工作区 chip（对齐 dsh EmptyHero 的 WorkspaceChip）：
 * - 未绑定路径：闭合文件夹 + 「选择工作区」占位
 * - 已绑定路径：打开文件夹 + 工作区名
 * - 常驻、始终可切换（作为 WorkspacePicker 的触发 slot 使用）
 */
const props = withDefaults(defineProps<{ size?: 'sm' | 'lg' }>(), { size: 'sm' })

const { t } = useI18n()
const workspaces = useWorkspaceStore()

const label = computed(() => workspaces.current?.name ?? t('chat.chooseWorkspace'))
const hasPath = computed(() => !!workspaces.current?.path)
</script>

<template>
  <button
    type="button"
    class="flex min-w-0 items-center gap-1.5 rounded-lg border text-muted-foreground transition-colors hover:bg-secondary hover:text-foreground"
    :class="
      props.size === 'lg'
        ? 'border-border px-3 py-2 text-sm'
        : 'border-border/60 px-2.5 py-1.5 text-xs'
    "
    :aria-label="t('chat.chooseWorkspace')"
    aria-haspopup="menu"
  >
    <FolderOpen
      v-if="hasPath"
      class="shrink-0 text-muted-foreground"
      :class="props.size === 'lg' ? 'size-4' : 'size-3.5'"
    />
    <Folder
      v-else
      class="shrink-0 text-muted-foreground"
      :class="props.size === 'lg' ? 'size-4' : 'size-3.5'"
    />
    <span
      class="truncate font-medium"
      :class="props.size === 'lg' ? 'max-w-48' : 'max-w-28'"
    >
      {{ label }}
    </span>
    <ChevronDown class="shrink-0" :class="props.size === 'lg' ? 'size-3.5' : 'size-3'" />
  </button>
</template>
