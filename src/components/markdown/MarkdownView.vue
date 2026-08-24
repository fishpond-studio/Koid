<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { marked } from 'marked'
import DOMPurify from 'dompurify'
import { highlightCode } from './highlighter'

/**
 * Markdown 渲染组件（§4.10）
 * - 完整 GFM（marked 默认开启）
 * - 代码块：语言标签 + 一键复制；完成后用 Shiki 异步高亮（双主题跟随明暗）
 * - 流式期间跳过 Shiki，仅渲染纯转义代码，结束后再高亮，避免抖动
 * - DOMPurify 兜底清洗；外链统一 target=_blank + noopener
 */
const props = withDefaults(defineProps<{ content: string; streaming?: boolean }>(), {
  streaming: false,
})

/** 预览事件：HTML/SVG 代码块点击预览按钮时抛出（Artifacts 模式，§4.10） */
const emit = defineEmits<{ preview: [payload: { lang: string; code: string }] }>()

const el = ref<HTMLElement | null>(null)
const html = ref('')

// ---------- marked 配置 ----------

const COPY_SVG =
  '<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect width="14" height="14" x="8" y="8" rx="2" ry="2"/><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"/></svg>'
const CHECK_SVG =
  '<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M20 6 9 17l-5-5"/></svg>'
const EYE_SVG =
  '<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M2.062 12.348a1 1 0 0 1 0-.696 10.75 10.75 0 0 1 19.876 0 1 1 0 0 1 0 .696 10.75 10.75 0 0 1-19.876 0"/><circle cx="12" cy="12" r="3"/></svg>'

// 可在沙箱 iframe 中预览的语言（Artifacts 模式）
const PREVIEWABLE = ['html', 'svg', 'xml']

function escapeHtml(s: string): string {
  return s
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
}

marked.use({
  gfm: true,
  breaks: true,
  renderer: {
    // marked v15+ 渲染器以 token 对象传参
    code(token) {
      const lang = (token.lang ?? '').trim() || 'text'
      const previewBtn = PREVIEWABLE.includes(lang.toLowerCase())
        ? `<button type="button" class="code-preview" title="Preview">${EYE_SVG}</button>`
        : ''
      return [
        '<div class="code-block">',
        `<div class="code-head"><span class="code-lang">${escapeHtml(lang)}</span><span class="code-actions">`,
        previewBtn,
        `<button type="button" class="code-copy" title="Copy">${COPY_SVG}</button></span></div>`,
        `<div class="code-body"><pre data-lang="${escapeHtml(lang)}"><code>${escapeHtml(token.text)}</code></pre></div>`,
        '</div>',
      ].join('')
    },
  },
})

// 外链安全属性统一注入
DOMPurify.addHook('afterSanitizeAttributes', (node) => {
  if (node.tagName === 'A') {
    node.setAttribute('target', '_blank')
    node.setAttribute('rel', 'noopener noreferrer')
  }
})

// ---------- 渲染调度 ----------

let scheduled = false

function scheduleRender() {
  if (scheduled) return
  scheduled = true
  requestAnimationFrame(() => {
    scheduled = false
    void render()
  })
}

async function render() {
  const raw = marked.parse(props.content, { async: false }) as string
  html.value = DOMPurify.sanitize(raw)
  if (!props.streaming) {
    await nextTick()
    void highlightAll()
  }
}

/** 对未高亮的代码块逐个执行 Shiki 高亮（内容替换为双主题 pre.shiki） */
async function highlightAll() {
  const root = el.value
  if (!root) return
  const bodies = Array.from(root.querySelectorAll<HTMLElement>('.code-body:not([data-hl])'))
  for (const body of bodies) {
    const pre = body.querySelector('pre')
    if (!pre) continue
    const code = pre.textContent ?? ''
    const lang = pre.dataset.lang ?? 'text'
    const out = await highlightCode(code, lang)
    // 异步返回时 DOM 可能已因新 chunk 重渲染，校验节点仍在文档中
    if (out && body.isConnected) {
      body.innerHTML = out
      body.setAttribute('data-hl', '1')
    }
  }
}

// ---------- 复制按钮（事件委托，v-html 无法绑定 Vue 事件） ----------

async function copyCode(btn: HTMLElement) {
  const pre = btn.closest('.code-block')?.querySelector('.code-body pre')
  if (!pre) return
  try {
    await navigator.clipboard.writeText(pre.textContent ?? '')
    btn.innerHTML = CHECK_SVG
    window.setTimeout(() => {
      btn.innerHTML = COPY_SVG
    }, 1500)
  } catch {
    /* 剪贴板不可用时静默 */
  }
}

