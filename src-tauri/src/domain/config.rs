use crate::domain::error::VeloxResult;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Persistent user configuration loaded from `%APPDATA%/velox/config.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VeloxConfig {
    /// Bounded per-pane scrollback ring buffer size in bytes (default: 2 MB).
    #[serde(default = "default_scrollback_bytes")]
    pub scrollback_bytes: usize,

    /// Automatically hibernate idle sessions after N minutes (0 = disabled).
    #[serde(default = "default_auto_hibernate_minutes")]
    pub auto_hibernate_minutes: u64,

    /// Show persisted sessions as Sleeping (0 MB RAM) on startup until clicked.
    #[serde(default = "default_true")]
    pub lazy_restore_on_startup: bool,

    /// Enable the embedded Rust Lazy MCP Gateway (scale-to-zero).
    #[serde(default = "default_true")]
    pub mcp_gateway_enabled: bool,

    /// Port for the Streamable HTTP Lazy MCP Gateway.
    #[serde(default = "default_mcp_port")]
    pub mcp_gateway_port: u16,

    /// Path to backends.json compatible with lazy-mcp-gateway.
    #[serde(default = "default_mcp_backends_path")]
    pub mcp_backends_path: String,

    /// Default working directory for new agent sessions.
    #[serde(default = "default_cwd")]
    pub default_cwd: String,

    /// Terminal font family.
    #[serde(default = "default_font_family")]
    pub font_family: String,

    /// Terminal font size in px.
    #[serde(default = "default_font_size")]
    pub font_size: u16,

    /// Animated per-agent terminal backgrounds.
    #[serde(default = "default_true")]
    pub animated_backgrounds: bool,

    /// Recently used project directories for quick selection in the "+" launcher.
    #[serde(default)]
    pub recent_dirs: Vec<String>,

    /// Velox Online: share sessions with teammates over Tailscale (see docs/online/PLAN.md).
    /// Must stay the LAST field: TOML emits tables after plain values.
    #[serde(default)]
    pub team: TeamConfig,
}

/// Velox Online settings (`[team]` table in config.toml).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamConfig {
    /// Master switch. When false no server is started and no peer is dialed.
    #[serde(default)]
    pub enabled: bool,

    /// Name teammates see on this machine's sessions (e.g. "Rodolfo").
    #[serde(default = "default_team_display_name")]
    pub display_name: String,

    /// TCP port of this machine's team WebSocket server.
    #[serde(default = "default_team_port")]
    pub listen_port: u16,

    /// Address the team server binds to. Empty = auto-detect the Tailscale IPv4 (100.x.y.z).
    /// "127.0.0.1" is allowed for local testing. Never bind 0.0.0.0.
    #[serde(default)]
    pub bind_host: String,

    /// Shared secret every peer must present (Bearer token). Same value on every teammate.
    #[serde(default)]
    pub token: String,

    /// Deliver task requests that come from this same machine without asking (agent -> local agent).
    #[serde(default)]
    pub auto_approve_local: bool,

    /// Teammates this machine connects to. Keep LAST (array of tables).
    #[serde(default)]
    pub peers: Vec<PeerConfig>,
}

/// One teammate machine to connect to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerConfig {
    /// Label used in the UI and in MCP tools (e.g. "Benjamin"). Unique per config.
    pub name: String,
    /// Tailscale MagicDNS name or IP, e.g. "benjamin" or "100.101.102.103".
    pub host: String,
    /// Peer's team server port.
    #[serde(default = "default_team_port")]
    pub port: u16,
}

fn default_team_port() -> u16 {
    43120
}

fn default_team_display_name() -> String {
    std::env::var("USERNAME").unwrap_or_else(|_| "Velox".to_string())
}

impl Default for TeamConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            display_name: default_team_display_name(),
            listen_port: default_team_port(),
            bind_host: String::new(),
            token: String::new(),
            auto_approve_local: false,
            peers: Vec::new(),
        }
    }
}

fn default_scrollback_bytes() -> usize {
    2 * 1024 * 1024
}

fn default_auto_hibernate_minutes() -> u64 {
    20
}

fn default_true() -> bool {
    true
}

fn default_mcp_port() -> u16 {
    43111
}

fn default_mcp_backends_path() -> String {
    let workspace_backends =
        std::path::PathBuf::from(r"D:\Workspace\lazy-mcp-gateway\backends.json");
    if workspace_backends.is_file() {
        return workspace_backends.to_string_lossy().into_owned();
    }
    velox_config_dir()
        .join("mcp-backends.json")
        .to_string_lossy()
        .into_owned()
}

fn default_cwd() -> String {
    let workspace = std::path::PathBuf::from(r"D:\Workspace");
    if workspace.is_dir() {
        return workspace.to_string_lossy().into_owned();
    }
    if let Some(home) = dirs::home_dir() {
        return home.to_string_lossy().into_owned();
    }
    ".".to_string()
}

fn default_font_family() -> String {
    "'JetBrains Mono', 'Cascadia Code', 'Cascadia Mono', Consolas, monospace".to_string()
}

fn default_font_size() -> u16 {
    13
}

impl Default for VeloxConfig {
    fn default() -> Self {
        Self {
            scrollback_bytes: default_scrollback_bytes(),
            auto_hibernate_minutes: default_auto_hibernate_minutes(),
            lazy_restore_on_startup: true,
            mcp_gateway_enabled: true,
            mcp_gateway_port: default_mcp_port(),
            mcp_backends_path: default_mcp_backends_path(),
            default_cwd: default_cwd(),
            font_family: default_font_family(),
            font_size: default_font_size(),
            animated_backgrounds: true,
            recent_dirs: vec![default_cwd()],
            team: TeamConfig::default(),
        }
    }
}

/// Directory storing Velox configuration and persisted state (`%APPDATA%/velox`).
/// `VELOX_CONFIG_DIR` overrides it so a second, isolated instance can run for testing.
pub fn velox_config_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("VELOX_CONFIG_DIR") {
        if !dir.trim().is_empty() {
            return PathBuf::from(dir);
        }
    }
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("velox")
}

impl VeloxConfig {
    pub fn config_path() -> PathBuf {
        velox_config_dir().join("config.toml")
    }

    pub fn load_or_default() -> Self {
        let path = Self::config_path();
        if let Ok(raw) = std::fs::read_to_string(&path) {
            if let Ok(cfg) = toml::from_str::<Self>(&raw) {
                return cfg;
            }
        }
        let cfg = Self::default();
        let _ = cfg.save();
        cfg
    }

    pub fn save(&self) -> VeloxResult<()> {
        let dir = velox_config_dir();
        std::fs::create_dir_all(&dir)?;
        let path = Self::config_path();
        let content = toml::to_string_pretty(self)
            .map_err(|e| crate::domain::error::VeloxError::Config(e.to_string()))?;
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, content)?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }

    pub fn record_recent_dir(&mut self, dir: &str) {
        let trimmed = dir.trim();
        if trimmed.is_empty() {
            return;
        }
        self.recent_dirs
            .retain(|d| !d.eq_ignore_ascii_case(trimmed));
        self.recent_dirs.insert(0, trimmed.to_string());
        self.recent_dirs.truncate(12);
    }
}
