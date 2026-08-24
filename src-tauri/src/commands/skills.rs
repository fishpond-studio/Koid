//! Skills 命令（§4.7）：列表/保存/删除/运行/交互

use crate::models::SkillDef;
use crate::services::skills;
use crate::state::AppState;
use std::collections::HashMap;
use tauri::{AppHandle, State};

#[tauri::command]
pub fn list_skills(app: AppHandle) -> Result<Vec<SkillDef>, String> {
    let mut all = skills::builtin_skills();
    all.extend(skills::load_user_skills(&app));
    Ok(all)
}

#[tauri::command]
pub fn save_skill(content: String, app: AppHandle) -> Result<SkillDef, String> {
    skills::save_user_skill(&app, &content)
}

#[tauri::command]
pub fn delete_skill(id: String, app: AppHandle) -> Result<(), String> {
    skills::delete_user_skill(&app, &id)
}

/// 后台执行 Skill；进度经 `skill:event` 事件推送
#[tauri::command]
pub fn run_skill(
    request_id: String,
    skill_id: String,
    vars: HashMap<String, String>,
    app: AppHandle,
    _state: State<'_, AppState>,
) -> Result<(), String> {
    let skill = skills::builtin_skills()
        .into_iter()
        .chain(skills::load_user_skills(&app))
        .find(|s| s.id == skill_id)
        .ok_or_else(|| format!("Skill 不存在: {skill_id}"))?;

    let app_bg = app.clone();
    tauri::async_runtime::spawn(async move {
        skills::run(app_bg, skill, request_id, vars).await;
    });
    Ok(())
}

/// 前端提交 input 步骤的答案；返回 false 表示无等待中的请求
#[tauri::command]
pub fn skill_respond(
    request_id: String,
    value: String,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    Ok(match state.take_skill_input(&request_id) {
        Some(tx) => tx.send(value).is_ok(),
        None => false,
    })
}

#[tauri::command]
pub fn skill_cancel(request_id: String, state: State<'_, AppState>) -> Result<(), String> {
    state.cancel_skill(&request_id);
    Ok(())
}
