//! One-line "what is this agent doing" summaries for the team sidebar.
//! Lane B owns this file.

use crate::domain::agent::AgentKind;
use serde_json::Value;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

const TAIL_BYTES: usize = 64 * 1024;
const MAX_SUMMARY_CHARS: usize = 80;
const MAX_BASH_CMD_CHARS: usize = 50;
const MAX_AGY_PROMPT_CHARS: usize = 60;

/// Latest agent action as one short line (max 80 chars), e.g. `Edit src/team/hub.rs`,
/// `Bash: cargo test`, `winhands: run`, or the first line of the latest assistant text.
/// `None` when unknown. Must be cheap: reads only the tail of the transcript file.
pub fn last_action_summary(
    kind: AgentKind,
    profile: Option<&str>,
    agent_session_id: Option<&str>,
) -> Option<String> {
    let session_id = agent_session_id.map(str::trim).filter(|s| !s.is_empty())?;
    match kind {
        AgentKind::Claude => {
            let path = crate::session::detector::find_claude_transcript(profile, session_id)?;
            summarize_claude_transcript(&path)
        }
        AgentKind::Agy => {
            let path = dirs::home_dir()?
                .join(".gemini")
                .join("antigravity-cli")
                .join("history.jsonl");
            summarize_agy_history(&path, session_id)
        }
        AgentKind::Codex | AgentKind::OpenCode | AgentKind::Shell => None,
    }
}

/// Read at most the last `max_bytes` bytes of `path` as lossy UTF-8, dropping any leading
/// partial line if the read started mid-file.
fn read_tail_utf8(path: &Path, max_bytes: usize) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    if len == 0 {
        return None;
    }

    let truncated = len > max_bytes as u64;
    if truncated {
        file.seek(SeekFrom::End(-(max_bytes as i64))).ok()?;
    }

    let cap = usize::try_from(len.min(max_bytes as u64)).unwrap_or(max_bytes);
    let mut buf = Vec::with_capacity(cap);
    file.take(max_bytes as u64).read_to_end(&mut buf).ok()?;

    let text = String::from_utf8_lossy(&buf);
    if truncated {
        let (_, rest) = text.split_once('\n')?;
        Some(rest.to_string())
    } else {
        Some(text.into_owned())
    }
}

