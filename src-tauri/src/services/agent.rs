//! Agent 循环（§agent）：让模型能自主调用工具探索工作区
//!
//! 流程：调用 LLM → 若返回 tool_calls → 执行工作区工具（list_dir / read_file）→
//! 把工具结果作为消息回传 → 再次调用 LLM，直到无 tool_calls 或到达轮数上限。
//!
//! 工具执行由本模块完成（复用 workspaces.rs 的安全路径逻辑），
//! 全程经 `chat:tool_call` 事件把工具调用推送给前端渲染。

use crate::models::{ChatMessage, ChatRequest, ChatResponse, ToolCall, ToolDef};
use crate::services::llm::{self, DeltaSink, LlmContext};
use crate::state::AppState;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{Emitter, Window};

const MAX_AGENT_ROUNDS: usize = 12;
const TOOL_EVENT: &str = "chat:tool_call";

/// 工具执行结果
struct ToolOutcome {
    content: String,
    is_error: bool,
}

/// 工作区工具定义（注入给模型，供其自主探索项目，对齐 opencode）
pub fn workspace_tools() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "list_dir".to_string(),
            description: "列出工作区指定目录下的文件和子目录（path 相对工作区根目录，省略表示根目录）。返回每条目的名称与类型。".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "相对工作区根目录的路径，留空表示根目录" }
                },
                "required": []
            }),
        },
        ToolDef {
            name: "read_file".to_string(),
            description: "读取工作区指定文件的内容（path 相对工作区根目录）。支持 start_line/end_line 读取部分内容，大文件请指定行范围。文件需小于 1MB。".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "相对工作区根目录的文件路径" },
                    "start_line": { "type": "integer", "description": "起始行号（1 起，省略表示从头）" },
                    "end_line": { "type": "integer", "description": "结束行号（含，省略表示到结尾）" }
                },
                "required": ["path"]
            }),
        },
        ToolDef {
            name: "grep".to_string(),
            description: "在工作区文件中搜索匹配正则表达式的行（类似 grep -rn）。返回文件路径、行号与匹配行。适用于查找定义、引用、关键词。".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "正则表达式" },
                    "include": { "type": "string", "description": "可选：限定文件扩展名，如 *.ts、*.rs" }
                },
                "required": ["pattern"]
            }),
        },
        ToolDef {
            name: "glob".to_string(),
            description: "按 glob 模式查找工作区文件路径（如 **/*.ts、src/**/*.rs）。返回匹配的文件路径列表。".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "glob 模式" }
                },
                "required": ["pattern"]
            }),
        },
        ToolDef {
            name: "write_file".to_string(),
            description: "创建或覆盖工作区文件。父目录不存在会自动创建。用于新建文件或整体重写文件。".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "相对工作区根目录的文件路径" },
                    "content": { "type": "string", "description": "完整的文件内容" }
                },
                "required": ["path", "content"]
            }),
        },
        ToolDef {
            name: "edit_file".to_string(),
            description: "精确字符串替换编辑：在文件中找到唯一的 old_string 并替换为 new_string。old_string 必须唯一匹配，否则报错；请提供足够上下文保证唯一。用于修改文件局部内容。".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "相对工作区根目录的文件路径" },
                    "old_string": { "type": "string", "description": "要被替换的原文（须唯一匹配）" },
                    "new_string": { "type": "string", "description": "替换后的新文本" }
                },
                "required": ["path", "old_string", "new_string"]
            }),
        },
        ToolDef {
            name: "delete_file".to_string(),
            description: "删除工作区文件（仅文件，不删目录）。".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "相对工作区根目录的文件路径" }
                },
                "required": ["path"]
            }),
        },
        ToolDef {
            name: "run_command".to_string(),
            description: "在工作区根目录执行 shell 命令并返回退出码与输出（Windows 用 cmd /C，类 Unix 用 sh -c）。用于安装依赖、构建、运行测试、git 等开发工作流。默认超时 120 秒（最长 600 秒），超时会被终止。非零退出码不视为错误，请根据输出内容自行判断下一步。长输出的中间部分会被截断。".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "要执行的命令" },
                    "timeout_seconds": { "type": "integer", "description": "可选：超时秒数，默认 120，最大 600" }
                },
                "required": ["command"]
            }),
        },
    ]
}

