import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import { modelsApi, toApiError } from '@/lib/api'
import type { Model, ModelInput } from '@/types'

const LAST_MODEL_KEY = 'koid-last-model'

/**
 * 模型 Store
 * model.id 是本地主键，会话引用它；真实 API 模型标识在 model.modelId
 */
export const useModelStore = defineStore('model', () => {
  const models = ref<Model[]>([])
  const loaded = ref(false)
  /** 最近使用的模型 id（响应式：选择后立即刷新 UI） */
  const preferredModelId = ref<string>(readLastUsed())

  const enabledModels = computed(() => models.value.filter((m) => m.enabled))

  function readLastUsed(): string {
    try {
      return localStorage.getItem(LAST_MODEL_KEY) ?? ''
    } catch {
      return ''
    }
  }

  async function load(force = false) {
    if (loaded.value && !force) return
    try {
      models.value = await modelsApi.list()
      loaded.value = true
    } catch (e) {
      throw toApiError(e)
    }
  }

  function get(id: string): Model | null {
    return models.value.find((m) => m.id === id) ?? null
  }

  function byProvider(providerId: string): Model[] {
    return models.value.filter((m) => m.providerId === providerId)
  }

  async function save(input: ModelInput): Promise<Model> {
    try {
      const saved = await modelsApi.save(input)
      await load(true)
      return saved
    } catch (e) {
      throw toApiError(e)
    }
  }

  async function remove(id: string) {
    try {
      await modelsApi.remove(id)
      await load(true)
    } catch (e) {
      throw toApiError(e)
    }
  }

  /** 记住模型并同步响应式状态，保证选择后立即反映到 UI */
  function remember(modelId: string) {
    preferredModelId.value = modelId
    try {
      localStorage.setItem(LAST_MODEL_KEY, modelId)
    } catch {
      /* 忽略 */
    }
  }

  function lastUsed(): string | null {
    try {
      return localStorage.getItem(LAST_MODEL_KEY)
    } catch {
      return null
    }
  }

  return {
    models,
    loaded,
    enabledModels,
    preferredModelId,
    load,
    get,
    byProvider,
    save,
    remove,
    remember,
    lastUsed,
  }
})
