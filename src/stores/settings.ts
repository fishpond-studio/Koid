import { ref } from 'vue'
import { defineStore } from 'pinia'
import { settingsApi, toApiError } from '@/lib/api'
import type { FailoverConfig, GlobalProxySettings } from '@/types'

export const KEY_GLOBAL_PROXY = 'global_proxy'
export const KEY_FAILOVER_CONFIG = 'failover_config'
export const KEY_DEFAULT_SYSTEM_PROMPT = 'default_system_prompt'
export const KEY_CLOSE_MODE = 'close_mode'
/** 自动总结上下文开关（默认关闭） */
export const KEY_AUTO_COMPACT = 'auto_compact'
/** 总结上下文使用的模型 id；空 = 默认（跟随会话当前模型） */
export const KEY_COMPACT_MODEL = 'compact_model'

/** 关闭行为：hide=隐藏到托盘 / quit=退出 / ask=每次询问（默认） */
export type CloseMode = 'hide' | 'quit' | 'ask'

/** 故障转移默认配置：与 Rust Default 实现保持一致 */
export function defaultFailoverConfig(): FailoverConfig {
  return {
    enabled: false,
    strategy: 'sequential',
    triggerConditions: ['timeout', '5xx', 'empty-response', 'content-filter'],
    excludedStatusCodes: [401, 403],
    backoffMultiplier: 2,
    maxBackoffSeconds: 16,
    fallbackChain: [],
  }
}

/**
 * 全局网络设置 Store（§4.4 代理 / §4.3 故障转移）
 * 存储于 SQLite settings 表（JSON 序列化），跨会话保留
 */
export const useSettingsStore = defineStore('settings', () => {
  const globalProxy = ref<GlobalProxySettings>({ proxyType: 'direct', proxyUrl: null })
  const failover = ref<FailoverConfig>(defaultFailoverConfig())
  /** 新会话默认 System Prompt（§4.6 层级 1） */
  const defaultSystemPrompt = ref('')
  /** 关闭行为：hide / quit / ask（默认每次询问） */
  const closeMode = ref<CloseMode>('ask')
  /** 自动总结上下文：上下文占用超阈值时发送前自动压缩（默认关闭） */
  const autoCompact = ref(false)
  /** 总结上下文使用的模型 id；null = 默认（跟随会话当前模型） */
  const compactModelId = ref<string | null>(null)
  const loaded = ref(false)

  async function load(force = false) {
    if (loaded.value && !force) return
    try {
      const [proxyJson, failoverJson, systemPrompt, closeModeVal, autoCompactVal, compactModel] =
        await Promise.all([
          settingsApi.get(KEY_GLOBAL_PROXY),
          settingsApi.get(KEY_FAILOVER_CONFIG),
          settingsApi.get(KEY_DEFAULT_SYSTEM_PROMPT),
          settingsApi.get(KEY_CLOSE_MODE),
          settingsApi.get(KEY_AUTO_COMPACT),
          settingsApi.get(KEY_COMPACT_MODEL),
        ])
      if (proxyJson) {
        try {
          globalProxy.value = JSON.parse(proxyJson) as GlobalProxySettings
        } catch {
          /* JSON 损坏时保持默认 direct */
        }
      }
      if (failoverJson) {
        try {
          failover.value = { ...defaultFailoverConfig(), ...JSON.parse(failoverJson) }
        } catch {
          failover.value = defaultFailoverConfig()
        }
      }
      defaultSystemPrompt.value = systemPrompt ?? ''
      if (closeModeVal === 'hide' || closeModeVal === 'quit' || closeModeVal === 'ask') {
        closeMode.value = closeModeVal
      }
      autoCompact.value = autoCompactVal === 'true'
      compactModelId.value = compactModel || null
      loaded.value = true
    } catch (e) {
      throw toApiError(e)
    }
  }

  async function saveGlobalProxy() {
    try {
      await settingsApi.set(KEY_GLOBAL_PROXY, JSON.stringify(globalProxy.value))
    } catch (e) {
      throw toApiError(e)
    }
  }

  async function saveFailover() {
    try {
      await settingsApi.set(KEY_FAILOVER_CONFIG, JSON.stringify(failover.value))
    } catch (e) {
      throw toApiError(e)
    }
  }

  async function saveDefaultSystemPrompt() {
    try {
      await settingsApi.set(KEY_DEFAULT_SYSTEM_PROMPT, defaultSystemPrompt.value)
    } catch (e) {
      throw toApiError(e)
    }
  }

  async function saveCloseMode() {
    try {
      await settingsApi.set(KEY_CLOSE_MODE, closeMode.value)
    } catch (e) {
      throw toApiError(e)
    }
  }

  async function saveAutoCompact() {
    try {
      await settingsApi.set(KEY_AUTO_COMPACT, String(autoCompact.value))
    } catch (e) {
      throw toApiError(e)
    }
  }

  async function saveCompactModel() {
    try {
      await settingsApi.set(KEY_COMPACT_MODEL, compactModelId.value ?? '')
    } catch (e) {
      throw toApiError(e)
    }
  }

  return {
    globalProxy,
    failover,
    defaultSystemPrompt,
    closeMode,
    autoCompact,
    compactModelId,
    loaded,
    load,
    saveGlobalProxy,
    saveFailover,
    saveDefaultSystemPrompt,
    saveCloseMode,
    saveAutoCompact,
    saveCompactModel,
  }
})