function onClick(e: MouseEvent) {
  const target = e.target as HTMLElement
  const copyBtn = target.closest<HTMLElement>('.code-copy')
  if (copyBtn) {
    void copyCode(copyBtn)
    return
  }
  // Artifacts 预览：取该代码块原始文本抛出事件，由上层决定呈现方式
  const previewBtn = target.closest<HTMLElement>('.code-preview')
  if (previewBtn) {
    const block = previewBtn.closest('.code-block')
    const pre = block?.querySelector('.code-body pre')
    if (!pre) return
    emit('preview', {
      lang: (pre as HTMLElement).dataset.lang ?? 'html',
      code: pre.textContent ?? '',
    })
  }
}

watch(() => [props.content, props.streaming], scheduleRender, { immediate: true })

onMounted(() => el.value?.addEventListener('click', onClick))
onBeforeUnmount(() => el.value?.removeEventListener('click', onClick))
</script>

<template>
  <div ref="el" class="md-body" v-html="html" />
</template>

<style>
/* Markdown 正文排版（非 scoped：v-html 节点不受 scoped 约束） */
.md-body {
  line-height: 1.7;
  word-break: break-word;
}
.md-body > :first-child {
  margin-top: 0;
}
.md-body > :last-child {
  margin-bottom: 0;
}
.md-body p {
  margin: 0.5rem 0;
}
.md-body h1,
.md-body h2,
.md-body h3,
.md-body h4 {
  margin: 1rem 0 0.5rem;
  font-weight: 600;
  line-height: 1.3;
}
.md-body h1 {
  font-size: 1.4rem;
}
.md-body h2 {
  font-size: 1.2rem;
}
.md-body h3 {
  font-size: 1.05rem;
}
.md-body ul,
.md-body ol {
  margin: 0.5rem 0;
  padding-left: 1.4rem;
}
.md-body ul {
  list-style: disc;
}
.md-body ol {
  list-style: decimal;
}
.md-body li {
  margin: 0.2rem 0;
}
.md-body blockquote {
  margin: 0.6rem 0;
  padding: 0.25rem 0.9rem;
  border-left: 3px solid hsl(var(--primary) / 0.5);
  background: hsl(var(--muted) / 0.5);
  border-radius: 0 var(--radius) var(--radius) 0;
  color: hsl(var(--muted-foreground));
}
.md-body a {
  color: hsl(var(--primary));
  text-decoration: underline;
  text-underline-offset: 2px;
}
.md-body hr {
  margin: 1rem 0;
  border-color: hsl(var(--border));
}
.md-body table {
  margin: 0.6rem 0;
  border-collapse: collapse;
  font-size: 0.9em;
}
.md-body th,
.md-body td {
  border: 1px solid hsl(var(--border));
  padding: 0.35rem 0.7rem;
}
.md-body th {
  background: hsl(var(--muted));
  font-weight: 600;
}
.md-body img {
  max-width: 100%;
  border-radius: var(--radius);
}
.md-body :not(pre) > code {
  padding: 0.1rem 0.35rem;
  border-radius: 0.3rem;
  background: hsl(var(--muted));
  font-family: var(--font-mono);
  font-size: 0.85em;
}

/* ---------- 代码块 ---------- */
.md-body .code-block {
  margin: 0.7rem 0;
  overflow: hidden;
  border: 1px solid hsl(var(--border));
  border-radius: calc(var(--radius) + 2px);
}
.md-body .code-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0.3rem 0.7rem;
  border-bottom: 1px solid hsl(var(--border));
  background: hsl(var(--muted) / 0.6);
}
.md-body .code-lang {
  font-family: var(--font-mono);
  font-size: 0.72rem;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  color: hsl(var(--muted-foreground));
}
.md-body .code-copy,
.md-body .code-preview {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 1.5rem;
  height: 1.5rem;
  border-radius: 0.3rem;
  color: hsl(var(--muted-foreground));
  transition: color 0.15s ease-out, background-color 0.15s ease-out;
}
.md-body .code-copy:hover,
.md-body .code-preview:hover {
  color: hsl(var(--foreground));
  background: hsl(var(--accent));
}
.md-body .code-actions {
  display: flex;
  align-items: center;
  gap: 0.2rem;
}
.md-body .code-body pre {
  margin: 0;
  padding: 0.75rem 1rem;
  overflow-x: auto;
  font-family: var(--font-mono);
  font-size: 0.84rem;
  line-height: 1.65;
}

/* Shiki 双主题：token 颜色以 CSS 变量输出，.dark 下切换到 dark 变量 */
.md-body .shiki {
  background-color: var(--shiki-light-bg);
  color: var(--shiki-light);
}
.md-body .shiki span {
  color: var(--shiki-light);
}
.dark .md-body .shiki {
  background-color: var(--shiki-dark-bg);
}
.dark .md-body .shiki,
.dark .md-body .shiki span {
  color: var(--shiki-dark);
}
</style>
