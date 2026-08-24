import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import { providersApi, toApiError } from '@/lib/api'
import type { Provider, ProviderInput } from '@/types'

export const useProviderStore = defineStore('provider', () => {
  const providers = ref<Provider[]>([])
  const loaded = ref(false)

  const enabledProviders = computed(() => providers.value.filter((p) => p.enabled))

  async function load(force = false) {
    if (loaded.value && !force) return
    try {
      providers.value = await providersApi.list()
      loaded.value = true
    } catch (e) {
      throw toApiError(e)
    }
  }

  function get(id: string): Provider | null {
    return providers.value.find((p) => p.id === id) ?? null
  }

  async function save(input: ProviderInput): Promise<Provider> {
    try {
      const saved = await providersApi.save(input)
      await load(true)
      return saved
    } catch (e) {
      throw toApiError(e)
    }
  }

  async function remove(id: string) {
    try {
      await providersApi.remove(id)
      await load(true)
    } catch (e) {
      throw toApiError(e)
    }
  }

  return { providers, loaded, enabledProviders, load, get, save, remove }
})
