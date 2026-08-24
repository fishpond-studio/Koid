# 插件 API

插件在 iframe 内使用 `window.parent.postMessage({ koidCall: {...} }, '*')` 调用，
主应用以 `{ koidResult: { id, result, error } }` 响应。命令面板执行时主应用推送
`{ koidExecute: { commandId } }`。

## 方法

| 方法 | 参数 | 说明 | 权限 |
|------|------|------|------|
| `koid.ui.notify` | `{ message }` | 全局 Toast 通知 | `notify` |
| `koid.llm.chat` | `{ prompt }` | 非流式调用首个启用模型 | `llm` |
| `koid.storage.get` | `{ key }` | 读取插件键值（`plugin:{id}:` 前缀隔离） | `storage` |
| `koid.storage.set` | `{ key, value }` | 写入插件键值 | `storage` |
| `koid.file.read` | `{ path }` | 读取插件工作区文件（`workspace/` 内） | `file` |
| `koid.file.write` | `{ path, content }` | 写入插件工作区文件 | `file` |
| `koid.command.register` | `{ commandId, title }` | 注册到命令面板（Cmd+K） | `command` |
| `koid.network.fetch` | `{ url, method?, headers?, body? }` | 走主进程代理的 HTTP 请求 | `network` |

## 示例插件

```html
<script>
  function call(method, params) {
    return new Promise((resolve, reject) => {
      const id = Math.floor(Math.random() * 1e9)
      const onMsg = (e) => {
        const r = e.data?.koidResult
        if (r && r.id === id) {
          window.removeEventListener('message', onMsg)
          r.error ? reject(new Error(r.error)) : resolve(r.result)
        }
      }
      window.addEventListener('message', onMsg)
      window.parent.postMessage({ koidCall: { id, method, params } }, '*')
    })
  }

  window.addEventListener('message', async (e) => {
    const exec = e.data?.koidExecute
    if (exec && exec.commandId === 'hello') {
      await call('koid.ui.notify', { message: 'Hello from plugin!' })
    }
  })

  call('koid.command.register', { commandId: 'hello', title: 'Hello Plugin' })
</script>
```
