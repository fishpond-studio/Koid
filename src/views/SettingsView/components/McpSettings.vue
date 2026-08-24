<script setup lang="ts">
import { onMounted, reactive, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue-sonner'
import { Loader2, Pencil, Play, Plus, Power, Trash2 } from 'lucide-vue-next'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { mcpApi, toApiError } from '@/lib/api'
import type { McpServer, McpServerInput, McpTool } from '@/types'

const { t } = useI18n()

const servers = ref<McpServer[]>([])
const loading = ref(false)

const dialogOpen = ref(false)
const editing = ref<McpServer | null>(null)
const form = reactive<McpServerInput>({
  name: '',
  transport: 'stdio',
  command: '',
  args: [],
  env: null,
  url: null,
})
const argsText = ref('')

const testing = ref<{ serverId: string; tool: string } | null>(null)
const toolArgs = ref('')
const toolResult = ref<{ tool: string; output: string } | null>(null)
const toolDialogOpen = ref(false)

onMounted(() => void load())

async function load() {
  loading.value = true
  try {
    servers.value = await mcpApi.list()
  } catch (e) {
    toast.error(toApiError(e).message)
  } finally {
    loading.value = false
  }
}

async function openAdd() {
  editing.value = null
  form.name = ''
  form.command = ''
  form.args = []
  form.env = null
  argsText.value = ''
  dialogOpen.value = true
}

async function openEdit(s: McpServer) {
  editing.value = s
  form.name = s.name
  form.command = s.command ?? ''
  form.args = s.args ?? []
  form.env = s.env ?? null
  argsText.value = (s.args ?? []).join(' ')
  dialogOpen.value = true
}

async function save() {
  try {
    const input: McpServerInput = {
      id: editing.value?.id ?? null,
      name: form.name,
      transport: 'stdio',
      command: form.command || null,
      args: argsText.value.split(/\s+/).filter(Boolean),
      env: form.env,
      url: null,
    }
    await mcpApi.save(input)
    toast.success(t('common.saved'))
    dialogOpen.value = false
    await load()
  } catch (e) {
    toast.error(toApiError(e).message)
  }
}

async function remove(s: McpServer) {
  if (!window.confirm(t('settings.mcp.deleteConfirm', { name: s.name }))) return
  try {
    await mcpApi.remove(s.id)
    await load()
  } catch (e) {
    toast.error(toApiError(e).message)
  }
}

async function toggle(s: McpServer) {
  try {
    if (s.status === 'connected') {
      await mcpApi.disconnect(s.id)
    } else {
      const tools = await mcpApi.connect(s.id)
      toast.success(
        t('settings.mcp.connected', { name: s.name, count: tools.length }),
      )
    }
    await load()
  } catch (e) {
    toast.error(toApiError(e).message)
  }
}

/** 工具测试调用（§4.8 手动调用） */
function openToolTest(s: McpServer, tool: McpTool) {
  toolArgs.value = '{}'
  toolResult.value = null
  testing.value = { serverId: s.id, tool: tool.name }
  toolDialogOpen.value = true
}

async function runToolTest() {
  if (!testing.value) return
  let args: unknown
  try {
    args = JSON.parse(toolArgs.value)
  } catch {
    toast.error(t('settings.mcp.argsInvalid'))
    return
  }
  try {
    const output = await mcpApi.callTool(testing.value.serverId, testing.value.tool, args)
    toolResult.value = { tool: testing.value.tool, output }
  } catch (e) {
    toolResult.value = { tool: testing.value.tool, output: toApiError(e).message }
  }
}
</script>

<template>
  <div class="space-y-4 pb-16">
    <div class="flex items-center justify-between">
      <p class="text-sm text-muted-foreground">{{ t('settings.mcp.subtitle') }}</p>
      <Button size="sm" class="gap-1" @click="() => void openAdd()">
        <Plus class="size-4" />
        {{ t('settings.mcp.add') }}
      </Button>
    </div>

    <p v-if="loading" class="flex items-center gap-2 py-8 text-sm text-muted-foreground">
      <Loader2 class="size-4 animate-spin" />
      {{ t('common.loading') }}
    </p>
    <p v-else-if="servers.length === 0" class="py-16 text-center text-sm text-muted-foreground">
      {{ t('settings.mcp.empty') }}
    </p>

    <div v-for="s in servers" :key="s.id" class="rounded-lg border px-4 py-3">
      <div class="flex items-center gap-2">
        <!-- 状态灯 -->
        <span
          class="size-2 shrink-0 rounded-full"
          :class="
            s.status === 'connected'
              ? 'bg-emerald-500'
              : s.status === 'error'
                ? 'bg-destructive'
                : 'bg-muted-foreground/40'
          "
        />
        <span class="font-medium">{{ s.name }}</span>
        <Badge variant="secondary" class="text-[10px] uppercase">{{ s.transport }}</Badge>
        <Badge
          variant="outline"
          class="text-[10px]"
          :class="s.status === 'connected' ? 'text-emerald-600' : 'text-muted-foreground'"
        >
          {{ s.tools.length }} {{ t('settings.mcp.tools') }}
        </Badge>
        <div class="ml-auto flex items-center gap-1">
          <Button
            variant="outline"
            size="sm"
            class="h-7 gap-1 text-xs"
            @click="() => void toggle(s)"
          >
            <Power class="size-3.5" :class="s.status === 'connected' && 'text-emerald-500'" />
            {{ s.status === 'connected' ? t('settings.mcp.disconnect') : t('settings.mcp.connect') }}
          </Button>
          <Button variant="ghost" size="icon" class="size-7" @click="() => void openEdit(s)">
            <Pencil class="size-3.5" />
          </Button>
          <Button
            variant="ghost"
            size="icon"
            class="size-7 hover:text-destructive"
            @click="() => void remove(s)"
          >
            <Trash2 class="size-3.5" />
          </Button>
        </div>
      </div>

      <p v-if="s.command" class="mt-1 truncate font-mono text-xs text-muted-foreground">
        {{ s.command }} {{ (s.args ?? []).join(' ') }}
      </p>
      <p v-if="s.errorMessage" class="mt-1 text-xs text-destructive">{{ s.errorMessage }}</p>

      <!-- 工具列表 -->
      <div v-if="s.tools.length" class="mt-3 grid grid-cols-1 gap-2">
        <div
          v-for="tool in s.tools"
          :key="tool.name"
          class="flex items-start gap-2 rounded-md border bg-muted/20 px-3 py-2"
        >
          <div class="min-w-0 flex-1">
            <p class="font-mono text-xs font-medium">{{ tool.name }}</p>
            <p v-if="tool.description" class="mt-0.5 line-clamp-1 text-[11px] text-muted-foreground">
              {{ tool.description }}
            </p>
          </div>
          <Button
            variant="outline"
            size="sm"
            class="h-6 shrink-0 gap-1 text-xs"
            :disabled="s.status !== 'connected'"
            @click="openToolTest(s, tool)"
          >
            <Play class="size-3" />
            {{ t('settings.mcp.test') }}
          </Button>
        </div>
      </div>
    </div>

    <!-- 服务器编辑 -->
    <Dialog :open="dialogOpen" @update:open="(v: boolean) => (dialogOpen = v)">
      <DialogContent class="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>
            {{ editing ? t('settings.mcp.edit') : t('settings.mcp.add') }}
          </DialogTitle>
        </DialogHeader>
        <div class="space-y-4 py-2">
          <div class="space-y-1.5">
            <Label>{{ t('settings.mcp.name') }}</Label>
            <Input v-model="form.name" />
          </div>
          <div class="space-y-1.5">
            <Label>{{ t('settings.mcp.command') }}</Label>
            <Input
              :model-value="form.command ?? ''"
              placeholder="npx"
              class="font-mono text-xs"
              @update:model-value="(v: string | number) => (form.command = String(v))"
            />
          </div>
          <div class="space-y-1.5">
            <Label>{{ t('settings.mcp.args') }}</Label>
            <Input v-model="argsText" placeholder="-y @modelcontextprotocol/server-filesystem /path" />
          </div>
        </div>
        <DialogFooter>
          <Button variant="outline" @click="dialogOpen = false">
            {{ t('common.cancel') }}
          </Button>
          <Button @click="() => void save()">{{ t('common.save') }}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>

    <!-- 工具调用测试 -->
    <Dialog :open="toolDialogOpen" @update:open="(v: boolean) => (toolDialogOpen = v)">
      <DialogContent class="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>
            {{ t('settings.mcp.callTool') }}
            <span v-if="testing" class="ml-2 font-mono text-xs font-normal text-muted-foreground">
              {{ testing.tool }}
            </span>
          </DialogTitle>
        </DialogHeader>
        <div class="space-y-3 py-2">
          <div class="space-y-1.5">
            <Label>{{ t('settings.mcp.arguments') }} (JSON)</Label>
            <textarea
              v-model="toolArgs"
              rows="4"
              spellcheck="false"
              class="w-full resize-y rounded-md border border-input bg-background px-3 py-2 font-mono text-xs outline-none focus:ring-1 focus:ring-ring"
            />
          </div>
          <pre
            v-if="toolResult"
            class="scrollbar-thin max-h-56 overflow-auto whitespace-pre-wrap rounded-md bg-muted p-3 font-mono text-xs"
          >{{ toolResult.output }}</pre>
        </div>
        <DialogFooter>
          <Button variant="outline" @click="toolDialogOpen = false">
            {{ t('common.cancel') }}
          </Button>
          <Button class="gap-1" @click="() => void runToolTest()">
            <Play class="size-3.5" />
            {{ t('settings.mcp.call') }}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  </div>
</template>
