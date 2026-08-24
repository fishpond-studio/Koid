# 快速开始

## 环境要求

- Node.js 20+
- Rust 1.85+（MSVC 工具链）
- 桌面系统：Windows / macOS / Linux

## 开发运行

```bash
# 安装前端依赖
npm install

# 启动开发（Vite :1420 + Tauri）
npm run tauri dev
```

## 首次配置

1. 打开 **设置 → 模型 → 新建供应商**
2. 填写名称、协议类型（OpenAI 兼容 / Anthropic / Ollama）、Base URL、API Key
   - API Key 只保存在系统凭据管理器，不会写入数据库
3. 为供应商添加模型（如 `gpt-4o`、`claude-3-5-sonnet`）
4. 回到对话页，**点击工作区 chip 选择一个本地项目目录**（Vibe Coding 的第一步）
5. 选择模型，开始发送

> 发送前可在对话页头部直接设置 **System Prompt** 与 **思考强度**（五档滑块）——
> 新会话延迟到首条消息才创建，此前的设置会作为待用值一并写入。详见
> [对话页](/modules/chat)。

> 首次关闭应用时会询问「隐藏到托盘 / 退出 / 下次再问」，可在 **设置 → 通用 → 关闭行为** 修改。

## 可选配置

- **代理**：设置 → 网络（全局代理，供应商可单独覆盖）
- **故障转移**：设置 → 网络 → 模型故障转移（主供应商失败自动切换）
- **提示词库**：设置 → 提示词（Snippets / Templates / System Prompt）
- **Skills**：技能页运行内置的 Code Review / 解释报错
- **MCP**：设置 → MCP（stdio 服务器，工具发现）

## 构建

```bash
npm run build        # 前端产物
npm run docs:build   # 文档站点
npm run tauri build -- --bundles nsis   # NSIS 安装包
npm run tauri build -- --bundles msi    # MSI 安装包（需联网下载 WiX）
```

产物输出到 `src-tauri/target/release/bundle/`。
