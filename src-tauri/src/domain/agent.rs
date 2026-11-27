use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Supported coding agents and shell runtime kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AgentKind {
    #[serde(rename = "claude")]
    Claude,
    #[serde(rename = "agy")]
    Agy,
    #[serde(rename = "codex")]
    Codex,
    #[serde(rename = "opencode")]
    OpenCode,
    #[serde(rename = "shell")]
    Shell,
}

impl AgentKind {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Claude => "Claude Code",
            Self::Agy => "Antigravity",
            Self::Codex => "Codex",
            Self::OpenCode => "OpenCode",
            Self::Shell => "PowerShell",
        }
    }

    pub fn accent_color(self) -> &'static str {
        match self {
            Self::Claude => "#D97757",
            Self::Agy => "#4E8DF5",
            Self::Codex => "#10A37F",
            Self::OpenCode => "#C4B5FD",
            Self::Shell => "#94A3B8",
        }
    }
}

/// Launchable agent preset exposed to the "+" launcher UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentPreset {
    pub id: String,
    pub kind: AgentKind,
    pub label: String,
    pub profile: Option<String>,
    pub description: String,
    pub accent_color: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub installed: bool,
}

/// Resolved executable command ready to spawn inside ConPTY.
#[derive(Debug, Clone)]
pub struct AgentSpawnSpec {
    pub program: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub cwd: PathBuf,
}

/// Locate an executable on PATH or known Windows user install paths.
pub fn find_executable(candidates: &[&str], fallback_paths: &[PathBuf]) -> Option<PathBuf> {
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            for name in candidates {
                let candidate = dir.join(name);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    for path in fallback_paths {
        if path.is_file() {
            return Some(path.clone());
        }
    }
    None
}

fn home_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

fn local_app_data_dir() -> PathBuf {
    dirs::data_local_dir().unwrap_or_else(home_dir)
}

fn roaming_npm_dir() -> PathBuf {
    dirs::config_dir().unwrap_or_else(home_dir).join("npm")
}

/// Locate the preferred PowerShell executable (`pwsh.exe`, then `powershell.exe`).
pub fn find_shell_executable() -> String {
    find_executable(
        &["pwsh.exe", "powershell.exe"],
        &[PathBuf::from(
            r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe",
        )],
    )
    .map(|p| p.to_string_lossy().into_owned())
    .unwrap_or_else(|| "powershell.exe".to_string())
}

/// Wrap a value in a PowerShell literal single-quoted string (`'` escaped as `''`).
pub fn ps_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// Encode a PowerShell script as UTF-16LE Base64 for `-EncodedCommand`.
pub fn encode_powershell_command(script: &str) -> String {
    let mut utf16le = Vec::with_capacity(script.len() * 2);
    for unit in script.encode_utf16() {
        utf16le.extend_from_slice(&unit.to_le_bytes());
    }
    BASE64.encode(&utf16le)
}

/// Decode a `-EncodedCommand` UTF-16LE Base64 payload back into UTF-8 text.
pub fn decode_powershell_command(encoded: &str) -> Option<String> {
    let bytes = BASE64.decode(encoded).ok()?;
    if bytes.len() % 2 != 0 {
        return None;
    }
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .collect();
    String::from_utf16(&units).ok()
}

/// Names usable as a profile id: letters, digits, '-' and '_'.
fn is_profile_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Sub-directory names of `dir` that are valid profile names, sorted.
fn sorted_subdir_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().to_str().map(ToOwned::to_owned))
        .filter(|n| is_profile_name(n))
        .collect();
    names.sort();
    names
}

/// A Claude config dir has been used by the CLI (so `.claude-<tool>` folders of other tools
/// are not mistaken for profiles).
fn looks_like_claude_config(dir: &Path) -> bool {
    dir.is_dir()
        && [".credentials.json", "projects", "settings.json", ".claude.json"]
            .iter()
            .any(|marker| dir.join(marker).exists())
}

