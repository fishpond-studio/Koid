import { ref } from 'vue'
import { defineStore } from 'pinia'
import { pluginsApi, toApiError } from '@/lib/api'
import type { PluginInfo } from '@/types'

/** 插件注册的命令（§4.9 koid.command.register） */
export interface PluginCommand {
  /** pluginId:commandId 唯一键 */
  id: string
  pluginId: string
  commandId: string
  title: string
  pluginName: string
}

/**
 * 插件 Store：列表 + 命令注册表
 * 执行回调由 PluginSettings 在 iframe 挂载时注册（setExecutor），
 * 命令面板选中后经回调 postMessage 回插件 iframe
 */
export const usePluginStore = defineStore('plugin', () => {
  const plugins = ref<PluginInfo[]>([])
  const commands = ref<PluginCommand[]>([])
  const loaded = ref(false)

  let executor: ((pluginId: string, commandId: string) => void) | null = null

  async function load(force = false) {
    if (loaded.value && !force) return
    try {
      plugins.value = await pluginsApi.list()
      loaded.value = true
    } catch (e) {
      throw toApiError(e)
    }
  }

  /** 清理某插件的全部命令（切页/卸载时调用） */
  function unregisterPlugin(pluginId: string) {
    commands.value = commands.value.filter((c) => c.pluginId !== pluginId)
  }

  function registerCommand(pluginId: string, commandId: string, title: string, pluginName: string) {
    const id = `${pluginId}:${commandId}`
    const existing = commands.value.find((c) => c.id === id)
    if (existing) {
      existing.title = title
      existing.pluginName = pluginName
      return
    }
    commands.value.push({ id, pluginId, commandId, title, pluginName })
  }

  function setExecutor(fn: ((pluginId: string, commandId: string) => void) | null) {
    executor = fn
  }

  /** 命令面板执行入口 */
  function execute(commandId: string) {
    const cmd = commands.value.find((c) => c.id === commandId)
    if (cmd && executor) {
      executor(cmd.pluginId, cmd.commandId)
    }
  }

  return {
    plugins,
    commands,
    loaded,
    load,
    unregisterPlugin,
    registerCommand,
    setExecutor,
    execute,
  }
})
