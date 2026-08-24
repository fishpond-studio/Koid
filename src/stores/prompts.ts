import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import { promptsApi, toApiError } from '@/lib/api'
import type { Prompt, PromptInput, PromptVersion } from '@/types'

/**
 * 提示词库 Store（§4.6）
 * builtin: 前缀为内置模板（不可删除），前端据此禁用删除按钮
 */
export const usePromptStore = defineStore('prompt', () => {
  const prompts = ref<Prompt[]>([])
  const loaded = ref(false)

  const snippets = computed(() => prompts.value.filter((p) => p.type === 'snippet'))
  const templates = computed(() => prompts.value.filter((p) => p.type === 'template'))

  function isBuiltin(id: string): boolean {
    return id.startsWith('builtin:')
  }

  async function load(force = false) {
    if (loaded.value && !force) return
    try {
      prompts.value = await promptsApi.list()
      loaded.value = true
    } catch (e) {
      throw toApiError(e)
    }
  }

  async function save(input: PromptInput): Promise<Prompt> {
    try {
      const saved = await promptsApi.save(input)
      await load(true)
      return saved
    } catch (e) {
      throw toApiError(e)
    }
  }

  async function remove(id: string) {
    try {
      await promptsApi.remove(id)
      await load(true)
    } catch (e) {
      throw toApiError(e)
    }
  }

  /** 使用计数自增：失败不影响主流程 */
  function bumpUsage(id: string) {
    void promptsApi.bumpUsage(id).then(() => load(true)).catch(() => {})
  }

  async function versions(promptId: string): Promise<PromptVersion[]> {
    try {
      return await promptsApi.versions(promptId)
    } catch (e) {
      throw toApiError(e)
    }
  }

  return { prompts, loaded, snippets, templates, isBuiltin, load, save, remove, bumpUsage, versions }
})
