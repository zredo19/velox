use crate::domain::agent::AgentKind;
use crate::domain::session::{SessionRecord, SessionStatus, SystemTelemetry};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;
use std::sync::Mutex;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

const BYTES_PER_MB: f64 = 1_048_576.0;

/// Combined telemetry and active-agent snapshot produced in a single process-tree pass.
#[derive(Debug, Clone)]
pub struct TelemetrySample {
    pub telemetry: SystemTelemetry,
    pub active_agents: HashMap<String, Option<AgentKind>>,
}

fn normalize_stem(raw: &str, exts: &[&str]) -> String {
    let normalized = raw.replace('\\', "/");
    let file = normalized.rsplit('/').next().unwrap_or(&normalized);
    let lower = file.to_ascii_lowercase();
    for ext in exts {
        if let Some(stripped) = lower.strip_suffix(ext) {
            return stripped.to_string();
        }
    }
    lower
}

fn match_agent_stem(stem: &str) -> Option<AgentKind> {
    match stem {
        "claude" | "claude-code" => Some(AgentKind::Claude),
        "agy" | "antigravity" | "antigravity-cli" => Some(AgentKind::Agy),
        "codex" => Some(AgentKind::Codex),
        "opencode" | "opencode-ai" => Some(AgentKind::OpenCode),
        _ => None,
    }
}

fn match_agent_package_path(arg: &str) -> Option<AgentKind> {
    let lower = arg.replace('\\', "/").to_ascii_lowercase();
    if lower.contains("/@anthropic-ai/claude-code/") || lower.contains("/node_modules/claude-code/")
    {
        return Some(AgentKind::Claude);
    }
    if lower.contains("/@google/antigravity/")
        || lower.contains("/antigravity-cli/")
        || lower.contains("/node_modules/agy/")
    {
        return Some(AgentKind::Agy);
    }
    if lower.contains("/@openai/codex/") || lower.contains("/node_modules/codex/") {
        return Some(AgentKind::Codex);
    }
    if lower.contains("/node_modules/opencode-ai/") || lower.contains("/node_modules/opencode/") {
        return Some(AgentKind::OpenCode);
    }
    None
}

/// Classify a single process as one of the supported coding agents, if recognizable.
pub fn classify_process(name: &str, exe: Option<&Path>, cmd: &[String]) -> Option<AgentKind> {
    let proc_stem = {
        let s = normalize_stem(name, &[".exe", ".cmd", ".bat"]);
        if s.is_empty() {
            exe.and_then(|p| p.file_stem())
                .and_then(|s| s.to_str())
                .map(|s| s.to_ascii_lowercase())
                .unwrap_or_default()
        } else {
            s
        }
    };

    if let Some(agent) = match_agent_stem(&proc_stem) {
        return Some(agent);
    }

    if matches!(
        proc_stem.as_str(),
        "node" | "nodejs" | "bun" | "deno" | "tsx"
    ) {
        // Inspect only the runtime entrypoint (first non-flag argument) to avoid false matches on user args.
        if let Some(script_arg) = cmd.iter().skip(1).find(|a| !a.starts_with('-')) {
            if let Some(agent) = match_agent_package_path(script_arg) {
                return Some(agent);
            }
            let script_stem = normalize_stem(
                script_arg,
                &[".js", ".mjs", ".cjs", ".ts", ".mts", ".cts", ".cmd", ".exe"],
            );
            if let Some(agent) = match_agent_stem(&script_stem) {
                return Some(agent);
            }
        }
        return None;
    }

    if proc_stem == "cmd" {
        let mut iter = cmd.iter().skip(1);
        while let Some(arg) = iter.next() {
            if arg.eq_ignore_ascii_case("/c") || arg.eq_ignore_ascii_case("/k") {
                if let Some(target) = iter.next() {
                    let target_stem = normalize_stem(target, &[".cmd", ".bat", ".exe"]);
                    return match_agent_stem(&target_stem);
                }
                break;
            }
        }
        return None;
    }

    if matches!(proc_stem.as_str(), "powershell" | "pwsh") {
        let mut iter = cmd.iter().skip(1);
        while let Some(arg) = iter.next() {
            if arg.eq_ignore_ascii_case("-file") {
                if let Some(target) = iter.next() {
                    let target_stem = normalize_stem(target, &[".ps1"]);
                    return match_agent_stem(&target_stem);
                }
                break;
            }
        }
        return None;
    }

    None
}

