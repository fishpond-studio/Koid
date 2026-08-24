//! 全局状态：SQLite 连接 / 流式请求中断 / Skill 执行协调 / MCP 连接池

use rusqlite::Connection;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use tokio::sync::oneshot;

/// MCP 活动连接：stdin 写入端 + 消息流（由 reader 任务填充）
pub struct McpHandle {
    pub process: tokio::process::Child,
    pub writer: tokio::sync::Mutex<tokio::process::ChildStdin>,
    pub rx: tokio::sync::mpsc::UnboundedReceiver<serde_json::Value>,
}

pub struct AppState {
    pub db: Mutex<Connection>,
    /// 进行中的流式请求：request_id -> 中断旗标（true = 请求已取消）
    active: Mutex<HashMap<String, Arc<AtomicBool>>>,
    /// Skill input 步骤等待通道：request_id -> 用户输入发送端
    pending_skill_inputs: Mutex<HashMap<String, oneshot::Sender<String>>>,
    /// 已取消的 Skill 运行：request_id 集合
    cancelled_skills: Mutex<HashSet<String>>,
    /// MCP 服务器活动连接：server_id -> handle
    pub mcp: Mutex<HashMap<String, McpHandle>>,
}

impl AppState {
    pub fn new(conn: Connection) -> Self {
        Self {
            db: Mutex::new(conn),
            active: Mutex::new(HashMap::new()),
            pending_skill_inputs: Mutex::new(HashMap::new()),
            cancelled_skills: Mutex::new(HashSet::new()),
            mcp: Mutex::new(HashMap::new()),
        }
    }

    /// 获取数据库锁；锁中毒转为可读错误而不是 panic（§7.2）
    pub fn db(&self) -> Result<MutexGuard<'_, Connection>, String> {
        self.db.lock().map_err(|e| format!("数据库锁异常: {e}"))
    }

    pub fn register_active(&self, request_id: &str, flag: Arc<AtomicBool>) {
        let Ok(mut map) = self.active.lock() else { return };
        map.insert(request_id.to_string(), flag);
    }

    pub fn remove_active(&self, request_id: &str) {
        let Ok(mut map) = self.active.lock() else { return };
        map.remove(request_id);
    }

    /// 置位中断旗标；返回是否存在该请求
    pub fn abort(&self, request_id: &str) -> bool {
        let Ok(map) = self.active.lock() else { return false };
        if let Some(flag) = map.get(request_id) {
            flag.store(true, Ordering::Relaxed);
            true
        } else {
            false
        }
    }

    // ---------- Skill 交互 ----------

    /// input 步骤开始前注册通道；引擎 await 接收端等待用户输入
    pub fn register_skill_input(&self, request_id: &str, tx: oneshot::Sender<String>) {
        let Ok(mut map) = self.pending_skill_inputs.lock() else { return };
        map.insert(request_id.to_string(), tx);
    }

    /// skill_respond 命令取出发送端送达用户输入
    pub fn take_skill_input(&self, request_id: &str) -> Option<oneshot::Sender<String>> {
        let Ok(mut map) = self.pending_skill_inputs.lock() else { return None };
        map.remove(request_id)
    }

    /// 取消 Skill 运行：标记 + 丢弃等待通道（惊醒 await 中的引擎）
    pub fn cancel_skill(&self, request_id: &str) {
        if let Ok(mut set) = self.cancelled_skills.lock() {
            set.insert(request_id.to_string());
        }
        let _ = self.take_skill_input(request_id);
        self.abort(request_id);
    }

    pub fn is_skill_cancelled(&self, request_id: &str) -> bool {
        self.cancelled_skills
            .lock()
            .map(|set| set.contains(request_id))
            .unwrap_or(false)
    }

    pub fn clear_skill_cancel(&self, request_id: &str) {
        let Ok(mut set) = self.cancelled_skills.lock() else { return };
        set.remove(request_id);
    }
}
