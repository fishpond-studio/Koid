//! 工作区管理（§4.5：Workspace → Folder → Session 顶层分组）+ 工作区文件访问
//!
//! 文件访问（vibe coding）：list_workspace_files / read_workspace_file
//! 限定在工作区 path 目录内，路径净化防穿越，单文件 1MB 上限（§7.3）

use crate::models::{Workspace, WorkspaceInput, WorkspaceFileEntry};
use crate::state::AppState;
use crate::utils;
use rusqlite::{params, Connection};
use std::path::{Component, Path, PathBuf};
use tauri::State;

const SELECT_COLS: &str = "id, name, order_index, created_at, path";

/// 文件树遍历上限：目录深度 + 总条目，避免大项目卡死
const MAX_DEPTH: usize = 8;
const MAX_ENTRIES: usize = 2000;
/// 跳过的大型/无关目录
const SKIP_DIRS: &[&str] = &["node_modules", ".git", "target", "dist", "build", ".next", ".venv", "vendor"];

fn workspace_from_row(row: &rusqlite::Row) -> rusqlite::Result<Workspace> {
    Ok(Workspace {
        id: row.get(0)?,
        name: row.get(1)?,
        order_index: row.get(2)?,
        created_at: row.get(3)?,
        path: row.get(4)?,
    })
}

fn load_workspace(conn: &Connection, id: &str) -> Result<Workspace, String> {
    conn.query_row(
        &format!("SELECT {SELECT_COLS} FROM workspaces WHERE id = ?1"),
        params![id],
        workspace_from_row,
    )
    .map_err(|_| format!("工作区不存在: {id}"))
}

/// 工作区根目录；未设置路径时报错
pub(crate) fn workspace_root(conn: &Connection, id: &str) -> Result<PathBuf, String> {
    let ws = load_workspace(conn, id)?;
    match ws.path.filter(|p| !p.trim().is_empty()) {
        Some(p) => {
            let root = PathBuf::from(p);
            if !root.is_dir() {
                return Err("工作区路径不是有效目录".to_string());
            }
            Ok(root)
        }
        None => Err("该工作区尚未设置项目路径".to_string()),
    }
}

/// 递归列出工作区文件（跳过隐藏文件与 SKIP_DIRS）
fn walk_dir(root: &Path, dir: &Path, depth: usize, out: &mut Vec<WorkspaceFileEntry>) -> Result<(), String> {
    if out.len() >= MAX_ENTRIES {
        return Ok(());
    }
    let entries = std::fs::read_dir(dir).map_err(|e| format!("读取目录失败: {e}"))?;
    let mut items: Vec<_> = entries.flatten().collect();
    items.sort_by_key(|e| e.file_name());

    for entry in items {
        if out.len() >= MAX_ENTRIES {
            return Ok(());
        }
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy().to_string();
        // 隐藏文件/目录跳过
        if name.starts_with('.') {
            continue;
        }
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if is_dir && SKIP_DIRS.contains(&name.as_str()) {
            continue;
        }
        let full_path = entry.path();
        let rel = full_path
            .strip_prefix(root)
            .unwrap_or(&full_path)
            .to_string_lossy()
            .replace('\\', "/");
        out.push(WorkspaceFileEntry {
            path: rel,
            name,
            is_dir,
        });
        if is_dir && depth < MAX_DEPTH {
            walk_dir(root, &full_path, depth + 1, out)?;
        }
    }
    Ok(())
}

/// 相对路径净化：拒绝绝对路径与 .. 穿越
fn safe_rel(rel: &str) -> Result<PathBuf, String> {
    let p = Path::new(rel);
    if p.is_absolute() {
        return Err("不允许绝对路径".to_string());
    }
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::Normal(seg) => out.push(seg),
            Component::CurDir => {}
            Component::ParentDir => return Err("路径不允许 .. 穿越".to_string()),
            _ => return Err("非法路径".to_string()),
        }
    }
    if out.as_os_str().is_empty() {
        return Err("路径为空".to_string());
    }
    Ok(out)
}