/// Lightweight Windows/cross-platform process-tree RAM and agent sampler backed by `sysinfo`.
pub struct TelemetryMonitor {
    inner: Mutex<System>,
}

impl TelemetryMonitor {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(System::new()),
        }
    }

    /// Sample exact process-tree RSS (in MB) for Velox itself and every awake agent session.
    pub fn sample(&self, sessions: &[SessionRecord], mcp_memory_mb: f64) -> SystemTelemetry {
        self.sample_with_agents(sessions, mcp_memory_mb).telemetry
    }

    /// Sample process-tree RSS and detect the closest active coding agent in each session's tree.
    pub fn sample_with_agents(
        &self,
        sessions: &[SessionRecord],
        mcp_memory_mb: f64,
    ) -> TelemetrySample {
        let mut sys = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        // Memory for the whole system in one pass (cheap); command line (PEB read) and exe
        // path, the expensive part, only for processes inside awake session trees.
        sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_memory(),
        );

        let mut children_map: HashMap<u32, Vec<u32>> = HashMap::new();
        for (pid, proc_info) in sys.processes() {
            if let Some(parent) = proc_info.parent() {
                children_map
                    .entry(parent.as_u32())
                    .or_default()
                    .push(pid.as_u32());
            }
        }

        let needs_cmd: Vec<Pid> = {
            let mut out = Vec::new();
            let mut visited = HashSet::new();
            let mut queue: VecDeque<u32> = sessions
                .iter()
                .filter(|s| !matches!(s.status, SessionStatus::Sleeping | SessionStatus::Exited))
                .filter_map(|s| s.pid)
                .collect();
            while let Some(pid) = queue.pop_front() {
                if !visited.insert(pid) {
                    continue;
                }
                if sys
                    .process(Pid::from_u32(pid))
                    .is_some_and(|p| p.cmd().is_empty() && p.exe().is_none())
                {
                    out.push(Pid::from_u32(pid));
                }
                if let Some(kids) = children_map.get(&pid) {
                    queue.extend(kids.iter().copied());
                }
            }
            out
        };
        if !needs_cmd.is_empty() {
            sys.refresh_processes_specifics(
                ProcessesToUpdate::Some(&needs_cmd),
                false,
                ProcessRefreshKind::nothing()
                    .with_cmd(UpdateKind::OnlyIfNotSet)
                    .with_exe(UpdateKind::OnlyIfNotSet),
            );
        }

        let app_memory_mb = sys
            .process(Pid::from_u32(std::process::id()))
            .map(|p| p.memory() as f64 / BYTES_PER_MB)
            .unwrap_or(0.0);

        let mut session_ram = HashMap::new();
        let mut active_agents = HashMap::new();
        let mut sessions_memory_mb = 0.0;
        let mut active_sessions = 0usize;
        let mut sleeping_sessions = 0usize;

        for session in sessions {
            match session.status {
                SessionStatus::Sleeping => {
                    sleeping_sessions += 1;
                    session_ram.insert(session.id.clone(), 0.0);
                    active_agents.insert(session.id.clone(), None);
                    continue;
                }
                SessionStatus::Exited => {
                    session_ram.insert(session.id.clone(), 0.0);
                    active_agents.insert(session.id.clone(), None);
                    continue;
                }
                _ => {
                    active_sessions += 1;
                }
            }

            let Some(root_pid) = session.pid else {
                session_ram.insert(session.id.clone(), 0.0);
                active_agents.insert(session.id.clone(), None);
                continue;
            };

            let mut total_bytes = 0u64;
            let mut detected_agent: Option<AgentKind> = None;
            let mut root_alive = false;
            let mut queue = VecDeque::new();
            let mut visited = HashSet::new();
            queue.push_back(root_pid);
            visited.insert(root_pid);

            while let Some(pid) = queue.pop_front() {
                if let Some(proc_info) = sys.process(Pid::from_u32(pid)) {
                    if pid == root_pid {
                        root_alive = true;
                    }
                    total_bytes = total_bytes.saturating_add(proc_info.memory());
                    if detected_agent.is_none() {
                        let name = proc_info.name().to_string_lossy();
                        let cmd: Vec<String> = proc_info
                            .cmd()
                            .iter()
                            .map(|s| s.to_string_lossy().into_owned())
                            .collect();
                        detected_agent = classify_process(&name, proc_info.exe(), &cmd);
                    }
                }
                if let Some(kids) = children_map.get(&pid) {
                    for &kid in kids {
                        if visited.insert(kid) {
                            queue.push_back(kid);
                        }
                    }
                }
            }

            let mb = (total_bytes as f64 / BYTES_PER_MB * 10.0).round() / 10.0;
            sessions_memory_mb += mb;
            session_ram.insert(session.id.clone(), mb);
            active_agents.insert(
                session.id.clone(),
                if root_alive {
                    Some(detected_agent.unwrap_or(AgentKind::Shell))
                } else {
                    None
                },
            );
        }

        let app_rounded = (app_memory_mb * 10.0).round() / 10.0;
        let sessions_rounded = (sessions_memory_mb * 10.0).round() / 10.0;
        let mcp_rounded = (mcp_memory_mb * 10.0).round() / 10.0;

        TelemetrySample {
            telemetry: SystemTelemetry {
                app_memory_mb: app_rounded,
                sessions_memory_mb: sessions_rounded,
                mcp_memory_mb: mcp_rounded,
                total_memory_mb: ((app_rounded + sessions_rounded + mcp_rounded) * 10.0).round()
                    / 10.0,
                active_sessions,
                sleeping_sessions,
                session_ram,
            },
            active_agents,
        }
    }
}

