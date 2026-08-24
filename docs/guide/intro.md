# 介绍

Koid 是一个开源的 **Vibe Coding 桌面端 Agent**。它的设计目标不是「又一个聊天框」，
而是一个以代码工作流为中心、插件优先、本地优先的创作工具。

## 名字由来

**Koid** 是 **Koi**（锦鲤 /kɔɪ/）与 **code**（代码 /kəʊd/）的谐音合体：
锦鲤象征好运与逆流而上的坚持，尾音 **-d** 让名字听起来像 *code*。
把代码项目当作一池锦鲤来养——Koid 就是「锦鲤码」，一句写给写代码的人的好运祝愿。

## 设计理念

| 理念 | 落地 |
|------|------|
| Vibe First | 主题系统、动效缓动曲线、Zen UI |
| Plugin Native | iframe 沙箱 + postMessage 桥，兼容 OpenCode 生态 |
| Model Agnostic | OpenAI / Anthropic / Ollama 统一内部格式 |
| Local First | SQLite + 系统 keyring，离线可浏览历史 |

## 技术栈

- **桌面壳**：Tauri 2（Rust + WebView）
- **前端**：Vue 3.4+ / TypeScript / Pinia / vue-router
- **UI**：shadcn-vue（reka-ui）+ TailwindCSS（CSS Variables 主题）
- **本地库**：SQLite（rusqlite，Rust 层独占）
- **网络**：reqwest（Rust 层，前端一律走 Tauri Command）
