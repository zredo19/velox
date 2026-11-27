use crate::domain::config::velox_config_dir;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

const HOOK_SCRIPT_BODY: &str = include_str!("session_link_hook.ps1");

#[cfg(windows)]
const AGY_HOOK_CMD_BODY: &str = "@echo off\r\nif \"%VELOX_SESSION_ID%\"==\"\" (\r\n    echo {}\r\n    exit /b 0\r\n)\r\npowershell -NoProfile -ExecutionPolicy Bypass -File \"%~dp0velox-session-link.ps1\" agy\r\n";

#[cfg(not(windows))]
const AGY_HOOK_SH_BODY: &str = "#!/bin/sh\nif [ -z \"$VELOX_SESSION_ID\" ]; then\n    printf '{}\\n'\n    exit 0\nfi\nexec pwsh -NoProfile -File \"$(dirname \"$0\")/velox-session-link.ps1\" agy\n";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentLinkRecord {
    pub velox_session_id: String,
    pub agent: String,
    pub agent_session_id: String,
    #[serde(default)]
    pub transcript_path: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub updated_at_ms: i64,
}

/// Restrict `velox_session_id` to safe filename characters so link files cannot escape `agent-links/`.
pub fn sanitize_session_id(id: &str) -> Option<String> {
    let clean: String = id
        .trim()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if clean.is_empty() {
        None
    } else {
        Some(clean)
    }
}

pub fn agent_links_dir() -> PathBuf {
    velox_config_dir().join("agent-links")
}

pub fn agent_link_path_in(links_dir: &Path, velox_session_id: &str) -> Option<PathBuf> {
    let safe = sanitize_session_id(velox_session_id)?;
    Some(links_dir.join(format!("{safe}.json")))
}

pub fn hooks_dir() -> PathBuf {
    velox_config_dir().join("hooks")
}

pub fn claude_settings_path() -> PathBuf {
    hooks_dir().join("claude-settings.json")
}

pub fn hook_script_path() -> PathBuf {
    hooks_dir().join("session-link.ps1")
}

fn write_if_changed(path: &Path, content: &str) -> std::io::Result<()> {
    if let Ok(existing) = std::fs::read_to_string(path) {
        if existing == content {
            return Ok(());
        }
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, content)
}

/// Build the Claude Code `--settings` JSON payload that registers Velox's `SessionStart` hook.
/// Claude Code merges `--settings` hooks with the user's own `settings.json` hooks.
pub fn build_claude_settings_json(script_path: &Path) -> String {
    let cmd = format!(
        "powershell -NoProfile -ExecutionPolicy Bypass -File \"{}\" claude",
        script_path.display()
    );
    let doc = json!({
        "hooks": {
            "SessionStart": [
                {
                    "matcher": "*",
                    "hooks": [
                        {
                            "type": "command",
                            "command": cmd,
                            "timeout": 15
                        }
                    ]
                }
            ]
        }
    });
    serde_json::to_string_pretty(&doc).unwrap_or_default()
}