fn collapse_whitespace(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn first_non_empty_line(input: &str) -> Option<String> {
    input
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(collapse_whitespace)
        .filter(|line| !line.is_empty())
}

fn truncate_chars(input: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let char_count = input.chars().count();
    if char_count <= max_chars {
        return input.to_string();
    }
    let take_count = max_chars.saturating_sub(1);
    let mut out: String = input.chars().take(take_count).collect();
    out.push('…');
    out
}

fn short_path(raw: &str) -> Option<String> {
    let parts: Vec<&str> = raw
        .trim()
        .split(['/', '\\'])
        .filter(|part| !part.is_empty())
        .collect();
    match parts.len() {
        0 => None,
        1 => Some(parts[0].to_string()),
        n => Some(format!("{}/{}", parts[n - 2], parts[n - 1])),
    }
}

fn summarize_tool_use(block: &Value) -> Option<String> {
    let name = block.get("name").and_then(Value::as_str)?.trim();
    if name.is_empty() {
        return None;
    }
    let input = block.get("input");

    let summary = match name {
        "Edit" | "Write" | "MultiEdit" | "NotebookEdit" => {
            let file = input
                .and_then(|v| {
                    v.get("file_path")
                        .or_else(|| v.get("notebook_path"))
                        .or_else(|| v.get("path"))
                })
                .and_then(Value::as_str)
                .and_then(short_path);
            match file {
                Some(p) => format!("Edit {p}"),
                None => "Edit".to_string(),
            }
        }
        "Read" => {
            let file = input
                .and_then(|v| v.get("file_path").or_else(|| v.get("path")))
                .and_then(Value::as_str)
                .and_then(short_path);
            match file {
                Some(p) => format!("Read {p}"),
                None => "Read".to_string(),
            }
        }
        "Bash" | "PowerShell" => {
            let cmd = input
                .and_then(|v| v.get("command"))
                .and_then(Value::as_str)
                .map(collapse_whitespace)
                .filter(|s| !s.is_empty());
            match cmd {
                Some(c) => format!("Bash: {}", truncate_chars(&c, MAX_BASH_CMD_CHARS)),
                None => "Bash".to_string(),
            }
        }
        "Grep" | "Glob" => {
            let pattern = input
                .and_then(|v| v.get("pattern"))
                .and_then(Value::as_str)
                .map(collapse_whitespace)
                .filter(|s| !s.is_empty());
            match pattern {
                Some(p) => format!("Search {p}"),
                None => "Search".to_string(),
            }
        }
        "Agent" | "Task" => {
            let desc = input
                .and_then(|v| v.get("description").or_else(|| v.get("prompt")))
                .and_then(Value::as_str)
                .map(collapse_whitespace)
                .filter(|s| !s.is_empty());
            match desc {
                Some(d) => format!("Agent: {d}"),
                None => "Agent".to_string(),
            }
        }
        other => {
            if let Some(rest) = other.strip_prefix("mcp__") {
                if let Some((server, tool)) = rest.split_once("__") {
                    format!("{server}: {tool}")
                } else {
                    other.to_string()
                }
            } else {
                other.to_string()
            }
        }
    };

    Some(truncate_chars(&summary, MAX_SUMMARY_CHARS))
}

fn summarize_claude_entry(entry: &Value) -> Option<String> {
    if entry.get("type").and_then(Value::as_str) != Some("assistant") {
        return None;
    }
    if entry.get("isSidechain").and_then(Value::as_bool) == Some(true) {
        return None;
    }

    let content = entry.get("message")?.get("content")?;
    if let Some(text) = content.as_str() {
        return first_non_empty_line(text).map(|l| truncate_chars(&l, MAX_SUMMARY_CHARS));
    }

    let blocks = content.as_array()?;
    for block in blocks.iter().rev() {
        let block_type = block.get("type").and_then(Value::as_str).unwrap_or("");
        match block_type {
            "thinking" | "redacted_thinking" => continue,
            "tool_use" => {
                if let Some(summary) = summarize_tool_use(block) {
                    return Some(summary);
                }
            }
            "text" => {
                if let Some(text) = block.get("text").and_then(Value::as_str) {
                    if let Some(line) = first_non_empty_line(text) {
                        return Some(truncate_chars(&line, MAX_SUMMARY_CHARS));
                    }
                }
            }
            _ => continue,
        }
    }

    None
}

/// Summarize the newest non-sidechain assistant action from the tail of a Claude JSONL transcript.
pub fn summarize_claude_transcript(path: &Path) -> Option<String> {
    let tail = read_tail_utf8(path, TAIL_BYTES)?;
    for line in tail.lines().rev() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(entry) = serde_json::from_str::<Value>(trimmed) else {
            continue;
        };
        if let Some(summary) = summarize_claude_entry(&entry) {
            return Some(summary);
        }
    }
    None
}

