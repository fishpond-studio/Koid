/**
 * Shiki 高亮器（§4.10：代码块高亮）
 * 双主题输出（github-light / github-dark），跟随 .dark 类切换，无需重新高亮
 */
import { createHighlighter, type Highlighter } from 'shiki'

// 常用语言预加载，避免每次渲染按需下载
const LANGS = [
  'typescript',
  'javascript',
  'json',
  'html',
  'css',
  'vue',
  'python',
  'rust',
  'go',
  'java',
  'c',
  'cpp',
  'bash',
  'sql',
  'yaml',
  'toml',
  'markdown',
]

let instance: Promise<Highlighter> | null = null

function getHighlighter(): Promise<Highlighter> {
  if (!instance) {
    instance = createHighlighter({
      themes: ['github-light', 'github-dark'],
      langs: LANGS,
    })
  }
  return instance
}

/** 语言简写归一化到 shiki 已加载语言 */
function normalizeLang(lang: string): string {
  const alias: Record<string, string> = {
    ts: 'typescript',
    tsx: 'typescript',
    js: 'javascript',
    jsx: 'javascript',
    py: 'python',
    rs: 'rust',
    sh: 'bash',
    shell: 'bash',
    yml: 'yaml',
    'c++': 'cpp',
    md: 'markdown',
  }
  return alias[lang] ?? lang
}

/** 返回 shiki 生成的 <pre class="shiki"> HTML；失败返回 null（降级为纯文本） */
export async function highlightCode(code: string, lang: string): Promise<string | null> {
  try {
    const hl = await getHighlighter()
    const target = normalizeLang(lang.toLowerCase())
    const finalLang = hl.getLoadedLanguages().includes(target) ? target : 'plaintext'
    return hl.codeToHtml(code, {
      lang: finalLang,
      // defaultColor: false → token 颜色走 CSS 变量，明暗主题纯 CSS 切换
      themes: { light: 'github-light', dark: 'github-dark' },
      defaultColor: false,
    })
  } catch {
    return null
  }
}
