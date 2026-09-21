//! Skills 执行引擎（§4.7）
//!
//! 步骤类型：llm / condition / message / input / tool
//! 变量作用域：启动入参 vars + 各步骤输出（键为 `step_id.output`）
//! 交互：input 步骤经 AppState 的 oneshot 通道等待前端提交；
//!       全程通过 `skill:event` 事件向前端推送进度

use crate::models::{ChatMessage, ChatRequest, Model, Provider, SkillDef, SkillEvent};
use crate::services::{keyring, llm, settings};
use crate::state::AppState;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};

const SKILL_EVENT: &str = "skill:event";
/// 条件循环保护上限，防止 condition 跳转成环导致死循环
const MAX_STEP_VISITS: usize = 64;

// ---------- YAML 解析与校验 ----------

/// 解析标准 Agent SKILL.md 文件（YAML Frontmatter + Markdown Body）
pub fn sanitize_skill_id(id: &str) -> String {
    id.chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, '-' | '_'))
        .collect()
}

pub fn parse_skill_markdown(content: &str, fallback_id: &str) -> Result<SkillDef, String> {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return Err("缺少 YAML Frontmatter 开始标记 (---)".to_string());
    }
    let rest = &trimmed[3..];
    let end_idx = rest.find("---").ok_or_else(|| "缺少 YAML Frontmatter 结束标记 (---)".to_string())?;
    let frontmatter = &rest[..end_idx];
    let body = rest[end_idx + 3..].trim();

    #[derive(serde::Deserialize)]
    struct Frontmatter {
        id: Option<String>,
        name: Option<String>,
        description: Option<String>,
        icon: Option<String>,
        model: Option<String>,
        system_prompt: Option<String>,
        #[serde(rename = "systemPrompt")]
        system_prompt_camel: Option<String>,
    }

    let fm: Frontmatter = serde_yml::from_str(frontmatter)
        .map_err(|e| format!("Frontmatter 解析失败: {e}"))?;

    let raw_id = fm.id.as_deref().unwrap_or(fallback_id).trim();
    if raw_id.is_empty() {
        return Err("Skill id 不能为空".to_string());
    }
    if !raw_id.chars().all(|c| c.is_alphanumeric() || matches!(c, '-' | '_')) {
        return Err(format!("Skill id 包含非法字符，仅支持字母数字与 - _: {raw_id}"));
    }
    let safe_id = raw_id.to_string();

    let name = match fm.name.as_deref().map(str::trim) {
        Some(n) if !n.is_empty() => n.to_string(),
        _ => return Err("Skill name 不能为空".to_string()),
    };

    let description = fm.description.unwrap_or_default();
    let sys_prompt = fm.system_prompt.or(fm.system_prompt_camel);
    let id = safe_id;

    // 标准 SKILL.md 自动适配为标准步骤：用户输入 -> LLM 处理（基于 Markdown 指导） -> 输出消息
    let prompt_template = format!(
        "{}\n\n---\n用户输入：\n{{{{input.output}}}}",
        if body.is_empty() { "请执行该任务。" } else { body }
    );

    let steps = vec![
        crate::models::SkillStep {
            id: "input".to_string(),
            step_type: "input".to_string(),
            prompt: None,
            content: Some("请输入需要处理的内容或要求".to_string()),
            condition: None,
            then_step: None,
            else_step: None,
            tool: None,
            server: None,
            args: None,
        },
        crate::models::SkillStep {
            id: "execute".to_string(),
            step_type: "llm".to_string(),
            prompt: Some(prompt_template),
            content: None,
            condition: None,
            then_step: None,
            else_step: None,
            tool: None,
            server: None,
            args: None,
        },
        crate::models::SkillStep {
            id: "output".to_string(),
            step_type: "message".to_string(),
            prompt: None,
            content: Some("{{execute.output}}".to_string()),
            condition: None,
            then_step: None,
            else_step: None,
            tool: None,
            server: None,
            args: None,
        },
    ];

    Ok(SkillDef {
        id,
        name,
        description,
        icon: fm.icon,
        model: fm.model,
        system_prompt: sys_prompt,
        steps,
        source: "user".to_string(),
        enabled: true,
    })
}

