# 架构

## 分层

```
┌──────────────────────────────────────────────┐
│  Vue 3 前端（Pinia / shadcn-vue / Router）     │
│  ─ 只发 Tauri Command，绝不直接访问网络        │
│  ─ 工作区门禁 / 关闭询问框 / 命令面板           │
├──────────────────────────────────────────────┤
│  Tauri Command 层（commands/）                 │
│  providers / models / sessions / messages      │
│  workspaces / prompts / skills / mcp           │
│  plugins / backup / settings / tray            │
├──────────────────────────────────────────────┤
│  Service 层（services/）                       │
│  llm（格式转换+SSE）/ proxy / failover          │
│  skills（执行引擎）/ mcp（JSON-RPC stdio）      │
│  keyring / backup（AES-256-GCM）/ settings      │
│  tray（关闭行为 + 托盘）                       │
├──────────────────────────────────────────────┤
│  SQLite（rusqlite，Rust 层独占，WAL）           │
│  系统 keyring（API Key，绝不下库）              │
└──────────────────────────────────────────────┘
```

## 数据流转约定

- 内部消息统一 **OpenAI Chat Completions** 格式；Anthropic 调用时由 Rust 层自动转换
- 所有命令错误返回 `CODE:可读信息`，前端按 CODE 映射 i18n 文案
- 流式对话：Rust 通过 `chat:chunk` / `chat:failover` / `chat:tool_call` 事件推送，
  前端按 `requestId` 过滤
- Agent 工具（`list_dir` / `read_file` / `grep` / `glob` / `write_file` / `edit_file` /
  `delete_file`）执行于 Rust 层，限定在工作区目录内，路径净化防穿越

## 工作区与门禁

- 工作区 = 分组 + 可选绑定的本地项目路径；同一路径幂等复用
- **门禁**：当前工作区未绑定路径时，输入框只读、发送禁用；需先经工作区选择器
  （原生目录对话框新建 / 切换 / 绑定路径）解除
- 冷启动自动恢复到「最近会话所属工作区」

## 关闭行为与托盘

- 窗口 `CloseRequested` 由 Rust 拦截，按 `close_mode`（hide / quit / ask）决策
- `ask` 时前端弹出询问框，`resolve_close` 记忆选择并执行（隐藏 / 退出）

## 主题系统

- 颜色全部映射到 CSS Variables（`--primary` 等）
- `data-theme` 切换 6 套预设，自定义 HEX 内联注入
- `data-density` 缩放根字号实现全局密度
- 切换零刷新、零闪烁（`index.html` 首帧脚本兜底）
