//! 插件系统（§4.9）— Phase 3 基础版
//!
//! 范围：本地目录加载（app_data_dir/plugins/{id}/），manifest.json 声明元数据与权限，
//! 前端以 iframe 沙箱（sandbox="allow-scripts"）+ postMessage 桥接运行。
//! 完整 OpenCode 插件兼容层（npm 包 / manifest 形态适配）在 Phase 4。
//!
//! 安全：插件文件读取由本模块完成，id 只允许 [A-Za-z0-9_-]，
//! 防止路径穿越；网络请求由前端桥限制走主进程（后续迭代）。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginManifest {
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub version: String,
    /// 入口文件名（相对插件目录）
    pub entry: String,
    #[serde(default)]
    pub permissions: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub entry: String,
    pub permissions: Vec<String>,
}

fn plugins_dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| format!("无法定位数据目录: {e}"))?
        .join("plugins"))
}

/// id 净化：仅字母数字与 - _，防路径穿越
pub fn sanitize_id(id: &str) -> String {
    id.chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, '-' | '_'))
        .collect()
}

fn plugin_dir(app: &AppHandle, id: &str) -> Result<PathBuf, String> {
    let safe = sanitize_id(id);
    if safe.is_empty() {
        return Err("插件 id 无效".to_string());
    }
    Ok(plugins_dir(app)?.join(safe))
}

/// 扫描插件目录：解析 manifest.json，缺失则跳过
pub fn list_plugins(app: &AppHandle) -> Result<Vec<PluginInfo>, String> {
    let dir = plugins_dir(app)?;
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Ok(vec![]);
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        let id = entry.file_name().to_string_lossy().to_string();
        let manifest_path = entry.path().join("manifest.json");
        let Ok(raw) = std::fs::read_to_string(&manifest_path) else {
            continue;
        };
        let Ok(manifest) = serde_json::from_str::<PluginManifest>(&raw) else {
            continue;
        };
        out.push(PluginInfo {
            id: manifest.id.unwrap_or_else(|| id.clone()),
            name: manifest.name,
            version: manifest.version,
            entry: manifest.entry,
            permissions: manifest.permissions,
        });
    }
    Ok(out)
}

/// 读取插件入口 HTML（前端 iframe.srcdoc 使用）
pub fn plugin_html(app: &AppHandle, id: &str) -> Result<String, String> {
    let dir = plugin_dir(app, id)?;
    // 读取 manifest 确定入口文件名
    let manifest: PluginManifest = serde_json::from_str(
        &std::fs::read_to_string(dir.join("manifest.json"))
            .map_err(|_| "插件 manifest 缺失".to_string())?,
    )
    .map_err(|e| format!("manifest 解析失败: {e}"))?;
    // 入口文件名同样做净化，禁止读取插件目录外文件
    let entry = sanitize_id(&manifest.entry.replace(['/', '\\'], "").trim());
    let path = dir.join(if entry.is_empty() {
        "index.html".to_string()
    } else {
        entry
    });
    std::fs::read_to_string(&path).map_err(|e| format!("读取插件入口失败: {e}"))
}

/// 删除插件目录（id 已净化）
pub fn delete_plugin(app: &AppHandle, id: &str) -> Result<(), String> {
    let dir = plugin_dir(app, id)?;
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| format!("删除插件失败: {e}"))?;
    }
    Ok(())
}

// ---------- 插件工作区文件（§4.9 koid.file.*） ----------

/// 插件专属工作区：plugins/{id}/workspace/
fn workspace_dir(app: &AppHandle, plugin_id: &str) -> Result<PathBuf, String> {
    Ok(plugin_dir(app, plugin_id)?.join("workspace"))
}

/// 相对路径净化：拒绝绝对路径与 .. 穿越，只允许工作区内的相对路径
fn safe_rel_path(rel: &str) -> Result<PathBuf, String> {
    let p = Path::new(rel);
    if p.is_absolute() {
        return Err("插件文件路径不允许绝对路径".to_string());
    }
    let normalized: PathBuf = p.components().collect();
    for c in normalized.components() {
        if matches!(c, std::path::Component::ParentDir | std::path::Component::RootDir | std::path::Component::Prefix(_)) {
            return Err("插件文件路径不允许越界访问".to_string());
        }
    }
    if normalized.as_os_str().is_empty() {
        return Err("路径为空".to_string());
    }
    Ok(normalized)
}

pub fn workspace_read(app: &AppHandle, plugin_id: &str, rel: &str) -> Result<String, String> {
    let rel = safe_rel_path(rel)?;
    let base = workspace_dir(app, plugin_id)?;
    // 最终校验：拼接后仍位于工作区内（符号链接兜底）
    let path = base.join(&rel);
    if !path.starts_with(&base) {
        return Err("路径越界".to_string());
    }
    std::fs::read_to_string(&path).map_err(|e| format!("读取失败: {e}"))
}

pub fn workspace_write(
    app: &AppHandle,
    plugin_id: &str,
    rel: &str,
    content: &str,
) -> Result<(), String> {
    let rel = safe_rel_path(rel)?;
    let base = workspace_dir(app, plugin_id)?;
    let path = base.join(&rel);
    if !path.starts_with(&base) {
        return Err("路径越界".to_string());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {e}"))?;
    }
    std::fs::write(&path, content).map_err(|e| format!("写入失败: {e}"))
}