pub fn parse_skill(content: &str) -> Result<SkillDef, String> {
    parse_skill_with_stem(content, "custom-skill")
}

pub fn parse_skill_with_stem(content: &str, stem: &str) -> Result<SkillDef, String> {
    let trimmed = content.trim_start();
    if trimmed.starts_with("---") {
        return parse_skill_markdown(trimmed, stem);
    }
    // C1: 只读兼容遗留旧版 YAML 工作流，防止升级后用户老旧技能静默损坏
    #[derive(serde::Deserialize)]
    struct LegacyYaml {
        id: Option<String>,
        name: Option<String>,
        description: Option<String>,
        icon: Option<String>,
        model: Option<String>,
        #[serde(rename = "systemPrompt")]
        system_prompt: Option<String>,
        #[serde(default)]
        steps: Vec<crate::models::SkillStep>,
    }
    if let Ok(legacy) = serde_yml::from_str::<LegacyYaml>(content) {
        let raw_id = legacy.id.as_deref().unwrap_or(stem).trim();
        let safe_id = sanitize_skill_id(raw_id);
        if !safe_id.is_empty() && legacy.name.is_some() && !legacy.steps.is_empty() {
            return Ok(SkillDef {
                id: safe_id,
                name: legacy.name.unwrap(),
                description: legacy.description.unwrap_or_default(),
                icon: legacy.icon,
                model: legacy.model,
                system_prompt: legacy.system_prompt,
                steps: legacy.steps,
                source: "user".to_string(),
                enabled: true,
            });
        }
    }
    Err("Skill 必须为标准 SKILL.md 格式（以 --- YAML Frontmatter 开头）或包含有效 steps 的工作流定义".to_string())
}

// ---------- 模板变量 ----------

/// 替换 {{key}}：key 支持 `name` 与 `step_id.output`，未定义变量替换为空串
pub fn resolve_template(text: &str, scope: &HashMap<String, String>) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        match after.find("}}") {
            Some(end) => {
                let key = after[..end].trim();
                out.push_str(scope.get(key).map(String::as_str).unwrap_or(""));
                rest = &after[end + 2..];
            }
            None => {
                out.push_str("{{");
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

// ---------- condition 表达式 ----------

/// 从最后一个引号段提取 needle；引号前为 source（变量引用或字面量）
fn parse_contains(expr: &str) -> Result<(String, String), String> {
    let e = expr.trim();
    let inner = e
        .strip_prefix("contains")
        .ok_or_else(|| "condition 仅支持 contains(...)".to_string())?
        .trim_start()
        .strip_prefix('(')
        .and_then(|s| s.trim_end().strip_suffix(')'))
        .ok_or_else(|| "condition 缺少括号".to_string())?
        .trim();

    // 定位最后一个引号段作为 needle
    let bytes = inner.as_bytes();
    let mut quote_char: Option<char> = None;
    let mut close_idx: Option<usize> = None;
    for (i, ch) in inner.char_indices().rev() {
        if ch == '\'' || ch == '"' {
            quote_char = Some(ch);
            close_idx = Some(i);
            break;
        }
        // needle 之后不允许出现其他字符（空白除外）
        if !ch.is_whitespace() {
            return Err("condition 的 needle 必须是引号字符串".to_string());
        }
        let _ = bytes;
    }
    let (Some(q), Some(close)) = (quote_char, close_idx) else {
        return Err("condition 缺少 needle".to_string());
    };
    let open = inner[..close]
        .rfind(q)
        .ok_or_else(|| "condition 引号不配对".to_string())?;
    let needle = inner[open + 1..close].to_string();
    let source = inner[..open].trim().trim_end_matches(',').trim().to_string();
    if source.is_empty() {
        return Err("condition 缺少 source".to_string());
    }
    Ok((source, needle))
}

/// 求值：contains( {{step.output}}, 'text' )，大小写不敏感
pub fn eval_condition(expr: &str, scope: &HashMap<String, String>) -> Result<bool, String> {
    let (source, needle) = parse_contains(expr)?;
    let resolved = if source.starts_with("{{") && source.ends_with("}}") {
        let key = source[2..source.len() - 2].trim();
        scope.get(key).cloned().unwrap_or_default()
    } else if (source.starts_with('\'') && source.ends_with('\''))
        || (source.starts_with('"') && source.ends_with('"'))
    {
        source[1..source.len() - 1].to_string()
    } else {
        return Err(format!("condition source 无法识别: {source}"));
    };
    Ok(resolved.to_lowercase().contains(&needle.to_lowercase()))
}

// ---------- 存储 ----------

/// 内置 Skill（编译期嵌入，保证开箱即用与验收样例存在）
pub fn builtin_skills() -> Vec<SkillDef> {
    let mut out = Vec::new();

    // 纯标准 Agent 规范：内置 SKILL.md (YAML Frontmatter + Markdown)
    let standard_skills: &[(&str, &str)] = &[
        ("code-review", include_str!("../../../skills/code-review/SKILL.md")),
        ("explain-error", include_str!("../../../skills/explain-error/SKILL.md")),
    ];
    for (id, content) in standard_skills {
        if let Ok(mut def) = parse_skill_markdown(content, id) {
            def.source = "builtin".to_string();
            out.push(def);
        }
    }

    out
}

pub fn user_skills_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("无法定位数据目录: {e}"))?
        .join("skills");
    Ok(dir)
}

