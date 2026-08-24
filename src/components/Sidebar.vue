<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRoute, useRouter } from 'vue-router'
import {
  Archive,
  ArchiveRestore,
  ChevronDown,
  ChevronRight,
  Loader2,
  MessageSquare,
  MessageSquarePlus,
  Search,
  Settings,
  Sparkles,
  Trash2,
} from 'lucide-vue-next'
import { Button } from '@/components/ui/button'
import { Separator } from '@/components/ui/separator'
import WorkspaceFileTree from '@/components/WorkspaceFileTree.vue'
import { useSessionStore } from '@/stores/session'
import { useWorkspaceStore } from '@/stores/workspace'
import { useDraftBus } from '@/composables/useDraftBus'
import { cn, highlightSnippet } from '@/lib/utils'
import type { MessageHit, SearchResults, Session } from '@/types'

const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const sessions = useSessionStore()
const workspaces = useWorkspaceStore()
const { appendToDraft } = useDraftBus()

/** 是否显示文件树 */
const showFiles = ref(true)

/** 扁平文件列表 → 目录树（按 / 分组） */
interface FileNode {
  name: string
  path: string
  isDir: boolean
  children: FileNode[]
}
const fileTree = computed<FileNode[]>(() => {
  const files = workspaces.files
  if (files.length === 0) return []
  const root: FileNode = { name: '', path: '', isDir: true, children: [] }
  const map = new Map<string, FileNode>()
  map.set('', root)
  for (const f of files) {
    const parts = f.path.split('/')
    let parent = root
    let acc = ''
    for (let i = 0; i < parts.length; i++) {
      acc = acc ? `${acc}/${parts[i]}` : parts[i]
      let node = map.get(acc)
      if (!node) {
        node = { name: parts[i], path: acc, isDir: i < parts.length - 1 || f.isDir, children: [] }
        map.set(acc, node)
        parent.children.push(node)
      }
      parent = node
    }
  }
  return root.children
})

/** 点击文件：把 @路径 引用追加进输入框（Codex 式 vibe coding） */
function insertFileRef(path: string) {
  appendToDraft(`@${path}`)
  if (route.name !== 'chat') void router.push('/chat')
}

/**
 * 侧边栏宽度拖拽（§4.5）：默认 280px，区间 [220, 400]
 * 拖拽期间禁用文本选中，避免误选列表文字
 */
const width = ref(280)

function onHandleDown(e: MouseEvent) {
  e.preventDefault()
  const startX = e.clientX
  const startW = width.value
  document.body.style.userSelect = 'none'
  const move = (ev: MouseEvent) => {
    width.value = Math.min(400, Math.max(220, startW + ev.clientX - startX))
  }
  const up = () => {
    document.body.style.userSelect = ''
    window.removeEventListener('mousemove', move)
    window.removeEventListener('mouseup', up)
  }
  window.addEventListener('mousemove', move)
  window.addEventListener('mouseup', up)
}

const navItems = computed(() => [
  { name: 'chat', icon: MessageSquare, label: t('nav.chat'), to: '/chat' },
  { name: 'skills', icon: Sparkles, label: t('nav.skills'), to: '/skills' },
  { name: 'settings', icon: Settings, label: t('nav.settings'), to: '/settings' },
])

const visibleSessions = computed(() => sessions.sessions.filter((s) => !s.isArchived))
const archivedSessions = computed(() => sessions.sessions.filter((s) => s.isArchived))

/** 按工作区分组（§4.5 自动分组）：无工作区 / 未分组的会话归入「默认工作区」之后 */
const sessionsByWorkspace = computed(() => {
  const map = new Map<string, Session[]>()
  for (const s of visibleSessions.value) {
    const key = s.workspaceId ?? 'default'
    if (!map.has(key)) map.set(key, [])
    map.get(key)!.push(s)
  }
  return map
})

