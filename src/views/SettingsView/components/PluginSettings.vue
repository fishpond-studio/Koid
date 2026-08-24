<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue-sonner'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { Boxes, Download, FolderOpen, Plug, RefreshCw, Trash2 } from 'lucide-vue-next'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { chatApi, pluginsApi, settingsApi, toApiError } from '@/lib/api'
import { useModelStore } from '@/stores/model'
import { usePluginStore } from '@/stores/plugins'
import { cn } from '@/lib/utils'
import type { ChatRequest, PluginInfo } from '@/types'

/**
 * 插件管理 + 沙箱预览（§4.9）
 * - 插件 HTML 由 Rust 读取后经 iframe.srcdoc 注入（规避 file:// 限制）
 * - iframe sandbox="allow-scripts"：无 same-origin，与主应用 DOM 完全隔离
 * - postMessage 桥：notify / llm.chat / storage / file / command / network
 * - 权限门控：manifest 声明 permissions 时逐方法校验（§4.9 权限申请）
 */
const { t } = useI18n()
const models = useModelStore()
const plugins = usePluginStore()

const pluginsList = ref<PluginInfo[]>([])
const selectedId = ref<string | null>(null)
const html = ref('')
const bridgeLog = ref<string[]>([])
const loadingHtml = ref(false)

const PLUGIN_STORAGE_PREFIX = 'plugin:'
const PLUGIN_FRAME_ID = 'plugin-frame'

const selected = computed<PluginInfo | null>(
  () => pluginsList.value.find((p) => p.id === selectedId.value) ?? null,
)

const installUrl = ref('')
const installing = ref(false)

async function load() {
  try {
    pluginsList.value = await pluginsApi.list()
    if (pluginsList.value.length && !selectedId.value) {
      selectedId.value = pluginsList.value[0].id
    }
  } catch (e) {
    toast.error(toApiError(e).message)
  }
}

async function openPlugin(id: string) {
  selectedId.value = id
  bridgeLog.value = []
  loadingHtml.value = true
  // 清理上一插件注册的命令
  plugins.commands.forEach((c) => plugins.unregisterPlugin(c.pluginId))
  try {
    html.value = await pluginsApi.html(id)
  } catch (e) {
    html.value = `<p style="font-family:sans-serif">${toApiError(e).message}</p>`
  } finally {
    loadingHtml.value = false
  }
}

watch(selectedId, (id) => {
  if (id) void openPlugin(id)
})

async function remove(p: PluginInfo) {
  if (!window.confirm(t('settings.plugins.deleteConfirm', { name: p.name }))) return
  try {
    await pluginsApi.remove(p.id)
    plugins.unregisterPlugin(p.id)
    if (selectedId.value === p.id) {
      selectedId.value = null
      html.value = ''
    }
    await load()
  } catch (e) {
    toast.error(toApiError(e).message)
  }
}

// ---------- 安装（市场：本地 zip / 远程 URL，§4.9 生态） ----------

async function installLocal() {
  const picked = await openDialog({
    multiple: false,
    filters: [{ name: 'Plugin zip', extensions: ['zip'] }],
  })
  if (!picked) return
  installing.value = true
  try {
    const info = await pluginsApi.installFromPath(String(picked))
    toast.success(t('settings.plugins.installed', { name: info.name }))
    selectedId.value = info.id
    await load()
  } catch (e) {
    toast.error(toApiError(e).message)
  } finally {
    installing.value = false
  }
}

async function installRemote() {
  const url = installUrl.value.trim()
  if (!url) return
  installing.value = true
  try {
    const info = await pluginsApi.installFromUrl(url)
    toast.success(t('settings.plugins.installed', { name: info.name }))
    installUrl.value = ''
    selectedId.value = info.id
    await load()
  } catch (e) {
    toast.error(toApiError(e).message)
  } finally {
    installing.value = false
  }
}

// ---------- postMessage 桥 ----------

/** 方法 → 所需权限 scope（manifest.permissions 为空 = 放行全部，兼容旧插件） */
const PERMISSION_MAP: Record<string, string> = {
  'koid.ui.notify': 'notify',
  'koid.llm.chat': 'llm',
  'koid.storage.get': 'storage',
  'koid.storage.set': 'storage',
  'koid.file.read': 'file',
  'koid.file.write': 'file',
  'koid.command.register': 'command',
  'koid.network.fetch': 'network',
}

function checkPermission(method: string): boolean {
  const perms = selected.value?.permissions ?? []
  if (perms.length === 0) return true // 未声明权限 = 全部放行（兼容）
  const need = PERMISSION_MAP[method]
  if (!need) return false
  return perms.some((p) => p === need || p === `koid.${need}` || p === `koid.${need}.*`)
}

function post(iframe: HTMLIFrameElement, id: number, result: unknown, error?: string) {
  iframe.contentWindow?.postMessage({ koidResult: { id, result, error } }, '*')
}