/// Claude profiles found in `home`: the `<name>` of every `~/.claude-<name>` directory, sorted.
/// Velox only treats them as profiles; the plain `~/.claude` is the default config.
pub fn claude_profile_names(home: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(home)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| looks_like_claude_config(&e.path()))
        .filter_map(|e| {
            e.file_name()
                .to_str()
                .and_then(|n| n.strip_prefix(".claude-"))
                .map(ToOwned::to_owned)
        })
        .filter(|n| is_profile_name(n))
        .collect();
    names.sort();
    names
}

/// Build the default list of agent presets auto-detected from the local machine.
pub fn build_default_presets() -> Vec<AgentPreset> {
    let home = home_dir();
    let local_app = local_app_data_dir();
    let npm_dir = roaming_npm_dir();

    let claude_exe = find_executable(
        &["claude.exe", "claude.cmd", "claude"],
        &[home.join(".local").join("bin").join("claude.exe")],
    );
    let agy_exe = find_executable(
        &["agy.exe", "agy.cmd", "agy"],
        &[local_app.join("agy").join("bin").join("agy.exe")],
    );
    let codex_exe = find_executable(
        &["codex.cmd", "codex.exe", "codex"],
        &[npm_dir.join("codex.cmd")],
    );
    let opencode_exe = find_executable(
        &["opencode.cmd", "opencode.exe", "opencode"],
        &[npm_dir.join("opencode.cmd")],
    );
    let shell_cmd = find_shell_executable();
    let shell_installed = Path::new(&shell_cmd).is_file();

    let mut presets = Vec::new();

    let claude_cmd = claude_exe
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "claude".to_string());
    let claude_installed = claude_exe.is_some();

    // Every Claude profile on this machine: `~/.claude-<name>` dirs (each its own login and
    // history, selected with CLAUDE_CONFIG_DIR). Without any, the default `~/.claude`.
    let claude_profiles = claude_profile_names(&home);
    for profile in &claude_profiles {
        let mut env = HashMap::new();
        env.insert(
            "CLAUDE_CONFIG_DIR".to_string(),
            home.join(format!(".claude-{profile}"))
                .to_string_lossy()
                .into_owned(),
        );
        presets.push(AgentPreset {
            id: format!("claude-{profile}"),
            kind: AgentKind::Claude,
            label: "Claude".to_string(),
            profile: Some(profile.clone()),
            description: format!("Claude profile ~/.claude-{profile} (skip permissions)"),
            accent_color: AgentKind::Claude.accent_color().to_string(),
            command: claude_cmd.clone(),
            args: vec!["--dangerously-skip-permissions".to_string()],
            env,
            installed: claude_installed,
        });
    }

    if claude_profiles.is_empty() {
        presets.push(AgentPreset {
            id: "claude".to_string(),
            kind: AgentKind::Claude,
            label: "Claude Code".to_string(),
            profile: None,
            description: "Anthropic Claude CLI".to_string(),
            accent_color: AgentKind::Claude.accent_color().to_string(),
            command: claude_cmd,
            args: vec![],
            env: HashMap::new(),
            installed: claude_installed,
        });
    }

    // Antigravity CLI (AGY) — detect credential profiles (~/.gemini/antigravity-cli/profiles/<name>)
    let agy_cmd = agy_exe
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "agy".to_string());
    let agy_installed = agy_exe.is_some();
    let mut agy_env = HashMap::new();
    agy_env.insert(
        "AGY_CLI_DISABLE_AUTO_UPDATE".to_string(),
        "true".to_string(),
    );

    let agy_profiles_dir = home
        .join(".gemini")
        .join("antigravity-cli")
        .join("profiles");
    // Every credential profile the Antigravity CLI knows about, whatever its name.
    let mut agy_profiles_added = false;
    for profile_name in sorted_subdir_names(&agy_profiles_dir) {
        presets.push(AgentPreset {
            id: format!("agy-{profile_name}"),
            kind: AgentKind::Agy,
            label: "Antigravity".to_string(),
            profile: Some(profile_name.clone()),
            description: format!("Antigravity account '{profile_name}' (shared history & MCPs)"),
            accent_color: AgentKind::Agy.accent_color().to_string(),
            command: agy_cmd.clone(),
            args: vec!["--dangerously-skip-permissions".to_string()],
            env: agy_env.clone(),
            installed: agy_installed,
        });
        agy_profiles_added = true;
    }

    if !agy_profiles_added {
        presets.push(AgentPreset {
            id: "agy".to_string(),
            kind: AgentKind::Agy,
            label: "Antigravity".to_string(),
            profile: None,
            description: "Google Antigravity CLI (auto-update off)".to_string(),
            accent_color: AgentKind::Agy.accent_color().to_string(),
            command: agy_cmd,
            args: vec!["--dangerously-skip-permissions".to_string()],
            env: agy_env,
            installed: agy_installed,
        });
    }

    // Codex CLI
    presets.push(AgentPreset {
        id: "codex".to_string(),
        kind: AgentKind::Codex,
        label: "Codex".to_string(),
        profile: None,
        description: "OpenAI Codex CLI".to_string(),
        accent_color: AgentKind::Codex.accent_color().to_string(),
        command: codex_exe
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "codex".to_string()),
        args: vec![],
        env: HashMap::new(),
        installed: codex_exe.is_some(),
    });

    // OpenCode CLI
    presets.push(AgentPreset {
        id: "opencode".to_string(),
        kind: AgentKind::OpenCode,
        label: "OpenCode".to_string(),
        profile: None,
        description: "OpenCode CLI (FreeLLMAPI / multi-model)".to_string(),
        accent_color: AgentKind::OpenCode.accent_color().to_string(),
        command: opencode_exe
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "opencode".to_string()),
        args: vec![],
        env: HashMap::new(),
        installed: opencode_exe.is_some(),
    });

    // Plain shell fallback
    presets.push(AgentPreset {
        id: "shell".to_string(),
        kind: AgentKind::Shell,
        label: "PowerShell".to_string(),
        profile: None,
        description: "Clean interactive terminal shell".to_string(),
        accent_color: AgentKind::Shell.accent_color().to_string(),
        command: shell_cmd,
        args: vec!["-NoLogo".to_string()],
        env: HashMap::new(),
        installed: shell_installed,
    });

    presets
}