/// 执行单个工具调用
async fn execute_tool(
    state: &AppState,
    workspace_id: &str,
    call: &ToolCall,
    abort: Arc<AtomicBool>,
) -> ToolOutcome {
    let params: Value = serde_json::from_str(&call.arguments).unwrap_or(Value::Null);
    let path = params
        .get("path")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    match call.name.as_str() {
        "list_dir" => {
            let conn = match state.db() {
                Ok(c) => c,
                Err(e) => return ToolOutcome { content: e, is_error: true },
            };
            match crate::commands::workspaces::list_workspace_dir(&conn, workspace_id, &path) {
                Ok(items) => {
                    let lines: Vec<String> = items
                        .iter()
                        .map(|(name, is_dir)| {
                            format!("{}{}", if *is_dir { "[dir] " } else { "      " }, name)
                        })
                        .collect();
                    ToolOutcome {
                        content: if lines.is_empty() {
                            "（空目录）".to_string()
                        } else {
                            lines.join("\n")
                        },
                        is_error: false,
                    }
                }
                Err(e) => ToolOutcome { content: e, is_error: true },
            }
        }
        "read_file" => {
            let conn = match state.db() {
                Ok(c) => c,
                Err(e) => return ToolOutcome { content: e, is_error: true },
            };
            let start = params.get("start_line").and_then(Value::as_u64).map(|v| v as usize);
            let end = params.get("end_line").and_then(Value::as_u64).map(|v| v as usize);
            match crate::commands::workspaces::read_workspace_file_content(&conn, workspace_id, &path) {
                Ok(content) => {
                    // 支持行范围：大文件只读部分（对齐 opencode read）
                    let sliced = match (start, end) {
                        (Some(s), Some(e)) => content.lines().skip(s.saturating_sub(1)).take(e.saturating_sub(s) + 1).collect::<Vec<_>>().join("\n"),
                        (Some(s), None) => content.lines().skip(s.saturating_sub(1)).collect::<Vec<_>>().join("\n"),
                        _ => content,
                    };
                    ToolOutcome { content: sliced, is_error: false }
                }
                Err(e) => ToolOutcome { content: e, is_error: true },
            }
        }
        "grep" => {
            let pattern = params.get("pattern").and_then(Value::as_str).unwrap_or("").to_string();
            if pattern.is_empty() {
                return ToolOutcome { content: "pattern 必填".to_string(), is_error: true };
            }
            let include = params.get("include").and_then(Value::as_str).map(str::to_string);
            let conn = match state.db() {
                Ok(c) => c,
                Err(e) => return ToolOutcome { content: e, is_error: true },
            };
            match crate::commands::workspaces::grep_workspace(&conn, workspace_id, &pattern, include.as_deref()) {
                Ok(hits) => ToolOutcome {
                    content: if hits.is_empty() { "无匹配".to_string() } else { hits.join("\n") },
                    is_error: false,
                },
                Err(e) => ToolOutcome { content: e, is_error: true },
            }
        }
        "glob" => {
            let pattern = params.get("pattern").and_then(Value::as_str).unwrap_or("").to_string();
            if pattern.is_empty() {
                return ToolOutcome { content: "pattern 必填".to_string(), is_error: true };
            }
            let conn = match state.db() {
                Ok(c) => c,
                Err(e) => return ToolOutcome { content: e, is_error: true },
            };
            match crate::commands::workspaces::glob_workspace(&conn, workspace_id, &pattern) {
                Ok(paths) => ToolOutcome {
                    content: if paths.is_empty() { "无匹配文件".to_string() } else { paths.join("\n") },
                    is_error: false,
                },
                Err(e) => ToolOutcome { content: e, is_error: true },
            }
        }
        "write_file" => {
            let content = params.get("content").and_then(Value::as_str).unwrap_or("").to_string();
            let conn = match state.db() {
                Ok(c) => c,
                Err(e) => return ToolOutcome { content: e, is_error: true },
            };
            match crate::commands::workspaces::write_workspace_file_content(
                &conn, workspace_id, &path, &content,
            ) {
                Ok(msg) => ToolOutcome { content: msg, is_error: false },
                Err(e) => ToolOutcome { content: e, is_error: true },
            }
        }
        "edit_file" => {
            let old_string = params.get("old_string").and_then(Value::as_str).unwrap_or("").to_string();
            let new_string = params.get("new_string").and_then(Value::as_str).unwrap_or("").to_string();
            let conn = match state.db() {
                Ok(c) => c,
                Err(e) => return ToolOutcome { content: e, is_error: true },
            };
            match crate::commands::workspaces::edit_workspace_file_content(
                &conn, workspace_id, &path, &old_string, &new_string,
            ) {
                Ok(msg) => ToolOutcome { content: msg, is_error: false },
                Err(e) => ToolOutcome { content: e, is_error: true },
            }
        }
        "delete_file" => {
            let conn = match state.db() {
                Ok(c) => c,
                Err(e) => return ToolOutcome { content: e, is_error: true },
            };
            match crate::commands::workspaces::delete_workspace_file_content(
                &conn, workspace_id, &path,
            ) {
                Ok(msg) => ToolOutcome { content: msg, is_error: false },
                Err(e) => ToolOutcome { content: e, is_error: true },
            }
        }
        "run_command" => {
            let command = params.get("command").and_then(Value::as_str).unwrap_or("").to_string();
            if command.trim().is_empty() {
                return ToolOutcome { content: "command 必填".to_string(), is_error: true };
            }
            let timeout_secs = params
                .get("timeout_seconds")
                .and_then(Value::as_u64)
                .unwrap_or(120)
                .clamp(1, 600);
            // 命令在工作区根目录执行（vibe coding 的项目目录）
            let cwd = {
                let conn = match state.db() {
                    Ok(c) => c,
                    Err(e) => return ToolOutcome { content: e, is_error: true },
                };
                match crate::commands::workspaces::workspace_root(&conn, workspace_id) {
                    Ok(p) => p,
                    Err(e) => return ToolOutcome { content: e, is_error: true },
                }
            };
            run_shell_command(
                &command,
                &cwd,
                std::time::Duration::from_secs(timeout_secs),
                abort,
            )
            .await
        }
        other => ToolOutcome {
            content: format!("未知工具: {other}"),
            is_error: true,
        },
    }
}