/** 命令面板执行回调：把执行请求回发给插件 iframe */
function pluginExecutor(_pluginId: string, commandId: string) {
  const iframe = document.getElementById(PLUGIN_FRAME_ID) as HTMLIFrameElement | null
  if (iframe?.contentWindow) {
    iframe.contentWindow.postMessage({ koidExecute: { commandId } }, '*')
  }
}

async function handleCall(msg: { id: number; method: string; params: unknown }, iframe: HTMLIFrameElement) {
  const pluginId = selectedId.value ?? ''
  const pluginName = selected.value?.name ?? pluginId
  const log = (m: string) => {
    bridgeLog.value.push(m)
  }

  if (!checkPermission(msg.method)) {
    post(iframe, msg.id, null, `权限不足: ${msg.method}`)
    log(`denied: ${msg.method}`)
    return
  }

  if (msg.method === 'koid.ui.notify') {
    const text = String((msg.params as { message?: string } | undefined)?.message ?? '')
    toast.info(text)
    log(`notify(${text.slice(0, 30)})`)
    post(iframe, msg.id, { ok: true })
    return
  }

  if (msg.method === 'koid.llm.chat') {
    const prompt = String((msg.params as { prompt?: string } | undefined)?.prompt ?? '')
    const model = models.enabledModels[0]
    if (!model) {
      post(iframe, msg.id, null, t('settings.plugins.noModel'))
      return
    }
    const request: ChatRequest = {
      requestId: `plugin-${Date.now()}`,
      providerId: model.providerId,
      modelId: model.modelId,
      messages: [{ role: 'user', content: prompt }],
      system: null,
      stream: false,
    }
    log(`llm.chat(${prompt.slice(0, 40)}…)`)
    try {
      const resp = await chatApi.chat(request)
      post(iframe, msg.id, resp.content)
    } catch (e) {
      post(iframe, msg.id, null, toApiError(e).message)
    }
    return
  }

  if (msg.method === 'koid.storage.get' || msg.method === 'koid.storage.set') {
    const p = msg.params as { key?: string; value?: string } | undefined
    const fullKey = PLUGIN_STORAGE_PREFIX + pluginId + ':' + (p?.key ?? '')
    try {
      if (msg.method === 'koid.storage.get') {
        const value = await settingsApi.get(fullKey)
        post(iframe, msg.id, value)
      } else {
        await settingsApi.set(fullKey, String(p?.value ?? ''))
        post(iframe, msg.id, { ok: true })
      }
    } catch (e) {
      post(iframe, msg.id, null, toApiError(e).message)
    }
    return
  }

  if (msg.method === 'koid.file.read' || msg.method === 'koid.file.write') {
    const p = msg.params as { path?: string; content?: string } | undefined
    try {
      if (msg.method === 'koid.file.read') {
        const content = await pluginsApi.fileRead(pluginId, p?.path ?? '')
        post(iframe, msg.id, content)
      } else {
        await pluginsApi.fileWrite(pluginId, p?.path ?? '', String(p?.content ?? ''))
        post(iframe, msg.id, { ok: true })
      }
    } catch (e) {
      post(iframe, msg.id, null, toApiError(e).message)
    }
    return
  }

  if (msg.method === 'koid.command.register') {
    const p = msg.params as { commandId?: string; title?: string } | undefined
    const commandId = String(p?.commandId ?? '')
    const title = String(p?.title ?? commandId)
    if (!commandId) {
      post(iframe, msg.id, null, 'commandId 必填')
      return
    }
    plugins.registerCommand(pluginId, commandId, title, pluginName)
    log(`command.register(${commandId})`)
    post(iframe, msg.id, { ok: true })
    return
  }

  if (msg.method === 'koid.network.fetch') {
    const p = msg.params as
      | { url?: string; method?: string; headers?: Record<string, string>; body?: string }
      | undefined
    log(`fetch(${String(p?.url ?? '').slice(0, 40)})`)
    try {
      const result = await pluginsApi.fetch(
        String(p?.url ?? ''),
        String(p?.method ?? 'GET'),
        p?.headers,
        p?.body,
      )
      post(iframe, msg.id, result)
    } catch (e) {
      post(iframe, msg.id, null, toApiError(e).message)
    }
    return
  }

  post(iframe, msg.id, null, `不支持的方法: ${msg.method}`)
}

function onMessage(e: MessageEvent) {
  const data = e.data as { koidCall?: { id: number; method: string; params: unknown } } | undefined
  const call = data?.koidCall
  if (!call) return
  const iframe = document.getElementById(PLUGIN_FRAME_ID) as HTMLIFrameElement | null
  if (!iframe || e.source !== iframe.contentWindow) return
  void handleCall(call, iframe)
}

onMounted(() => {
  window.addEventListener('message', onMessage)
  plugins.setExecutor(pluginExecutor)
  void load()
})