/// Resolve the exact executable, arguments (including --resume when waking), and environment.
///
/// Agent presets are launched inside PowerShell (`-NoLogo -NoExit -EncodedCommand ...`) so that
/// quitting an agent drops the user back into an interactive PowerShell prompt instead of closing
/// the PTY. The `shell` preset itself spawns directly without wrapping.
pub fn resolve_spawn_spec(
    preset: &AgentPreset,
    cwd: &Path,
    resume_session_id: Option<&str>,
) -> AgentSpawnSpec {
    let launch = match resume_session_id.filter(|s| !s.trim().is_empty()) {
        Some(id) => AgentLaunch::Resume(id),
        None => AgentLaunch::Fresh,
    };
    resolve_launch_spec(preset, cwd, launch)
}

/// How an agent CLI should be started inside a Velox session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentLaunch<'a> {
    /// Start a brand-new conversation with whatever ID the CLI picks.
    Fresh,
    /// Start a brand-new conversation pinned to a known ID (Claude `--session-id`), so Velox
    /// never has to guess which transcript belongs to this tab.
    NewWithId(&'a str),
    /// Resume an existing conversation.
    Resume(&'a str),
}

/// Per-session environment and prompt context injected when Velox spawns a PTY.
#[derive(Debug, Clone, Default)]
pub struct SessionLaunchContext<'a> {
    pub session_id: &'a str,
    pub session_title: &'a str,
    pub gateway_url: Option<&'a str>,
    pub cli_bin_dir: Option<&'a Path>,
}

