# MCP（Model Context Protocol）

Koid 内置 MCP Client（stdio 传输，JSON-RPC 2.0）。

## 配置

**设置 → MCP → 添加服务器**：

- 名称
- 传输方式（stdio / SSE）
- 启动命令（如 `npx`）+ 参数（stdio）
- 环境变量（可选）
- URL（SSE 传输）

连接后自动完成 `initialize` 握手并发现 `tools/list`。

## 使用

- **管理界面**：连接/断开、工具列表、单个工具 JSON 参数调用测试
- **Skill 集成**：Skill 的 `tool` 步骤可直接调用 MCP 工具：

```yaml
- id: list-files
  type: tool
  server: filesystem
  tool: read_file
  args: '{"path": "{{file}}"}'
```

## 安全

- 工具调用由用户显式配置与触发
- 对话自动注入 / 权限（ask/allow/deny）为后续迭代
