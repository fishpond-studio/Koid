<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue-sonner'
import { Check, FolderOpen, FolderPlus, HardDrive, Trash2 } from 'lucide-vue-next'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { toApiError } from '@/lib/api'
import { useSessionStore } from '@/stores/session'
import { useWorkspaceStore } from '@/stores/workspace'

/**
 * 工作区选择器（对齐 dsh 的 WorkspacePickFlow）：
 * - 列表展示全部工作区，点击切换并 connect（落到该工作区的空会话）
 * - 「选择目录并新建工作区」：唯一新增路径 = 挑一个目录（新的或已有的），幂等复用
 * - 没有可用（已绑定路径）工作区时：打开选择器直接进目录选择，不弹空菜单（dsh addIsTheOnlyEntry）
 * - 采纳目录失败 → 错误对话框 +「重新选择」（dsh folder-error modal）
 * 触发按钮由插槽提供（英雄区大按钮 / 输入区 chip）。
 * 传 `open`/`v-model:open` 时受控；不传则内部自管理（供多个实例复用）。
 */
const props = defineProps<{ open?: boolean }>()
const emit = defineEmits<{ 'update:open': [value: boolean] }>()

const { t } = useI18n()
const workspaces = useWorkspaceStore()
const sessions = useSessionStore()

const internalOpen = ref(false)
const openModel = computed({
  get: () => props.open ?? internalOpen.value,
  set: (v: boolean) => {
    if (props.open !== undefined) {
      emit('update:open', v)
    } else {
      internalOpen.value = v
    }
  },
})

/** 是否有可用的（已绑定路径）工作区；没有时「打开」即等于「添加」 */
const hasUsable = computed(() => workspaces.workspaces.some((w) => w.path))

// dsh addIsTheOnlyEntry：无可用工作区时锚点手势就是添加动作本身
watch(openModel, (open) => {
  if (open && !hasUsable.value) {
    openModel.value = false
    void addFromDirectory()
  }
})

/** 采纳目录失败的错误对话框（dsh folder-error modal + Choose again） */
const errorOpen = ref(false)
const errorMessage = ref('')
const retryAction = ref<(() => void) | null>(null)

function fail(e: unknown) {
  errorMessage.value = toApiError(e).message
  errorOpen.value = true
}

function closeError() {
  errorOpen.value = false
  retryAction.value = null
}

function retry() {
  const action = retryAction.value
  closeError()
  action?.()
}

/** 原生目录选择对话框 */
async function openDirectory(): Promise<string | null> {
  const { open } = await import('@tauri-apps/plugin-dialog')
  const picked = await open({
    directory: true,
    multiple: false,
    title: t('chat.workspaceAddFromDir'),
  })
  return picked ? String(picked) : null
}

function basename(p: string): string {
  const trimmed = p.replace(/[\\/]+$/, '')
  const parts = trimmed.split(/[\\/]/).filter(Boolean)
  return parts[parts.length - 1] ?? trimmed
}

/** 切换到指定工作区并 connect（对齐 dsh selectWorkspace：清空不属于它的会话，落到该工作区空态） */
function select(id: string) {
  workspaces.persistCurrent(id)
  void workspaces.loadFiles()
  const cur = sessions.current
  if (cur?.workspaceId && cur.workspaceId !== id) {
    sessions.clearCurrent()
  }
}

/** 从目录新建工作区（幂等：同一路径复用已有工作区） */
async function addFromDirectory() {
  const path = await openDirectory()
  if (!path) return
  try {
    const saved = await workspaces.save({ name: basename(path), path })
    workspaces.persistCurrent(saved.id)
    await workspaces.loadFiles()
    sessions.clearCurrent()
    toast.success(t('chat.workspaceAddDone', { name: saved.name }))
  } catch (e) {
    retryAction.value = () => void addFromDirectory()
    fail(e)
  }
}