/// 输出截断上限（chars）：超长保留头尾，中间省略
const COMMAND_OUTPUT_LIMIT: usize = 20_000;

/// 轮询中断旗标；命中即返回（供 select! 与命令等待并行）
async fn wait_for_abort(flag: Arc<AtomicBool>) {
    loop {
        if flag.load(Ordering::Relaxed) {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    }
}

/// 在工作区目录执行 shell 命令：Windows cmd /C、类 Unix sh -c；
/// 三路等待：正常退出 / 超时终止 / 用户停止（abort）；输出超限头尾保留
async fn run_shell_command(
    command: &str,
    cwd: &std::path::Path,
    timeout: std::time::Duration,
    abort: Arc<AtomicBool>,
) -> ToolOutcome {
    use tokio::io::AsyncReadExt;

    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = tokio::process::Command::new("cmd");
        c.arg("/C").arg(command);
        c
    };
    #[cfg(not(target_os = "windows"))]
    let mut cmd = {
        let mut c = tokio::process::Command::new("sh");
        c.arg("-c").arg(command);
        c
    };
    cmd.current_dir(cwd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    // Windows 下隐藏控制台窗口
    #[cfg(target_os = "windows")]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return ToolOutcome { content: format!("命令启动失败: {e}"), is_error: true },
    };

    let mut stdout = child.stdout.take().expect("stdout piped");
    let mut stderr = child.stderr.take().expect("stderr piped");
    let out_task = tokio::spawn(async move {
        let mut buf: Vec<u8> = Vec::new();
        let _ = stdout.read_to_end(&mut buf).await;
        buf
    });
    let err_task = tokio::spawn(async move {
        let mut buf: Vec<u8> = Vec::new();
        let _ = stderr.read_to_end(&mut buf).await;
        buf
    });

    // 等待：正常退出 / 超时 / 用户停止；select 结束后借用释放，方可 kill
    enum Exit {
        Code(i32),
        Timeout,
        Aborted,
    }
    let exit = tokio::select! {
        r = child.wait() => match r {
            Ok(status) => Exit::Code(status.code().unwrap_or(-1)),
            Err(e) => {
                return ToolOutcome { content: format!("等待进程退出失败: {e}"), is_error: true };
            }
        },
        _ = tokio::time::sleep(timeout) => Exit::Timeout,
        _ = wait_for_abort(abort) => Exit::Aborted,
    };
    if !matches!(exit, Exit::Code(_)) {
        let _ = child.kill().await;
    }

    // 进程结束后管道关闭，读任务自然收尾
    let out_bytes = out_task.await.unwrap_or_default();
    let err_bytes = err_task.await.unwrap_or_default();
    let stdout_txt = String::from_utf8_lossy(&out_bytes);
    let stderr_txt = String::from_utf8_lossy(&err_bytes);

    let (exit_code, stop_note) = match exit {
        Exit::Code(c) => (c, None),
        Exit::Timeout => (-1, Some(format!("命令超过 {} 秒被强制终止", timeout.as_secs()))),
        Exit::Aborted => (-1, Some("已被用户停止".to_string())),
    };

    let mut content = format!("exit_code: {exit_code}");
    if !stdout_txt.trim().is_empty() {
        content.push_str("\n--- stdout ---\n");
        content.push_str(stdout_txt.trim_end());
    }
    if !stderr_txt.trim().is_empty() {
        content.push_str("\n--- stderr ---\n");
        content.push_str(stderr_txt.trim_end());
    }
    if let Some(note) = stop_note {
        content.push_str(&format!("\n（{note}）"));
    }

    // 超长输出：保留头尾，中间省略
    let chars: Vec<char> = content.chars().collect();
    if chars.len() > COMMAND_OUTPUT_LIMIT {
        let head: String = chars[..COMMAND_OUTPUT_LIMIT / 2].iter().collect();
        let tail: String = chars[chars.len() - COMMAND_OUTPUT_LIMIT / 2..].iter().collect();
        content = format!("{head}\n\n…[输出过长，中间已截断]…\n\n{tail}");
    }

    // 启动/等待失败才是工具错误；非零退出码交由模型自行判断
    ToolOutcome { content, is_error: false }
}

