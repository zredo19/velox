//! In-process API of the team hub, used by the built-in MCP server (`team::mcp`) and by the
//! Tauri IPC commands. CONTRACT FILE (coordinator-owned).

use crate::domain::error::VeloxResult;
use crate::team::protocol::{RemoteSessionInfo, TaskState};
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::pin::Pin;

/// Boxed `Send` future, so `TeamApi` stays object safe (`Arc<dyn TeamApi>`).
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Connection state of one configured teammate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerStatus {
    /// `PeerConfig.name` (the label used everywhere in the UI and MCP tools).
    pub name: String,
    pub host: String,
    pub port: u16,
    pub online: bool,
    /// `displayName` the peer announced in its `Hello`.
    pub display_name: Option<String>,
    pub last_error: Option<String>,
    /// That peer's shared sessions (empty while offline).
    pub sessions: Vec<RemoteSessionInfo>,
}

/// One session on THIS machine, as listed to local agents and the UI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalSessionInfo {
    #[serde(flatten)]
    pub info: RemoteSessionInfo,
    pub shared: bool,
}

/// Everything the UI and the MCP tools need to know about Velox Online, in one value.
/// Emitted as the `team-updated` Tauri event and returned by `get_team_status`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamSnapshot {
    pub enabled: bool,
    pub display_name: String,
    /// "100.123.172.42:43120" while the team server is listening.
    pub listen_addr: Option<String>,
    pub server_error: Option<String>,
    pub tailscale_ip: Option<String>,
    pub peers: Vec<PeerStatus>,
    /// Live (non-sleeping) sessions of this machine.
    pub local_sessions: Vec<LocalSessionInfo>,
    /// Display names of teammates currently connected to this machine's server.
    pub watchers: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TaskDirection {
    /// Sent from this machine (to a peer, or to a local session).
    Outgoing,
    /// Received by this machine; needs a human decision while `Pending`.
    Incoming,
}

/// A task request as tracked by either side. Emitted (as a list) via `team-tasks-updated`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskTicket {
    pub request_id: String,
    pub direction: TaskDirection,
    /// Outgoing: target peer name (`None` = this machine). Incoming: requester peer display name
    /// (`None` = requested by a local agent).
    pub peer: Option<String>,
    pub session_id: String,
    pub session_title: Option<String>,
    /// Display name of the requesting machine.
    pub from_name: String,
    /// Title of the requesting session, when an agent sent it.
    pub from_session: Option<String>,
    pub text: String,
    pub state: TaskState,
    pub message: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Request to type a task into a session (remote or local).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendTaskRequest {
    /// Peer name from `PeerConfig.name`; `None` targets a session on this machine.
    pub peer: Option<String>,
    pub session_id: String,
    pub text: String,
    /// Title of the requesting session, when known.
    pub from_session: Option<String>,
}

/// A session somewhere in the team.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionTarget {
    /// `None` = this machine.
    pub peer: Option<String>,
    pub session_id: String,
}

/// Tauri event names emitted by the hub (payload types below / in this file).
pub const EVT_TEAM_UPDATED: &str = "team-updated"; // TeamSnapshot
pub const EVT_TEAM_TASKS_UPDATED: &str = "team-tasks-updated"; // Vec<TaskTicket>, newest first, max 50
pub const EVT_TEAM_PTY_OUTPUT: &str = "team-pty-output"; // TeamPtyOutputEvent
pub const EVT_TEAM_PTY_SNAPSHOT: &str = "team-pty-snapshot"; // TeamPtySnapshotEvent
pub const EVT_TEAM_PTY_RESIZE: &str = "team-pty-resize"; // TeamPtyResizeEvent
pub const EVT_TEAM_SESSION_GONE: &str = "team-session-gone"; // TeamSessionGoneEvent

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamPtyOutputEvent {
    pub peer: String,
    pub session_id: String,
    pub generation: u64,
    pub end_offset: u64,
    pub data_b64: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamPtySnapshotEvent {
    pub peer: String,
    pub session_id: String,
    pub generation: u64,
    pub end_offset: u64,
    pub data_b64: String,
    pub cols: u16,
    pub rows: u16,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamPtyResizeEvent {
    pub peer: String,
    pub session_id: String,
    pub cols: u16,
    pub rows: u16,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamSessionGoneEvent {
    pub peer: String,
    pub session_id: String,
}

/// Implemented by `team::hub::TeamHub` (Lane A). Consumed by `team::mcp` (Lane B) and IPC.
pub trait TeamApi: Send + Sync {
    fn snapshot(&self) -> TeamSnapshot;

    /// Remote: forwards a `TaskRequest` and returns the ticket in `Pending`.
    /// Local: queues an incoming ticket for this machine's human (or delivers it right away when
    /// `team.autoApproveLocal` is on).
    fn send_task(&self, req: SendTaskRequest) -> BoxFuture<'_, VeloxResult<TaskTicket>>;

    fn task_status(&self, request_id: &str) -> Option<TaskTicket>;

    /// ANSI-stripped last `max_lines` lines of a session (remote: shared sessions only).
    fn read_session(
        &self,
        target: SessionTarget,
        max_lines: u32,
    ) -> BoxFuture<'_, VeloxResult<String>>;
}
