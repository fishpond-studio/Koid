# 错误码

Rust 命令错误统一返回 `CODE:可读信息`，前端按 CODE 映射 i18n。

| CODE | 含义 | 前端文案 |
|------|------|----------|
| `NETWORK` | 连接失败 | 无法连接到服务器，请检查网络或代理设置 |
| `TIMEOUT` | 请求超时 | 请求超时，已尝试故障转移 |
| `UNAUTHORIZED` | 凭证无效 | API Key 无效或权限不足 |
| `RATE_LIMITED` | 请求过频 | 请求过于频繁，请稍后再试 |
| `SERVER` | 服务端错误 | 服务器错误，已尝试备用供应商 |
| `EMPTY` | 空响应 | 模型返回空响应（可转移故障） |
| `ABORTED` | 用户中断 | 不提示，静默停止 |
| `FORBIDDEN` | 权限不足 | 内置/受保护资源不可修改 |
| `UNSUPPORTED` | 暂不支持 | 功能未开放 |

## 故障转移判定

- 触发条件：`timeout`（含 `NETWORK`）/ `5xx` / `empty-response` / `content-filter`
- 不转移：`401/403`（凭证问题）、`429`、`BAD_REQUEST`、`ABORTED`
