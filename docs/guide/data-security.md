# 数据与安全

## 存储

- **SQLite**：供应商/模型、工作区、会话/消息、提示词、MCP、故障日志、设置
  - 表由 Rust 层迁移（`user_version` 顺序执行）
  - WAL 模式 + 外键约束
- **系统 keyring**：所有 API Key（Windows 凭据管理器 / macOS Keychain / Linux Secret Service）

## 备份

**设置 → 备份** 提供端到端加密备份（§4.11 云端同步基础）：

- 格式：`KOIDBK1` 魔数 + 随机 nonce + **AES-256-GCM** 密文
- 密钥：`SHA-256(口令)` 派生
- 备份内容为 SQLite 一致性快照（SQLite 在线 Backup API）
- 明文永不落盘；忘记口令即无法恢复

## 安全约定

- 前端禁止直接访问网络：一律经 Rust（reqwest + 代理配置）
- 插件运行于 `sandbox="allow-scripts"` iframe，无 same-origin，无法触达应用 DOM
- 插件权限由 manifest 声明，桥接层逐方法校验（`notify/llm/storage/file/command/network`）
- 插件文件读写限定在插件工作区目录，路径穿越被双重拦截
- 工作区文件访问（Agent 工具 / 文件树 / `@文件` 引用）限定在工作区根目录内，
  拒绝绝对路径与 `..` 穿越，单文件读取上限 1MB
- 关闭行为记忆仅存本地设置，不涉及网络
