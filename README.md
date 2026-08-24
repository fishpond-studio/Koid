# Koid

> **Koid** /kɔɪd/ — **Not just code. It's Koid.**

开源 Vibe Coding 桌面端 Agent —— 本地优先、插件优先，兼顾桌面端的美观体验。

## 为什么叫 Koid

**Koid** 是 **Koi**（锦鲤 /kɔɪ/）与 **code**（代码 /kəʊd/）的谐音合体。

- **Koi**：锦鲤。象征好运、向上与逆流而上的坚持。
- **-d**：尾音落在 /d/，让它听起来像 *code*——它是写代码的工具。

把代码项目当作一池锦鲤来养：耐心投喂、持续打磨，让它在时间里慢慢长大。
Koid，就是「锦鲤码」——一句给每个写代码的人的好运祝愿。

Koid 基于 **Tauri 2 + Vue 3 + TypeScript** 构建，插件优先、模型无关、本地优先。

## 核心特性

- **Vibe First**：界面让人产生「想写代码」的冲动。工作区是开始 Vibe Coding 的第一步，选择本地项目目录后，模型即可读取、编辑、新建、删除项目文件并自主编码。
- **Plugin Native**：插件是核心架构的第一公民，兼容 OpenCode 插件生态，iframe 沙箱运行。
- **Model Agnostic**：多供应商自由切换、自动故障转移、请求格式透明。
- **Local First**：SQLite 本地存储，API Key 只进系统凭据管理器（Windows 凭据管理器 / macOS Keychain），绝不落盘。

## 主要功能

- **工作区门禁**：先选择/创建工作区（绑定本地项目目录）才能开始 Vibe Coding；最近工作区自动恢复
- **Agent 工具**：`list_dir` / `read_file` / `grep` / `glob` 探索项目，`write_file` / `edit_file` / `delete_file` 修改项目（模型自主编码）
- **多模型供应商**：OpenAI 兼容 / Anthropic / Ollama 等，模型自动发现
- **会话管理**：按工作区自动分组、搜索、分支、归档
- **思考强度调节**：五档滑块（默认/低/中/高/最大），自动映射 OpenAI `reasoning_effort` 与 Anthropic thinking 预算；最大档配 Claude Code 同款 WebGL 能量流，配色跟随主题色
- **消息撤回 / 编辑重发**：用户消息悬停即达，截断后续内容重新生成
- **上下文占用**：头部实时显示 token 用量与窗口占比，>80% 红色预警联动 `/compact`
- **中文安装器**：NSIS 安装界面全中文，内置用户协议页（Apache-2.0）
- **MCP 支持**：Model Context Protocol 服务器（stdio 传输）
- **Skills 工作流**：可复用的 AI 工作流（内置 + 用户 YAML 编辑）
- **提示词系统**：Snippets / Templates / System Prompt，带版本历史
- **系统托盘**：关闭行为可配置（隐藏到托盘 / 直接退出 / 每次询问）

## 技术栈

| 层 | 技术 |
| --- | --- |
| 桌面壳 | Tauri 2（Rust） |
| 前端 | Vue 3 + TypeScript + Pinia + Vue Router |
| UI | shadcn-vue + Tailwind CSS |
| 存储 | SQLite（bundled，WAL） |
| 密钥 | 系统 keyring（Windows 凭据管理器 / macOS Keychain） |

## 快速开始

```bash
npm install
npm run tauri dev
```

## 构建打包

```bash
# 完整打包（前端构建 + release 编译 + 安装包）
npm run tauri build -- --bundles nsis   # NSIS 安装包
npm run tauri build -- --bundles msi    # MSI 安装包（需联网下载 WiX）

# 仅前端
npm run build
```

产物输出到 `src-tauri/target/release/bundle/`。

## 下载安装

从 [GitHub Releases](https://github.com/fishpond-studio/Koid/releases) 下载对应平台安装包：

- **Windows**：`Koid_0.1.0_x64-setup.exe`（NSIS）
- **macOS**：`Koid_0.1.0_universal.dmg`（universal，Intel / Apple Silicon 通用）
  - 因未做 Apple 签名公证，首次打开可能提示"已损坏"，解决方法见 [macOS 安装说明](docs/guide/macos-install.md)
- **Linux**：`koid_0.1.0_amd64.deb` / `Koid_0.1.0_amd64.AppImage`

## 文档

完整的用户与开发文档见 [docs/](docs/index.md)，本地预览：

```bash
npm run docs:dev
```

## 数据与隐私

- 数据目录：`%APPDATA%/studio.fishpond.koid/`（Windows），全部业务数据存于 `koid.db`（SQLite）
- API Key：仅存于系统凭据管理器，数据库与备份均不包含
- 设置内提供加密备份/恢复（AES-256-GCM）

## 许可证

[Apache-2.0](LICENSE) © 2026 [Fishpond Studio](https://github.com/fishpond-studio/)
