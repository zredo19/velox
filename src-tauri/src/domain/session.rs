use crate::domain::agent::AgentKind;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Lifecycle state of an agent terminal session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SessionStatus {
    Starting,
    Running,
    Idle,
    Blocked,
    Sleeping,
    Exited,
}

/// Complete session record shared across persistence, backend, and frontend UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRecord {
    pub id: String,
    pub title: String,
    pub agent_kind: AgentKind,
    #[serde(default)]
    pub active_agent: Option<AgentKind>,
    pub preset_id: String,
    pub profile: Option<String>,
    pub cwd: String,
    pub status: SessionStatus,
    pub agent_session_id: Option<String>,
    /// Prior conversation IDs preserved whenever the active ID is replaced or skipped on wake,
    /// so no conversation is ever permanently orphaned by a transient resumability failure.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub previous_agent_session_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub wake_as_shell: bool,
    pub pid: Option<u32>,
    pub memory_mb: f64,
    pub created_at: String,
    pub last_active_at: String,
    pub exit_code: Option<i32>,
    /// Velox Online: teammates may see this session (read-only) and send it task requests.
    #[serde(default)]
    pub shared: bool,
}

const MAX_PREVIOUS_AGENT_SESSION_IDS: usize = 20;

impl SessionRecord {
    /// Archive `old_id` into `previous_agent_session_ids` (deduplicated, bounded) before replacing
    /// `agent_session_id`.
    pub fn preserve_previous_agent_session_id(&mut self, old_id: &str) {
        let trimmed = old_id.trim();
        if trimmed.is_empty() {
            return;
        }
        if let Some(pos) = self
            .previous_agent_session_ids
            .iter()
            .position(|existing| existing == trimmed)
        {
            self.previous_agent_session_ids.remove(pos);
        }
        self.previous_agent_session_ids.push(trimmed.to_string());
        if self.previous_agent_session_ids.len() > MAX_PREVIOUS_AGENT_SESSION_IDS {
            let overflow = self.previous_agent_session_ids.len() - MAX_PREVIOUS_AGENT_SESSION_IDS;
            self.previous_agent_session_ids.drain(..overflow);
        }
    }

    /// Update `agent_session_id` to `next`, preserving any distinct prior ID in
    /// `previous_agent_session_ids`. Returns `true` if `agent_session_id` changed.
    pub fn set_agent_session_id_preserving(&mut self, next: Option<String>) -> bool {
        if self.agent_session_id == next {
            return false;
        }
        if let Some(old) = self.agent_session_id.take() {
            if next.as_deref() != Some(old.as_str()) {
                self.preserve_previous_agent_session_id(&old);
            }
        }
        self.agent_session_id = next;
        true
    }
}

/// Streamed PTY output chunk emitted via Tauri event `"pty-output"`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PtyOutputEvent {
    pub session_id: String,
    pub data_b64: String,
    /// PTY spawn generation (changes when a session is respawned).
    pub generation: u64,
    /// Stream offset just past this chunk.
    pub end_offset: u64,
}

/// Scrollback snapshot together with the stream position it covers.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScrollbackSnapshot {
    pub data_b64: String,
    pub generation: u64,
    pub end_offset: u64,
}

/// Session state update emitted via Tauri event `"session-updated"`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionUpdatedEvent {
    pub session: SessionRecord,
}

/// Mode of the Lazy MCP Gateway inside Velox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum McpGatewayMode {
    Embedded,
    ExternalAttached,
    Disabled,
    Error,
}

/// Live status for a single scale-to-zero stdio MCP backend.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpBackendStatus {
    pub name: String,
    pub running: bool,
    pub pid: Option<u32>,
    pub memory_mb: f64,
    pub idle_ms: u64,
    pub remaining_idle_ms: Option<u64>,
    pub in_flight: usize,
    pub cached_methods: usize,
    pub last_used_at: Option<String>,
}

/// Aggregated status snapshot for the Lazy MCP Gateway.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpGatewaySnapshot {
    pub mode: McpGatewayMode,
    pub listen_addr: String,
    pub total_backends: usize,
    pub active_backends: usize,
    pub total_memory_mb: f64,
    pub backends: Vec<McpBackendStatus>,
    pub error: Option<String>,
}

/// Periodic RAM and session count telemetry emitted via `"telemetry-updated"` and `get_telemetry`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemTelemetry {
    pub app_memory_mb: f64,
    pub sessions_memory_mb: f64,
    pub mcp_memory_mb: f64,
    pub total_memory_mb: f64,
    pub active_sessions: usize,
    pub sleeping_sessions: usize,
    pub session_ram: HashMap<String, f64>,
}
