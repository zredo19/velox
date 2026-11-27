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

const MAX_ACTIVITY_SCROLLBACK_BYTES: usize = 256 * 1024;

/// Live activity state read from an agent's rendered ConPTY screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentActivity {
    Working,
    Blocked,
    Idle,
}

fn compiled_regex(cell: &'static OnceLock<Option<Regex>>, pattern: &str) -> Option<&'static Regex> {
    cell.get_or_init(|| Regex::new(pattern).ok()).as_ref()
}

fn matches_re(cell: &'static OnceLock<Option<Regex>>, pattern: &str, text: &str) -> bool {
    compiled_regex(cell, pattern).is_some_and(|re| re.is_match(text))
}

fn join_lower(lines: &[&str]) -> String {
    lines.join("\n").to_lowercase()
}

fn has_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| text.contains(n))
}

/// Extracts the latest OSC window title (`ESC ] 0; ...` or `ESC ] 2; ...`) from raw PTY bytes.
pub fn last_osc_title(raw: &[u8]) -> String {
    let mut latest = String::new();
    let mut i = 0;
    while i + 4 <= raw.len() {
        if raw[i] == 0x1b && raw[i + 1] == b']' && (raw[i + 2] == b'0' || raw[i + 2] == b'2') && raw[i + 3] == b';' {
            let start = i + 4;
            let mut end = start;
            let mut next_i = raw.len();
            while end < raw.len() && end - start <= 512 {
                if raw[end] == 0x07 {
                    next_i = end + 1;
                    break;
                }
                if raw[end] == 0x1b && end + 1 < raw.len() && raw[end + 1] == b'\\' {
                    next_i = end + 2;
                    break;
                }
                end += 1;
            }
            if next_i <= raw.len() && end > start {
                latest = String::from_utf8_lossy(&raw[start..end]).into_owned();
            }
            i = next_i.min(raw.len());
            continue;
        }
        i += 1;
    }
    latest
}

/// Renders visible terminal viewport lines for `raw_scrollback` via `vt100`.
pub fn render_screen_lines(raw_scrollback: &[u8], cols: u16, rows: u16) -> Vec<String> {
    let tail = if raw_scrollback.len() > MAX_ACTIVITY_SCROLLBACK_BYTES {
        &raw_scrollback[raw_scrollback.len() - MAX_ACTIVITY_SCROLLBACK_BYTES..]
    } else {
        raw_scrollback
    };
    let cols = cols.max(10);
    let rows = rows.max(2);
    let mut parser = vt100::Parser::new(rows, cols, 0);
    parser.process(tail);
    parser
        .screen()
        .rows(0, cols)
        .map(|row| row.trim_end().to_string())
        .collect()
}