fn global_skills_dirs(app: &AppHandle) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(home) = app.path().home_dir() {
        // 1. npx skills -g 默认全局安装目录: ~/.skills/
        dirs.push(home.join(".skills"));
        // 2. 常见 agent 生态目录: ~/.agents/skills/
        dirs.push(home.join(".agents").join("skills"));
    }
    dirs
}

fn scan_skills_in_dir(dir: &std::path::Path, source_label: &str) -> Vec<SkillDef> {
    let Ok(entries) = std::fs::read_dir(dir) else { return vec![] };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        // 1. 标准 Agent 技能目录: <dir>/<name>/SKILL.md
        if path.is_dir() {
            let skill_md = path.join("SKILL.md");
            if skill_md.is_file() {
                if let Ok(content) = std::fs::read_to_string(&skill_md) {
                    let dir_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("skill");
                    if let Ok(mut def) = parse_skill_markdown(&content, dir_name) {
                        def.source = source_label.to_string();
                        out.push(def);
                        continue;
                    }
                }
            }
        }

        // 2. 支持单文件 .md 以及遗留 .yaml / .yml (只读兼容)
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if matches!(ext, "md" | "yaml" | "yml") {
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("skill");
            if let Ok(content) = std::fs::read_to_string(&path) {
                match parse_skill_with_stem(&content, stem) {
                    Ok(mut def) => {
                        // S2 安全门：全局目录 ~/.skills 等不受信来源默认 enabled = false，需用户在界面上显式确认
                        def.source = source_label.to_string();
                        if source_label == "global" {
                            def.enabled = false;
                        }
                        out.push(def);
                    }
                    Err(e) => {
                        eprintln!("[koid::skills] 忽略无效技能文件 {:?}: {e}", path);
                    }
                }
            }
        }
    }
    out
}
pub fn load_user_skills(app: &AppHandle) -> Vec<SkillDef> {
    let mut out = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();

    // 1. 扫描 Koid 自身应用数据目录
    if let Ok(dir) = user_skills_dir(app) {
        for s in scan_skills_in_dir(&dir, "user") {
            if seen_ids.insert(s.id.clone()) {
                out.push(s);
            }
        }
    }

    // 2. 扫描系统全局目录 (如 npx skills add -g 安装的 ~/.skills/)
    for global_dir in global_skills_dirs(app) {
        for s in scan_skills_in_dir(&global_dir, "global") {
            if seen_ids.insert(s.id.clone()) {
                out.push(s);
            }
        }
    }

    out
}