const sessionGroups = computed(() =>
  workspaces.workspaces
    .map((w) => ({
      id: w.id,
      name: w.name,
      sessions: sessionsByWorkspace.value.get(w.id) ?? [],
    }))
    .filter((g) => g.sessions.length > 0),
)

function newChat() {
  // 延迟创建：先回到空的输入态，首条消息发出时才真正建会话（避免产生大量空会话）
  sessions.clearCurrent()
  void router.push('/chat')
}

function openSession(id: string) {
  void sessions.open(id).then(() => {
    // 会话与工作区绑定：打开会话时同步工作区上下文（门禁语义）
    const s = sessions.sessions.find((x) => x.id === id)
    if (s?.workspaceId) workspaces.persistCurrent(s.workspaceId)
  })
  if (route.name !== 'chat') void router.push('/chat')
}

function removeSession(id: string) {
  if (window.confirm(t('sidebar.deleteConfirm'))) {
    void sessions.remove(id)
  }
}

function archiveSession(id: string, archived: boolean) {
  void sessions.toggleArchive(id, archived)
}

function removeWorkspace(id: string) {
  if (id === 'default') return
  if (window.confirm(t('sidebar.workspaceDeleteConfirm'))) {
    void workspaces.remove(id)
  }
}

// ---------- 全局搜索（§4.5.3：300ms 防抖，标题 + 消息内容） ----------

const query = ref('')
const searching = ref(false)
const results = ref<SearchResults | null>(null)
let debounceTimer: number | undefined

watch(query, (q) => {
  window.clearTimeout(debounceTimer)
  const text = q.trim()
  if (!text) {
    results.value = null
    searching.value = false
    return
  }
  searching.value = true
  debounceTimer = window.setTimeout(() => {
    void sessions
      .search(text)
      .then((r) => (results.value = r))
      .catch(() => (results.value = null))
      .finally(() => (searching.value = false))
  }, 300)
})

/** 点击消息命中项：打开所属会话并滚动定位到该消息 */
function openMessageHit(hit: MessageHit) {
  void sessions.open(hit.message.sessionId)
  sessions.scrollToMessageId = hit.message.id
  query.value = ''
  results.value = null
  if (route.name !== 'chat') void router.push('/chat')
}

function clearSearch() {
  query.value = ''
  results.value = null
}

/** 会话时间：当天显示 HH:mm，跨天显示 MM-DD */
function fmtTime(ts: number): string {
  const d = new Date(ts)
  const now = new Date()
  const sameDay =
    d.getFullYear() === now.getFullYear() &&
    d.getMonth() === now.getMonth() &&
    d.getDate() === now.getDate()
  const pad = (n: number) => String(n).padStart(2, '0')
  return sameDay
    ? `${pad(d.getHours())}:${pad(d.getMinutes())}`
    : `${pad(d.getMonth() + 1)}-${pad(d.getDate())}`
}
</script>

