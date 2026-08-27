/**
 * Shiki 高亮器（§4.10：代码块高亮）
 * 双主题输出（github-light / github-dark），跟随 .dark 类切换，无需重新高亮
 *
 * 性能关键：必须从 'shiki/core' 引入！
 * 主入口 'shiki' 是全量 bundle（200+ 语言全部进入构建，拖出几百个语言 chunk，
 * 并使其共享依赖被复制几十份）。core + 惰性 import 只打包列出的语言。
 */
import { createHighlighterCore } from 'shiki/core'
import { createJavaScriptRegexEngine } from 'shiki/engine/javascript'
import type { HighlighterCore, LanguageInput } from 'shiki/core'

// 常用语言（精简集；未列出的语言降级为 plaintext，不影响渲染）
const LANG_IMPORTS: LanguageInput[] = [
  () => import('@shikijs/langs/typescript'),
  () => import('@shikijs/langs/javascript'),
  () => import('@shikijs/langs/json'),
  () => import('@shikijs/langs/html'),
  () => import('@shikijs/langs/css'),
  () => import('@shikijs/langs/vue'),
  () => import('@shikijs/langs/python'),
  () => import('@shikijs/langs/rust'),
  () => import('@shikijs/langs/bash'),
  () => import('@shikijs/langs/sql'),
  () => import('@shikijs/langs/yaml'),
  () => import('@shikijs/langs/markdown'),
]

// 语言简写归一化到已加载语言
const ALIAS: Record<string, string> = {
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

let instance: Promise<HighlighterCore> | null = null

function getHighlighter(): Promise<HighlighterCore> {
  if (!instance) {
    instance = createHighlighterCore({
      themes: [
        () => import('@shikijs/themes/github-light'),
        () => import('@shikijs/themes/github-dark'),
      ],
      langs: LANG_IMPORTS,
      // 纯 JS 正则引擎：无需 oniguruma wasm（省 ~600KB 且避免 wasm 加载延迟）
      engine: createJavaScriptRegexEngine(),
    })
  }
  return instance
}

/** 返回 shiki 生成的 <pre class="shiki"> HTML；失败返回 null（降级为纯文本） */
export async function highlightCode(code: string, lang: string): Promise<string | null> {
  try {
    const hl = await getHighlighter()
    const target = ALIAS[lang.toLowerCase()] ?? lang.toLowerCase()
    const loaded = hl.getLoadedLanguages() as readonly string[]
    const finalLang = loaded.includes(target) ? target : 'plaintext'
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