/// Prepend `bin_dir` to `PATH` inside `env` without duplicating it if already present.
pub fn prepend_path_env(env: &mut HashMap<String, String>, bin_dir: &Path) {
    let existing_os = env
        .remove("PATH")
        .or_else(|| env.remove("Path"))
        .map(std::ffi::OsString::from)
        .or_else(|| std::env::var_os("PATH"));

    let mut paths: Vec<PathBuf> = vec![bin_dir.to_path_buf()];
    if let Some(raw) = existing_os {
        let target_norm = bin_dir
            .to_string_lossy()
            .trim_end_matches(['\\', '/'])
            .to_ascii_lowercase();
        for p in std::env::split_paths(&raw) {
            let p_norm = p
                .to_string_lossy()
                .trim_end_matches(['\\', '/'])
                .to_ascii_lowercase();
            if p_norm.is_empty() || p_norm != target_norm {
                paths.push(p);
            }
        }
    }

    if let Ok(joined) = std::env::join_paths(paths) {
        env.insert("PATH".to_string(), joined.to_string_lossy().into_owned());
    } else {
        env.insert("PATH".to_string(), bin_dir.to_string_lossy().into_owned());
    }
}

/// System prompt addendum injected for Claude Code sessions running inside Velox.
pub fn claude_session_system_prompt(session_id: &str, session_title: &str) -> String {
    let short_id: String = session_id.chars().take(8).collect();
    format!(
        "You are running inside a Velox session tab titled \"{session_title}\" (VELOX_SESSION_ID={session_id}, short id {short_id}). \
        To collaborate with other local Velox sessions on this PC: run `velox list` to list sessions, \
        `velox msg \"<session>\" \"<text>\"` to send a direct message without manual approval (or use `velox-team` MCP tools `session_list` and `session_send`), \
        and `velox read \"<session>\" [lines]` to inspect another session's terminal output. \
        When the user asks you to use Velox to talk to another session (for example \"usa velox para hablarle a X\"), run `velox list` and `velox msg` right away without asking for confirmation. \
        Incoming messages starting with `[Velox · mensaje de ...]` come from another Velox session; reply to them with `velox msg`."
    )
}

/// Like [`resolve_spawn_spec`] but with an explicit launch mode.
pub fn resolve_launch_spec(
    preset: &AgentPreset,
    cwd: &Path,
    launch: AgentLaunch<'_>,
) -> AgentSpawnSpec {
    resolve_launch_spec_with_options(preset, cwd, launch, None, None)
}

/// Like [`resolve_launch_spec`] with optional per-session Velox messaging identity and CLI PATH.
pub fn resolve_launch_spec_with_context(
    preset: &AgentPreset,
    cwd: &Path,
    launch: AgentLaunch<'_>,
    ctx: Option<&SessionLaunchContext<'_>>,
) -> AgentSpawnSpec {
    resolve_launch_spec_with_options(preset, cwd, launch, ctx, None)
}

/// Like [`resolve_launch_spec`] and optionally injects `--settings <path>` for Claude Code so
/// Velox's `SessionStart` hook is merged alongside the user's own hooks.
pub fn resolve_launch_spec_with_settings(
    preset: &AgentPreset,
    cwd: &Path,
    launch: AgentLaunch<'_>,
    claude_settings_path: Option<&Path>,
) -> AgentSpawnSpec {
    resolve_launch_spec_with_options(preset, cwd, launch, None, claude_settings_path)
}