/// 工作区文件树（§vibe coding）
#[tauri::command]
pub fn list_workspace_files(
    workspace_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<WorkspaceFileEntry>, String> {
    let conn = state.db()?;
    let root = workspace_root(&conn, &workspace_id)?;
    let mut out = Vec::new();
    walk_dir(&root, &root, 0, &mut out)?;
    Ok(out)
}

/// 读取工作区文件内容（≤1MB，§7.3）
#[tauri::command]
pub fn read_workspace_file(
    workspace_id: String,
    rel_path: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let conn = state.db()?;
    read_workspace_file_content(&conn, &workspace_id, &rel_path)
}

/// Agent 工具：列出指定目录（直接项，非递归）
pub(crate) fn list_workspace_dir(
    conn: &Connection,
    workspace_id: &str,
    rel: &str,
) -> Result<Vec<(String, bool)>, String> {
    let root = workspace_root(conn, workspace_id)?;
    let dir = if rel.trim().is_empty() {
        root.clone()
    } else {
        let safe = safe_rel(rel)?;
        let full = root.join(&safe);
        if !full.starts_with(&root) {
            return Err("路径越界".to_string());
        }
        full
    };
    if !dir.is_dir() {
        return Err(format!("目录不存在: {rel}"));
    }
    let entries = std::fs::read_dir(&dir).map_err(|e| format!("读取目录失败: {e}"))?;
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        out.push((name, is_dir));
    }
    out.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
    Ok(out)
}

/// Agent 工具：读取文件内容
pub(crate) fn read_workspace_file_content(
    conn: &Connection,
    workspace_id: &str,
    rel_path: &str,
) -> Result<String, String> {
    let root = workspace_root(conn, workspace_id)?;
    let rel = safe_rel(rel_path)?;
    let full = root.join(&rel);
    if !full.starts_with(&root) {
        return Err("路径越界".to_string());
    }
    if !full.is_file() {
        return Err(format!("文件不存在: {rel_path}"));
    }
    let meta = std::fs::metadata(&full).map_err(|e| e.to_string())?;
    if meta.len() > 1024 * 1024 {
        return Err("文件超过 1MB，请分块处理（§7.3）".to_string());
    }
    std::fs::read_to_string(&full).map_err(|e| format!("读取失败: {e}"))
}

/// 递归遍历工作区（跳过隐藏文件与 SKIP_DIRS），返回相对路径列表（仅文件）
fn walk_files(root: &Path, dir: &Path, depth: usize, out: &mut Vec<PathBuf>) -> Result<(), String> {
    if out.len() >= MAX_ENTRIES {
        return Ok(());
    }
    let entries = std::fs::read_dir(dir).map_err(|e| format!("读取目录失败: {e}"))?;
    for entry in entries.flatten() {
        if out.len() >= MAX_ENTRIES {
            return Ok(());
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if is_dir && SKIP_DIRS.contains(&name.as_str()) {
            continue;
        }
        let full = entry.path();
        if is_dir {
            if depth < MAX_DEPTH {
                walk_files(root, &full, depth + 1, out)?;
            }
        } else {
            out.push(full);
        }
    }
    Ok(())
}

/// 简单 glob → 正则：`**` → 任意层级，`*` → 非分隔符任意，`?` → 单字符
fn glob_to_regex(pattern: &str) -> String {
    let mut re = String::from("^");
    let bytes = pattern.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        match c {
            '*' => {
                // ** 或 *，处理边界
                let is_double = i + 1 < bytes.len() && bytes[i + 1] as char == '*';
                if is_double {
                    re.push_str(".*");
                    i += 1;
                } else {
                    re.push_str("[^/]*");
                }
            }
            '?' => re.push_str("[^/]"),
            '.' | '+' | '(' | ')' | '[' | ']' | '{' | '}' | '^' | '$' | '|' | '\\' => {
                re.push('\\');
                re.push(c);
            }
            _ => re.push(c),
        }
        i += 1;
    }
    re.push('$');
    re
}

