<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRouter } from 'vue-router'
import { Boxes, MessageSquare, Search, Settings, Sparkles } from 'lucide-vue-next'
import {
  Dialog,
  DialogContent,
  DialogTitle,
} from '@/components/ui/dialog'
import { usePluginStore } from '@/stores/plugins'
import { cn } from '@/lib/utils'
import { toast } from 'vue-sonner'

/**
 * 命令面板（§4.9 生态完善）：Cmd/Ctrl+K 唤起
 * - 内置导航命令
 * - 插件通过 koid.command.register 注册的命令
 */
const { t } = useI18n()
const router = useRouter()
const plugins = usePluginStore()

const open = ref(false)
const query = ref('')
const index = ref(0)
const inputEl = ref<HTMLInputElement | null>(null)

interface Item {
  id: string
  label: string
  hint?: string
  icon: typeof MessageSquare
  run: () => void
}

const navItems: Item[] = [
  {
    id: 'nav:chat',
    label: t('nav.chat'),
    icon: MessageSquare,
    run: () => void router.push('/chat'),
  },
  {
    id: 'nav:skills',
    label: t('nav.skills'),
    icon: Sparkles,
    run: () => void router.push('/skills'),
  },
  {
    id: 'nav:settings',
    label: t('nav.settings'),
    icon: Settings,
    run: () => void router.push('/settings'),
  },
]

const items = computed<Item[]>(() => {
  const q = query.value.trim().toLowerCase()
  const out: Item[] = []
  for (const n of navItems) {
    if (!q || n.label.toLowerCase().includes(q)) out.push(n)
  }
  for (const c of plugins.commands) {
    if (!q || c.title.toLowerCase().includes(q) || c.pluginName.toLowerCase().includes(q)) {
      out.push({
        id: c.id,
        label: c.title,
        hint: c.pluginName,
        icon: Boxes,
        run: () => {
          plugins.execute(c.id)
        },
      })
    }
  }
  return out
})

function reset() {
  query.value = ''
  index.value = 0
}

function openPalette() {
  open.value = true
  reset()
  requestAnimationFrame(() => inputEl.value?.focus())
}

function onKeydown(e: KeyboardEvent) {
  if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') {
    e.preventDefault()
    openPalette()
  }
}

watch(open, (v) => {
  if (v) requestAnimationFrame(() => inputEl.value?.focus())
})

function pick(item: Item) {
  open.value = false
  // 插件命令若无可用执行器（插件页未挂载）则提示
  if (item.id.startsWith('plugin:') && !plugins.commands.some((c) => c.id === item.id)) {
    toast.error(t('commandPalette.pluginUnavailable'))
    return
  }
  item.run()
}

onMounted(() => window.addEventListener('keydown', onKeydown))
onBeforeUnmount(() => window.removeEventListener('keydown', onKeydown))
</script>

<template>
  <Dialog :open="open" @update:open="(v: boolean) => (open = v)">
    <DialogContent class="top-[20%] p-0 sm:max-w-lg">
      <DialogTitle class="sr-only">{{ t('commandPalette.title') }}</DialogTitle>

      <div class="flex items-center gap-2 border-b px-3">
        <Search class="size-4 text-muted-foreground" />
        <input
          ref="inputEl"
          v-model="query"
          :placeholder="t('commandPalette.placeholder')"
          class="h-11 w-full bg-transparent text-sm outline-none placeholder:text-muted-foreground"
          @keydown.enter="items[index] && pick(items[index])"
          @keydown.down.prevent="index = (index + 1) % items.length"
          @keydown.up.prevent="index = (index - 1 + items.length) % items.length"
        />
      </div>

      <div class="scrollbar-thin max-h-72 overflow-y-auto p-1.5">
        <p v-if="items.length === 0" class="px-3 py-6 text-center text-sm text-muted-foreground">
          {{ t('commandPalette.empty') }}
        </p>
        <button
          v-for="(item, i) in items"
          :key="item.id"
          class="flex w-full items-center gap-2 rounded-md px-2.5 py-2 text-left text-sm transition-colors"
          :class="cn(i === index ? 'bg-secondary' : 'hover:bg-secondary/60')"
          @mouseenter="index = i"
          @click="pick(item)"
        >
          <component :is="item.icon" class="size-4 shrink-0 text-muted-foreground" />
          <span class="min-w-0 flex-1 truncate">{{ item.label }}</span>
          <span v-if="item.hint" class="shrink-0 font-mono text-[10px] text-muted-foreground">
            {{ item.hint }}
          </span>
        </button>
      </div>
    </DialogContent>
  </Dialog>
</template>
