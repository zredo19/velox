use crate::domain::agent::AgentKind;
use crate::domain::session::SessionStatus;
use chrono::{DateTime, Utc};
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::SystemTime;

static ANSI_RE: OnceLock<Option<Regex>> = OnceLock::new();
static UUID_RE: OnceLock<Option<Regex>> = OnceLock::new();

fn ansi_regex() -> Option<&'static Regex> {
    ANSI_RE
        .get_or_init(|| {
            Regex::new(r"\x1B(?:\[[0-?]*[ -/]*[@-~]|\][^\x07\x1B]*(?:\x07|\x1B\\)|[@-Z\\-_])").ok()
        })
        .as_ref()
}

fn uuid_regex() -> Option<&'static Regex> {
    UUID_RE
        .get_or_init(|| {
            Regex::new(
                r"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$",
            )
            .ok()
        })
        .as_ref()
}

/// Remove CSI and OSC ANSI terminal escape sequences from text.
pub fn strip_ansi(input: &str) -> String {
    match ansi_regex() {
        Some(re) => re.replace_all(input, "").into_owned(),
        None => input.to_string(),
    }
}

/// Heuristically classify agent lifecycle status from a streamed PTY chunk.
pub fn detect_status_from_output(_kind: AgentKind, raw_chunk: &[u8]) -> Option<SessionStatus> {
    let text = String::from_utf8_lossy(raw_chunk);
    let clean = strip_ansi(&text);
    let trimmed = clean.trim();
    if trimmed.is_empty() {
        return None;
    }

    let lower = trimmed.to_ascii_lowercase();
    if lower.contains("allow once")
        || lower.contains("do you want to proceed")
        || lower.contains("[y/n]")
        || lower.contains("(y/n)")
        || lower.contains("permission required")
        || lower.contains("esc to cancel")
    {
        return Some(SessionStatus::Blocked);
    }

    if trimmed.contains('❯')
        || lower.contains("ask anything")
        || trimmed.lines().any(|line| {
            let l = line.trim();
            l == ">" || l.starts_with("> ") || l.starts_with("PS ")
        })
    {
        return Some(SessionStatus::Idle);
    }

    if trimmed.chars().filter(|c| !c.is_control()).count() >= 4 {
        return Some(SessionStatus::Running);
    }

    None
}

fn is_uuid(s: &str) -> bool {
    uuid_regex().is_some_and(|re| re.is_match(s))
}

fn normalize_path(p: &str) -> String {
    p.trim()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_ascii_lowercase()
}

/// Claude config dirs that may hold transcripts for a given Velox profile.
pub fn claude_config_bases(profile: Option<&str>) -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    match profile {
        Some(p) => vec![home.join(format!(".claude-{p}"))],
        None => std::iter::once(home.join(".claude"))
            .chain(
                crate::domain::agent::claude_profile_names(&home)
                    .into_iter()
                    .map(|p| home.join(format!(".claude-{p}"))),
            )
            .collect(),
    }
}

/// Claude stores transcripts in `projects/<cwd with every non-alphanumeric char replaced by '-'>`.
pub fn claude_project_dir_name(cwd: &str) -> String {
    cwd.trim()
        .trim_end_matches(['\\', '/'])
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// Project directories (across profile bases) whose name matches `cwd`.
fn claude_project_dirs_for_cwd(profile: Option<&str>, cwd: &str) -> Vec<PathBuf> {
    let encoded = claude_project_dir_name(cwd).to_ascii_lowercase();
    let with_suffix = format!("{encoded}.");
    let mut out = Vec::new();
    for base in claude_config_bases(profile) {
        let Ok(entries) = std::fs::read_dir(base.join("projects")) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let lower = name.to_ascii_lowercase();
            if lower == encoded || lower.starts_with(&with_suffix) {
                out.push(path);
            }
        }
    }
    out
}

/// True when a Claude transcript contains at least one real user turn (resuming anything else
/// opens an empty conversation).
fn claude_transcript_has_messages(path: &Path) -> bool {
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .any(|line| line.contains("\"type\":\"user\""))
}