/// Agent 工具：正则全文搜索（对齐 opencode grep）
pub(crate) fn grep_workspace(
    conn: &Connection,
    workspace_id: &str,
    pattern: &str,
    include: Option<&str>,
) -> Result<Vec<String>, String> {
    let root = workspace_root(conn, workspace_id)?;
    let mut files = Vec::new();
    walk_files(&root, &root, 0, &mut files)?;

    // 正则匹配（工具描述向模型宣称的是正则语义）；编译失败回退为字面量包含
    let re = regex::Regex::new(pattern).ok();
    // 总读取字节上限：约束最坏情况，防止大项目 grep 长时间占用 worker
    const MAX_TOTAL_BYTES: u64 = 32 * 1024 * 1024;
    let mut total_read: u64 = 0;

    let mut out = Vec::new();
    for file in files {
        if out.len() >= 200 || total_read >= MAX_TOTAL_BYTES {
            break;
        }
        let rel = file
            .strip_prefix(&root)
            .unwrap_or(&file)
            .to_string_lossy()
            .replace('\\', "/");
        // include 过滤（如 *.ts）
        if let Some(inc) = include {
            let inc = inc.trim_start_matches("*");
            if !rel.ends_with(inc) {
                continue;
            }
        }
        // 只搜索文本文件（跳过明显二进制）& 1MB 上限
        if let Ok(meta) = std::fs::metadata(&file) {
            if meta.len() > 1024 * 1024 {
                continue;
            }
        }
        let Ok(content) = std::fs::read_to_string(&file) else {
            continue;
        };
        total_read += content.len() as u64;
        for (idx, line) in content.lines().enumerate() {
            let hit = match &re {
                Some(r) => r.is_match(line),
                None => line.contains(pattern),
            };
            if hit {
                let num = idx + 1;
                out.push(format!("{rel}:{num}: {line}"));
                if out.len() >= 200 {
                    break;
                }
            }
        }
    }
    Ok(out)
}

/// Agent 工具：glob 模式查找文件（对齐 opencode glob）
pub(crate) fn glob_workspace(
    conn: &Connection,
    workspace_id: &str,
    pattern: &str,
) -> Result<Vec<String>, String> {
    let root = workspace_root(conn, workspace_id)?;
    let mut files = Vec::new();
    walk_files(&root, &root, 0, &mut files)?;

    let regex = glob_to_regex(pattern);
    let mut out: Vec<String> = files
        .iter()
        .map(|f| {
            f.strip_prefix(&root)
                .unwrap_or(f)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .filter(|rel| {
            // 简化匹配：也允许 `**` 开头的模式命中根层
            let full_match = simple_regex_match(&regex, rel);
            if full_match {
                return true;
            }
            // 处理 **/ 前缀（任意层级）
            if let Some(stripped) = pattern.strip_prefix("**/") {
                let re2 = glob_to_regex(stripped);
                return simple_regex_match(&re2, rel);
            }
            false
        })
        .take(200)
        .collect();
    out.sort();
    Ok(out)
}

/// 极简正则匹配（仅支持我们生成的 ^...$ + .* [^/]* 结构）
fn simple_regex_match(re: &str, text: &str) -> bool {
    // 把 glob_to_regex 的输出转回可直接比较的形式
    let body = re.trim_start_matches('^').trim_end_matches('$');
    // 无通配符：直接相等
    if !body.contains(".*") && !body.contains("[^/]*") && !body.contains("\\") {
        return body == text;
    }
    // 简单通配匹配：拆段
    let mut parts: Vec<&str> = Vec::new();
    let mut rest = body;
    while let Some(pos) = rest.find(".*") {
        parts.push(&rest[..pos]);
        rest = &rest[pos + 2..];
    }
    parts.push(rest);
    // 全为空段：.* 通配任意
    if parts.iter().all(|p| p.is_empty() || *p == "[^/]*") {
        return true;
    }
    // 检查各段按序出现
    let mut search = text;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() || *part == "[^/]*" {
            continue;
        }
        let Some(pos) = search.find(part) else { return false };
        if i == 0 && pos != 0 && body.starts_with("[^/]*") {
            // 前有任意前缀，跳过
        } else if i == 0 && pos != 0 && !body.starts_with(".*") && !body.starts_with("[^/]*") {
            return false;
        }
        search = &search[pos + part.len()..];
    }
    // 末尾是否完整
    let last = parts.last().unwrap_or(&"");
    if last.is_empty() || *last == "[^/]*" {
        return true;
    }
    search.ends_with(last) || body.ends_with("*")
}

#[tauri::command]
pub fn list_workspaces(state: State<'_, AppState>) -> Result<Vec<Workspace>, String> {
    let conn = state.db()?;
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {SELECT_COLS} FROM workspaces ORDER BY order_index, created_at"
        ))
        .map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], workspace_from_row).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

