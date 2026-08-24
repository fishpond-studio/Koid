import { onMounted, onUnmounted, watch } from 'vue'
import { storeToRefs } from 'pinia'
import { useThemeStore } from '@/stores/theme'

/**
 * 主题副作用管理：
 * 1. 监听 store 中所有主题字段变化 → apply()
 * 2. mode = system 时监听系统配色变化实时跟随（不刷新页面）
 * 在 App.vue 顶层调用一次即可
 */
export function useThemeWatcher() {
  const store = useThemeStore()
  const { mode, color, customPrimary, density, codeFont } = storeToRefs(store)

  let media: MediaQueryList | null = null
  const onSystemChange = () => store.apply()

  onMounted(() => {
    store.apply()
    media = window.matchMedia('(prefers-color-scheme: dark)')
    media.addEventListener('change', onSystemChange)
  })

  onUnmounted(() => {
    media?.removeEventListener('change', onSystemChange)
  })

  watch([mode, color, customPrimary, density, codeFont], () => store.apply())
}