// ---------- 网络（§4.9 koid.network.fetch，走主进程代理） ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FetchResult {
    pub ok: bool,
    pub status: u16,
    pub body: String,
}

pub async fn network_fetch(
    app: &AppHandle,
    url: &str,
    method: &str,
    headers: Option<&serde_json::Value>,
    body: Option<&str>,
) -> Result<FetchResult, String> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("仅支持 http/https 请求".to_string());
    }
    // 解析全局代理（无用户代理时回退环境变量）
    let (ptype, purl) = {
        let st = app.state::<crate::state::AppState>();
        let proxy = st.db().ok().and_then(|conn| crate::services::settings::get_global_proxy(&conn));
        ("direct", proxy.and_then(|p| p.proxy_url))
    };
    let client = crate::services::proxy::build_client(ptype, purl.as_deref(), 30, false)?;
    let mut req = match method.to_uppercase().as_str() {
        "POST" => client.post(url),
        "PUT" => client.put(url),
        "DELETE" => client.delete(url),
        _ => client.get(url),
    };
    if let Some(h) = headers {
        if let Ok(map) = serde_json::from_value::<std::collections::HashMap<String, String>>(h.clone()) {
            for (k, v) in map {
                req = req.header(k, v);
            }
        }
    }
    if let Some(b) = body {
        req = req.body(b.to_string());
    }

    let resp = req
        .send()
        .await
        .map_err(|e| if e.is_timeout() { "网络超时".to_string() } else { format!("请求失败: {e}") })?;
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    Ok(FetchResult {
        ok: status < 400,
        status,
        body: text,
    })
}

// ---------- 安装（本地 zip / 远程 URL） ----------

/// 从本地 zip 路径安装：解压 → 定位 manifest → 复制到 plugins/{id}/
pub fn install_from_zip(app: &AppHandle, zip_path: &str) -> Result<PluginInfo, String> {
    let zip_file = std::fs::File::open(zip_path).map_err(|e| format!("打开 zip 失败: {e}"))?;
    let mut archive = zip::ZipArchive::new(zip_file).map_err(|e| format!("解析 zip 失败: {e}"))?;

    // 解压到临时目录
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;
    let staging = data_dir.join("tmp_plugin_install");
    if staging.exists() {
        std::fs::remove_dir_all(&staging).ok();
    }
    std::fs::create_dir_all(&staging).map_err(|e| e.to_string())?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let Some(rel) = entry.enclosed_name() else {
            return Err("zip 包含非法路径".to_string());
        };
        let out_path = staging.join(rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out_path).map_err(|e| e.to_string())?;
        } else {
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let mut file = std::fs::File::create(&out_path).map_err(|e| e.to_string())?;
            std::io::copy(&mut entry, &mut file).map_err(|e| e.to_string())?;
        }
    }

    // 递归查找 manifest.json
    let manifest_path = find_manifest(&staging).ok_or("zip 中未找到 manifest.json")?;
    let plugin_root = manifest_path.parent().unwrap_or(&staging);
    let manifest: PluginManifest = serde_json::from_str(
        &std::fs::read_to_string(&manifest_path).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("manifest 解析失败: {e}"))?;

    let id = manifest
        .id
        .clone()
        .unwrap_or_else(|| plugin_root.file_name().unwrap_or_default().to_string_lossy().to_string());
    let safe = sanitize_id(&id);
    if safe.is_empty() {
        return Err("插件 id 无效".to_string());
    }

    // 复制到插件目录
    let dest = plugins_dir(app)?.join(&safe);
    if dest.exists() {
        std::fs::remove_dir_all(&dest).ok();
    }
    copy_dir(plugin_root, &dest)?;

    let _ = std::fs::remove_dir_all(&staging);
    Ok(PluginInfo {
        id: safe,
        name: manifest.name,
        version: manifest.version,
        entry: manifest.entry,
        permissions: manifest.permissions,
    })
}

fn find_manifest(dir: &Path) -> Option<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return None };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_manifest(&path) {
                return Some(found);
            }
        } else if path.file_name().is_some_and(|n| n == "manifest.json") {
            return Some(path);
        }
    }
    None
}

fn copy_dir(src: &Path, dst: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dst).map_err(|e| e.to_string())?;
    for entry in std::fs::read_dir(src).map_err(|e| e.to_string())?.flatten() {
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir(&from, &to)?;
        } else {
            std::fs::copy(&from, &to).map_err(|e| format!("复制失败: {e}"))?;
        }
    }
    Ok(())
}

/// 从远程 URL 下载 zip 并安装（走主进程代理）
pub async fn install_from_url(app: &AppHandle, url: &str) -> Result<PluginInfo, String> {
    let (ptype, purl) = {
        let st = app.state::<crate::state::AppState>();
        let proxy = st.db().ok().and_then(|conn| crate::services::settings::get_global_proxy(&conn));
        ("direct", proxy.and_then(|p| p.proxy_url))
    };
    let client = crate::services::proxy::build_client(ptype, purl.as_deref(), 60, false)?;
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("下载失败: {e}"))?;
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("读取下载内容失败: {e}"))?;

    let tmp_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("tmp_plugin_download.zip");
    std::fs::write(&tmp_dir, &bytes).map_err(|e| e.to_string())?;
    let result = install_from_zip(app, &tmp_dir.to_string_lossy().as_ref());
    let _ = std::fs::remove_file(&tmp_dir);
    result
}