onBeforeUnmount(() => {
  window.removeEventListener('message', onMessage)
  plugins.setExecutor(null)
  // 离开页面时清理所有插件命令
  plugins.commands.forEach((c) => plugins.unregisterPlugin(c.pluginId))
})
</script>

<template>
  <div class="space-y-4 pb-16">
    <!-- 安装区（市场） -->
    <div class="rounded-lg border p-3">
      <div class="flex items-center gap-2">
        <Download class="size-4 shrink-0 text-muted-foreground" />
        <Input
          v-model="installUrl"
          :placeholder="t('settings.plugins.installUrlPlaceholder')"
          class="h-8 font-mono text-xs"
        />
        <Button
          variant="outline"
          size="sm"
          class="h-8 shrink-0 gap-1"
          :disabled="installing || !installUrl.trim()"
          @click="() => void installRemote()"
        >
          <Download class="size-3.5" />
          {{ t('settings.plugins.installUrl') }}
        </Button>
        <Button
          variant="secondary"
          size="sm"
          class="h-8 shrink-0 gap-1"
          :disabled="installing"
          @click="() => void installLocal()"
        >
          <FolderOpen class="size-3.5" />
          {{ t('settings.plugins.installLocal') }}
        </Button>
      </div>
      <p class="mt-1.5 text-xs text-muted-foreground">{{ t('settings.plugins.installHint') }}</p>
    </div>

    <div class="flex items-center justify-between">
      <p class="text-sm text-muted-foreground">{{ t('settings.plugins.subtitle') }}</p>
      <Button size="sm" variant="outline" class="gap-1" @click="() => void load()">
        <RefreshCw class="size-3.5" />
        {{ t('settings.plugins.refresh') }}
      </Button>
    </div>

    <p
      v-if="pluginsList.length === 0"
      class="flex flex-col items-center gap-2 py-16 text-sm text-muted-foreground"
    >
      <Plug class="size-8" />
      {{ t('settings.plugins.empty') }}
    </p>

    <template v-else>
      <!-- 插件列表 -->
      <div class="flex flex-wrap gap-2">
        <button
          v-for="p in pluginsList"
          :key="p.id"
          class="group flex items-center gap-2 rounded-lg border px-3 py-2 text-sm transition-colors"
          :class="cn(selectedId === p.id ? 'border-primary bg-primary/10' : 'hover:bg-secondary/60')"
          @click="selectedId = p.id"
        >
          <Boxes class="size-4 text-muted-foreground" />
          <span class="font-medium">{{ p.name }}</span>
          <Badge variant="secondary" class="text-[10px]">v{{ p.version }}</Badge>
          <button
            class="ml-1 rounded p-0.5 text-muted-foreground opacity-0 transition-opacity hover:text-destructive group-hover:opacity-100"
            :title="t('common.delete')"
            @click.stop="() => void remove(p)"
          >
            <Trash2 class="size-3.5" />
          </button>
        </button>
      </div>

      <!-- 权限声明 -->
      <div v-if="selected" class="text-xs text-muted-foreground">
        {{ t('settings.plugins.permissions') }}:
        <Badge v-for="perm in selected.permissions" :key="perm" variant="outline" class="ml-1 font-mono text-[10px]">
          {{ perm }}
        </Badge>
      </div>

      <!-- 沙箱预览 -->
      <div class="overflow-hidden rounded-xl border">
        <div class="flex items-center justify-between border-b bg-muted/30 px-3 py-1.5">
          <span class="text-xs font-medium">{{ t('settings.plugins.preview') }}</span>
          <span class="font-mono text-[10px] text-muted-foreground">{{ selected?.entry }}</span>
        </div>
        <iframe
          :id="PLUGIN_FRAME_ID"
          v-if="!loadingHtml && html"
          :srcdoc="html"
          sandbox="allow-scripts"
          class="h-[480px] w-full border-0 bg-white"
          :title="selected?.name"
        />
        <div v-else-if="loadingHtml" class="flex h-[480px] items-center justify-center text-sm text-muted-foreground">
          {{ t('common.loading') }}
        </div>
      </div>

      <!-- 桥接日志 -->
      <div class="rounded-lg border">
        <p class="border-b px-3 py-1.5 text-xs font-medium">{{ t('settings.plugins.bridgeLog') }}</p>
        <div v-if="bridgeLog.length === 0" class="px-3 py-3 text-xs text-muted-foreground">
          {{ t('settings.plugins.noLog') }}
        </div>
        <ul v-else class="scrollbar-thin max-h-32 overflow-y-auto px-3 py-2 font-mono text-[11px]">
          <li v-for="(line, i) in bridgeLog" :key="i" class="py-0.5 text-muted-foreground">
            {{ line }}
          </li>
        </ul>
      </div>
    </template>
  </div>
</template>