/// 新建 / 重命名工作区
#[tauri::command]
pub fn save_workspace(input: WorkspaceInput, state: State<'_, AppState>) -> Result<Workspace, String> {
    let name = input.name.trim().to_string();
    if name.is_empty() {
        return Err("工作区名称不能为空".to_string());
    }
    let conn = state.db()?;

    match input.id.clone() {
        Some(id) => {
            let cur = load_workspace(&conn, &id)?;
            conn.execute(
                "UPDATE workspaces SET name=?2, order_index=?3, path=?4 WHERE id=?1",
                params![
                    id,
                    name,
                    input.order_index.unwrap_or(cur.order_index),
                    input.path.or(cur.path),
                ],
            )
            .map_err(|e| format!("更新工作区失败: {e}"))?;
            load_workspace(&conn, &id)
        }
        None => {
            // 幂等：同一项目路径只创建一个工作区（对齐 dsh 的 resolveByPath）
            if let Some(p) = input.path.as_deref().filter(|p| !p.trim().is_empty()) {
                let existing: Option<String> = conn
                    .query_row(
                        "SELECT id FROM workspaces WHERE path = ?1 AND path IS NOT NULL",
                        params![p],
                        |r| r.get(0),
                    )
                    .ok();
                if let Some(id) = existing {
                    return load_workspace(&conn, &id);
                }
            }
            let id = utils::new_id();
            conn.execute(
                "INSERT INTO workspaces (id, name, order_index, created_at, path)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![id, name, input.order_index.unwrap_or(0), utils::now_ms(), input.path],
            )
            .map_err(|e| format!("创建工作区失败: {e}"))?;
            load_workspace(&conn, &id)
        }
    }
}

/// 删除工作区：其下会话 workspace_id 置 NULL（自动归入默认展示）
/// 默认工作区不可删除
#[tauri::command]
pub fn delete_workspace(id: String, state: State<'_, AppState>) -> Result<(), String> {
    if id == "default" {
        return Err("FORBIDDEN:默认工作区不可删除".to_string());
    }
    let conn = state.db()?;
    conn.execute("DELETE FROM workspaces WHERE id = ?1", params![id])
        .map_err(|e| format!("删除工作区失败: {e}"))?;
    Ok(())
}

// ==================== 文件写入（Vibe Coding：模型自主编辑项目） ====================

/// 写入上限：单文件 10MB，避免模型写出超大文件撑爆磁盘
const MAX_WRITE_BYTES: usize = 10 * 1024 * 1024;

/// 工作区文件绝对路径（净化 + 越界校验）；供写入类操作复用
fn workspace_abs(conn: &Connection, workspace_id: &str, rel_path: &str) -> Result<PathBuf, String> {
    let root = workspace_root(conn, workspace_id)?;
    let rel = safe_rel(rel_path)?;
    let full = root.join(&rel);
    if !full.starts_with(&root) {
        return Err("路径越界".to_string());
    }
    Ok(full)
}

