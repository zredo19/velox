use crate::domain::error::VeloxResult;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

fn default_idle_ms() -> u64 {
    120_000
}

fn default_timeout_ms() -> u64 {
    120_000
}

/// Configuration for a single on-demand stdio MCP backend (compatible with `lazy-mcp-gateway/backends.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpBackendConfig {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(rename = "idleMs", default = "default_idle_ms")]
    pub idle_ms: u64,
    #[serde(rename = "timeoutMs", default = "default_timeout_ms")]
    pub timeout_ms: u64,
}

/// Top-level `backends.json` file schema.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct McpBackendsFile {
    #[serde(default)]
    pub backends: BTreeMap<String, McpBackendConfig>,
}

/// Load `backends.json` cleanly, returning an empty map if the file does not exist yet.
pub fn load_backends_config(path: &Path) -> VeloxResult<McpBackendsFile> {
    if !path.is_file() {
        return Ok(McpBackendsFile::default());
    }
    let content = std::fs::read_to_string(path)?;
    let parsed: McpBackendsFile = serde_json::from_str(&content)?;
    Ok(parsed)
}

/// Stable short SHA-256 fingerprint for schema-cache invalidation when command/args/cwd change.
pub fn config_fingerprint(cfg: &McpBackendConfig) -> String {
    let mut hasher = Sha256::new();
    hasher.update(cfg.command.as_bytes());
    hasher.update(b"\0");
    for arg in &cfg.args {
        hasher.update(arg.as_bytes());
        hasher.update(b"\0");
    }
    if let Some(ref cwd) = cfg.cwd {
        hasher.update(cwd.as_bytes());
    }
    let digest = hasher.finalize();
    let hex: String = digest.iter().take(8).map(|b| format!("{b:02x}")).collect();
    hex
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backends_config_parses_lazy_mcp_gateway_format() {
        let dir = std::env::temp_dir().join(format!("velox-mcp-cfg-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let path = dir.join("backends.json");

        let json = serde_json::json!({
            "backends": {
                "codegraph": {
                    "command": "node.exe",
                    "args": ["codegraph.js", "serve", "--mcp"],
                    "cwd": "C:\\Projects",
                    "env": { "CODEGRAPH_NO_WATCHDOG": "1" },
                    "idleMs": 120000
                },
                "chrome-browser": {
                    "command": "node.exe",
                    "args": ["index.mjs"],
                    "idleMs": 0
                }
            }
        });
        std::fs::write(&path, serde_json::to_string(&json).expect("serialize")).expect("write");

        let cfg = load_backends_config(&path).expect("load backends");
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(cfg.backends.len(), 2);
        let cg = cfg.backends.get("codegraph").expect("codegraph present");
        assert_eq!(cg.command, "node.exe");
        assert_eq!(cg.idle_ms, 120_000);
        assert_eq!(cg.timeout_ms, 120_000);
        assert_eq!(
            cg.env.get("CODEGRAPH_NO_WATCHDOG").map(String::as_str),
            Some("1")
        );
        assert_eq!(
            cfg.backends.get("chrome-browser").expect("chrome").idle_ms,
            0
        );
    }

    #[test]
    fn config_fingerprint_changes_when_args_change() {
        let a = McpBackendConfig {
            command: "node.exe".to_string(),
            args: vec!["--a".to_string()],
            cwd: Some("/work".to_string()),
            env: HashMap::new(),
            idle_ms: 120_000,
            timeout_ms: 120_000,
        };
        let b = McpBackendConfig {
            args: vec!["--b".to_string()],
            ..a.clone()
        };
        assert_ne!(config_fingerprint(&a), config_fingerprint(&b));
    }

    #[test]
    fn missing_backends_file_returns_empty_without_error() {
        let path =
            std::env::temp_dir().join(format!("velox-missing-{}.json", uuid::Uuid::new_v4()));
        let cfg = load_backends_config(&path).expect("missing file should return empty");
        assert!(cfg.backends.is_empty());
    }
}
