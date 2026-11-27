//! Wire protocol between two Velox instances (JSON text frames over one WebSocket).
//!
//! Topology: every Velox runs ONE team server (`ws://<tailscale-ip>:<listenPort>/team/ws`) and
//! dials every configured peer as a client. The dialing side is the *viewer/requester*, the
//! accepting side is the *publisher/executor*: my client connects to Benjamin's server to see his
//! shared sessions and to send him task requests; his client connects to mine for the reverse.
//!
//! Handshake: the client sends `Authorization: Bearer <team.token>` on the HTTP upgrade (401 if it
//! does not match), then both sides send `Hello`. A `protocol` mismatch closes the socket with
//! an `Error` frame. Keepalive: WebSocket ping every 20 s; a peer silent for 60 s is offline.
//!
//! CONTRACT FILE (coordinator-owned). Lanes must not change shapes here.

use crate::domain::agent::AgentKind;
use crate::domain::session::SessionStatus;
use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 1;
/// HTTP path of the team WebSocket endpoint.
pub const WS_PATH: &str = "/team/ws";
/// Largest scrollback tail sent in a `Snapshot` (the newest bytes are kept).
pub const MAX_SNAPSHOT_BYTES: usize = 1024 * 1024;
/// Upper bound for `ReadRequest.max_lines`.
pub const MAX_READ_LINES: u32 = 400;
/// Upper bound for a task text, in bytes.
pub const MAX_TASK_TEXT_BYTES: usize = 16 * 1024;
/// Upper bound for a single raw remote terminal input frame (decoded bytes).
pub const MAX_TERMINAL_INPUT_BYTES: usize = 16 * 1024;

/// A shared session as a teammate sees it. Never includes the full cwd path, env or pid.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteSessionInfo {
    pub id: String,
    pub title: String,
    pub agent_kind: AgentKind,
    pub active_agent: Option<AgentKind>,
    pub status: SessionStatus,
    /// Basename of the session cwd (e.g. "velox"), never the full path.
    pub folder: String,
    pub last_active_at: String,
    /// One-line description of the latest agent action, e.g. "Edit src/team/hub.rs".
    pub summary: Option<String>,
    /// Current PTY size; a viewer renders the mirror at exactly this size.
    pub cols: u16,
    pub rows: u16,
}

/// Lifecycle of a task request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TaskState {
    /// Waiting for the human on the receiving machine.
    Pending,
    /// Accepted by the human; being typed into the target terminal.
    Accepted,
    /// Typed into the target terminal and submitted.
    Delivered,
    /// Rejected by the human on the receiving machine.
    Rejected,
    /// Could not be delivered (session asleep/closed, not shared, write error...).
    Failed,
    /// Nobody answered within 30 minutes.
    Expired,
}

/// Frames sent by the accepting side (server) to the dialing side (client).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ServerMsg {
    Hello {
        protocol: u32,
        /// `team.displayName` of the server machine.
        peer_name: String,
        /// Whether the server has enabled mutual full control for this peer (also updated via `FullControl`).
        #[serde(default)]
        full_control: bool,
    },
    /// Announces the server's current full-control toggle for this connected peer (after handshake and on live toggle).
    FullControl { enabled: bool },
    /// Full list of shared sessions. Sent after `Hello` and again (debounced ~500 ms) on change.
    Sessions { sessions: Vec<RemoteSessionInfo> },
    /// Reply to `Subscribe`: scrollback tail + the stream position it covers.
    Snapshot {
        session_id: String,
        generation: u64,
        end_offset: u64,
        data_b64: String,
        cols: u16,
        rows: u16,
    },
    /// Live PTY bytes for a subscribed session (only after its `Snapshot`).
    Output {
        session_id: String,
        generation: u64,
        end_offset: u64,
        data_b64: String,
    },
    /// The PTY of a subscribed session was resized.
    Resize {
        session_id: String,
        cols: u16,
        rows: u16,
    },
    /// The shared session went away (unshared, closed or slept); drop the mirror.
    SessionGone { session_id: String },
    /// Progress of a `TaskRequest` sent by this client.
    TaskUpdate {
        request_id: String,
        state: TaskState,
        message: Option<String>,
    },
    /// Reply to `ReadRequest`: ANSI-stripped tail of the session screen/scrollback.
    ReadResult {
        request_id: String,
        text: Option<String>,
        error: Option<String>,
    },
    Error { message: String },
}