pub fn save_user_skill(app: &AppHandle, yaml: &str) -> Result<SkillDef, String> {
    let def = parse_skill(yaml)?;
    // 文件名净化：仅保留安全字符，避免路径穿越
    let safe_id: String = def
        .id
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, '-' | '_'))
        .collect();
    if safe_id.is_empty() {
        return Err("id 仅支持字母数字与 - _".to_string());
    }
    let dir = user_skills_dir(app)?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建目录失败: {e}"))?;
    let file_name = format!("{safe_id}.md");
    std::fs::write(dir.join(file_name), yaml)
        .map_err(|e| format!("写入 Skill 失败: {e}"))?;
    let mut saved = def;
    saved.source = "user".to_string();
    Ok(saved)
}

pub fn delete_user_skill(app: &AppHandle, id: &str) -> Result<(), String> {
    let safe_id = sanitize_skill_id(id.trim());
    if safe_id.is_empty() {
        return Err("非法或空的 Skill id".to_string());
    }
    let dir = user_skills_dir(app)?;
    if !dir.exists() {
        return Err("用户技能目录不存在".to_string());
    }
    let canon_dir = dir.canonicalize().map_err(|e| format!("无法解析目录路径: {e}"))?;

    // 1. 删除单文件 <id>.md
    let file_path = dir.join(format!("{safe_id}.md"));
    if file_path.exists() {
        if let Ok(canon_file) = file_path.canonicalize() {
            if canon_file.starts_with(&canon_dir) && canon_file != canon_dir {
                return std::fs::remove_file(&canon_file).map_err(|e| format!("删除失败: {e}"));
            }
        }
    }

    // 2. 删除技能子目录 <id>/
    let sub_dir = dir.join(&safe_id);
    if sub_dir.is_dir() {
        if let Ok(canon_subdir) = sub_dir.canonicalize() {
            if canon_subdir.starts_with(&canon_dir) && canon_subdir != canon_dir {
                return std::fs::remove_dir_all(&canon_subdir).map_err(|e| format!("删除目录失败: {e}"));
            }
        }
    }

    Err("内置或全局 Skill 不支持在应用内直接删除，或技能文件不存在".to_string())
}

// ---------- 模型解析 ----------

/// 选定执行模型：按 hint（model_id/displayName）匹配，否则取首个可用
fn resolve_model(st: &AppState, hint: Option<&str>) -> Result<(Model, Provider), String> {
    let (models, providers) = {
        let conn = st.db()?;
        (
            crate::commands::models::load_models(&conn, None)?,
            crate::commands::providers::load_providers(&conn)?,
        )
    };
    let usable: Vec<(Model, Provider)> = models
        .into_iter()
        .filter(|m| m.enabled)
        .filter_map(|m| {
            providers
                .iter()
                .find(|p| p.id == m.provider_id && p.enabled)
                .map(|p| (m, p.clone()))
        })
        .collect();

    if usable.is_empty() {
        return Err("UNAUTHORIZED:没有可用的供应商/模型，请先在设置中配置".to_string());
    }
    if let Some(h) = hint {
        if let Some(hit) = usable
            .iter()
            .find(|(m, _)| m.model_id == h || m.display_name == h)
        {
            return Ok(hit.clone());
        }
    }
    Ok(usable.into_iter().next().unwrap())
}

// ---------- 执行引擎 ----------