/// Locate the transcript file for a Claude session ID in any project dir of the profile.
pub fn find_claude_transcript(profile: Option<&str>, session_id: &str) -> Option<PathBuf> {
    let file_name = format!("{session_id}.jsonl");
    for base in claude_config_bases(profile) {
        let Ok(entries) = std::fs::read_dir(base.join("projects")) else {
            continue;
        };
        for entry in entries.flatten() {
            let candidate = entry.path().join(&file_name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn agy_root() -> Option<PathBuf> {
    Some(dirs::home_dir()?.join(".gemini").join("antigravity-cli"))
}

struct AgyHistoryEntry {
    timestamp_ms: i64,
    workspace: String,
    conversation_id: Option<String>,
}

/// Parse `~/.gemini/antigravity-cli/history.jsonl` (one entry per prompt) in file order.
fn read_agy_history() -> Option<Vec<AgyHistoryEntry>> {
    let raw = std::fs::read_to_string(agy_root()?.join("history.jsonl")).ok()?;
    Some(
        raw.lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .map(|v| AgyHistoryEntry {
                timestamp_ms: v.get("timestamp").and_then(|t| t.as_i64()).unwrap_or(0),
                workspace: v
                    .get("workspace")
                    .and_then(|w| w.as_str())
                    .map(normalize_path)
                    .unwrap_or_default(),
                conversation_id: v
                    .get("conversationId")
                    .and_then(|c| c.as_str())
                    .filter(|c| is_uuid(c))
                    .map(ToOwned::to_owned),
            })
            .collect(),
    )
}

/// Folder (normalized) where each AGY conversation was first used.
fn agy_origins(history: &[AgyHistoryEntry]) -> HashMap<String, String> {
    let mut origins = HashMap::new();
    for entry in history {
        if let Some(conv) = &entry.conversation_id {
            if !entry.workspace.is_empty() {
                origins
                    .entry(conv.clone())
                    .or_insert_with(|| entry.workspace.clone());
            }
        }
    }
    origins
}

/// Whether a conversation was started in `cwd` (Claude: transcript lives in that folder's project
/// dir; AGY: first prompt was sent from that folder).
pub fn agent_session_belongs_to_cwd(
    kind: AgentKind,
    profile: Option<&str>,
    cwd: &str,
    session_id: &str,
) -> bool {
    match kind {
        AgentKind::Claude => {
            let file_name = format!("{session_id}.jsonl");
            claude_project_dirs_for_cwd(profile, cwd)
                .iter()
                .any(|dir| dir.join(&file_name).is_file())
        }
        AgentKind::Agy => read_agy_history().is_some_and(|h| {
            agy_origins(&h).get(session_id) == Some(&normalize_path(cwd))
        }),
        _ => false,
    }
}

/// Find the most recent conversation of `cwd` that no other Velox tab owns. Used to repair a tab
/// whose stored conversation turned out to belong to another tab.
pub fn recover_agent_session_id(
    kind: AgentKind,
    profile: Option<&str>,
    cwd: &str,
    claimed: &HashSet<String>,
) -> Option<String> {
    discover_agent_session_id(kind, profile, cwd, DateTime::<Utc>::UNIX_EPOCH, claimed)
}

/// Whether `session_id` points at a conversation the CLI can actually resume with content.
pub fn agent_session_is_resumable(
    kind: AgentKind,
    profile: Option<&str>,
    session_id: &str,
) -> bool {
    match kind {
        AgentKind::Claude => {
            is_uuid(session_id)
                && find_claude_transcript(profile, session_id)
                    .is_some_and(|p| claude_transcript_has_messages(&p))
        }
        AgentKind::Agy => {
            is_uuid(session_id)
                && agy_root()
                    .map(|root| root.join("conversations").join(format!("{session_id}.db")))
                    .is_some_and(|p| p.is_file())
        }
        AgentKind::Codex | AgentKind::OpenCode => !session_id.trim().is_empty(),
        AgentKind::Shell => false,
    }
}

/// Discover the conversation that a Velox tab is currently driving, so that hibernating or
/// closing the app can later resume exactly that one (`--resume` / `--conversation`).
///
/// Only conversations started in `cwd`, active since `started_after`, containing at least one user
/// message and not already owned by another Velox tab (`claimed`) are considered.
pub fn discover_agent_session_id(
    kind: AgentKind,
    profile: Option<&str>,
    cwd: &str,
    started_after: DateTime<Utc>,
    claimed: &HashSet<String>,
) -> Option<String> {
    match kind {
        AgentKind::Claude => {
            let mut candidates: Vec<(SystemTime, String, PathBuf)> = Vec::new();
            for dir in claude_project_dirs_for_cwd(profile, cwd) {
                let Ok(files) = std::fs::read_dir(&dir) else {
                    continue;
                };
                for entry in files.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                        continue;
                    }
                    let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                        continue;
                    };
                    if !is_uuid(stem) || claimed.contains(stem) {
                        continue;
                    }
                    let Some(modified) = entry.metadata().ok().and_then(|m| m.modified().ok())
                    else {
                        continue;
                    };
                    let mod_dt: DateTime<Utc> = modified.into();
                    if mod_dt >= started_after {
                        candidates.push((modified, stem.to_string(), path));
                    }
                }
            }
            candidates.sort_by_key(|c| std::cmp::Reverse(c.0));
            candidates
                .into_iter()
                .find(|(_, _, path)| claude_transcript_has_messages(path))
                .map(|(_, id, _)| id)
        }
        AgentKind::Agy => {
            // history.jsonl records every prompt with its workspace and conversation ID, which is
            // far more reliable than brain/ directory mtimes (those don't change on writes).
            // A conversation belongs to the folder where it was first used; later entries from
            // other folders only mean it was (wrongly) resumed there.
            let root = agy_root()?;
            let history = read_agy_history()?;
            let origins = agy_origins(&history);
            let want_cwd = normalize_path(cwd);
            let min_ms = started_after.timestamp_millis();
            for entry in history.iter().rev() {
                if entry.timestamp_ms < min_ms {
                    break;
                }
                let Some(conv) = entry.conversation_id.as_deref() else {
                    continue;
                };
                if claimed.contains(conv) || origins.get(conv) != Some(&want_cwd) {
                    continue;
                }
                if root
                    .join("conversations")
                    .join(format!("{conv}.db"))
                    .is_file()
                {
                    return Some(conv.to_string());
                }
            }
            None
        }
        AgentKind::Codex | AgentKind::OpenCode | AgentKind::Shell => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_project_dir_name_matches_claude_encoding() {
        assert_eq!(claude_project_dir_name(r"D:\Workspace"), "D--Workspace");
        assert_eq!(
            claude_project_dir_name(r"D:\Workspace\winhands background\"),
            "D--Workspace-winhands-background"
        );
    }

    #[test]
    fn strip_ansi_removes_csi_and_osc_sequences() {
        let input = "\x1B[31mHello\x1B[0m \x1B]0;Window Title\x07World";
        assert_eq!(strip_ansi(input), "Hello World");
    }

    #[test]
    fn detect_status_identifies_blocked_approval_prompts() {
        assert_eq!(
            detect_status_from_output(
                AgentKind::Claude,
                b"\x1b[33mDo you want to proceed [Y/n]\x1b[0m"
            ),
            Some(SessionStatus::Blocked)
        );
        assert_eq!(
            detect_status_from_output(AgentKind::OpenCode, b"Permission required: Allow once"),
            Some(SessionStatus::Blocked)
        );
    }

    #[test]
    fn detect_status_identifies_idle_and_running_output() {
        assert_eq!(
            detect_status_from_output(AgentKind::OpenCode, b"Ask anything..."),
            Some(SessionStatus::Idle)
        );
        assert_eq!(
            detect_status_from_output(AgentKind::Claude, b"Compiling velox v0.1.0"),
            Some(SessionStatus::Running)
        );
    }
}
