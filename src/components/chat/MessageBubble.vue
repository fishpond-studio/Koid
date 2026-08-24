<script setup lang="ts">
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { Brain, Check, Copy, GitBranch, Pencil, Undo2 } from 'lucide-vue-next'
import MarkdownView from '@/components/markdown/MarkdownView.vue'
import type { Message } from '@/types'

/**
 * 消息气泡（§4.10 视觉规范）
 * - 用户：右对齐，primary 背景，hover 显示 编辑 / 撤回
 * - assistant：左对齐，muted 背景，Markdown 渲染 + 思考过程折叠
 * - data-mid：供搜索跳转滚动定位（§4.5.3）
 */
const props = defineProps<{ message: Message }>()
const emit = defineEmits<{
  branch: [messageId: string]
  preview: [payload: { lang: string; code: string }]
  /** 编辑该用户消息（截断其后并重新发送） */
  edit: [messageId: string]
  /** 撤回该用户消息及其后的全部消息 */
  retract: [messageId: string]
}>()
const { t } = useI18n()

const copied = ref(false)

async function copy() {
  try {
    await navigator.clipboard.writeText(props.message.content)
    copied.value = true
    window.setTimeout(() => (copied.value = false), 1500)
  } catch {
    /* 剪贴板不可用时静默 */
  }
}

const latencyText = computed(() => {
  const ms = props.message.latencyMs
  if (!ms) return null
  return ms >= 1000 ? `${(ms / 1000).toFixed(1)}s` : `${ms}ms`
})
</script>

<template>
  <!-- 用户消息 -->
  <div
    v-if="message.role === 'user'"
    :data-mid="message.id"
    class="group flex items-end justify-end gap-1.5"
  >
    <!-- hover 操作：编辑 / 撤回 -->
    <div
      class="flex shrink-0 items-center gap-0.5 self-center text-muted-foreground opacity-0 transition-opacity group-hover:opacity-100"
    >
      <button
        class="rounded p-1.5 transition-colors hover:bg-secondary hover:text-foreground"
        :title="t('chat.message.edit')"
        @click="emit('edit', message.id)"
      >
        <Pencil class="size-3" />
      </button>
      <button
        class="rounded p-1.5 transition-colors hover:bg-destructive/10 hover:text-destructive"
        :title="t('chat.message.retract')"
        @click="emit('retract', message.id)"
      >
        <Undo2 class="size-3" />
      </button>
    </div>
    <div
      class="max-w-[85%] whitespace-pre-wrap break-words rounded-2xl rounded-br-md bg-primary px-4 py-2.5 text-primary-foreground"
    >
      {{ message.content }}
    </div>
  </div>

  <!-- assistant 消息 -->
  <div v-else :data-mid="message.id" class="group flex flex-col">
    <div class="max-w-full rounded-2xl bg-muted px-4 py-3">
      <!-- 思考过程：默认折叠（§4.10 Thinking） -->
      <details v-if="message.reasoning" class="mb-2">
        <summary
          class="flex w-fit cursor-pointer select-none items-center gap-1.5 text-xs text-muted-foreground transition-colors hover:text-foreground"
        >
          <Brain class="size-3.5" />
          {{ t('chat.message.reasoning') }}
        </summary>
        <div
          class="mt-2 whitespace-pre-wrap border-l-2 border-border pl-3 text-sm italic text-muted-foreground"
        >
          {{ message.reasoning }}
        </div>
      </details>

      <MarkdownView :content="message.content" @preview="(p) => emit('preview', p)" />
    </div>

    <!-- 元信息：耗时 / 复制 / 分支（§4.5.4）；token 用量集中在聊天头部展示 -->
    <div class="mt-1 flex items-center gap-2 pl-1 text-xs text-muted-foreground">
      <span v-if="latencyText">{{ latencyText }}</span>
      <button
        class="flex items-center gap-1 opacity-0 transition-opacity group-hover:opacity-100"
        :title="t('chat.message.copy')"
        @click="copy"
      >
        <Check v-if="copied" class="size-3" />
        <Copy v-else class="size-3" />
        {{ copied ? t('common.copied') : t('chat.message.copy') }}
      </button>
      <button
        class="flex items-center gap-1 opacity-0 transition-opacity group-hover:opacity-100"
        :title="t('chat.message.branch')"
        @click="emit('branch', message.id)"
      >
        <GitBranch class="size-3" />
        {{ t('chat.message.branch') }}
      </button>
    </div>
  </div>
</template>
