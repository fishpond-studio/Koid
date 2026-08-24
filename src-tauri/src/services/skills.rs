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

pub fn parse_skill(yaml: &str) -> Result<SkillDef, String> {
    let mut def: SkillDef =
        serde_yml::from_str(yaml).map_err(|e| format!("YAML 解析失败: {e}"))?;
    validate(&def)?;
    if def.source.is_empty() {
        def.source = "user".to_string();
    }
    def.enabled = true;
    Ok(def)
}

fn validate(def: &SkillDef) -> Result<(), String> {
    if def.id.trim().is_empty() {
        return Err("缺少 id".to_string());
    }
    if def.name.trim().is_empty() {
        return Err("缺少 name".to_string());
    }
    if def.steps.is_empty() {
        return Err("Skill 至少需要一个步骤".to_string());
    }
    // 步骤 id 唯一
    let mut seen = std::collections::HashSet::new();
    for s in &def.steps {
        if s.id.trim().is_empty() {
            return Err("存在无 id 的步骤".to_string());
        }
        if !seen.insert(s.id.clone()) {
            return Err(format!("步骤 id 重复: {}", s.id));
        }
    }
    // 步骤必填字段 + condition 跳转目标存在性
    for s in &def.steps {
        match s.step_type.as_str() {
            "llm" => {
                if s.prompt.as_deref().unwrap_or("").is_empty() {
                    return Err(format!("llm 步骤 {} 缺少 prompt", s.id));
                }
            }
            "condition" => {
                if s.condition.as_deref().unwrap_or("").is_empty() {
                    return Err(format!("condition 步骤 {} 缺少 condition", s.id));
                }
                for target in [&s.then_step, &s.else_step].into_iter().flatten() {
                    if !seen.contains(target) {
                        return Err(format!("步骤 {} 跳转目标不存在: {target}", s.id));
                    }
                }
            }
            "message" | "input" => {
                if s.content.as_deref().unwrap_or("").is_empty() {
                    return Err(format!("{} 步骤 {} 缺少 content", s.step_type, s.id));
                }
            }
            "tool" => {
                if s.tool.as_deref().unwrap_or("").is_empty() {
                    return Err(format!("tool 步骤 {} 缺少 tool", s.id));
                }
            }
            other => return Err(format!("未知步骤类型: {other}")),
        }
    }
    Ok(())
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
    let yamls: &[&str] = &[
        include_str!("../../builtin-skills/code-review.yaml"),
        include_str!("../../builtin-skills/explain-error.yaml"),
    ];
    yamls
        .iter()
        .filter_map(|y| {
            parse_skill(y)
                .ok()
                .map(|mut d| {
                    d.source = "builtin".to_string();
                    d
                })
        })
        .collect()
}

pub fn user_skills_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("无法定位数据目录: {e}"))?
        .join("skills");
    Ok(dir)
}

pub fn load_user_skills(app: &AppHandle) -> Vec<SkillDef> {
    let Ok(dir) = user_skills_dir(app) else { return vec![] };
    let Ok(entries) = std::fs::read_dir(&dir) else { return vec![] };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let is_yaml = matches!(path.extension().and_then(|e| e.to_str()), Some("yaml") | Some("yml"));
        if !is_yaml {
            continue;
        }
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(mut def) = parse_skill(&content) {
                def.source = "user".to_string();
                out.push(def);
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
    std::fs::write(dir.join(format!("{safe_id}.yaml")), yaml)
        .map_err(|e| format!("写入 Skill 失败: {e}"))?;
    let mut saved = def;
    saved.source = "user".to_string();
    Ok(saved)
}

pub fn delete_user_skill(app: &AppHandle, id: &str) -> Result<(), String> {
    let dir = user_skills_dir(app)?;
    for ext in ["yaml", "yml"] {
        let path = dir.join(format!("{id}.{ext}"));
        if path.exists() {
            return std::fs::remove_file(&path).map_err(|e| format!("删除失败: {e}"));
        }
    }
    Err("内置 Skill 不可删除或不存在".to_string())
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
                    system: skill.system_prompt.clone(),
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
    fn builtin_yaml_parses() {
        let skills = builtin_skills();
        assert_eq!(skills.len(), 2);
        let review = skills.iter().find(|s| s.id == "code-review").unwrap();
        assert_eq!(review.steps.len(), 5);
        let ids: Vec<&str> = review.steps.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec!["read", "review", "check", "fix", "done"]);
    }

    #[test]
    fn validation_catches_bad_skill() {
        assert!(parse_skill("id: x\nname: y\nsteps: []").is_err());
        assert!(parse_skill("not yaml: [").is_err());
    }
}
