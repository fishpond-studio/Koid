# Skill YAML 参考

## 顶层字段

| 字段 | 类型 | 说明 |
|------|------|------|
| `id` | string | 唯一标识（仅字母数字与 `- _`） |
| `name` | string | 展示名称 |
| `description` | string | 描述 |
| `icon` | string（可选） | 图标名：`git-pull-request` / `bug` |
| `model` | string（可选） | 按 `model_id`/`displayName` 指定，缺省取首个可用模型 |
| `systemPrompt` | string（可选） | 各 llm 步骤共用的 system prompt |
| `steps` | array | 步骤定义 |

## 步骤

### input

```yaml
- id: ask
  type: input
  content: 提示用户输入的文字
```

### llm

```yaml
- id: analyze
  type: llm
  prompt: |
    处理以下内容：

    {{ask.output}}
```

### condition

```yaml
- id: check
  type: condition
  condition: contains( {{analyze.output}}, 'bug' )
  then: fix          # 命中跳转的步骤 id
  else: done         # 未命中跳转的步骤 id
```

### tool

```yaml
- id: call
  type: tool
  server: filesystem   # 可选：MCP 服务器名，缺省取首个已连接服务器
  tool: read_file
  args: '{"path": "{{file}}"}'
```

### message

```yaml
- id: done
  type: message
  content: "{{analyze.output}}"
```

> `message` 步骤会终止整个流程。

### condition

`condition` 仅支持 `contains(...)` 一种函数（双花括号包裹引用），例如：

```text
contains( {{analyze.output}}, 'bug' )
```

## 运行约束

整个流程最多执行 **64 步**（防止死循环）。

## 变量

- 启动入参：`selection`、`file`、`clipboard`（运行对话框中提供，双花括号包裹）
- 步骤输出：`{step_id}.output`，写作双花括号包裹的引用，如 `{ask}.output`
- 未定义的变量替换为空字符串
