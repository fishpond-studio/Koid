//! Rust 工具函数

/// 毫秒级时间戳（前端 JS Date.now() 同口径）
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 生成 UUID v4 主键
pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}