/// Summarize the newest prompt for `agent_session_id` from the tail of an AGY `history.jsonl`.
pub fn summarize_agy_history(path: &Path, agent_session_id: &str) -> Option<String> {
    let tail = read_tail_utf8(path, TAIL_BYTES)?;
    for line in tail.lines().rev() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(entry) = serde_json::from_str::<Value>(trimmed) else {
            continue;
        };
        if entry.get("conversationId").and_then(Value::as_str) != Some(agent_session_id) {
            continue;
        }
        let Some(display) = entry.get("display").and_then(Value::as_str) else {
            continue;
        };
        if let Some(prompt_line) = first_non_empty_line(display) {
            let short_prompt = truncate_chars(&prompt_line, MAX_AGY_PROMPT_CHARS);
            return Some(truncate_chars(
                &format!("Prompt: {short_prompt}"),
                MAX_SUMMARY_CHARS,
            ));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;

    fn temp_jsonl(name: &str, content: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("velox-summary-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let path = dir.join(name);
        std::fs::write(&path, content).expect("write fixture");
        path
    }

    fn cleanup_temp_file(path: &Path) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::remove_dir_all(parent);
        }
    }

    #[test]
    fn summarizes_claude_tool_uses_and_skips_thinking_and_sidechains() {
        let fixture = [
            r#"{"type":"user","message":{"role":"user","content":"fix it"}}"#,
            r#"{"type":"assistant","isSidechain":false,"message":{"content":[{"type":"tool_use","name":"Read","input":{"file_path":"D:\\Workspace\\velox\\src-tauri\\src\\team\\hub.rs"}}]}}"#,
            r#"{"type":"assistant","isSidechain":false,"message":{"content":[{"type":"thinking","thinking":"hmm"}]}}"#,
            r#"{"type":"assistant","isSidechain":true,"message":{"content":[{"type":"text","text":"sidechain message"}]}}"#,
        ]
        .join("\n");
        let path = temp_jsonl("claude.jsonl", &fixture);
        let summary = summarize_claude_transcript(&path);
        cleanup_temp_file(&path);
        assert_eq!(summary.as_deref(), Some("Read team/hub.rs"));
    }

    #[test]
    fn summarizes_claude_edit_bash_search_agent_and_mcp_tools() {
        let cases = vec![
            (
                r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Edit","input":{"file_path":"src/team/mcp.rs"}}]}}"#,
                "Edit team/mcp.rs",
            ),
            (
                r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Write","input":{"file_path":"README.md"}}]}}"#,
                "Edit README.md",
            ),
            (
                r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"PowerShell","input":{"command":"cargo test --all-targets -- --nocapture --verbose-extra-long-flag"}}]}}"#,
                "Bash: cargo test --all-targets -- --nocapture --verbose…",
            ),
            (
                r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Grep","input":{"pattern":"TeamApi"}}]}}"#,
                "Search TeamApi",
            ),
            (
                r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Agent","input":{"description":"Explore protocol"}}]}}"#,
                "Agent: Explore protocol",
            ),
            (
                r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"mcp__winhands__run","input":{}}]}}"#,
                "winhands: run",
            ),
            (
                r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"CustomTool","input":{}}]}}"#,
                "CustomTool",
            ),
        ];

        for (line, expected) in cases {
            let path = temp_jsonl("case.jsonl", line);
            let got = summarize_claude_transcript(&path);
            cleanup_temp_file(&path);
            assert_eq!(got.as_deref(), Some(expected), "failed for line: {line}");
        }
    }

    #[test]
    fn summarizes_claude_text_first_non_empty_line_and_truncates_utf8_safely() {
        let text = "\n   \nPrimera línea con acentos áéíóú y más texto que supera ampliamente los ochenta caracteres permitidos\nSegunda línea";
        let line = serde_json::json!({
            "type": "assistant",
            "message": {
                "content": [
                    { "type": "thinking", "thinking": "secret" },
                    { "type": "text", "text": text }
                ]
            }
        })
        .to_string();
        let path = temp_jsonl("utf8.jsonl", &line);
        let summary = summarize_claude_transcript(&path).expect("summary");
        cleanup_temp_file(&path);

        assert_eq!(summary.chars().count(), 80);
        assert!(summary.ends_with('…'));
        assert!(summary.starts_with("Primera línea con acentos"));
    }

    #[test]
    fn reads_only_last_64kb_of_large_transcript() {
        let dir = std::env::temp_dir().join(format!("velox-large-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("large.jsonl");
        let mut f = File::create(&path).expect("create large file");

        let filler = serde_json::json!({
            "type": "assistant",
            "message": { "content": [{ "type": "text", "text": "x".repeat(500) }] }
        })
        .to_string();
        for _ in 0..200 {
            writeln!(f, "{filler}").expect("write filler");
        }
        writeln!(
            f,
            r#"{{"type":"assistant","message":{{"content":[{{"type":"tool_use","name":"Bash","input":{{"command":"cargo clippy"}}}}]}}}}"#
        )
        .expect("write final");
        drop(f);

        let summary = summarize_claude_transcript(&path);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(summary.as_deref(), Some("Bash: cargo clippy"));
    }

    #[test]
    fn summarizes_agy_history_for_matching_conversation() {
        let fixture = [
            r#"{"display":"old prompt","timestamp":1,"conversationId":"conv-1"}"#,
            r#"{"display":"other conv","timestamp":2,"conversationId":"conv-2"}"#,
            r#"{"display":"Implement team MCP tools and unit tests cleanly","timestamp":3,"conversationId":"conv-1"}"#,
        ]
        .join("\n");
        let path = temp_jsonl("history.jsonl", &fixture);
        let summary = summarize_agy_history(&path, "conv-1");
        let missing = summarize_agy_history(&path, "conv-999");
        cleanup_temp_file(&path);

        assert_eq!(
            summary.as_deref(),
            Some("Prompt: Implement team MCP tools and unit tests cleanly")
        );
        assert_eq!(missing, None);
    }

    #[test]
    fn returns_none_for_unsupported_kinds_or_missing_session_id() {
        assert_eq!(last_action_summary(AgentKind::Shell, None, Some("123")), None);
        assert_eq!(last_action_summary(AgentKind::Codex, None, Some("123")), None);
        assert_eq!(last_action_summary(AgentKind::Claude, None, None), None);
        assert_eq!(last_action_summary(AgentKind::Claude, None, Some("   ")), None);
    }
}