/// Frames sent by the dialing side (client) to the accepting side (server).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ClientMsg {
    Hello {
        protocol: u32,
        /// `team.displayName` of the client machine.
        peer_name: String,
        /// Whether this client has enabled mutual full control for the server peer.
        #[serde(default)]
        full_control: bool,
    },
    /// Announces a live change to this client's full-control toggle for the server peer.
    FullControl { enabled: bool },
    /// Start mirroring a shared session (server answers with `Snapshot`, then `Output`s).
    Subscribe { session_id: String },
    Unsubscribe { session_id: String },
    /// Raw keyboard/terminal bytes sent to a shared session (ONLY accepted when mutual full control is active).
    TerminalInput {
        session_id: String,
        data_b64: String,
    },
    /// Ask the server machine's human to type `text` into one of its shared sessions.
    TaskRequest {
        request_id: String,
        session_id: String,
        /// Title of the requesting session, when an agent sent it (shown to the human).
        from_session: Option<String>,
        text: String,
    },
    /// Read-only: last `max_lines` lines of a shared session, ANSI stripped.
    ReadRequest {
        request_id: String,
        session_id: String,
        max_lines: u32,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_msg_json_shape_is_stable() {
        let msg = ServerMsg::Output {
            session_id: "s1".into(),
            generation: 2,
            end_offset: 10,
            data_b64: "aGk=".into(),
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "type": "output", "sessionId": "s1", "generation": 2, "endOffset": 10, "dataB64": "aGk="
            })
        );
        let back: ServerMsg = serde_json::from_value(json).unwrap();
        assert_eq!(back, msg);
    }

    #[test]
    fn client_msg_json_shape_is_stable() {
        let msg = ClientMsg::TaskRequest {
            request_id: "r1".into(),
            session_id: "s1".into(),
            from_session: Some("Velox Lider".into()),
            text: "run the tests".into(),
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["type"], "taskRequest");
        assert_eq!(json["requestId"], "r1");
        assert_eq!(json["fromSession"], "Velox Lider");
        let back: ClientMsg = serde_json::from_value(json).unwrap();
        assert_eq!(back, msg);
    }

    #[test]
    fn full_control_and_terminal_input_frames_roundtrip() {
        let srv_fc = ServerMsg::FullControl { enabled: true };
        let srv_json = serde_json::to_value(&srv_fc).unwrap();
        assert_eq!(srv_json, serde_json::json!({ "type": "fullControl", "enabled": true }));
        assert_eq!(serde_json::from_value::<ServerMsg>(srv_json).unwrap(), srv_fc);

        let cli_in = ClientMsg::TerminalInput {
            session_id: "s1".into(),
            data_b64: "Aw==".into(), // Ctrl+C (0x03)
        };
        let cli_json = serde_json::to_value(&cli_in).unwrap();
        assert_eq!(
            cli_json,
            serde_json::json!({ "type": "terminalInput", "sessionId": "s1", "dataB64": "Aw==" })
        );
        assert_eq!(serde_json::from_value::<ClientMsg>(cli_json).unwrap(), cli_in);

        // Older Hello frames without fullControl still deserialize with false.
        let legacy_hello = serde_json::json!({ "type": "hello", "protocol": 1, "peerName": "Old" });
        assert_eq!(
            serde_json::from_value::<ClientMsg>(legacy_hello).unwrap(),
            ClientMsg::Hello {
                protocol: 1,
                peer_name: "Old".into(),
                full_control: false,
            }
        );
    }
}
