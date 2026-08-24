<script setup lang="ts">
import { ref } from 'vue'
import { ChevronDown, ChevronRight, FileText, Folder } from 'lucide-vue-next'

/** 文件树节点：children 递归，isDir 决定展开或引用 */
interface FileTreeNode {
  name: string
  path: string
  isDir: boolean
  children: FileTreeNode[]
}

/**
 * 工作区文件树（Codex Desktop 式）：递归展开目录，点击文件把 @路径 引用进输入框
 */
const props = defineProps<{ nodes: FileTreeNode[] }>()

const emit = defineEmits<{ pick: [path: string] }>()

const expanded = ref<Set<string>>(new Set())

function toggle(path: string) {
  const next = new Set(expanded.value)
  if (next.has(path)) next.delete(path)
  else next.add(path)
  expanded.value = next
}
</script>

<template>
  <ul class="space-y-px">
    <li v-for="node in nodes" :key="node.path">
      <!-- 目录：可展开 -->
      <button
        v-if="node.isDir"
        class="flex w-full items-center gap-1 rounded px-1.5 py-0.5 text-left text-xs text-muted-foreground hover:bg-secondary/60"
        @click="toggle(node.path)"
      >
        <ChevronDown v-if="expanded.has(node.path)" class="size-3 shrink-0" />
        <ChevronRight v-else class="size-3 shrink-0" />
        <Folder class="size-3.5 shrink-0" />
        <span class="truncate">{{ node.name }}</span>
      </button>
      <div
        v-if="node.isDir && expanded.has(node.path)"
        class="ml-2 border-l border-border/60 pl-1.5"
      >
        <WorkspaceFileTree :nodes="node.children" @pick="(p) => emit('pick', p)" />
      </div>

      <!-- 文件：点击引用 -->
      <button
        v-else
        class="flex w-full items-center gap-1 rounded px-1.5 py-0.5 pl-5 text-left text-xs text-muted-foreground transition-colors hover:bg-secondary/60 hover:text-foreground"
        :title="node.path"
        @click="emit('pick', node.path)"
      >
        <FileText class="size-3.5 shrink-0" />
        <span class="truncate">{{ node.name }}</span>
      </button>
    </li>
  </ul>
</template>
