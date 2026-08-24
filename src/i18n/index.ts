import { createI18n } from 'vue-i18n'
import zhCN from './zh-CN'
import enUS from './en-US'

/**
 * 全局 i18n 实例
 * 强制 Composition API 模式（legacy: false），与 <script setup> 保持一致
 * 默认跟随系统语言，无法识别时回退英文
 */
function detectLocale(): string {
  try {
    const stored = localStorage.getItem('koid-locale')
    if (stored === 'zh-CN' || stored === 'en-US') return stored
  } catch {
    /* localStorage 不可用时忽略 */
  }
  return navigator.language?.toLowerCase().startsWith('zh') ? 'zh-CN' : 'en-US'
}

export const i18n = createI18n({
  legacy: false,
  locale: detectLocale(),
  fallbackLocale: 'en-US',
  messages: {
    'zh-CN': zhCN,
    'en-US': enUS,
  },
})

export function setLocale(locale: 'zh-CN' | 'en-US') {
  i18n.global.locale.value = locale
  try {
    localStorage.setItem('koid-locale', locale)
  } catch {
    /* 忽略持久化失败 */
  }
}
