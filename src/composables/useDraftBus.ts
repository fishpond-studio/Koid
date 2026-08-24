import { ref } from 'vue'

/**
 * 输入框草稿共享总线：
 * 侧边栏文件树点击 → 把 `@路径` 引用追加进 ChatView 的输入框（Codex 式）
 * 模块级单例，跨组件共享同一草稿
 */
const draftText = ref('')

export function useDraftBus() {
  return {
    draftText,
    appendToDraft: (text: string) => {
      const cur = draftText.value
      draftText.value = cur ? `${cur} ${text}` : text
    },
  }
}