impl Default for TelemetryMonitor {
    fn default() -> Self {
        Self::new()
    }
}

/// BFS helper for tests/tree inspection: returns the agent closest to `root_pid`.
#[cfg(test)]
fn detect_closest_agent_in_tree<F>(
    root_pid: u32,
    children_map: &HashMap<u32, Vec<u32>>,
    mut classify_pid: F,
) -> Option<AgentKind>
where
    F: FnMut(u32) -> Option<AgentKind>,
{
    let mut queue = VecDeque::new();
    let mut visited = HashSet::new();
    queue.push_back(root_pid);
    visited.insert(root_pid);

    while let Some(pid) = queue.pop_front() {
        if let Some(agent) = classify_pid(pid) {
            return Some(agent);
        }
        if let Some(kids) = children_map.get(&pid) {
            for &kid in kids {
                if visited.insert(kid) {
                    queue.push_back(kid);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_computes_counts_for_awake_and_sleeping_sessions() {
        let monitor = TelemetryMonitor::new();
        let sessions = vec![
            SessionRecord {
                id: "awake".to_string(),
                title: "Awake".to_string(),
                agent_kind: AgentKind::Shell,
                active_agent: None,
                preset_id: "shell".to_string(),
                profile: None,
                cwd: ".".to_string(),
                status: SessionStatus::Running,
                agent_session_id: None,
                wake_as_shell: false,
                pid: Some(std::process::id()),
                memory_mb: 0.0,
                created_at: "2026-11-24T00:00:00Z".to_string(),
                last_active_at: "2026-11-24T00:00:00Z".to_string(),
                exit_code: None,
                shared: false,
            },
            SessionRecord {
                id: "asleep".to_string(),
                title: "Asleep".to_string(),
                agent_kind: AgentKind::Claude,
                active_agent: None,
                preset_id: "claude-personal".to_string(),
                profile: Some("personal".to_string()),
                cwd: ".".to_string(),
                status: SessionStatus::Sleeping,
                agent_session_id: None,
                wake_as_shell: false,
                pid: None,
                memory_mb: 0.0,
                created_at: "2026-11-24T00:00:00Z".to_string(),
                last_active_at: "2026-11-24T00:00:00Z".to_string(),
                exit_code: None,
                shared: false,
            },
        ];

        let sample = monitor.sample_with_agents(&sessions, 12.5);
        assert_eq!(sample.telemetry.active_sessions, 1);
        assert_eq!(sample.telemetry.sleeping_sessions, 1);
        assert_eq!(sample.telemetry.session_ram.get("asleep"), Some(&0.0));
        assert!(sample.telemetry.app_memory_mb > 0.0);
        assert_eq!(sample.telemetry.mcp_memory_mb, 12.5);
        assert_eq!(
            sample.active_agents.get("awake"),
            Some(&Some(AgentKind::Shell))
        );
        assert_eq!(sample.active_agents.get("asleep"), Some(&None));
    }

    #[test]
    fn classify_process_detects_direct_and_node_hosted_agents() {
        assert_eq!(
            classify_process("claude.exe", None, &[]),
            Some(AgentKind::Claude)
        );
        assert_eq!(classify_process("agy.exe", None, &[]), Some(AgentKind::Agy));
        assert_eq!(
            classify_process(
                "node.exe",
                None,
                &[
                    "C:\\Program Files\\nodejs\\node.exe".into(),
                    "C:\\Users\\test\\AppData\\Roaming\\npm\\node_modules\\@openai\\codex\\bin\\codex.js".into(),
                ]
            ),
            Some(AgentKind::Codex)
        );
        assert_eq!(
            classify_process(
                "node.exe",
                None,
                &[
                    "node".into(),
                    "C:\\Users\\test\\AppData\\Roaming\\npm\\node_modules\\opencode-ai\\bin\\opencode".into(),
                ]
            ),
            Some(AgentKind::OpenCode)
        );
        assert_eq!(
            classify_process(
                "node.exe",
                None,
                &[
                    "node".into(),
                    "C:\\Users\\test\\AppData\\Roaming\\npm\\node_modules\\@anthropic-ai\\claude-code\\cli.js".into(),
                ]
            ),
            Some(AgentKind::Claude)
        );
        assert_eq!(
            classify_process(
                "node.exe",
                None,
                &[
                    "node".into(),
                    "--max-old-space-size=4096".into(),
                    "C:\\Users\\test\\antigravity-cli\\dist\\cli.js".into(),
                ]
            ),
            Some(AgentKind::Agy)
        );
    }

    #[test]
    fn classify_process_rejects_unrelated_scripts_and_encoded_shells() {
        assert_eq!(
            classify_process(
                "node.exe",
                None,
                &[
                    "node".into(),
                    "D:\\Workspace\\claude\\server.js".into(),
                    "agy".into(),
                ]
            ),
            None
        );
        assert_eq!(
            classify_process(
                "powershell.exe",
                None,
                &[
                    "powershell.exe".into(),
                    "-NoLogo".into(),
                    "-NoExit".into(),
                    "-EncodedCommand".into(),
                    "JgAnAGMAbABhAHUAZABlACcA".into(),
                ]
            ),
            None
        );
    }

    #[test]
    fn bfs_picks_agent_closest_to_root_process() {
        // Tree: 1 (powershell) -> 2 (claude) -> 3 (cmd) -> 4 (node opencode)
        let mut children = HashMap::new();
        children.insert(1, vec![2]);
        children.insert(2, vec![3]);
        children.insert(3, vec![4]);

        let picked = detect_closest_agent_in_tree(1, &children, |pid| match pid {
            2 => Some(AgentKind::Claude),
            4 => Some(AgentKind::OpenCode),
            _ => None,
        });
        assert_eq!(picked, Some(AgentKind::Claude));
    }
}