/// Unified spawn-spec builder combining Velox messaging context and Claude hook settings.
pub fn resolve_launch_spec_with_options(
    preset: &AgentPreset,
    cwd: &Path,
    launch: AgentLaunch<'_>,
    ctx: Option<&SessionLaunchContext<'_>>,
    claude_settings_path: Option<&Path>,
) -> AgentSpawnSpec {
    let mut agent_args = preset.args.clone();
    let mut env = preset.env.clone();

    // Ensure rich terminal capabilities for ConPTY agents
    env.entry("TERM".to_string())
        .or_insert_with(|| "xterm-256color".to_string());
    env.entry("COLORTERM".to_string())
        .or_insert_with(|| "truecolor".to_string());

    if preset.kind == AgentKind::Claude {
        if let Some(settings_path) = claude_settings_path {
            agent_args.push("--settings".to_string());
            agent_args.push(settings_path.to_string_lossy().into_owned());
        }
    }

    if let AgentLaunch::NewWithId(id) = launch {
        if preset.kind == AgentKind::Claude && !id.trim().is_empty() {
            agent_args.push("--session-id".to_string());
            agent_args.push(id.to_string());
        }
    }

    let resume_session_id = match launch {
        AgentLaunch::Resume(id) if !id.trim().is_empty() => Some(id),
        _ => None,
    };
    if let Some(session_id) = resume_session_id {
        match preset.kind {
            AgentKind::Claude => {
                agent_args.push("--resume".to_string());
                agent_args.push(session_id.to_string());
            }
            AgentKind::Agy => {
                agent_args.push("--conversation".to_string());
                agent_args.push(session_id.to_string());
            }
            AgentKind::OpenCode => {
                agent_args.push("--session".to_string());
                agent_args.push(session_id.to_string());
            }
            AgentKind::Codex => {
                agent_args.push("resume".to_string());
                agent_args.push(session_id.to_string());
            }
            AgentKind::Shell => {}
        }
    }

    if let Some(session_ctx) = ctx {
        if !session_ctx.session_id.trim().is_empty() {
            env.insert(
                "VELOX_SESSION_ID".to_string(),
                session_ctx.session_id.to_string(),
            );
        }
        if !session_ctx.session_title.trim().is_empty() {
            env.insert(
                "VELOX_SESSION_TITLE".to_string(),
                session_ctx.session_title.to_string(),
            );
        }
        if let Some(url) = session_ctx.gateway_url.filter(|u| !u.trim().is_empty()) {
            env.insert("VELOX_GATEWAY_URL".to_string(), url.to_string());
        }
        if let Some(bin_dir) = session_ctx.cli_bin_dir {
            prepend_path_env(&mut env, bin_dir);
        }
        if preset.kind == AgentKind::Claude && !session_ctx.session_id.trim().is_empty() {
            agent_args.push("--append-system-prompt".to_string());
            agent_args.push(claude_session_system_prompt(
                session_ctx.session_id,
                session_ctx.session_title,
            ));
        }
    }

    let (program, final_args) = if preset.kind == AgentKind::Shell {
        (preset.command.clone(), agent_args)
    } else {
        let mut ps_parts = Vec::with_capacity(2 + agent_args.len());
        ps_parts.push("&".to_string());
        ps_parts.push(ps_single_quote(&preset.command));
        for arg in &agent_args {
            ps_parts.push(ps_single_quote(arg));
        }
        let script = ps_parts.join(" ");
        let encoded = encode_powershell_command(&script);
        (
            find_shell_executable(),
            vec![
                "-NoLogo".to_string(),
                "-NoExit".to_string(),
                "-EncodedCommand".to_string(),
                encoded,
            ],
        )
    };

    AgentSpawnSpec {
        program,
        args: final_args,
        env,
        cwd: cwd.to_path_buf(),
    }
}