/// Agent 循环：在单个供应商/模型上执行，直到模型不再调用工具
pub async fn run(
    _app: &tauri::AppHandle,
    state: &AppState,
    req: ChatRequest,
    provider: crate::models::Provider,
    api_key: Option<String>,
    global_proxy: Option<crate::models::GlobalProxy>,
    abort: Arc<AtomicBool>,
    window: Window,
    sink: &DeltaSink,
) -> Result<ChatResponse, String> {
    // 工作区通过会话关联：读会话的 workspace_id
    let workspace_id = {
        let sid = req.session_id.clone().unwrap_or_default();
        if sid.is_empty() {
            None
        } else {
            match state.db() {
                Ok(conn) => conn
                    .query_row(
                        "SELECT workspace_id FROM sessions WHERE id = ?1",
                        rusqlite::params![sid],
                        |r| r.get::<_, Option<String>>(0),
                    )
                    .ok()
                    .flatten(),
                Err(_) => None,
            }
        }
    };

    let tools: Option<Vec<ToolDef>> = workspace_id.as_deref().map(|_| workspace_tools());

    // 内部消息列表：ChatMessage（含 tool_calls / tool_call_id），llm.rs 负责两协议序列化
    let mut messages: Vec<ChatMessage> = req.messages.clone();

    let mut final_content = String::new();
    let mut final_reasoning: Option<String> = None;
    let mut final_usage: Option<crate::models::TokenUsage> = None;

    for round in 0..MAX_AGENT_ROUNDS {
        if abort.load(std::sync::atomic::Ordering::Relaxed) {
            return Err("ABORTED:用户停止了生成".to_string());
        }

        // 构造本轮请求
        let mut round_req = req.clone();
        round_req.messages = messages.clone();
        round_req.tools = tools.clone();

        let ctx = LlmContext {
            provider: provider.clone(),
            api_key: api_key.clone(),
            abort: abort.clone(),
            global_proxy: global_proxy.clone(),
        };
        // 全程流式输出（工具调用轮一般无 text delta，最终回答轮正常流式）
        let mut resp = llm::execute(ctx, round_req, sink).await?;

        // DSML 泄漏救援：部分 DeepSeek 端点把工具调用以内部标记文本写进 content，
        // 解析回真正的工具调用并剔除垃圾标记，否则表现为"说完就停没下文"
        if crate::services::dsml::looks_like_dsml(&resp.content) {
            let (cleaned, leaked) = crate::services::dsml::extract_tool_calls(&resp.content);
            if !leaked.is_empty() {
                let mut calls = resp.tool_calls.take().unwrap_or_default();
                for (name, args) in leaked {
                    calls.push(ToolCall {
                        id: crate::utils::new_id(),
                        name,
                        arguments: args,
                    });
                }
                resp.content = cleaned;
                resp.tool_calls = Some(calls);
            }
        }

        // 只有「无工具调用」的轮次才算最终回答；工具调用轮的 content 是前言，
        // 不应作为最终输出（否则会把"先查看…"这类前言当结论截断）
        let is_final_round = resp.tool_calls.is_none();
        if is_final_round {
            final_content = resp.content.clone();
            final_reasoning = resp.reasoning.clone();
        }
        if resp.usage.is_some() {
            final_usage = resp.usage.clone();
        }

        let Some(calls) = resp.tool_calls else {
            // 无工具调用：本轮即最终回答
            break;
        };

        // 有工具调用：追加 assistant 消息（含 tool_calls），执行工具，追加结果
        let tool_calls_json: Value = json!(
            calls.iter().map(|c| json!({
                "id": c.id,
                "type": "function",
                "function": { "name": c.name, "arguments": c.arguments }
            })).collect::<Vec<_>>()
        );
        messages.push(ChatMessage {
            role: "assistant".to_string(),
            content: resp.content.clone(),
            tool_calls: Some(tool_calls_json),
            tool_call_id: None,
            tool_name: None,
        });

        let wid = workspace_id.clone().unwrap_or_default();
        for call in &calls {
            // 推送工具调用事件给前端
            let _ = window.emit(
                TOOL_EVENT,
                &json!({
                    "requestId": req.request_id,
                    "round": round,
                    "tool": call.name,
                    "arguments": call.arguments,
                    "status": "running",
                }),
            );

            let outcome = execute_tool(state, &wid, call, abort.clone()).await;
            let _ = window.emit(
                TOOL_EVENT,
                &json!({
                    "requestId": req.request_id,
                    "round": round,
                    "tool": call.name,
                    "arguments": call.arguments,
                    "status": outcome.is_error.then_some("error").unwrap_or("done"),
                    "result": outcome.content,
                    "isError": outcome.is_error,
                }),
            );

            // 写操作成功后通知前端刷新工作区文件树
            if !outcome.is_error
                && matches!(call.name.as_str(), "write_file" | "edit_file" | "delete_file")
            {
                let _ = window.emit("workspace:changed", &json!({ "workspaceId": wid }));
            }

            // 工具结果：tool 角色（OpenAI）；llm.rs 对 Anthropic 转 tool_result 块
            messages.push(ChatMessage {
                role: "tool".to_string(),
                content: outcome.content,
                tool_calls: None,
                tool_call_id: Some(call.id.clone()),
                tool_name: Some(call.name.clone()),
            });
        }
    }

    // 到达轮数上限被截断时，补一轮「无工具」收尾调用：
    // 让模型基于已有工具结果给出最终回答，而不是把工具轮的前言当结论
    if final_content.is_empty() && !messages.is_empty() {
        if abort.load(std::sync::atomic::Ordering::Relaxed) {
            return Err("ABORTED:用户停止了生成".to_string());
        }
        let mut closing_req = req.clone();
        closing_req.messages = messages.clone();
        closing_req.tools = None; // 不给工具，强制收尾
        let ctx = LlmContext {
            provider: provider.clone(),
            api_key: api_key.clone(),
            abort: abort.clone(),
            global_proxy: global_proxy.clone(),
        };
        let resp = llm::execute(ctx, closing_req, sink).await?;
        final_content = resp.content;
        final_reasoning = resp.reasoning;
        if resp.usage.is_some() {
            final_usage = resp.usage;
        }
    }

    Ok(ChatResponse {
        content: final_content,
        reasoning: final_reasoning,
        tool_calls: None,
        usage: final_usage,
        latency_ms: 0,
        provider_used: provider.name.clone(),
    })
}