/// Agent 工具：写入文件（创建或覆盖；父目录不存在则自动创建）
pub(crate) fn write_workspace_file_content(
    conn: &Connection,
    workspace_id: &str,
    rel_path: &str,
    content: &str,
) -> Result<String, String> {
    let full = workspace_abs(conn, workspace_id, rel_path)?;
    let bytes = content.as_bytes();
    if bytes.len() > MAX_WRITE_BYTES {
        return Err(format!("内容超过 {MAX_WRITE_BYTES} 字节上限"));
    }
    if let Some(parent) = full.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {e}"))?;
    }
    let existed = full.exists();
    std::fs::write(&full, content).map_err(|e| format!("写入失败: {e}"))?;
    Ok(format!(
        "{} {}（{} 字节）",
        if existed { "已覆盖" } else { "已创建" },
        rel_path,
        bytes.len()
    ))
}

/// Agent 工具：精确字符串替换编辑（old_string 须在文件中唯一出现，对齐 Aider/opencode）
pub(crate) fn edit_workspace_file_content(
    conn: &Connection,
    workspace_id: &str,
    rel_path: &str,
    old_string: &str,
    new_string: &str,
) -> Result<String, String> {
    if old_string.is_empty() {
        return Err("old_string 不能为空".to_string());
    }
    if old_string == new_string {
        return Err("old_string 与 new_string 相同".to_string());
    }
    let full = workspace_abs(conn, workspace_id, rel_path)?;
    if !full.is_file() {
        return Err(format!("文件不存在: {rel_path}"));
    }
    let original = std::fs::read_to_string(&full).map_err(|e| format!("读取失败: {e}"))?;
    let occurrences = original.matches(old_string).count();
    if occurrences == 0 {
        return Err(format!("未在 {rel_path} 中找到匹配内容（old_string 不存在）"));
    }
    if occurrences > 1 {
        return Err(format!(
            "old_string 在 {rel_path} 中出现 {occurrences} 次，不是唯一匹配；请补充更多上下文"
        ));
    }
    let edited = original.replacen(old_string, new_string, 1);
    let new_bytes = edited.as_bytes().len();
    if new_bytes > MAX_WRITE_BYTES {
        return Err(format!("编辑后内容超过 {MAX_WRITE_BYTES} 字节上限"));
    }
    std::fs::write(&full, &edited).map_err(|e| format!("写入失败: {e}"))?;
    Ok(format!("已编辑 {rel_path}（{new_bytes} 字节）"))
}

/// Agent 工具：删除文件（仅文件，不递归删目录）
pub(crate) fn delete_workspace_file_content(
    conn: &Connection,
    workspace_id: &str,
    rel_path: &str,
) -> Result<String, String> {
    let full = workspace_abs(conn, workspace_id, rel_path)?;
    if !full.exists() {
        return Err(format!("文件不存在: {rel_path}"));
    }
    if full.is_dir() {
        return Err(format!("{rel_path} 是目录，仅支持删除文件"));
    }
    std::fs::remove_file(&full).map_err(|e| format!("删除失败: {e}"))?;
    Ok(format!("已删除 {rel_path}"))
}

/// 命令：写入文件（前端可直接调用）
#[tauri::command]
pub fn write_workspace_file(
    workspace_id: String,
    rel_path: String,
    content: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let conn = state.db()?;
    write_workspace_file_content(&conn, &workspace_id, &rel_path, &content)
}

/// 命令：编辑文件（字符串替换）
#[tauri::command]
pub fn edit_workspace_file(
    workspace_id: String,
    rel_path: String,
    old_string: String,
    new_string: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let conn = state.db()?;
    edit_workspace_file_content(&conn, &workspace_id, &rel_path, &old_string, &new_string)
}

/// 命令：删除文件
#[tauri::command]
pub fn delete_workspace_file(
    workspace_id: String,
    rel_path: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let conn = state.db()?;
    delete_workspace_file_content(&conn, &workspace_id, &rel_path)
}
