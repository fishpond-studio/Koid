//! DeepSeek DSML 标记救援（工具调用文本泄漏）
//!
//! 背景：部分 DeepSeek 模型/端点（deepseek-reasoner、第三方中转等）不支持
//! 原生 function calling，模型会把工具调用以内部标记文本形式写进 content：
//!
//! ```text
//! <｜DSML｜tool_calls>
//!   <｜DSML｜invoke name="read_file">
//!     <｜DSML｜parameter name="path" string="true">src/main.rs</｜DSML｜parameter>
//!   </｜DSML｜invoke>
//! </｜DSML｜tool_calls>
//! ```
//!
//! 处理：extract_tool_calls 把这些块解析为真正的工具调用并从正文剔除，
//! 让 Agent 循环继续执行（否则模型"说完就停"，表现为没下文）。
//! 匹配对全角 ｜(U+FF5C) 与半角 | 都宽松兼容。

use regex::Regex;
use std::sync::OnceLock;

fn re(pattern: &'static str) -> Regex {
    Regex::new(pattern).expect("invalid dsml regex")
}

fn re_block() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        re(r"(?is)<[｜|]{1,4}\s*dsml[｜|]*\s*tool_calls\s*>(.*?)</\s*[｜|]*\s*dsml[｜|]*\s*tool_calls\s*>")
    })
}

fn re_invoke() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        re(r#"(?is)<[｜|]{1,4}\s*dsml[｜|]*\s*invoke\s+name="([^"]+)"[^>]*>(.*?)</\s*[｜|]*\s*dsml[｜|]*\s*invoke\s*>"#)
    })
}

fn re_param() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        re(r#"(?is)<[｜|]{1,4}\s*dsml[｜|]*\s*parameter\s+name="([^"]+)"[^>]*>(.*?)</\s*[｜|]*\s*dsml[｜|]*\s*parameter\s*>"#)
    })
}

/// 是否包含疑似 DSML 泄漏标记（快速判定，用于跳过无泄漏的常规路径）
pub fn looks_like_dsml(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("dsml")
}

/// 从 content 中提取泄漏的 DSML 工具调用块。
///
/// 返回 (清理后的正文, 工具调用列表)；每项为 (工具名, 参数 JSON 字符串)。
/// 无泄漏时原样返回，不做任何改动。
pub fn extract_tool_calls(content: &str) -> (String, Vec<(String, String)>) {
    if !looks_like_dsml(content) {
        return (content.to_string(), Vec::new());
    }

    let mut calls: Vec<(String, String)> = Vec::new();
    let cleaned = re_block().replace_all(content, "").to_string();

    for block in re_block().captures_iter(content) {
        let body = block.get(1).map(|m| m.as_str()).unwrap_or("");
        for inv in re_invoke().captures_iter(body) {
            let name = inv.get(1).map(|m| m.as_str().trim().to_string());
            let Some(name) = name.filter(|n| !n.is_empty()) else {
                continue;
            };
            let invoke_body = inv.get(2).map(|m| m.as_str()).unwrap_or("");
            let mut map = serde_json::Map::new();
            for p in re_param().captures_iter(invoke_body) {
                let pname = p.get(1).map(|m| m.as_str()).unwrap_or("").to_string();
                let pvalue = p.get(2).map(|m| m.as_str()).unwrap_or("").to_string();
                if !pname.is_empty() {
                    map.insert(pname, serde_json::Value::String(pvalue));
                }
            }
            let args = serde_json::Value::Object(map).to_string();
            calls.push((name, args));
        }
    }

    (cleaned.trim().to_string(), calls)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"好的，先读取文件。
<｜｜DSML｜｜tool_calls><｜｜DSML｜｜invoke name="read_file">
<｜｜DSML｜｜parameter name="path" string="true">src/components/ui/button.jsx</｜｜DSML｜｜parameter>
</｜｜DSML｜｜invoke>
<｜｜DSML｜｜invoke name="read_file">
<｜｜DSML｜｜parameter name="path" string="true">vite.config.js</｜｜DSML｜｜parameter>
</｜｜DSML｜｜invoke>
</｜｜DSML｜｜tool_calls>"#;

    #[test]
    fn extracts_leaked_tool_calls_and_cleans_content() {
        assert!(looks_like_dsml(SAMPLE));
        let (cleaned, calls) = extract_tool_calls(SAMPLE);
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].0, "read_file");
        let args0: serde_json::Value = serde_json::from_str(&calls[0].1).unwrap();
        assert_eq!(
            args0.get("path").and_then(|v| v.as_str()),
            Some("src/components/ui/button.jsx")
        );
        let args1: serde_json::Value = serde_json::from_str(&calls[1].1).unwrap();
        assert_eq!(args1.get("path").and_then(|v| v.as_str()), Some("vite.config.js"));
        assert!(!cleaned.contains("DSML"));
        assert!(cleaned.contains("好的，先读取文件。"));
    }

    #[test]
    fn plain_text_is_untouched() {
        let text = "正常回答，含 <div> 与 | 竖线。";
        let (cleaned, calls) = extract_tool_calls(text);
        assert!(calls.is_empty());
        assert_eq!(cleaned, text);
    }

    #[test]
    fn single_bar_variant_matches() {
        let text = "<｜DSML｜tool_calls><｜DSML｜invoke name=\"list_dir\"><｜DSML｜parameter name=\"path\" string=\"true\"></｜DSML｜parameter></｜DSML｜invoke></｜DSML｜tool_calls>";
        let (_, calls) = extract_tool_calls(text);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "list_dir");
    }
}
