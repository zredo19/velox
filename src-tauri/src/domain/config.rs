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
        }
    }
}

/// Directory storing Velox configuration and persisted state (`%APPDATA%/velox`).
pub fn velox_config_dir() -> PathBuf {
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
