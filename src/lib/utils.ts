import { clsx, type ClassValue } from 'clsx'
import { twMerge } from 'tailwind-merge'

// shadcn-vue 标配：合并条件 class 并智能去重冲突的 Tailwind 工具类
export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}

/**
 * HEX 颜色 → HSL 三元组字符串（如 "239 84% 67%"）
 * 用于用户自定义主题色：把 HEX 转成 CSS Variables 所需的 HSL 格式后内联注入
 */
export function hexToHsl(hex: string): string | null {
  const m = /^#?([a-f\d]{2})([a-f\d]{2})([a-f\d]{2})$/i.exec(hex.trim())
  if (!m) return null
  const r = parseInt(m[1], 16) / 255
  const g = parseInt(m[2], 16) / 255
  const b = parseInt(m[3], 16) / 255
  const max = Math.max(r, g, b)
  const min = Math.min(r, g, b)
  const l = (max + min) / 2
  let h = 0
  let s = 0
  if (max !== min) {
    const d = max - min
    s = l > 0.5 ? d / (2 - max - min) : d / (max + min)
    switch (max) {
      case r:
        h = ((g - b) / d + (g < b ? 6 : 0)) / 6
        break
      case g:
        h = ((b - r) / d + 2) / 6
        break
      case b:
        h = ((r - g) / d + 4) / 6
        break
    }
  }
  return `${Math.round(h * 360)} ${Math.round(s * 100)}% ${Math.round(l * 100)}%`
}

/** 判断 HSL 颜色是否需要深色前景文字（亮度阈值 0.6） */
export function hslNeedsDarkForeground(hsl: string): boolean {
  const parts = hsl.split(/\s+/)
  const l = parseFloat(parts[2]) / 100
  return l > 0.6
}

export function escapeHtml(s: string): string {
  return s
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
}

/**
 * 搜索结果摘要：截取首个命中前后 radius 字符并用 <mark> 包裹命中词。
 * 输出为可信 HTML（已转义原文），供 v-html 渲染
 */
export function highlightSnippet(text: string, query: string, radius = 40): string {
  const idx = text.toLowerCase().indexOf(query.toLowerCase())
  if (idx < 0) return escapeHtml(text.slice(0, radius * 2))
  const start = Math.max(0, idx - radius)
  const end = Math.min(text.length, idx + query.length + radius)
  const before = (start > 0 ? '…' : '') + text.slice(start, idx)
  const match = text.slice(idx, idx + query.length)
  const after = text.slice(idx + query.length, end) + (end < text.length ? '…' : '')
  return escapeHtml(before) + '<mark>' + escapeHtml(match) + '</mark>' + escapeHtml(after)
}