fn detect_claude_activity(title: &str, lines: &[String]) -> Option<AgentActivity> {
    static BUSY_TITLE: OnceLock<Option<Regex>> = OnceLock::new();
    static IDLE_TITLE: OnceLock<Option<Regex>> = OnceLock::new();
    static INTERRUPT_LINE: OnceLock<Option<Regex>> = OnceLock::new();
    static SPINNER_LINE: OnceLock<Option<Regex>> = OnceLock::new();
    static BG_SHELLS: OnceLock<Option<Regex>> = OnceLock::new();
    static BG_AGENTS: OnceLock<Option<Regex>> = OnceLock::new();
    static HORIZ_RULE: OnceLock<Option<Regex>> = OnceLock::new();
    static BOX_PROMPT: OnceLock<Option<Regex>> = OnceLock::new();
    static EMPTY_PROMPT: OnceLock<Option<Regex>> = OnceLock::new();
    static PROCEED_CHOICE: OnceLock<Option<Regex>> = OnceLock::new();

    const P_BUSY_TITLE: &str = r"^[\u{2800}-\u{28ff}\u{25d0}-\u{25d3}] ";
    const P_IDLE_TITLE: &str = r"^✳ ";
    const P_INTERRUPT: &str = r"^\s*[⏸⏵].*esc to interrupt(?:\s|·|$)";
    const P_SPINNER: &str = r"^\s*[*·✢✶✻✽]\s+\S.*…(?:\s+\(\d+[smh](?:\s|·)|\s*$)";
    const P_BG_SHELLS: &str = r"^\s*[⏸⏵].*·\s+[1-9]\d*\s+shells?\s+(?:·|$)";
    const P_BG_AGENTS: &str = r"^\s*[*·✢✶✻✽]\s+Waiting for [1-9]\d* background agents? to finish\s*$";
    const P_HORIZ_RULE: &str = r"^\s*[─━]{8,}";
    const P_BOX_PROMPT: &str = r"^\s*[❯>]";
    const P_EMPTY_PROMPT: &str = r"^\s*❯\s*$";
    const P_PROCEED_CHOICE: &str = r"(?im)^\s*❯?\s*\d\.\s*(?:yes|no)\b";

    let filled: Vec<&str> = lines
        .iter()
        .map(String::as_str)
        .filter(|l| !l.trim().is_empty())
        .collect();
    let rules: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter_map(|(i, l)| matches_re(&HORIZ_RULE, P_HORIZ_RULE, l).then_some(i))
        .collect();
    let footer_slice: Vec<&str> = if let Some(&last_rule) = rules.last() {
        lines[last_rule + 1..].iter().map(String::as_str).collect()
    } else {
        lines.iter().map(String::as_str).collect()
    };
    let footer = join_lower(&footer_slice);
    let all_refs: Vec<&str> = lines.iter().map(String::as_str).collect();
    let screen = join_lower(&all_refs);

    let last_three = &filled[filled.len().saturating_sub(3)..];
    if join_lower(last_three).contains("showing detailed transcript") {
        return None;
    }

    if footer.contains("esc to cancel")
        && (footer.contains("enter to confirm")
            || (footer.contains("enter to select")
                && has_any(
                    &footer,
                    &[
                        "arrow keys",
                        "arrows to navigate",
                        "↑/↓ to navigate",
                        "↑↓ to navigate",
                    ],
                )))
    {
        return Some(AgentActivity::Blocked);
    }

    if matches_re(&BUSY_TITLE, P_BUSY_TITLE, title) {
        return Some(AgentActivity::Working);
    }
    let recent = &filled[filled.len().saturating_sub(12)..];
    if recent.iter().any(|l| {
        matches_re(&INTERRUPT_LINE, P_INTERRUPT, l) || matches_re(&SPINNER_LINE, P_SPINNER, l)
    }) {
        return Some(AgentActivity::Working);
    }
    let last_five = &filled[filled.len().saturating_sub(5)..];
    if last_five
        .iter()
        .any(|l| matches_re(&BG_SHELLS, P_BG_SHELLS, l))
    {
        return Some(AgentActivity::Working);
    }
    if recent
        .iter()
        .any(|l| matches_re(&BG_AGENTS, P_BG_AGENTS, l))
    {
        return Some(AgentActivity::Working);
    }

    if rules.len() >= 2 {
        let start = rules[rules.len() - 2] + 1;
        let end = rules[rules.len() - 1];
        if start < end
            && lines[start..end]
                .iter()
                .any(|l| matches_re(&BOX_PROMPT, P_BOX_PROMPT, l))
        {
            return Some(AgentActivity::Idle);
        }
    }

    if screen.contains("do you want to proceed?")
        && matches_re(&PROCEED_CHOICE, P_PROCEED_CHOICE, &screen)
    {
        return Some(AgentActivity::Blocked);
    }

    if !lines
        .iter()
        .any(|l| matches_re(&EMPTY_PROMPT, P_EMPTY_PROMPT, l))
        && (has_any(
            &screen,
            &[
                "waiting for permission",
                "do you want to allow this connection?",
                "tab to amend",
                "ctrl+e to explain",
                "review your answers",
                "skip interview and plan immediately",
            ],
        ) || (has_any(&screen, &["do you want to", "would you like to"])
            && has_any(&screen, &["yes", "❯"])))
    {
        return Some(AgentActivity::Blocked);
    }

    if matches_re(&IDLE_TITLE, P_IDLE_TITLE, title) {
        Some(AgentActivity::Idle)
    } else {
        None
    }
}

fn detect_agy_activity(lines: &[String]) -> AgentActivity {
    static AGY_SPINNER: OnceLock<Option<Regex>> = OnceLock::new();
    static AGY_BG_TASKS: OnceLock<Option<Regex>> = OnceLock::new();
    static AGY_NAV_CONFIRM: OnceLock<Option<Regex>> = OnceLock::new();

    const P_SPINNER: &str = r"(?u)^\s*[\u{2800}-\u{28ff}]+\s+\p{L}+ing\b";
    const P_BG_TASKS: &str = r"(?i)·\s*[1-9]\d*\s+task";
    const P_NAV_CONFIRM: &str = r"navigate.*enter\s+(?:to\s+)?(?:confirm|select)";

    let all_refs: Vec<&str> = lines.iter().map(String::as_str).collect();
    let screen = join_lower(&all_refs);
    let filled: Vec<&str> = lines
        .iter()
        .map(String::as_str)
        .filter(|l| !l.trim().is_empty())
        .collect();
    let bottom = &filled[filled.len().saturating_sub(5)..];
    let bottom_lower = join_lower(bottom);

    if screen.contains("requesting permission for:")
        && has_any(
            &screen,
            &["do you want to proceed?", "tab amend", "edit command"],
        )
    {
        return AgentActivity::Blocked;
    }
    if matches_re(&AGY_NAV_CONFIRM, P_NAV_CONFIRM, &bottom_lower) {
        return AgentActivity::Blocked;
    }
    if lines
        .iter()
        .any(|l| matches_re(&AGY_SPINNER, P_SPINNER, l))
    {
        return AgentActivity::Working;
    }
    if bottom
        .iter()
        .any(|l| matches_re(&AGY_BG_TASKS, P_BG_TASKS, l))
    {
        AgentActivity::Working
    } else {
        AgentActivity::Idle
    }
}

