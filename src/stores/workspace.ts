import { computed, ref, watch } from 'vue'
import { defineStore } from 'pinia'
import { toApiError, workspacesApi } from '@/lib/api'
import type { Workspace, WorkspaceFileEntry, WorkspaceInput } from '@/types'

const CURRENT_KEY = 'koid-current-workspace'

/**
 * 工作区 Store（§4.5 Workspace → Folder → Session 第一层）
 * 会话按 workspaceId 自动分组；新建会话默认归入当前工作区
 * 工作区可绑定本地项目路径（vibe coding），files 为文件树
 */
export const useWorkspaceStore = defineStore('workspace', () => {
  const workspaces = ref<Workspace[]>([])
  const loaded = ref(false)
  /** 当前工作区 id（持久化） */
  const currentId = ref<string>(readCurrent())
  /** 当前工作区文件树 */
  const files = ref<WorkspaceFileEntry[]>([])

  const current = computed<Workspace | null>(
    () => workspaces.value.find((w) => w.id === currentId.value) ?? null,
  )

  function readCurrent(): string {
    try {
      return localStorage.getItem(CURRENT_KEY) ?? 'default'
    } catch {
      return 'default'
    }
  }

  function persistCurrent(id: string) {
    currentId.value = id
    try {
      localStorage.setItem(CURRENT_KEY, id)
    } catch {
      /* 忽略 */
    }
  }

  // 工作区切换 / 路径变化时自动刷新文件树（immediate：冷启动也生效）
  watch([currentId, () => current.value?.path], () => {
    void loadFiles()
  }, { immediate: true })

  async function load(force = false) {
    if (loaded.value && !force) return
    try {
      workspaces.value = await workspacesApi.list()
      loaded.value = true
      // 当前工作区若被删除则回退默认
      if (!workspaces.value.some((w) => w.id === currentId.value)) {
        persistCurrent('default')
      }
      // 工作区加载完成后再加载文件树，保证模型能感知项目内容
      await loadFiles()
    } catch (e) {
      throw toApiError(e)
    }
  }

  /** 加载当前工作区的文件树（无路径则为空） */
  async function loadFiles() {
    const ws = current.value
    if (!ws?.path) {
      files.value = []
      return
    }
    try {
      files.value = await workspacesApi.listFiles(ws.id)
    } catch (e) {
      // 路径失效等场景静默，保留旧列表
      void e
    }
  }

  async function save(input: WorkspaceInput): Promise<Workspace> {
    try {
      const saved = await workspacesApi.save(input)
      await load(true)
      return saved
    } catch (e) {
      throw toApiError(e)
    }
  }

  async function remove(id: string) {
    try {
      await workspacesApi.remove(id)
      if (currentId.value === id) persistCurrent('default')
      await load(true)
    } catch (e) {
      throw toApiError(e)
    }
  }

  /**
   * 为当前工作区选择项目路径（原生目录对话框）
   * 返回是否成功绑定；用于「新建对话 → 引导选路径」流程
   */
  async function pickPathForCurrent(): Promise<boolean> {
    const ws = current.value
    if (!ws) return false
    const { open } = await import('@tauri-apps/plugin-dialog')
    const picked = await open({
      directory: true,
      multiple: false,
      title: '选择项目路径',
    })
    if (!picked) return false
    const path = String(picked)
    try {
      await save({ id: ws.id, name: ws.name, path })
      await loadFiles()
      return true
    } catch (e) {
      throw toApiError(e)
    }
  }

  return {
    workspaces,
    loaded,
    currentId,
    current,
    files,
    load,
    loadFiles,
    save,
    remove,
    persistCurrent,
    pickPathForCurrent,
  }
})