/// Idempotently install Velox's `hooks/` wrapper beside `hooks.json` and add or update
/// the `"velox"` named `PreInvocation` hook inside `~/.gemini/config/hooks.json`,
/// preserving `"herdr"` and any other user hooks.
///
/// Using a quote-free relative wrapper path (`hooks\velox-session-link.cmd`) avoids
/// Go `os/exec` escaping embedded quotes into `-File '"..."'` on Windows and handles
/// spaces in `%USERPROFILE%`.
pub fn upsert_agy_hooks_json(hooks_path: &Path) -> Result<(), String> {
    let mut root_map: Map<String, Value> = if hooks_path.is_file() {
        let raw = std::fs::read_to_string(hooks_path)
            .map_err(|e| format!("failed to read {}: {e}", hooks_path.display()))?;
        if raw.trim().is_empty() {
            Map::new()
        } else {
            serde_json::from_str::<Map<String, Value>>(&raw).map_err(|e| {
                format!(
                    "refusing to overwrite malformed {}: {e}",
                    hooks_path.display()
                )
            })?
        }
    } else {
        Map::new()
    };

    let config_root = hooks_path
        .parent()
        .ok_or_else(|| format!("missing parent directory for {}", hooks_path.display()))?;
    let hooks_subdir = config_root.join("hooks");
    write_if_changed(&hooks_subdir.join("velox-session-link.ps1"), HOOK_SCRIPT_BODY)
        .map_err(|e| format!("failed to write AGY ps1 wrapper: {e}"))?;

    #[cfg(windows)]
    let cmd = {
        write_if_changed(&hooks_subdir.join("velox-session-link.cmd"), AGY_HOOK_CMD_BODY)
            .map_err(|e| format!("failed to write AGY cmd wrapper: {e}"))?;
        "hooks\\velox-session-link.cmd"
    };
    #[cfg(not(windows))]
    let cmd = {
        write_if_changed(&hooks_subdir.join("velox-session-link.sh"), AGY_HOOK_SH_BODY)
            .map_err(|e| format!("failed to write AGY sh wrapper: {e}"))?;
        "hooks/velox-session-link.sh"
    };

    let velox_entry = json!({
        "PreInvocation": [
            {
                "type": "command",
                "command": cmd,
                "timeout": 10
            }
        ]
    });

    if root_map.get("velox") == Some(&velox_entry) {
        return Ok(());
    }

    root_map.insert("velox".to_string(), velox_entry);
    let serialized = serde_json::to_string_pretty(&Value::Object(root_map))
        .map_err(|e| format!("failed to serialize {}: {e}", hooks_path.display()))?;
    std::fs::create_dir_all(config_root)
        .map_err(|e| format!("failed to create {}: {e}", config_root.display()))?;
    let tmp = PathBuf::from(format!("{}.tmp", hooks_path.display()));
    std::fs::write(&tmp, serialized)
        .map_err(|e| format!("failed to write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, hooks_path)
        .map_err(|e| format!("failed to replace {}: {e}", hooks_path.display()))?;
    Ok(())
}

/// Ensure the hook script, Claude `--settings` file, and (outside unit tests) AGY global hook
/// are written on disk. Returns the Claude settings path when available.
pub fn ensure_agent_hooks() -> Option<PathBuf> {
    let script = hook_script_path();
    if let Err(err) = write_if_changed(&script, HOOK_SCRIPT_BODY) {
        tracing::warn!(path = %script.display(), %err, "Failed to write Velox session link hook script");
    }

    let settings = claude_settings_path();
    let claude_json = build_claude_settings_json(&script);
    let claude_ready = match write_if_changed(&settings, &claude_json) {
        Ok(()) => true,
        Err(err) => {
            tracing::warn!(path = %settings.display(), %err, "Failed to write Velox Claude hook settings");
            settings.is_file()
        }
    };

    #[cfg(not(test))]
    {
        if let Some(home) = dirs::home_dir() {
            let agy_hooks = home.join(".gemini").join("config").join("hooks.json");
            if let Err(err) = upsert_agy_hooks_json(&agy_hooks) {
                tracing::warn!(path = %agy_hooks.display(), %err, "Failed to register AGY session hook");
            }
        }
    }

    claude_ready.then_some(settings)
}

pub fn read_agent_link_from(links_dir: &Path, velox_session_id: &str) -> Option<AgentLinkRecord> {
    let path = agent_link_path_in(links_dir, velox_session_id)?;
    let raw = std::fs::read_to_string(path).ok()?;
    let record: AgentLinkRecord = serde_json::from_str(&raw).ok()?;
    if record.agent_session_id.trim().is_empty() {
        return None;
    }
    Some(record)
}

pub fn read_agent_link(velox_session_id: &str) -> Option<AgentLinkRecord> {
    read_agent_link_from(&agent_links_dir(), velox_session_id)
}

pub fn remove_agent_link_from(links_dir: &Path, velox_session_id: &str) {
    if let Some(path) = agent_link_path_in(links_dir, velox_session_id) {
        let _ = std::fs::remove_file(path);
    }
}

pub fn remove_agent_link(velox_session_id: &str) {
    remove_agent_link_from(&agent_links_dir(), velox_session_id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_session_id_prevents_path_traversal() {
        assert_eq!(
            sanitize_session_id("abc-123_XYZ"),
            Some("abc-123_XYZ".to_string())
        );
        assert_eq!(
            sanitize_session_id("../evil/path"),
            Some("evilpath".to_string())
        );
        assert_eq!(sanitize_session_id("..."), None);
    }

    #[test]
    fn upsert_agy_hooks_preserves_existing_named_hooks_and_is_idempotent() {
        let dir = std::env::temp_dir().join(format!("velox-hooks-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let hooks_path = dir.join("hooks.json");

        let existing = json!({
            "herdr": {
                "PreInvocation": [
                    {
                        "type": "command",
                        "command": "powershell -File herdr.ps1",
                        "timeout": 10
                    }
                ]
            }
        });
        std::fs::write(
            &hooks_path,
            serde_json::to_string_pretty(&existing).unwrap(),
        )
        .expect("write existing hooks.json");

        upsert_agy_hooks_json(&hooks_path).expect("first upsert");
        let after_first = std::fs::read_to_string(&hooks_path).expect("read after first");
        let parsed: Value = serde_json::from_str(&after_first).expect("valid json");
        assert!(parsed.get("herdr").is_some(), "herdr hook must be preserved");
        let velox_cmd = parsed["velox"]["PreInvocation"][0]["command"]
            .as_str()
            .expect("velox command string");
        assert!(
            !velox_cmd.contains('"'),
            "AGY command must be quote-free for Go cmd /c: {velox_cmd}"
        );
        assert!(dir.join("hooks").join("velox-session-link.ps1").is_file());
        #[cfg(windows)]
        assert!(dir.join("hooks").join("velox-session-link.cmd").is_file());

        // Second upsert is a no-op
        upsert_agy_hooks_json(&hooks_path).expect("second upsert");
        let after_second = std::fs::read_to_string(&hooks_path).expect("read after second");
        assert_eq!(after_first, after_second);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn upsert_agy_hooks_refuses_to_overwrite_malformed_json() {
        let dir = std::env::temp_dir().join(format!("velox-hooks-bad-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let hooks_path = dir.join("hooks.json");
        std::fs::write(&hooks_path, "{broken json").expect("write malformed");

        let res = upsert_agy_hooks_json(&hooks_path);
        assert!(res.is_err());
        assert_eq!(
            std::fs::read_to_string(&hooks_path).unwrap(),
            "{broken json"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_and_remove_agent_link_round_trip() {
        let dir = std::env::temp_dir().join(format!("velox-link-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let link = AgentLinkRecord {
            velox_session_id: "tab-1".to_string(),
            agent: "claude".to_string(),
            agent_session_id: "11111111-2222-3333-4444-555555555555".to_string(),
            transcript_path: Some("C:\\tmp\\t.jsonl".to_string()),
            cwd: Some("D:\\Workspace".to_string()),
            source: Some("clear".to_string()),
            updated_at_ms: 123456,
        };
        let path = agent_link_path_in(&dir, "tab-1").unwrap();
        std::fs::write(&path, serde_json::to_string(&link).unwrap()).unwrap();

        let read_back = read_agent_link_from(&dir, "tab-1").expect("link should load");
        assert_eq!(read_back, link);

        remove_agent_link_from(&dir, "tab-1");
        assert!(read_agent_link_from(&dir, "tab-1").is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
