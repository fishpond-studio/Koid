# 多供应商模型

- 供应商 CRUD：类型（OpenAI 兼容 / Anthropic / OpenAI Response / Ollama / 自定义）、Base URL、代理、超时、重试
  - **OpenAI Response** 类型目前返回 `UNSUPPORTED`（Phase 2 支持），请使用 OpenAI 兼容
  - 代理支持全局 / 供应商两级覆盖，含 HTTP（认证）/ SOCKS5
- API Key 只存系统 keyring，界面显示掩码
- 模型按供应商管理，启停开关；支持 `/v1/models` 自动发现
- 每会话独立覆盖 temperature / top_p / max_tokens / system prompt

内部统一 OpenAI 消息格式；Anthropic 请求/响应由 Rust 层自动转换。