/// Swap the active credential to the preset's profile before spawning, when supported.
pub fn prepare_agent_profile(preset: &AgentPreset) {
    if preset.kind != AgentKind::Agy {
        return;
    }
    let Some(profile) = preset.profile.as_deref() else {
        return;
    };
    let helper = home_dir()
        .join(".gemini")
        .join("antigravity-cli")
        .join("agy-profile.ps1");
    if !helper.is_file() {
        return;
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        // Tabs now start in parallel (off the IPC thread): two swaps of the single shared
        // credential must not interleave.
        static PROFILE_SWAP: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _swap = PROFILE_SWAP.lock().unwrap_or_else(|e| e.into_inner());

        let mut cmd = std::process::Command::new("powershell.exe");
        cmd.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(&helper)
            .args(["-Profile", profile, "-PrepareOnly"])
            .creation_flags(CREATE_NO_WINDOW);

        match cmd.status() {
            Ok(status) if !status.success() => {
                tracing::warn!(
                    profile,
                    ?status,
                    "agy-profile.ps1 -PrepareOnly exited non-zero"
                );
            }
            Err(err) => {
                tracing::warn!(profile, %err, "Failed to invoke agy-profile.ps1");
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_preset(kind: AgentKind, command: &str) -> AgentPreset {
        AgentPreset {
            id: "test".to_string(),
            kind,
            label: "Test".to_string(),
            profile: None,
            description: "Test preset".to_string(),
            accent_color: kind.accent_color().to_string(),
            command: command.to_string(),
            args: vec![],
            env: HashMap::new(),
            installed: true,
        }
    }

    fn decoded_ps_script(spec: &AgentSpawnSpec) -> String {
        assert_eq!(spec.args.len(), 4);
        assert_eq!(spec.args[0], "-NoLogo");
        assert_eq!(spec.args[1], "-NoExit");
        assert_eq!(spec.args[2], "-EncodedCommand");
        decode_powershell_command(&spec.args[3]).expect("valid UTF-16LE base64 EncodedCommand")
    }

    #[test]
    fn resolve_spawn_spec_adds_resume_flag_for_claude() {
        let preset = sample_preset(AgentKind::Claude, "claude.exe");
        let spec = resolve_spawn_spec(&preset, Path::new("."), Some("abc-123"));
        let lower_prog = spec.program.to_ascii_lowercase();
        assert!(lower_prog.ends_with("pwsh.exe") || lower_prog.ends_with("powershell.exe"));
        assert_eq!(
            decoded_ps_script(&spec),
            "& 'claude.exe' '--resume' 'abc-123'"
        );
    }

    #[test]
    fn resolve_launch_spec_pins_session_id_for_new_claude_sessions() {
        let preset = sample_preset(AgentKind::Claude, "claude.exe");
        let spec = resolve_launch_spec(&preset, Path::new("."), AgentLaunch::NewWithId("id-1"));
        assert_eq!(
            decoded_ps_script(&spec),
            "& 'claude.exe' '--session-id' 'id-1'"
        );

        let settings = Path::new(r"C:\velox\hooks\claude-settings.json");
        let with_settings = resolve_launch_spec_with_settings(
            &preset,
            Path::new("."),
            AgentLaunch::Resume("id-1"),
            Some(settings),
        );
        assert_eq!(
            decoded_ps_script(&with_settings),
            r"& 'claude.exe' '--settings' 'C:\velox\hooks\claude-settings.json' '--resume' 'id-1'"
        );

        // Non-Claude agents ignore NewWithId and claude_settings_path
        let agy = sample_preset(AgentKind::Agy, "agy.exe");
        let spec = resolve_launch_spec_with_settings(
            &agy,
            Path::new("."),
            AgentLaunch::NewWithId("id-1"),
            Some(settings),
        );
        assert_eq!(decoded_ps_script(&spec), "& 'agy.exe'");
    }

    #[test]
    fn resolve_spawn_spec_adds_conversation_flag_for_agy() {
        let preset = sample_preset(AgentKind::Agy, "agy.exe");
        let spec = resolve_spawn_spec(&preset, Path::new("."), Some("conv-456"));
        assert_eq!(
            decoded_ps_script(&spec),
            "& 'agy.exe' '--conversation' 'conv-456'"
        );
    }

    #[test]
    fn resolve_spawn_spec_adds_session_flag_for_opencode() {
        let preset = sample_preset(AgentKind::OpenCode, "opencode");
        let spec = resolve_spawn_spec(&preset, Path::new("."), Some("sess-789"));
        assert_eq!(
            decoded_ps_script(&spec),
            "& 'opencode' '--session' 'sess-789'"
        );
    }

    #[test]
    fn resolve_spawn_spec_quotes_spaces_quotes_and_cmd_wrappers() {
        let mut preset = sample_preset(
            AgentKind::Codex,
            r"C:\Program Files\O'Brien Tools\codex wrapper.cmd",
        );
        preset.args = vec!["say \"hi\"".to_string(), "it's ok".to_string()];
        preset
            .env
            .insert("CUSTOM_KEY".to_string(), "custom_val".to_string());

        let spec = resolve_spawn_spec(&preset, Path::new(r"C:\Work\My Repo"), None);
        assert_eq!(spec.cwd, Path::new(r"C:\Work\My Repo"));
        assert_eq!(
            spec.env.get("CUSTOM_KEY").map(String::as_str),
            Some("custom_val")
        );
        assert_eq!(
            decoded_ps_script(&spec),
            r#"& 'C:\Program Files\O''Brien Tools\codex wrapper.cmd' 'say "hi"' 'it''s ok'"#
        );
    }

    #[test]
    fn resolve_spawn_spec_leaves_shell_preset_unwrapped() {
        let mut preset = sample_preset(AgentKind::Shell, "powershell.exe");
        preset.args = vec!["-NoLogo".to_string()];
        let spec = resolve_spawn_spec(&preset, Path::new("."), Some("ignored"));
        assert_eq!(spec.program, "powershell.exe");
        assert_eq!(spec.args, vec!["-NoLogo"]);
    }

    #[test]
    fn build_default_presets_includes_agy_preset_or_profiles() {
        let presets = build_default_presets();
        let agy_presets: Vec<_> = presets
            .iter()
            .filter(|p| p.kind == AgentKind::Agy)
            .collect();
        assert!(!agy_presets.is_empty());
        for p in agy_presets {
            assert!(p
                .args
                .contains(&"--dangerously-skip-permissions".to_string()));
            assert_eq!(
                p.env.get("AGY_CLI_DISABLE_AUTO_UPDATE").map(String::as_str),
                Some("true")
            );
        }
    }

    #[test]
    fn resolve_launch_spec_with_context_injects_env_and_claude_prompt() {
        let bin_dir = PathBuf::from(r"C:\Users\Test\AppData\Local\velox\bin");
        let ctx = SessionLaunchContext {
            session_id: "12345678-aaaa-bbbb-cccc-1234567890ab",
            session_title: "Backend Worker",
            gateway_url: Some("http://127.0.0.1:43111"),
            cli_bin_dir: Some(bin_dir.as_path()),
        };

        let claude = sample_preset(AgentKind::Claude, "claude.exe");
        let spec = resolve_launch_spec_with_context(
            &claude,
            Path::new("."),
            AgentLaunch::Resume("conv-1"),
            Some(&ctx),
        );
        assert_eq!(
            spec.env.get("VELOX_SESSION_ID").map(String::as_str),
            Some("12345678-aaaa-bbbb-cccc-1234567890ab")
        );
        assert_eq!(
            spec.env.get("VELOX_SESSION_TITLE").map(String::as_str),
            Some("Backend Worker")
        );
        assert_eq!(
            spec.env.get("VELOX_GATEWAY_URL").map(String::as_str),
            Some("http://127.0.0.1:43111")
        );
        let path_val = spec.env.get("PATH").expect("PATH injected");
        let entries: Vec<_> = std::env::split_paths(path_val).collect();
        assert_eq!(entries.first(), Some(&bin_dir));
        let script = decoded_ps_script(&spec);
        assert!(script.contains("'--resume' 'conv-1'"));
        assert!(script.contains("'--append-system-prompt'"));
        assert!(script.contains("Backend Worker"));
        assert!(script.contains("velox msg"));

        let agy = sample_preset(AgentKind::Agy, "agy.exe");
        let agy_spec =
            resolve_launch_spec_with_context(&agy, Path::new("."), AgentLaunch::Fresh, Some(&ctx));
        assert_eq!(
            agy_spec.env.get("VELOX_SESSION_ID").map(String::as_str),
            Some("12345678-aaaa-bbbb-cccc-1234567890ab")
        );
        assert!(!decoded_ps_script(&agy_spec).contains("--append-system-prompt"));
    }
}
