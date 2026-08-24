import { defineStore } from 'pinia'
import { hexToHsl, hslNeedsDarkForeground } from '@/lib/utils'
import type { ThemeColor, ThemeMode, ThemeSettings, UiDensity } from '@/types'

/** 代码字体选项（§4.1） */
export const CODE_FONTS = [
  "'JetBrains Mono', monospace",
  "'Fira Code', monospace",
  "'Cascadia Code', monospace",
  "'SF Mono', monospace",
] as const

const DEFAULT_CODE_FONT = CODE_FONTS[0]

/**
 * 主题 Store（§4.1）
 * 所有变更通过 apply() 同步到 <html> 属性与内联变量，禁止刷新页面
 * persist key 固定为 koid-theme，与 index.html 首帧脚本约定一致
 */
export const useThemeStore = defineStore('theme', {
  state: (): ThemeSettings => ({
    mode: 'system',
    color: 'indigo',
    customPrimary: null,
    density: 'default',
    codeFont: DEFAULT_CODE_FONT,
  }),
  actions: {
    /** 将当前主题状态写入 DOM：class=dark、data-theme、data-density、自定义色变量 */
    apply() {
      const el = document.documentElement
      const systemDark = window.matchMedia('(prefers-color-scheme: dark)').matches
      const dark = this.mode === 'dark' || (this.mode === 'system' && systemDark)
      el.classList.toggle('dark', dark)

      if (this.color === 'custom') {
        // 自定义 HEX：CSS 无对应预设，转 HSL 后内联注入三个联动变量
        el.removeAttribute('data-theme')
        if (this.customPrimary) {
          const hsl = hexToHsl(this.customPrimary)
          if (hsl) {
            el.style.setProperty('--primary', hsl)
            el.style.setProperty('--ring', hsl)
            el.style.setProperty(
              '--primary-foreground',
              hslNeedsDarkForeground(hsl) ? '222.2 47.4% 11.2%' : '0 0% 100%',
            )
          }
        }
      } else {
        el.setAttribute('data-theme', this.color)
        el.style.removeProperty('--primary')
        el.style.removeProperty('--ring')
        el.style.removeProperty('--primary-foreground')
      }

      if (this.density === 'default') {
        el.removeAttribute('data-density')
      } else {
        el.setAttribute('data-density', this.density)
      }

      el.style.setProperty('--font-mono', this.codeFont || DEFAULT_CODE_FONT)
    },
    setMode(mode: ThemeMode) {
      this.mode = mode
      this.apply()
    },
    setColor(color: ThemeColor, customHex?: string) {
      this.color = color
      if (customHex !== undefined) this.customPrimary = customHex
      this.apply()
    },
    setDensity(density: UiDensity) {
      this.density = density
      this.apply()
    },
    setCodeFont(font: string) {
      this.codeFont = font
      this.apply()
    },
  },
  persist: {
    key: 'koid-theme',
  },
})
