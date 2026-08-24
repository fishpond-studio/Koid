//! SQLite 持久化层（计划 §五）
//!
//! 设计约定：
//! - 数据库由 Rust 层独占，前端只能通过 Tauri Command 访问（§架构决策）
//! - WAL 模式：读写并发，流式写消息时不阻塞会话列表查询
//! - `providers.api_key` 列按 §五 建表保留，但恒为空串；真实 Key 只进系统 keyring（§7.4）

use rusqlite::Connection;
use std::path::Path;

/// v1 迁移：核心业务表 + 计划要求的索引（§7.3）
const MIGRATION_V1: &str = r#"
CREATE TABLE IF NOT EXISTS providers (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  type TEXT NOT NULL,
  base_url TEXT NOT NULL,
  -- 安全约定：恒为空串，禁止写入真实 Key；Key 由 keyring 服务管理
  api_key TEXT NOT NULL DEFAULT '',
  proxy_type TEXT NOT NULL DEFAULT 'direct',
  proxy_url TEXT,
  timeout INTEGER NOT NULL DEFAULT 60,
  retries INTEGER NOT NULL DEFAULT 2,
  order_index INTEGER NOT NULL DEFAULT 0,
  enabled INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE IF NOT EXISTS models (
  id TEXT PRIMARY KEY,
  provider_id TEXT NOT NULL REFERENCES providers(id) ON DELETE CASCADE,
  model_id TEXT NOT NULL,
  display_name TEXT NOT NULL,
  context_window INTEGER,
  capabilities TEXT, -- JSON array
  enabled INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE IF NOT EXISTS folders (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  parent_id TEXT REFERENCES folders(id) ON DELETE CASCADE,
  order_index INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS sessions (
  id TEXT PRIMARY KEY,
  title TEXT NOT NULL,
  folder_id TEXT REFERENCES folders(id) ON DELETE SET NULL,
  model_id TEXT,
  system_prompt TEXT,
  temperature REAL,
  top_p REAL,
  max_tokens INTEGER,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  is_pinned INTEGER NOT NULL DEFAULT 0,
  is_archived INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS messages (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  role TEXT NOT NULL,
  content TEXT NOT NULL,
  reasoning TEXT,
  tool_calls TEXT, -- JSON
  tool_results TEXT, -- JSON
  tokens_used INTEGER,
  latency_ms INTEGER,
  created_at INTEGER NOT NULL,
  parent_id TEXT REFERENCES messages(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS prompts (
  id TEXT PRIMARY KEY,
  title TEXT NOT NULL,
  content TEXT NOT NULL,
  variables TEXT, -- JSON array
  type TEXT NOT NULL,
  tags TEXT, -- JSON array
  usage_count INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS mcp_servers (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  transport TEXT NOT NULL,
  command TEXT,
  args TEXT, -- JSON array
  env TEXT, -- JSON object
  url TEXT,
  status TEXT NOT NULL DEFAULT 'disconnected',
  tools_json TEXT,
  error_message TEXT
);

CREATE TABLE IF NOT EXISTS failover_logs (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id TEXT,
  message_id TEXT,
  from_provider TEXT,
  to_provider TEXT,
  reason TEXT,
  created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS plugins (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  version TEXT NOT NULL,
  manifest TEXT NOT NULL, -- JSON
  entry_url TEXT NOT NULL,
  permissions TEXT NOT NULL, -- JSON array
  enabled INTEGER NOT NULL DEFAULT 1,
  created_at INTEGER NOT NULL
);

-- 性能要求（§7.3）：会话按更新时间排序、消息按会话+时间查询
CREATE INDEX IF NOT EXISTS idx_sessions_updated ON sessions(updated_at);
CREATE INDEX IF NOT EXISTS idx_messages_session ON messages(session_id, created_at);
CREATE INDEX IF NOT EXISTS idx_models_provider ON models(provider_id);
"#;

/// v2 迁移：通用键值设置表（全局代理、故障转移配置等 JSON 落库）
const MIGRATION_V2: &str = r#"
CREATE TABLE IF NOT EXISTS settings (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
"#;

/// v3 迁移：提示词版本历史（§4.6：修改后保留最近 10 个版本）+ 内置模板种子
///
/// 内置模板 id 统一以 builtin: 前缀标识，删除接口据此拦截（不可删除）
const MIGRATION_V3: &str = r#"
CREATE TABLE IF NOT EXISTS prompt_versions (
  id TEXT PRIMARY KEY,
  prompt_id TEXT NOT NULL REFERENCES prompts(id) ON DELETE CASCADE,
  content TEXT NOT NULL,
  created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_prompt_versions ON prompt_versions(prompt_id, created_at);

INSERT OR IGNORE INTO prompts (id, title, content, variables, type, tags, usage_count, created_at) VALUES
('builtin:explain-code', '代码解释',
 '请先说明意图与关键点，再逐行解释以下 {{language}} 代码：

{{code}}',
 '["language","code"]', 'template', '["内置"]', 0, 0),

('builtin:refactor', '重构建议',
 '请审查以下 {{language}} 代码并给出重构建议，聚焦可读性、可维护性与性能，指出改动原因：

{{code}}',
 '["language","code"]', 'template', '["内置"]', 0, 0),

('builtin:code-review', 'Code Review',
 '你是一位严谨的资深工程师。请审查以下 {{language}} 代码并分三部分列出：
1. 潜在 Bug 与边界问题
2. 安全隐患
3. 风格与最佳实践建议

代码：

{{code}}',
 '["language","code"]', 'template', '["内置"]', 0, 0),

('builtin:unit-test', '写单元测试',
 '请为以下 {{language}} 代码编写单元测试，覆盖正常路径、边界情况与异常输入，使用该语言主流测试框架：

{{code}}',
 '["language","code"]', 'template', '["内置"]', 0, 0),

('builtin:commit-message', '生成 Commit Message',
 '根据以下 diff 生成一条符合 Conventional Commits 规范的提交信息，只输出提交信息本身：

{{diff}}',
 '["diff"]', 'template', '["内置"]', 0, 0),

('builtin:explain-error', '解释报错',
 '请解释以下报错信息：定位根因，并给出可执行的修复步骤。

报错信息：
{{error}}

相关上下文（可选）：
{{context}}',
 '["error","context"]', 'template', '["内置"]', 0, 0),

('builtin:snippet:fix', '修 Bug',
 '请修复下面代码中的 Bug，并解释原因：

',
 '[]', 'snippet', '["内置"]', 0, 0),

('builtin:snippet:translate', '翻译成英文',
 '请将以下内容翻译成英文，保留专业术语：

',
 '[]', 'snippet', '["内置"]', 0, 0);
"#;

/// v4 迁移（§4.5 层级 Workspace→Folder→Session）：
/// - workspaces 表：顶层工作区
/// - sessions.workspace_id：会话归属工作区（自动分组）
/// - 创建「默认工作区」并把存量会话归入
const MIGRATION_V4: &str = r#"
CREATE TABLE IF NOT EXISTS workspaces (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  order_index INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL
);

INSERT OR IGNORE INTO workspaces (id, name, order_index, created_at) VALUES
('default', '默认工作区', 0, 0);

-- 已有数据库补列（SQLite ALTER TABLE ADD COLUMN 带默认值回填）
ALTER TABLE sessions ADD COLUMN workspace_id TEXT REFERENCES workspaces(id) ON DELETE SET NULL;

UPDATE sessions SET workspace_id = 'default' WHERE workspace_id IS NULL;
"#;

/// v5 迁移：工作区关联本地项目路径（§4.5 vibe coding 基础）
const MIGRATION_V5: &str = r#"
ALTER TABLE workspaces ADD COLUMN path TEXT;
"#;

/// v6 迁移：会话级思考强度（default / low / medium / high / max；default = 不发送参数，用模型默认）
const MIGRATION_V6: &str = r#"
ALTER TABLE sessions ADD COLUMN thinking_level TEXT;
"#;

/// 打开数据库并按 user_version 顺序执行迁移
///
/// 为什么手写迁移而不用迁移框架：表结构变更少且可控，
/// 顺序执行 + user_version 标记足够，避免引入额外依赖
pub fn init(path: &Path) -> Result<Connection, String> {
    let conn = Connection::open(path).map_err(|e| format!("打开数据库失败: {e}"))?;

    // WAL：允许边流式写消息边查询会话列表
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(|e| e.to_string())?;
    // 外键约束 SQLite 默认关闭，必须显式开启
    conn.pragma_update(None, "foreign_keys", "ON")
        .map_err(|e| e.to_string())?;

    let version: u32 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|e| e.to_string())?;

    if version < 1 {
        conn.execute_batch(MIGRATION_V1)
            .map_err(|e| format!("迁移 v1 失败: {e}"))?;
        conn.pragma_update(None, "user_version", 1)
            .map_err(|e| e.to_string())?;
    }
    if version < 2 {
        conn.execute_batch(MIGRATION_V2)
            .map_err(|e| format!("迁移 v2 失败: {e}"))?;
        conn.pragma_update(None, "user_version", 2)
            .map_err(|e| e.to_string())?;
    }
    if version < 3 {
        conn.execute_batch(MIGRATION_V3)
            .map_err(|e| format!("迁移 v3 失败: {e}"))?;
        conn.pragma_update(None, "user_version", 3)
            .map_err(|e| e.to_string())?;
    }
    if version < 4 {
        conn.execute_batch(MIGRATION_V4)
            .map_err(|e| format!("迁移 v4 失败: {e}"))?;
        conn.pragma_update(None, "user_version", 4)
            .map_err(|e| e.to_string())?;
    }
    if version < 5 {
        conn.execute_batch(MIGRATION_V5)
            .map_err(|e| format!("迁移 v5 失败: {e}"))?;
        conn.pragma_update(None, "user_version", 5)
            .map_err(|e| e.to_string())?;
    }
    if version < 6 {
        conn.execute_batch(MIGRATION_V6)
            .map_err(|e| format!("迁移 v6 失败: {e}"))?;
        conn.pragma_update(None, "user_version", 6)
            .map_err(|e| e.to_string())?;
    }

    Ok(conn)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 验证 v1 迁移：全部业务表创建成功 + 重复执行幂等（user_version 生效）
    #[test]
    fn migration_creates_all_tables_and_is_idempotent() {
        let dir = std::env::temp_dir().join("koid-db-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("test.db");

        let conn = init(&db_path).unwrap();
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        for t in [
            "providers",
            "models",
            "folders",
            "sessions",
            "messages",
            "prompts",
            "mcp_servers",
            "failover_logs",
            "plugins",
            "workspaces",
        ] {
            assert!(tables.iter().any(|x| x == t), "缺少表: {t}");
        }
        drop(conn);

        // 二次打开不应重复执行迁移
        let conn2 = init(&db_path).unwrap();
        let v: u32 = conn2
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(v, 6);

        // 默认工作区种子生效且幂等
        let ws_count: i64 = conn2
            .query_row("SELECT COUNT(*) FROM workspaces", [], |r| r.get(0))
            .unwrap();
        assert_eq!(ws_count, 1);

        // 内置模板种子生效且幂等（OR IGNORE）
        let builtin_count: i64 = conn2
            .query_row(
                "SELECT COUNT(*) FROM prompts WHERE id LIKE 'builtin:%'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(builtin_count, 8);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