/// 运行 Skill：在后台任务中逐步执行并推送事件
pub async fn run(app: AppHandle, skill: SkillDef, request_id: String, vars: HashMap<String, String>) {
    let st = app.state::<AppState>();
    st.clear_skill_cancel(&request_id);

    let emit = |kind: &str,
                step_id: Option<String>,
                label: Option<String>,
                content: Option<String>,
                error: Option<String>,
                progress: Option<f64>| {
        let _ = app.emit(
            SKILL_EVENT,
            &SkillEvent {
                request_id: request_id.clone(),
                skill_id: skill.id.clone(),
                kind: kind.to_string(),
                step_id,
                label,
                content,
                error,
                progress,
            },
        );
    };

    emit("started", None, None, None, None, None);

    // 解析执行模型（hint 优先，回退首个可用）
    let (model, provider) = match resolve_model(&st, skill.model.as_deref()) {
        Ok(x) => x,
        Err(e) => {
            emit("error", None, None, None, Some(e), None);
            return;
        }
    };
    let api_key = keyring::get_api_key(&provider.id);
    let global_proxy = { st.db().ok().and_then(|conn| settings::get_global_proxy(&conn)) };

    let abort = Arc::new(AtomicBool::new(false));
    st.register_active(&request_id, abort.clone());

    let mut scope = vars;
    let step_index: HashMap<String, usize> = skill
        .steps
        .iter()
        .enumerate()
        .map(|(i, s)| (s.id.clone(), i))
        .collect();

    let total = skill.steps.len() as f64;
    let mut i = 0usize;
    let mut visits = 0usize;

    while i < skill.steps.len() {
        if st.is_skill_cancelled(&request_id) || abort.load(Ordering::Relaxed) {
            emit("cancelled", None, None, None, None, None);
            st.remove_active(&request_id);
            return;
        }
        visits += 1;
        if visits > MAX_STEP_VISITS {
            emit(
                "error",
                None,
                None,
                None,
                Some("步骤执行次数超限，可能存在条件死循环".to_string()),
                None,
            );
            st.remove_active(&request_id);
            return;
        }

        let step = &skill.steps[i];
        emit(
            "step",
            Some(step.id.clone()),
            None,
            None,
            None,
            Some(i as f64 / total),
        );

        match step.step_type.as_str() {
            "input" => {
                let rx = {
                    let (tx, rx) = tokio::sync::oneshot::channel();
                    st.register_skill_input(&request_id, tx);
                    rx
                };
                emit(
                    "input-required",
                    Some(step.id.clone()),
                    Some(step.content.clone().unwrap_or_default()),
                    None,
                    None,
                    None,
                );
                // 等待用户提交；通道被丢弃 = 运行被取消
                let value = rx.await.unwrap_or_default();
                if st.is_skill_cancelled(&request_id) {
                    emit("cancelled", None, None, None, None, None);
                    st.remove_active(&request_id);
                    return;
                }
                scope.insert(format!("{}.output", step.id), value);
                i += 1;
            }
            "llm" => {
                let prompt = resolve_template(step.prompt.as_deref().unwrap_or(""), &scope);
                let ctx = llm::LlmContext {
                    provider: provider.clone(),
                    api_key: api_key.clone(),
                    abort: abort.clone(),
                    global_proxy: global_proxy.clone(),
                };
                let req = ChatRequest {
                    request_id: format!("{}:{}", request_id, step.id),
                    provider_id: provider.id.clone(),
                    model_id: model.model_id.clone(),
                    messages: vec![ChatMessage {
                        role: "user".to_string(),
                        content: prompt,
                        tool_calls: None,
                        tool_call_id: None,
                        tool_name: None,
                    }],
                    temperature: None,
                    top_p: None,
                    max_tokens: None,
                    system: if skill.source == "global" { None } else { skill.system_prompt.clone() },
                    stream: false,
                    thinking_level: None,
                    session_id: None,
                    tools: None,
                };
                let noop: llm::DeltaSink = Box::new(|_, _| {});
                match llm::execute(ctx, req, &noop).await {
                    Ok(resp) => {
                        scope.insert(format!("{}.output", step.id), resp.content.clone());
                        emit("output", Some(step.id.clone()), None, Some(resp.content), None, None);
                        i += 1;
                    }
                    Err(e) => {
                        emit("error", Some(step.id.clone()), None, None, Some(e), None);
                        st.remove_active(&request_id);
                        return;
                    }
                }
            }
            "condition" => {
                match eval_condition(step.condition.as_deref().unwrap_or(""), &scope) {
                    Ok(pass) => {
                        let target = if pass {
                            step.then_step.as_deref()
                        } else {
                            step.else_step.as_deref()
                        };
                        match target.and_then(|t| step_index.get(t)) {
                            Some(&j) => i = j,
                            None => i += 1,
                        }
                    }
                    Err(e) => {
                        emit("error", Some(step.id.clone()), None, None, Some(e), None);
                        st.remove_active(&request_id);
                        return;
                    }
                }
            }
            "message" => {
                let content = resolve_template(step.content.as_deref().unwrap_or(""), &scope);
                emit("message", Some(step.id.clone()), None, Some(content), None, Some(1.0));
                emit("done", None, None, None, None, Some(1.0));
                st.remove_active(&request_id);
                st.clear_skill_cancel(&request_id);
                return;
            }
            "tool" => {
                // MCP 工具调用：args 支持模板变量
                let args = resolve_template(step.args.as_deref().unwrap_or("{}"), &scope);
                match crate::services::mcp::call_tool_by_name(
                    &st,
                    step.server.as_deref(),
                    step.tool.as_deref().unwrap_or(""),
                    &args,
                )
                .await
                {
                    Ok(out) => {
                        scope.insert(format!("{}.output", step.id), out.clone());
                        emit("output", Some(step.id.clone()), None, Some(out), None, None);
                        i += 1;
                    }
                    Err(e) => {
                        emit("error", Some(step.id.clone()), None, None, Some(e), None);
                        st.remove_active(&request_id);
                        return;
                    }
                }
            }
            other => {
                emit(
                    "error",
                    Some(step.id.clone()),
                    None,
                    None,
                    Some(format!("未知步骤类型: {other}")),
                    None,
                );
                st.remove_active(&request_id);
                return;
            }
        }
    }

    // 所有步骤顺序走完但无 message 终止步骤：补发 done
    emit("done", None, None, None, None, Some(1.0));
    st.remove_active(&request_id);
    st.clear_skill_cancel(&request_id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_resolves_vars_and_step_outputs() {
        let mut scope = HashMap::new();
        scope.insert("language".to_string(), "Rust".to_string());
        scope.insert("read.output".to_string(), "fn main() {}".to_string());
        let out = resolve_template("用 {{language}} 审查：{{read.output}}（缺省 {{missing}}）", &scope);
        assert_eq!(out, "用 Rust 审查：fn main() {}（缺省 ）");
        // 未闭合 {{ 原样保留
        assert_eq!(resolve_template("半截 {{x", &scope), "半截 {{x");
    }

    #[test]
    fn condition_contains_with_ref_and_case_insensitive() {
        let mut scope = HashMap::new();
        scope.insert("review.output".to_string(), "Found a BUG in line 3".to_string());
        assert!(eval_condition("contains( {{review.output}}, 'bug' )", &scope).unwrap());
        assert!(!eval_condition("contains( {{review.output}}, 'memory leak' )", &scope).unwrap());
        // 缺失变量视为空串
        assert!(!eval_condition("contains( {{nope.output}}, 'x' )", &scope).unwrap());
        // 字面量 source
        assert!(eval_condition("contains('hello world', 'WORLD')", &scope).unwrap());
    }

    #[test]
    fn condition_rejects_malformed() {
        let scope = HashMap::new();
        assert!(eval_condition("not_contains(x, 'y')", &scope).is_err());
        assert!(eval_condition("contains(x, y)", &scope).is_err());
    }

    #[test]
    fn builtin_markdown_parses() {
        let skills = builtin_skills();
        assert_eq!(skills.len(), 2);
        let review = skills.iter().find(|s| s.id == "code-review").unwrap();
        // 标准 SKILL.md 自动映射为三段标准流程：input -> execute(llm) -> output(message)
        assert_eq!(review.steps.len(), 3);
        let ids: Vec<&str> = review.steps.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec!["input", "execute", "output"]);
        assert!(review.system_prompt.as_deref().unwrap().contains("代码审查"));
    }

    #[test]
    fn validation_catches_bad_skill() {
        // B2/S1: 针对 parse_skill_markdown 的真实字段与格式校验
        assert!(parse_skill("not markdown or yaml").is_err());
        assert!(parse_skill("---\nid: \"\"\nname: \"\"\n---\nbody").is_err());
        assert!(parse_skill("---\nid: valid-id\nname: \"\"\n---\nbody").is_err());
        assert!(parse_skill("---\nid: \"invalid/id/with/slash\"\nname: Test\n---\nbody").is_err());
        assert!(parse_skill("---\nname: MissingIdFallback\n---\nbody").is_ok());
    }
}