/// Classifies agent activity directly from rendered terminal lines and window title.
pub fn detect_activity_from_lines(
    kind: AgentKind,
    title: &str,
    lines: &[String],
) -> Option<AgentActivity> {
    match kind {
        AgentKind::Claude => detect_claude_activity(title, lines),
        AgentKind::Agy => Some(detect_agy_activity(lines)),
        AgentKind::Shell => Some(AgentActivity::Idle),
        AgentKind::Codex | AgentKind::OpenCode => {
            let filled: Vec<&str> = lines
                .iter()
                .map(String::as_str)
                .filter(|l| !l.trim().is_empty())
                .collect();
            let bottom = join_lower(&filled[filled.len().saturating_sub(8)..]);
            if has_any(&bottom, &["permission required", "allow command?", "[y/n]"]) {
                Some(AgentActivity::Blocked)
            } else if has_any(&bottom, &["esc interrupt", "esc to interrupt"]) {
                Some(AgentActivity::Working)
            } else {
                Some(AgentActivity::Idle)
            }
        }
    }
}

/// Renders raw ConPTY scrollback through `vt100` and classifies current agent activity.
pub fn detect_screen_activity(
    kind: AgentKind,
    raw_scrollback: &[u8],
    cols: u16,
    rows: u16,
) -> Option<AgentActivity> {
    let title = last_osc_title(raw_scrollback);
    let lines = render_screen_lines(raw_scrollback, cols, rows);
    detect_activity_from_lines(kind, &title, &lines)
}

/// Returns `true` when the agent inside a PTY is ready to accept direct prompt input
/// (TUI/shell prompt is mounted and not blocked on a permission/selection prompt).
pub fn is_agent_ready_for_input(
    kind: AgentKind,
    raw_scrollback: &[u8],
    cols: u16,
    rows: u16,
) -> bool {
    static HORIZ_RULE: OnceLock<Option<Regex>> = OnceLock::new();
    const P_HORIZ_RULE: &str = r"^\s*[─━]{8,}";

    let title = last_osc_title(raw_scrollback);
    let lines = render_screen_lines(raw_scrollback, cols, rows);
    let activity = detect_activity_from_lines(kind, &title, &lines);
    if activity == Some(AgentActivity::Blocked) {
        return false;
    }

    match kind {
        AgentKind::Shell => lines.iter().any(|l| !l.trim().is_empty()),
        AgentKind::Claude => matches!(
            activity,
            Some(AgentActivity::Idle | AgentActivity::Working)
        ),
        AgentKind::Agy => {
            let has_rule = lines
                .iter()
                .any(|l| matches_re(&HORIZ_RULE, P_HORIZ_RULE, l));
            let has_agy_cue = lines.iter().any(|l| {
                let t = l.trim();
                t == ">"
                    || t.starts_with("> ")
                    || l.contains("Antigravity")
                    || l.contains("Gemini")
            });
            has_rule && has_agy_cue
        }
        AgentKind::Codex | AgentKind::OpenCode => lines.iter().any(|l| {
            let t = l.trim();
            t.starts_with('›')
                || t == ">"
                || t.starts_with("> ")
                || l.to_ascii_lowercase().contains("ask anything")
                || matches_re(&HORIZ_RULE, P_HORIZ_RULE, l)
        }),
    }
}

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

    #[test]
    fn detect_screen_activity_handles_claude_and_agy_states() {
        let claude_ready = "\
─────────────\n\
❯ \n\
─────────────\n\
  bypass permissions on\n";
        let raw_claude_idle = claude_ready.as_bytes();
        assert_eq!(
            detect_screen_activity(AgentKind::Claude, raw_claude_idle, 80, 12),
            Some(AgentActivity::Idle)
        );
        assert!(is_agent_ready_for_input(AgentKind::Claude, raw_claude_idle, 80, 12));

        let agy_ps_boot = b"Windows PowerShell\r\nPS D:\\Workspace\\velox> ";
        assert!(!is_agent_ready_for_input(AgentKind::Agy, agy_ps_boot, 100, 24));

        let agy_ready = "\
        Antigravity CLI 1.5.3\n\
────────────────────────────────────────────────────────────\n\
>\n\
────────────────────────────────────────────────────────────\n";
        assert_eq!(
            detect_screen_activity(AgentKind::Agy, agy_ready.as_bytes(), 100, 24),
            Some(AgentActivity::Idle)
        );
        assert!(is_agent_ready_for_input(AgentKind::Agy, agy_ready.as_bytes(), 100, 24));

        let agy_trust_blocked = "\
Do you trust the contents of this project?\n\
> Yes, I trust this folder\n\
  ↑/↓ Navigate · enter Confirm\n";
        assert_eq!(
            detect_screen_activity(AgentKind::Agy, agy_trust_blocked.as_bytes(), 100, 24),
            Some(AgentActivity::Blocked)
        );
        assert!(!is_agent_ready_for_input(
            AgentKind::Agy,
            agy_trust_blocked.as_bytes(),
            100,
            24
        ));

        let osc = b"\x1b]0;\xe2\xa0\x8b Working\x07";
        assert_eq!(last_osc_title(osc), "⠋ Working");
        assert_eq!(
            detect_screen_activity(AgentKind::Claude, osc, 80, 12),
            Some(AgentActivity::Working)
        );
    }
}