/** 为当前工作区绑定项目路径 */
async function bindPath() {
  const ws = workspaces.current
  if (!ws) return
  const path = await openDirectory()
  if (!path) return
  try {
    await workspaces.save({ id: ws.id, name: ws.name, path })
    await workspaces.loadFiles()
    toast.success(t('chat.pickPathDone'))
  } catch (e) {
    retryAction.value = () => void bindPath()
    fail(e)
  }
}

/** 删除当前工作区（默认工作区不可删；确认后其会话归入默认工作区） */
function removeCurrent() {
  const ws = workspaces.current
  if (!ws || ws.id === 'default') return
  if (!window.confirm(t('sidebar.workspaceDeleteConfirm'))) return
  void workspaces
    .remove(ws.id)
    .then(() => toast.success(t('common.deleted')))
    .catch((e) => toast.error(toApiError(e).message))
}

function shortPath(p: string | null): string | null {
  if (!p) return null
  const parts = p.replaceAll('\\', '/').split('/').filter(Boolean)
  return parts.length > 3 ? '…/' + parts.slice(-3).join('/') : p
}
</script>

<template>
  <DropdownMenu v-model:open="openModel">
    <DropdownMenuTrigger as-child>
      <slot />
    </DropdownMenuTrigger>
    <DropdownMenuContent class="w-80" align="start">
      <DropdownMenuLabel>{{ t('chat.workspacePickerTitle') }}</DropdownMenuLabel>
      <template v-if="workspaces.workspaces.length">
        <DropdownMenuItem
          v-for="w in workspaces.workspaces"
          :key="w.id"
          class="flex items-start gap-2 py-2"
          @select="select(w.id)"
        >
          <HardDrive class="mt-0.5 size-3.5 shrink-0 text-muted-foreground" />
          <span class="min-w-0 flex-1">
            <span class="flex items-center gap-1.5">
              <span class="truncate text-sm font-medium">{{ w.name }}</span>
              <Check
                v-if="w.id === workspaces.currentId"
                class="size-3.5 shrink-0 text-primary"
              />
            </span>
            <span
              class="mt-0.5 block truncate font-mono text-[10px]"
              :class="w.path ? 'text-muted-foreground' : 'text-muted-foreground/50'"
            >
              {{ shortPath(w.path) ?? t('chat.workspaceNoPath') }}
            </span>
          </span>
        </DropdownMenuItem>
      </template>
      <p v-else class="px-2 py-1.5 text-xs text-muted-foreground">
        {{ t('chat.workspaceNoneHint') }}
      </p>
      <DropdownMenuSeparator />
      <DropdownMenuItem @select="() => void addFromDirectory()">
        <FolderPlus class="size-4" />
        <span>{{ t('chat.workspaceAddFromDir') }}</span>
      </DropdownMenuItem>
      <DropdownMenuItem
        v-if="workspaces.current && !workspaces.current.path"
        @select="() => void bindPath()"
      >
        <FolderOpen class="size-4" />
        <span>{{ t('chat.workspaceBindPath') }}</span>
      </DropdownMenuItem>
      <DropdownMenuItem
        v-if="workspaces.current && workspaces.current.id !== 'default'"
        class="text-destructive focus:text-destructive"
        @select="removeCurrent"
      >
        <Trash2 class="size-4" />
        <span>{{ t('sidebar.deleteWorkspace') }}</span>
      </DropdownMenuItem>
    </DropdownMenuContent>
  </DropdownMenu>

  <!-- 采纳目录失败错误框（dsh folder-error modal） -->
  <Dialog v-model:open="errorOpen">
    <DialogContent class="sm:max-w-md">
      <DialogHeader>
        <DialogTitle>{{ t('chat.workspaceErrorTitle') }}</DialogTitle>
        <DialogDescription class="break-words">{{ errorMessage }}</DialogDescription>
      </DialogHeader>
      <DialogFooter>
        <Button variant="outline" @click="closeError">
          {{ t('common.cancel') }}
        </Button>
        <Button @click="retry">
          {{ t('chat.workspaceErrorRetry') }}
        </Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>