<template>
  <aside
    :style="{ width: `${width}px` }"
    class="glass relative flex h-full shrink-0 flex-col border-r"
  >
    <!-- 头部：品牌 -->
    <div class="flex items-center gap-2 px-4 pb-2 pt-4">
      <img
        src="@/assets/icon.png"
        alt="Koid"
        class="size-7 shrink-0 rounded-lg object-cover"
      />
      <span class="text-base font-semibold tracking-tight">{{ t('common.appName') }}</span>
    </div>

    <!-- 新建会话 -->
    <div class="px-3 py-2">
      <Button variant="outline" class="w-full justify-start gap-2" @click="newChat">
        <MessageSquarePlus class="size-4" />
        {{ t('nav.newChat') }}
      </Button>
    </div>

    <!-- 全局搜索框（§4.5.3） -->
    <div class="px-3 pb-2">
      <div class="relative">
        <Search class="absolute left-2.5 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" />
        <input
          v-model="query"
          :placeholder="t('sidebar.searchPlaceholder')"
          class="h-8 w-full rounded-md border border-input bg-background/50 pl-8 pr-2 text-xs outline-none focus:ring-1 focus:ring-ring"
        />
      </div>
    </div>

    <!-- 工作区文件树（Codex 式：模型读取工作区文件） -->
    <div v-if="workspaces.current?.path" class="border-b px-3 pb-2">
      <button
        class="flex w-full items-center gap-1.5 rounded px-1 py-1 text-[10px] font-medium uppercase text-muted-foreground hover:bg-secondary/50"
        @click="showFiles = !showFiles"
      >
        <ChevronDown v-if="showFiles" class="size-3" />
        <ChevronRight v-else class="size-3" />
        {{ t('sidebar.workspaceFiles') }}
      </button>
      <div v-if="showFiles" class="scrollbar-thin max-h-52 overflow-y-auto">
        <p
          v-if="workspaces.files.length === 0"
          class="px-1 py-1 text-[10px] text-muted-foreground/60"
        >
          {{ t('sidebar.noWorkspaceFiles') }}
        </p>
        <WorkspaceFileTree :nodes="fileTree" @pick="insertFileRef" />
      </div>
    </div>

    <!-- 搜索结果模式 -->
    <div v-if="query.trim()" class="scrollbar-thin flex-1 overflow-y-auto px-3 py-1">
      <div v-if="searching" class="flex items-center gap-2 px-2 py-3 text-xs text-muted-foreground">
        <Loader2 class="size-3.5 animate-spin" />
        {{ t('common.loading') }}
      </div>
      <template v-else-if="results">
        <p
          v-if="results.sessions.length === 0 && results.messages.length === 0"
          class="px-2 py-3 text-xs text-muted-foreground"
        >
          {{ t('sidebar.noResults') }}
        </p>

        <template v-if="results.sessions.length">
          <p class="px-2 pb-1 pt-2 text-[10px] font-medium uppercase text-muted-foreground">
            {{ t('nav.chat') }}
          </p>
          <div
            v-for="s in results.sessions"
            :key="s.id"
            class="mb-0.5 cursor-pointer truncate rounded-md px-2.5 py-2 text-sm hover:bg-secondary/60"
            @click="
              () => {
                openSession(s.id)
                clearSearch()
              }
            "
          >
            {{ s.title }}
          </div>
        </template>

        <template v-if="results.messages.length">
          <p class="px-2 pb-1 pt-2 text-[10px] font-medium uppercase text-muted-foreground">
            {{ t('sidebar.messagesSection') }}
          </p>
          <div
            v-for="hit in results.messages"
            :key="hit.message.id"
            class="mb-0.5 cursor-pointer rounded-md px-2.5 py-2 hover:bg-secondary/60"
            @click="openMessageHit(hit)"
          >
            <p class="truncate text-[10px] text-muted-foreground">{{ hit.sessionTitle }}</p>
            <!-- 安全 HTML：highlightSnippet 内已转义原文 -->
            <p class="line-clamp-2 text-xs" v-html="highlightSnippet(hit.message.content, query.trim())" />
          </div>
        </template>
      </template>
    </div>

    <!-- 会话列表模式（按工作区自动分组，§4.5） -->
    <div v-else class="scrollbar-thin flex-1 overflow-y-auto px-3 py-1">
      <p
        v-if="visibleSessions.length === 0 && archivedSessions.length === 0"
        class="px-2 py-2 text-xs text-muted-foreground"
      >
        {{ t('sidebar.noSessions') }}
      </p>

      <!-- 按工作区分组的会话 -->
      <section v-for="g in sessionGroups" :key="g.id" class="mb-1">
        <div class="flex items-center gap-1 px-2 pb-0.5 pt-1.5">
          <span class="truncate text-[10px] font-medium uppercase text-muted-foreground">
            {{ g.name }}
          </span>
          <span class="text-[10px] text-muted-foreground/50">{{ g.sessions.length }}</span>
          <button
            v-if="g.id !== 'default'"
            class="ml-auto shrink-0 rounded p-0.5 text-muted-foreground/50 opacity-0 transition-opacity hover:text-destructive group-hover:opacity-100"
            :title="t('common.delete')"
            @click="removeWorkspace(g.id)"
          >
            <Trash2 class="size-3" />
          </button>
        </div>

        <div
          v-for="s in g.sessions"
          :key="s.id"
          class="group relative mb-0.5 flex cursor-pointer items-center gap-2 rounded-md px-2.5 py-2 text-sm transition-colors"
          :class="cn(s.id === sessions.currentId ? 'bg-secondary' : 'hover:bg-secondary/60')"
          @click="openSession(s.id)"
        >
          <!-- 当前会话高亮竖线（§4.5 UI 规范） -->
          <span
            v-if="s.id === sessions.currentId"
            class="absolute inset-y-1 left-0 w-[3px] rounded-full bg-primary"
          />
          <span class="min-w-0 flex-1 truncate">{{ s.title }}</span>
          <span class="shrink-0 text-[10px] text-muted-foreground">{{ fmtTime(s.updatedAt) }}</span>
          <button
            class="shrink-0 rounded p-0.5 text-muted-foreground opacity-0 transition-opacity hover:text-foreground group-hover:opacity-100"
            :title="t('sidebar.archive')"
            @click.stop="archiveSession(s.id, true)"
          >
            <Archive class="size-3.5" />
          </button>
          <button
            class="shrink-0 rounded p-0.5 text-muted-foreground opacity-0 transition-opacity hover:text-destructive group-hover:opacity-100"
            :title="t('common.delete')"
            @click.stop="removeSession(s.id)"
          >
            <Trash2 class="size-3.5" />
          </button>
        </div>
      </section>

      <!-- 归档区（§4.5：归档后可还原） -->
      <template v-if="archivedSessions.length">
        <button
          class="mt-2 flex w-full items-center gap-1.5 rounded px-2 py-1 text-[10px] font-medium uppercase text-muted-foreground hover:bg-secondary/50"
          @click="sessions.showArchived = !sessions.showArchived"
        >
          <Archive class="size-3" />
          {{ t('sidebar.archivedSection') }} ({{ archivedSessions.length }})
        </button>
        <template v-if="sessions.showArchived">
          <div
            v-for="s in archivedSessions"
            :key="s.id"
            class="group mb-0.5 flex cursor-pointer items-center gap-2 rounded-md px-2.5 py-1.5 text-sm text-muted-foreground hover:bg-secondary/60"
            @click="openSession(s.id)"
          >
            <span class="min-w-0 flex-1 truncate">{{ s.title }}</span>
            <button
              class="shrink-0 rounded p-0.5 opacity-0 transition-opacity hover:text-foreground group-hover:opacity-100"
              :title="t('sidebar.restore')"
              @click.stop="archiveSession(s.id, false)"
            >
              <ArchiveRestore class="size-3.5" />
            </button>
            <button
              class="shrink-0 rounded p-0.5 opacity-0 transition-opacity hover:text-destructive group-hover:opacity-100"
              :title="t('common.delete')"
              @click.stop="removeSession(s.id)"
            >
              <Trash2 class="size-3.5" />
            </button>
          </div>
        </template>
      </template>
    </div>

    <Separator />

    <!-- 底部导航 -->
    <nav class="flex items-center gap-1 p-3">
      <Button
        v-for="item in navItems"
        :key="item.name"
        :variant="route.name === item.name ? 'secondary' : 'ghost'"
        size="icon"
        :title="item.label"
        @click="router.push(item.to)"
      >
        <component :is="item.icon" class="size-4" />
      </Button>
    </nav>

    <!-- 宽度拖拽手柄 -->
    <div
      class="absolute inset-y-0 right-0 w-1 cursor-col-resize transition-colors hover:bg-primary/40"
      @mousedown="onHandleDown"
    />
  </aside>
</template>
