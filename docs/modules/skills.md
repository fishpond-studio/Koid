# Skills（可复用 AI 工作流）

Skills 是可复用的 AI 工作流单元：YAML 定义 → 引擎逐步执行。

## 步骤类型

| 类型 | 说明 |
|------|------|
| `input` | 弹窗询问用户输入（或粘贴文件内容） |
| `llm` | 调用模型执行任务 |
| `condition` | 条件分支：`contains( step.output, 'text' )`（双花括号包裹引用） |
| `tool` | 调用已连接 MCP 服务器的工具 |
| `message` | 展示最终结果（Markdown 渲染），并终止执行 |

> 运行细节：`condition` 仅支持 `contains(...)` 一种函数；整个流程最多执行 64 步；
> 遇到 `message` 步骤后结束。

## 变量

- 启动入参：`selection`、`file`、`clipboard`（双花括号包裹，运行对话框内提供）
- 步骤输出：`{step_id}.output`，写作双花括号包裹的引用

## 示例

```yaml
id: code-review
name: Code Review
description: Review code for bugs and style issues
icon: git-pull-request
systemPrompt: You are a senior code reviewer.
steps:
  - id: read
    type: input
    content: Paste the code to review
  - id: review
    type: llm
    prompt: |
      Please review the following code...

      {{read.output}}
  - id: check
    type: condition
    condition: contains( {{review.output}}, 'bug' )
    then: fix
    else: done
  - id: fix
    type: llm
    prompt: |
      Provide the fixed code based on:

      {{review.output}}
  - id: done
    type: message
    content: "{{review.output}}"
```

详见 [Skill YAML 参考](/reference/skill-yaml)。
