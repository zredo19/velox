//! Built-in MCP server `velox-team` (served at `http://127.0.0.1:<mcpGatewayPort>/mcp/velox-team`).
//! Lane B owns this file.

use crate::mcp::builtin::BuiltinMcp;
use crate::team::api::{
    resolve_session_by_query, BoxFuture, LocalMessageStatus, PeerStatus, SendLocalMessageRequest,
    SendTaskRequest, SessionTarget, TeamApi, TeamSnapshot,
};
use crate::team::protocol::{RemoteSessionInfo, TaskState, MAX_READ_LINES, MAX_TASK_TEXT_BYTES};
use serde_json::{json, Value};
use std::fmt::Write as _;
use std::sync::Arc;

/// Name under which the gateway serves this MCP server.
pub const TEAM_MCP_NAME: &str = "velox-team";

const DEFAULT_PROTOCOL_VERSION: &str = "2025-06-18";
const DEFAULT_READ_LINES: u32 = 60;

const JSONRPC_INVALID_REQUEST: i64 = -32600;
const JSONRPC_METHOD_NOT_FOUND: i64 = -32601;
const JSONRPC_INVALID_PARAMS: i64 = -32602;

pub struct TeamMcp {
    api: Arc<dyn TeamApi>,
}

impl TeamMcp {
    pub fn new(api: Arc<dyn TeamApi>) -> Self {
        Self { api }
    }

    async fn dispatch(
        &self,
        method: &str,
        params: Option<&Value>,
        id: Value,
        caller_session_id: Option<&str>,
    ) -> Value {
        match method {
            "initialize" => {
                let protocol_version = params
                    .and_then(|p| p.get("protocolVersion"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .unwrap_or(DEFAULT_PROTOCOL_VERSION);
                jsonrpc_ok(
                    id,
                    json!({
                        "protocolVersion": protocol_version,
                        "capabilities": {
                            "tools": { "listChanged": false }
                        },
                        "serverInfo": {
                            "name": TEAM_MCP_NAME,
                            "version": env!("CARGO_PKG_VERSION"),
                        },
                        "instructions": "Collaborate with local Velox sessions and remote teammates. For direct local session-to-session messaging on this PC (no approval needed): call session_list, then session_send with target and text (and team_read_session for output). For teammates' shared sessions: call team_list first, then use team_send_task — a human on the receiving machine must accept every task; nothing runs automatically. Poll team_task_status with the returned request_id."
                    }),
                )
            }
            "ping" => jsonrpc_ok(id, json!({})),
            "tools/list" => jsonrpc_ok(id, json!({ "tools": tool_definitions() })),
            "tools/call" => self.handle_tools_call(params, id, caller_session_id).await,
            _ => jsonrpc_error(id, JSONRPC_METHOD_NOT_FOUND, format!("Method not found: {method}")),
        }
    }

    async fn handle_tools_call(
        &self,
        params: Option<&Value>,
        id: Value,
        caller_session_id: Option<&str>,
    ) -> Value {
        let Some(params_obj) = params.and_then(Value::as_object) else {
            return jsonrpc_error(id, JSONRPC_INVALID_PARAMS, "Invalid params: expected an object");
        };

        let Some(tool_name) = params_obj.get("name").and_then(Value::as_str) else {
            return jsonrpc_error(
                id,
                JSONRPC_INVALID_PARAMS,
                "Invalid params: missing string tool name",
            );
        };

        let args = match params_obj.get("arguments") {
            None | Some(Value::Null) => serde_json::Map::new(),
            Some(Value::Object(map)) => map.clone(),
            Some(_) => {
                return jsonrpc_error(
                    id,
                    JSONRPC_INVALID_PARAMS,
                    "Invalid params: arguments must be an object",
                );
            }
        };

        let result = match tool_name {
            "session_list" => self.tool_session_list(caller_session_id),
            "session_send" => self.tool_session_send(&args, caller_session_id).await,
            "team_list" => self.tool_list(),
            "team_send_task" => self.tool_send_task(&args, caller_session_id).await,
            "team_task_status" => self.tool_task_status(&args),
            "team_read_session" => self.tool_read_session(&args).await,
            _ => {
                return jsonrpc_error(
                    id,
                    JSONRPC_INVALID_PARAMS,
                    format!("Unknown tool: {tool_name}"),
                );
            }
        };

        jsonrpc_ok(id, result)
    }

    fn tool_session_list(&self, caller_session_id: Option<&str>) -> Value {
        let sessions = self.api.all_local_sessions();
        let mut out = String::new();
        let _ = writeln!(out, "Local Velox sessions ({}):", sessions.len());
        if sessions.is_empty() {
            let _ = writeln!(out, "  (none)");
        } else {
            for s in &sessions {
                let is_self = caller_session_id
                    .is_some_and(|cid| cid.eq_ignore_ascii_case(&s.info.id));
                let tag = if is_self { Some("current session") } else { None };
                write_session_line(&mut out, tag, &s.info);
            }
        }

        let structured_sessions: Vec<Value> = sessions
            .iter()
            .map(|s| {
                let is_self = caller_session_id
                    .is_some_and(|cid| cid.eq_ignore_ascii_case(&s.info.id));
                json!({
                    "id": s.info.id,
                    "shortId": s.info.id.chars().take(8).collect::<String>(),
                    "title": s.info.title,
                    "agentKind": s.info.agent_kind,
                    "activeAgent": s.info.active_agent,
                    "status": s.info.status,
                    "folder": s.info.folder,
                    "shared": s.shared,
                    "summary": s.info.summary,
                    "lastActiveAt": s.info.last_active_at,
                    "isSelf": is_self,
                })
            })
            .collect();

        json!({
            "content": [{ "type": "text", "text": out.trim_end() }],
            "structuredContent": {
                "currentSessionId": caller_session_id,
                "sessions": structured_sessions,
            },
            "isError": false
        })
    }

    async fn tool_session_send(
        &self,
        args: &serde_json::Map<String, Value>,
        caller_session_id: Option<&str>,
    ) -> Value {
        let to = match required_non_empty_string(args, "to") {
            Ok(v) => v,
            Err(msg) => return tool_error(msg),
        };
        let text = match args.get("text").and_then(Value::as_str) {
            Some(t) if !t.trim().is_empty() => t.to_string(),
            Some(_) => return tool_error("text must not be empty or whitespace-only"),
            None => return tool_error("Missing required string argument: text"),
        };
        if text.len() > MAX_TASK_TEXT_BYTES {
            return tool_error(format!(
                "text is {} bytes, which exceeds the maximum of {MAX_TASK_TEXT_BYTES} bytes",
                text.len()
            ));
        }
        let from_session = match optional_string_arg(args, "from_session") {
            Ok(v) => v,
            Err(msg) => return tool_error(msg),
        };

        let req = SendLocalMessageRequest {
            to,
            text,
            from_session_id: caller_session_id
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            from_session_query: from_session,
        };

        match self.api.send_local_message(req).await {
            Ok(receipt) => {
                let text_summary = match receipt.status {
                    LocalMessageStatus::Delivered => {
                        let wake_note = if receipt.woke_up {
                            " (woke sleeping session)"
                        } else {
                            ""
                        };
                        format!(
                            "Message delivered directly from \"{}\" to \"{}\"{wake_note}.",
                            receipt.from_title, receipt.to_title
                        )
                    }
                    LocalMessageStatus::Queued => {
                        let detail = receipt
                            .detail
                            .as_deref()
                            .map(|d| format!(" {d}"))
                            .unwrap_or_default();
                        format!(
                            "Message queued for \"{}\" from \"{}\".{detail}",
                            receipt.to_title, receipt.from_title
                        )
                    }
                    LocalMessageStatus::Failed => {
                        let detail = receipt
                            .detail
                            .as_deref()
                            .unwrap_or("Failed to deliver message");
                        return tool_error(detail);
                    }
                };
                let structured = serde_json::to_value(&receipt).unwrap_or(Value::Null);
                json!({
                    "content": [{ "type": "text", "text": text_summary }],
                    "structuredContent": structured,
                    "isError": false
                })
            }
            Err(err) => tool_error(format!("Failed to send message: {err}")),
        }
    }

    fn tool_list(&self) -> Value {
        let snap = self.api.snapshot();
        let text = format_team_snapshot(&snap);
        let structured = serde_json::to_value(&snap).unwrap_or(Value::Null);
        json!({
            "content": [{ "type": "text", "text": text }],
            "structuredContent": structured,
            "isError": false
        })
    }

    async fn tool_send_task(
        &self,
        args: &serde_json::Map<String, Value>,
        caller_session_id: Option<&str>,
    ) -> Value {
        let peer_arg = match optional_string_arg(args, "peer") {
            Ok(v) => v,
            Err(msg) => return tool_error(msg),
        };
        let session_query = match required_non_empty_string(args, "session") {
            Ok(v) => v,
            Err(msg) => return tool_error(msg),
        };
        let text = match args.get("text").and_then(Value::as_str) {
            Some(t) if !t.trim().is_empty() => t.to_string(),
            Some(_) => return tool_error("text must not be empty or whitespace-only"),
            None => return tool_error("Missing required string argument: text"),
        };
        if text.len() > MAX_TASK_TEXT_BYTES {
            return tool_error(format!(
                "text is {} bytes, which exceeds the maximum of {MAX_TASK_TEXT_BYTES} bytes",
                text.len()
            ));
        }
        let explicit_from = match optional_string_arg(args, "from_session") {
            Ok(v) => v,
            Err(msg) => return tool_error(msg),
        };
        let from_session = explicit_from.or_else(|| {
            caller_session_id.and_then(|cid| {
                self.api
                    .all_local_sessions()
                    .into_iter()
                    .find(|s| s.info.id.eq_ignore_ascii_case(cid))
                    .map(|s| s.info.title)
            })
        });

        let snap = self.api.snapshot();
        let resolved = match resolve_target(&snap, peer_arg.as_deref(), &session_query) {
            Ok(target) => target,
            Err(msg) => return tool_error(msg),
        };

        let req = SendTaskRequest {
            peer: resolved.peer_name.clone(),
            session_id: resolved.session.id.clone(),
            text,
            from_session,
        };

        let full_control_active = resolved.peer_name.as_deref().is_some_and(|pname| {
            snap.peers
                .iter()
                .any(|p| p.name == pname && p.full_control_active)
        });

        match self.api.send_task(req).await {
            Ok(ticket) => {
                let status_line = match ticket.state {
                    TaskState::Pending if full_control_active => format!(
                        "Task sent (request_id: {}, state: pending). Mutual Control total is active with {} so it delivers automatically without manual approval. Poll team_task_status with this request_id to confirm delivery.",
                        ticket.request_id, resolved.human_name
                    ),
                    TaskState::Pending => format!(
                        "Task queued (request_id: {}, state: pending). Waiting for {} to accept in Velox. Poll team_task_status with this request_id to check progress.",
                        ticket.request_id, resolved.human_name
                    ),
                    TaskState::Delivered => format!(
                        "Task delivered (request_id: {}, state: delivered) to \"{}\".",
                        ticket.request_id, resolved.session.title
                    ),
                    other => format!(
                        "Task created (request_id: {}, state: {}).{}",
                        ticket.request_id,
                        task_state_label(other),
                        ticket
                            .message
                            .as_deref()
                            .map(|m| format!(" {m}"))
                            .unwrap_or_default()
                    ),
                };
                let structured = serde_json::to_value(&ticket).unwrap_or(Value::Null);
                json!({
                    "content": [{ "type": "text", "text": status_line }],
                    "structuredContent": structured,
                    "isError": false
                })
            }
            Err(err) => tool_error(format!("Failed to send task: {err}")),
        }
    }

    fn tool_task_status(&self, args: &serde_json::Map<String, Value>) -> Value {
        let request_id = match required_non_empty_string(args, "request_id") {
            Ok(v) => v,
            Err(msg) => return tool_error(msg),
        };

        let Some(ticket) = self.api.task_status(&request_id) else {
            return tool_error(format!("Unknown task request_id: {request_id}"));
        };

        let mut summary = format!(
            "Task {} for session \"{}\": state={}",
            ticket.request_id,
            ticket
                .session_title
                .as_deref()
                .unwrap_or(ticket.session_id.as_str()),
            task_state_label(ticket.state)
        );
        if let Some(msg) = ticket.message.as_deref().filter(|m| !m.trim().is_empty()) {
            let _ = write!(summary, " — {msg}");
        }
        if ticket.state == TaskState::Pending {
            summary.push_str(
                ". Still waiting for the human to accept or reject in Velox; poll team_task_status again shortly.",
            );
        }

        let structured = serde_json::to_value(&ticket).unwrap_or(Value::Null);
        json!({
            "content": [{ "type": "text", "text": summary }],
            "structuredContent": structured,
            "isError": false
        })
    }

    async fn tool_read_session(&self, args: &serde_json::Map<String, Value>) -> Value {
        let peer_arg = match optional_string_arg(args, "peer") {
            Ok(v) => v,
            Err(msg) => return tool_error(msg),
        };
        let session_query = match required_non_empty_string(args, "session") {
            Ok(v) => v,
            Err(msg) => return tool_error(msg),
        };
        let max_lines = match parse_lines_arg(args.get("lines")) {
            Ok(n) => n,
            Err(msg) => return tool_error(msg),
        };

        let snap = self.api.snapshot();
        let resolved = match resolve_target(&snap, peer_arg.as_deref(), &session_query) {
            Ok(target) => target,
            Err(msg) => return tool_error(msg),
        };

        let target = SessionTarget {
            peer: resolved.peer_name,
            session_id: resolved.session.id,
        };
        match self.api.read_session(target, max_lines).await {
            Ok(output) => json!({
                "content": [{ "type": "text", "text": output }],
                "isError": false
            }),
            Err(err) => tool_error(format!("Failed to read session: {err}")),
        }
    }
}

impl BuiltinMcp for TeamMcp {
    fn handle_jsonrpc(&self, request: Value) -> BoxFuture<'_, Option<Value>> {
        self.handle_jsonrpc_with_session(request, None)
    }

    fn handle_jsonrpc_with_session(
        &self,
        request: Value,
        session_id: Option<String>,
    ) -> BoxFuture<'_, Option<Value>> {
        Box::pin(async move {
            if request.is_array() {
                return Some(jsonrpc_error(
                    Value::Null,
                    JSONRPC_INVALID_REQUEST,
                    "Invalid Request: batch requests are not supported",
                ));
            }

            let Some(obj) = request.as_object() else {
                return Some(jsonrpc_error(
                    Value::Null,
                    JSONRPC_INVALID_REQUEST,
                    "Invalid Request: expected a JSON object",
                ));
            };

            let raw_id = obj.get("id");
            let id_valid = raw_id
                .map(|v| v.is_string() || v.is_number() || v.is_null())
                .unwrap_or(true);
            if !id_valid {
                return Some(jsonrpc_error(
                    Value::Null,
                    JSONRPC_INVALID_REQUEST,
                    "Invalid Request: id must be string, number, or null",
                ));
            }
            let error_id = raw_id.cloned().unwrap_or(Value::Null);

            if obj.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
                return Some(jsonrpc_error(
                    error_id,
                    JSONRPC_INVALID_REQUEST,
                    "Invalid Request: jsonrpc must be \"2.0\"",
                ));
            }

            let Some(method) = obj
                .get("method")
                .and_then(Value::as_str)
                .filter(|m| !m.is_empty())
            else {
                return Some(jsonrpc_error(
                    error_id,
                    JSONRPC_INVALID_REQUEST,
                    "Invalid Request: missing method string",
                ));
            };

            if method.starts_with("notifications/") || raw_id.is_none() {
                return None;
            }

            let id = raw_id.cloned().unwrap_or(Value::Null);
            let params = obj.get("params");
            Some(
                self.dispatch(method, params, id, session_id.as_deref())
                    .await,
            )
        })
    }
}

fn jsonrpc_ok(id: Value, result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    })
}

fn jsonrpc_error(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message.into()
        }
    })
}

fn tool_error(message: impl Into<String>) -> Value {
    json!({
        "content": [{ "type": "text", "text": message.into() }],
        "isError": true
    })
}

fn optional_string_arg(
    args: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<Option<String>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                Ok(None)
            } else {
                Ok(Some(trimmed.to_string()))
            }
        }
        Some(_) => Err(format!("Argument '{key}' must be a string")),
    }
}

fn required_non_empty_string(
    args: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<String, String> {
    match args.get(key) {
        Some(Value::String(s)) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                Err(format!("Argument '{key}' must not be empty"))
            } else {
                Ok(trimmed.to_string())
            }
        }
        Some(_) => Err(format!("Argument '{key}' must be a string")),
        None => Err(format!("Missing required string argument: {key}")),
    }
}

fn parse_lines_arg(value: Option<&Value>) -> Result<u32, String> {
    match value {
        None | Some(Value::Null) => Ok(DEFAULT_READ_LINES),
        Some(Value::Number(n)) => {
            let Some(lines) = n.as_u64() else {
                return Err("Argument 'lines' must be a positive integer".to_string());
            };
            if lines == 0 {
                return Err("Argument 'lines' must be at least 1".to_string());
            }
            let capped = u32::try_from(lines.min(u64::from(MAX_READ_LINES)))
                .unwrap_or(MAX_READ_LINES);
            Ok(capped)
        }
        Some(_) => Err("Argument 'lines' must be an integer".to_string()),
    }
}

struct ResolvedTarget {
    peer_name: Option<String>,
    human_name: String,
    session: RemoteSessionInfo,
}

fn resolve_target(
    snap: &TeamSnapshot,
    peer_arg: Option<&str>,
    session_query: &str,
) -> Result<ResolvedTarget, String> {
    let is_local = match peer_arg {
        None => true,
        Some(p) => p.trim().is_empty() || p.trim().eq_ignore_ascii_case("local"),
    };

    if is_local {
        let candidates: Vec<&RemoteSessionInfo> =
            snap.local_sessions.iter().map(|s| &s.info).collect();
        let session = resolve_session(&candidates, session_query, "this machine")?.clone();
        return Ok(ResolvedTarget {
            peer_name: None,
            human_name: snap.display_name.clone(),
            session,
        });
    }

    let peer_query = peer_arg.unwrap_or("").trim();
    let peer = resolve_peer(&snap.peers, peer_query)?;
    if !peer.online {
        let err_suffix = peer
            .last_error
            .as_deref()
            .map(|e| format!(" ({e})"))
            .unwrap_or_default();
        return Err(format!("Peer '{}' is currently offline{err_suffix}", peer.name));
    }

    let candidates: Vec<&RemoteSessionInfo> = peer.sessions.iter().collect();
    let scope = format!("peer '{}'", peer.name);
    let session = resolve_session(&candidates, session_query, &scope)?.clone();
    let human_name = peer
        .display_name
        .clone()
        .unwrap_or_else(|| peer.name.clone());

    Ok(ResolvedTarget {
        peer_name: Some(peer.name.clone()),
        human_name,
        session,
    })
}

fn resolve_peer<'a>(peers: &'a [PeerStatus], query: &str) -> Result<&'a PeerStatus, String> {
    if let Some(exact) = peers.iter().find(|p| p.name == query) {
        return Ok(exact);
    }
    let ci_matches: Vec<&PeerStatus> = peers
        .iter()
        .filter(|p| p.name.eq_ignore_ascii_case(query))
        .collect();
    if ci_matches.len() == 1 {
        return Ok(ci_matches[0]);
    }
    if ci_matches.len() > 1 {
        let names = ci_matches
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!("Ambiguous peer '{query}'. Matches: {names}"));
    }

    let available = if peers.is_empty() {
        "none configured".to_string()
    } else {
        peers
            .iter()
            .map(|p| {
                let state = if p.online { "online" } else { "offline" };
                format!("{} ({state})", p.name)
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    Err(format!("Unknown peer '{query}'. Available peers: {available}"))
}

fn resolve_session<'a>(
    candidates: &[&'a RemoteSessionInfo],
    query: &str,
    scope_label: &str,
) -> Result<&'a RemoteSessionInfo, String> {
    resolve_session_by_query(candidates, query, scope_label, |s| &s.id, |s| &s.title).copied()
}

fn task_state_label(state: TaskState) -> &'static str {
    match state {
        TaskState::Pending => "pending",
        TaskState::Accepted => "accepted",
        TaskState::Delivered => "delivered",
        TaskState::Rejected => "rejected",
        TaskState::Failed => "failed",
        TaskState::Expired => "expired",
    }
}

fn format_team_snapshot(snap: &TeamSnapshot) -> String {
    let mut out = String::new();
    let listen = snap.listen_addr.as_deref().unwrap_or("not listening");
    let _ = writeln!(
        out,
        "This machine: {} | team={} | listen={listen}",
        snap.display_name,
        if snap.enabled { "enabled" } else { "disabled" }
    );
    if let Some(ip) = snap.tailscale_ip.as_deref() {
        let _ = writeln!(out, "Tailscale IP: {ip}");
    }
    if let Some(err) = snap.server_error.as_deref() {
        let _ = writeln!(out, "Server note: {err}");
    }
    if !snap.watchers.is_empty() {
        let _ = writeln!(out, "Connected watchers: {}", snap.watchers.join(", "));
    }

    let _ = writeln!(out, "\nLocal live sessions ({}):", snap.local_sessions.len());
    if snap.local_sessions.is_empty() {
        let _ = writeln!(out, "  (none)");
    } else {
        for s in &snap.local_sessions {
            let tag = if s.shared { "shared" } else { "not shared" };
            write_session_line(&mut out, Some(tag), &s.info);
        }
    }

    let _ = writeln!(out, "\nPeers ({}):", snap.peers.len());
    if snap.peers.is_empty() {
        let _ = writeln!(out, "  (none)");
    } else {
        for peer in &snap.peers {
            let state = if peer.online {
                if peer.full_control_active {
                    "online · full control"
                } else {
                    "online"
                }
            } else {
                "offline"
            };
            let display = peer
                .display_name
                .as_deref()
                .filter(|d| *d != peer.name)
                .map(|d| format!(" (display: {d})"))
                .unwrap_or_default();
            let err = peer
                .last_error
                .as_deref()
                .map(|e| format!(" — {e}"))
                .unwrap_or_default();
            let _ = writeln!(
                out,
                "  * {}{} [{state}] ({}:{}){err}",
                peer.name, display, peer.host, peer.port
            );
            if peer.sessions.is_empty() {
                let _ = writeln!(out, "      (no shared sessions)");
            } else {
                for s in &peer.sessions {
                    write_session_line(&mut out, None, s);
                }
            }
        }
    }

    out.trim_end().to_string()
}

fn write_session_line(out: &mut String, tag: Option<&str>, s: &RemoteSessionInfo) {
    let agent = s.active_agent.unwrap_or(s.agent_kind).display_name();
    let tag_prefix = tag.map(|t| format!("[{t}] ")).unwrap_or_default();
    let summary = s
        .summary
        .as_deref()
        .filter(|m| !m.trim().is_empty())
        .map(|m| format!(" — {m}"))
        .unwrap_or_default();
    let _ = writeln!(
        out,
        "    - {tag_prefix}\"{}\" (id={}, agent={}, status={:?}, folder={}){summary}",
        s.title, s.id, agent, s.status, s.folder
    );
}

fn tool_definitions() -> Value {
    json!([
        {
            "name": "team_list",
            "description": "List this machine (display name, listen address), configured teammates (online/offline) with their shared sessions, and this machine's live sessions (marked shared or not). Note: when sending tasks with team_send_task, a human on the receiving machine must accept every task; nothing runs automatically.",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        },
        {
            "name": "team_send_task",
            "description": "Send a task request to a teammate's shared session or to a live session on this machine. A human on the receiving machine must accept every task; nothing runs automatically. Returns a ticket with request_id to poll via team_task_status.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "peer": {
                        "type": "string",
                        "description": "Teammate name from team_list. Omit, pass empty string, or pass \"local\" to target a session on this machine."
                    },
                    "session": {
                        "type": "string",
                        "description": "Target session ID or title (case-insensitive; exact ID wins, then exact title, then unique prefix)."
                    },
                    "text": {
                        "type": "string",
                        "description": "Task prompt to type into the target session once the receiving human accepts it (non-empty, max 16384 bytes)."
                    },
                    "from_session": {
                        "type": "string",
                        "description": "Optional title of your own session so the receiving human knows who sent the task."
                    }
                },
                "required": ["session", "text"]
            }
        },
        {
            "name": "team_task_status",
            "description": "Check the current status of a task sent with team_send_task. Because a human on the receiving machine must accept every task (nothing runs automatically), poll this tool while the state is pending.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "request_id": {
                        "type": "string",
                        "description": "The request_id returned by team_send_task."
                    }
                },
                "required": ["request_id"]
            }
        },
        {
            "name": "team_read_session",
            "description": "Read-only inspection of the tail of a session's terminal output (ANSI stripped). Remote peers only expose shared sessions; cannot send input (use team_send_task instead, which requires human acceptance).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "peer": {
                        "type": "string",
                        "description": "Teammate name from team_list. Omit, pass empty string, or pass \"local\" for this machine."
                    },
                    "session": {
                        "type": "string",
                        "description": "Session ID or title (case-insensitive; exact ID, then exact title, then unique prefix)."
                    },
                    "lines": {
                        "type": "integer",
                        "description": "Number of lines to read from the end (default 60, max 400).",
                        "minimum": 1,
                        "maximum": MAX_READ_LINES
                    }
                },
                "required": ["session"]
            }
        },
        {
            "name": "session_list",
            "description": "List all non-exited local Velox sessions on this machine (both live and sleeping), including IDs, titles, agent types, statuses, folders, and summaries.",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        },
        {
            "name": "session_send",
            "description": "Send a direct message to another Velox session on this same machine with zero manual inbox approval. Automatically wakes sleeping targets and queues behind confirmation prompts.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "to": {
                        "type": "string",
                        "description": "Target local session ID, exact title, or unique prefix (case-insensitive)."
                    },
                    "text": {
                        "type": "string",
                        "description": "Message text to deliver to the target session (non-empty, max 16384 bytes)."
                    },
                    "from_session": {
                        "type": "string",
                        "description": "Optional fallback title or ID of your own session if X-Velox-Session header is absent."
                    }
                },
                "required": ["to", "text"]
            }
        }
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::agent::AgentKind;
    use crate::domain::error::VeloxError;
    use crate::domain::session::SessionStatus;
    use crate::team::api::{LocalMessageReceipt, LocalSessionInfo, TaskDirection, TaskTicket};
    use std::collections::HashMap;
    use std::sync::Mutex;

    type ReadReplyKey = (Option<String>, String, u32);

    struct FakeTeam {
        snapshot: TeamSnapshot,
        tickets: Mutex<HashMap<String, TaskTicket>>,
        sent_requests: Mutex<Vec<SendTaskRequest>>,
        sent_local: Mutex<Vec<SendLocalMessageRequest>>,
        read_replies: Mutex<HashMap<ReadReplyKey, String>>,
    }

    impl FakeTeam {
        fn sample() -> Self {
            let local_a = RemoteSessionInfo {
                id: "local-1111-aaaa".into(),
                title: "Velox Lider".into(),
                agent_kind: AgentKind::Claude,
                active_agent: None,
                status: SessionStatus::Idle,
                folder: "velox".into(),
                last_active_at: "2026-11-26T00:00:00Z".into(),
                summary: Some("Read team/api.rs".into()),
                cols: 120,
                rows: 40,
            };
            let local_b = RemoteSessionInfo {
                id: "local-2222-bbbb".into(),
                title: "Velox Build".into(),
                agent_kind: AgentKind::Shell,
                active_agent: None,
                status: SessionStatus::Running,
                folder: "velox".into(),
                last_active_at: "2026-11-26T00:00:00Z".into(),
                summary: None,
                cols: 120,
                rows: 40,
            };
            let ben_a = RemoteSessionInfo {
                id: "rem-aaaa-1111".into(),
                title: "API Server".into(),
                agent_kind: AgentKind::Claude,
                active_agent: None,
                status: SessionStatus::Idle,
                folder: "api".into(),
                last_active_at: "2026-11-26T00:00:00Z".into(),
                summary: Some("Edit routes.rs".into()),
                cols: 100,
                rows: 30,
            };
            let ben_b = RemoteSessionInfo {
                id: "rem-bbbb-2222".into(),
                title: "API Tests".into(),
                agent_kind: AgentKind::Agy,
                active_agent: None,
                status: SessionStatus::Running,
                folder: "api".into(),
                last_active_at: "2026-11-26T00:00:00Z".into(),
                summary: Some("Prompt: run integration suite".into()),
                cols: 100,
                rows: 30,
            };

            let mut read_replies = HashMap::new();
            read_replies.insert(
                (Some("Benjamin".into()), "rem-aaaa-1111".into(), 60),
                "server listening on :8080".into(),
            );
            read_replies.insert(
                (None, "local-1111-aaaa".into(), 10),
                "local tail output".into(),
            );

            let mut tickets = HashMap::new();
            tickets.insert(
                "req-done".into(),
                TaskTicket {
                    request_id: "req-done".into(),
                    direction: TaskDirection::Outgoing,
                    peer: Some("Benjamin".into()),
                    session_id: "rem-aaaa-1111".into(),
                    session_title: Some("API Server".into()),
                    from_name: "Rodolfo".into(),
                    from_session: Some("Velox Lider".into()),
                    text: "check logs".into(),
                    state: TaskState::Delivered,
                    message: Some("Typed into terminal".into()),
                    created_at: "2026-11-26T00:00:00Z".into(),
                    updated_at: "2026-11-26T00:01:00Z".into(),
                },
            );

            Self {
                snapshot: TeamSnapshot {
                    enabled: true,
                    display_name: "Rodolfo".into(),
                    listen_addr: Some("100.64.0.10:43120".into()),
                    server_error: None,
                    tailscale_ip: Some("100.64.0.10".into()),
                    peers: vec![
                        PeerStatus {
                            name: "Benjamin".into(),
                            host: "benjamin".into(),
                            port: 43120,
                            online: true,
                            display_name: Some("Benjamin".into()),
                            last_error: None,
                            sessions: vec![ben_a, ben_b],
                            local_full_control: false,
                            remote_full_control: false,
                            full_control_active: false,
                        },
                        PeerStatus {
                            name: "OfflinePeer".into(),
                            host: "offline".into(),
                            port: 43120,
                            online: false,
                            display_name: None,
                            last_error: Some("connection refused".into()),
                            sessions: vec![],
                            local_full_control: false,
                            remote_full_control: false,
                            full_control_active: false,
                        },
                    ],
                    local_sessions: vec![
                        LocalSessionInfo {
                            info: local_a,
                            shared: true,
                        },
                        LocalSessionInfo {
                            info: local_b,
                            shared: false,
                        },
                    ],
                    watchers: vec!["Benjamin".into()],
                    full_control_peers: vec![],
                },
                tickets: Mutex::new(tickets),
                sent_requests: Mutex::new(Vec::new()),
                sent_local: Mutex::new(Vec::new()),
                read_replies: Mutex::new(read_replies),
            }
        }
    }

    impl TeamApi for FakeTeam {
        fn snapshot(&self) -> TeamSnapshot {
            self.snapshot.clone()
        }

        fn send_task(&self, req: SendTaskRequest) -> BoxFuture<'_, crate::domain::error::VeloxResult<TaskTicket>> {
            Box::pin(async move {
                self.sent_requests.lock().expect("lock").push(req.clone());
                let ticket = TaskTicket {
                    request_id: "req-1".into(),
                    direction: TaskDirection::Outgoing,
                    peer: req.peer,
                    session_id: req.session_id,
                    session_title: Some("API Server".into()),
                    from_name: "Rodolfo".into(),
                    from_session: req.from_session,
                    text: req.text,
                    state: TaskState::Pending,
                    message: None,
                    created_at: "2026-11-26T00:00:00Z".into(),
                    updated_at: "2026-11-26T00:00:00Z".into(),
                };
                self.tickets
                    .lock()
                    .expect("lock")
                    .insert(ticket.request_id.clone(), ticket.clone());
                Ok(ticket)
            })
        }

        fn task_status(&self, request_id: &str) -> Option<TaskTicket> {
            self.tickets.lock().expect("lock").get(request_id).cloned()
        }

        fn read_session(
            &self,
            target: SessionTarget,
            max_lines: u32,
        ) -> BoxFuture<'_, crate::domain::error::VeloxResult<String>> {
            Box::pin(async move {
                self.read_replies
                    .lock()
                    .expect("lock")
                    .get(&(target.peer, target.session_id, max_lines))
                    .cloned()
                    .ok_or_else(|| VeloxError::SessionNotFound("not found".into()))
            })
        }

        fn all_local_sessions(&self) -> Vec<LocalSessionInfo> {
            self.snapshot.local_sessions.clone()
        }

        fn send_local_message(
            &self,
            req: SendLocalMessageRequest,
        ) -> BoxFuture<'_, crate::domain::error::VeloxResult<LocalMessageReceipt>> {
            Box::pin(async move {
                self.sent_local.lock().expect("lock").push(req.clone());
                let from_title = req
                    .from_session_id
                    .as_deref()
                    .and_then(|id| {
                        self.snapshot
                            .local_sessions
                            .iter()
                            .find(|s| s.info.id == id)
                            .map(|s| s.info.title.clone())
                    })
                    .or(req.from_session_query.clone())
                    .unwrap_or_else(|| "Rodolfo".into());
                Ok(LocalMessageReceipt {
                    message_id: "msg-1".into(),
                    from_session_id: req.from_session_id,
                    from_title,
                    to_session_id: "local-2222-bbbb".into(),
                    to_title: req.to,
                    status: LocalMessageStatus::Delivered,
                    woke_up: false,
                    detail: None,
                })
            })
        }
    }

    fn make_mcp() -> (TeamMcp, Arc<FakeTeam>) {
        let team = Arc::new(FakeTeam::sample());
        let mcp = TeamMcp::new(team.clone());
        (mcp, team)
    }

    #[tokio::test]
    async fn initialize_ping_notifications_and_errors() {
        let (mcp, _) = make_mcp();

        let init = mcp
            .handle_jsonrpc(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": { "protocolVersion": "2025-03-26" }
            }))
            .await
            .expect("initialize response");
        assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(init["result"]["serverInfo"]["name"], "velox-team");
        assert_eq!(init["result"]["capabilities"]["tools"]["listChanged"], false);
        assert!(init["result"]["instructions"]
            .as_str()
            .unwrap()
            .contains("accept every task"));

        let ping = mcp
            .handle_jsonrpc(json!({"jsonrpc": "2.0", "id": 2, "method": "ping"}))
            .await
            .expect("ping response");
        assert_eq!(ping["result"], json!({}));

        assert!(mcp
            .handle_jsonrpc(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
            .await
            .is_none());

        let unknown = mcp
            .handle_jsonrpc(json!({"jsonrpc": "2.0", "id": 3, "method": "foo/bar"}))
            .await
            .expect("error response");
        assert_eq!(unknown["error"]["code"], -32601);

        let batch = mcp
            .handle_jsonrpc(json!([{"jsonrpc": "2.0", "id": 1, "method": "ping"}]))
            .await
            .expect("batch error");
        assert_eq!(batch["error"]["code"], -32600);

        let malformed = mcp
            .handle_jsonrpc(json!({"not_jsonrpc": true}))
            .await
            .expect("malformed error");
        assert_eq!(malformed["error"]["code"], -32600);
    }

    #[tokio::test]
    async fn tools_list_exposes_six_tools_with_approval_and_direct_messaging() {
        let (mcp, _) = make_mcp();
        let resp = mcp
            .handle_jsonrpc(json!({"jsonrpc": "2.0", "id": 10, "method": "tools/list"}))
            .await
            .expect("tools/list");
        let tools = resp["result"]["tools"].as_array().expect("tools array");
        assert_eq!(tools.len(), 6);

        let names: Vec<&str> = tools
            .iter()
            .map(|t| t["name"].as_str().expect("name"))
            .collect();
        assert_eq!(
            names,
            vec![
                "team_list",
                "team_send_task",
                "team_task_status",
                "team_read_session",
                "session_list",
                "session_send"
            ]
        );
        for tool in tools {
            assert_eq!(tool["inputSchema"]["type"], "object");
        }
        let send_desc = tools[1]["description"].as_str().expect("desc");
        assert!(send_desc.contains("human on the receiving machine must accept every task"));
    }

    #[tokio::test]
    async fn team_list_and_happy_paths_work() {
        let (mcp, team) = make_mcp();

        let list = mcp
            .handle_jsonrpc(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/call",
                "params": { "name": "team_list", "arguments": {} }
            }))
            .await
            .expect("team_list");
        assert_eq!(list["result"]["isError"], false);
        let list_text = list["result"]["content"][0]["text"].as_str().unwrap();
        assert!(list_text.contains("Rodolfo"));
        assert!(list_text.contains("Benjamin"));
        assert!(list_text.contains("[shared]"));
        assert!(list_text.contains("[not shared]"));
        assert_eq!(list["result"]["structuredContent"]["displayName"], "Rodolfo");

        let sent = mcp
            .handle_jsonrpc(json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/call",
                "params": {
                    "name": "team_send_task",
                    "arguments": {
                        "peer": "Benjamin",
                        "session": "api server",
                        "text": "run cargo test",
                        "from_session": "Velox Lider"
                    }
                }
            }))
            .await
            .expect("send_task");
        assert_eq!(sent["result"]["isError"], false);
        let sent_text = sent["result"]["content"][0]["text"].as_str().unwrap();
        assert!(sent_text.contains("req-1"));
        assert!(sent_text.contains("Waiting for Benjamin to accept in Velox"));
        assert_eq!(
            team.sent_requests.lock().expect("lock")[0].session_id,
            "rem-aaaa-1111"
        );

        let status_pending = mcp
            .handle_jsonrpc(json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "tools/call",
                "params": { "name": "team_task_status", "arguments": { "request_id": "req-1" } }
            }))
            .await
            .expect("status pending");
        assert_eq!(status_pending["result"]["isError"], false);
        assert!(status_pending["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("poll team_task_status"));

        let status_done = mcp
            .handle_jsonrpc(json!({
                "jsonrpc": "2.0",
                "id": 4,
                "method": "tools/call",
                "params": { "name": "team_task_status", "arguments": { "request_id": "req-done" } }
            }))
            .await
            .expect("status done");
        assert_eq!(status_done["result"]["isError"], false);
        assert!(status_done["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("delivered"));

        let read_remote = mcp
            .handle_jsonrpc(json!({
                "jsonrpc": "2.0",
                "id": 5,
                "method": "tools/call",
                "params": {
                    "name": "team_read_session",
                    "arguments": { "peer": "Benjamin", "session": "rem-aaaa" }
                }
            }))
            .await
            .expect("read remote");
        assert_eq!(read_remote["result"]["isError"], false);
        assert_eq!(
            read_remote["result"]["content"][0]["text"],
            "server listening on :8080"
        );

        let read_local = mcp
            .handle_jsonrpc(json!({
                "jsonrpc": "2.0",
                "id": 6,
                "method": "tools/call",
                "params": {
                    "name": "team_read_session",
                    "arguments": { "peer": "local", "session": "LOCAL-1111-AAAA", "lines": 10 }
                }
            }))
            .await
            .expect("read local");
        assert_eq!(read_local["result"]["isError"], false);
        assert_eq!(
            read_local["result"]["content"][0]["text"],
            "local tail output"
        );
    }

    #[tokio::test]
    async fn session_resolution_handles_ambiguous_unknown_and_validation_errors() {
        let (mcp, _) = make_mcp();

        // Ambiguous prefix "API" matches both "API Server" and "API Tests"
        let ambiguous = mcp
            .handle_jsonrpc(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/call",
                "params": {
                    "name": "team_send_task",
                    "arguments": { "peer": "Benjamin", "session": "API", "text": "hello" }
                }
            }))
            .await
            .expect("ambiguous");
        assert_eq!(ambiguous["result"]["isError"], true);
        let amb_msg = ambiguous["result"]["content"][0]["text"].as_str().unwrap();
        assert!(amb_msg.contains("Ambiguous"));
        assert!(amb_msg.contains("API Server"));
        assert!(amb_msg.contains("API Tests"));

        // Unknown session lists candidates
        let unknown_sess = mcp
            .handle_jsonrpc(json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/call",
                "params": {
                    "name": "team_read_session",
                    "arguments": { "peer": "Benjamin", "session": "missing" }
                }
            }))
            .await
            .expect("unknown session");
        assert_eq!(unknown_sess["result"]["isError"], true);
        assert!(unknown_sess["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("API Server"));

        // Empty text and oversized text -> tool errors
        let empty_text = mcp
            .handle_jsonrpc(json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "tools/call",
                "params": {
                    "name": "team_send_task",
                    "arguments": { "session": "Velox Lider", "text": "   " }
                }
            }))
            .await
            .expect("empty text");
        assert_eq!(empty_text["result"]["isError"], true);

        let too_big = "x".repeat(MAX_TASK_TEXT_BYTES + 1);
        let big_text = mcp
            .handle_jsonrpc(json!({
                "jsonrpc": "2.0",
                "id": 4,
                "method": "tools/call",
                "params": {
                    "name": "team_send_task",
                    "arguments": { "session": "Velox Lider", "text": too_big }
                }
            }))
            .await
            .expect("oversized text");
        assert_eq!(big_text["result"]["isError"], true);

        // Unknown tool -> JSON-RPC -32602
        let bad_tool = mcp
            .handle_jsonrpc(json!({
                "jsonrpc": "2.0",
                "id": 5,
                "method": "tools/call",
                "params": { "name": "nonexistent_tool", "arguments": {} }
            }))
            .await
            .expect("unknown tool");
        assert_eq!(bad_tool["error"]["code"], -32602);
    }

    #[test]
    fn resolve_session_prefers_exact_id_then_exact_title_then_unique_prefix() {
        let s1 = RemoteSessionInfo {
            id: "alpha".into(),
            title: "API".into(),
            agent_kind: AgentKind::Claude,
            active_agent: None,
            status: SessionStatus::Idle,
            folder: "repo".into(),
            last_active_at: "2026-11-26T00:00:00Z".into(),
            summary: None,
            cols: 80,
            rows: 24,
        };
        let s2 = RemoteSessionInfo {
            id: "api".into(),
            title: "API Tests".into(),
            agent_kind: AgentKind::Claude,
            active_agent: None,
            status: SessionStatus::Idle,
            folder: "repo".into(),
            last_active_at: "2026-11-26T00:00:00Z".into(),
            summary: None,
            cols: 80,
            rows: 24,
        };
        let s3 = RemoteSessionInfo {
            id: "gamma-1".into(),
            title: "Unique Worker".into(),
            agent_kind: AgentKind::Agy,
            active_agent: None,
            status: SessionStatus::Running,
            folder: "repo".into(),
            last_active_at: "2026-11-26T00:00:00Z".into(),
            summary: None,
            cols: 80,
            rows: 24,
        };
        let list = vec![&s1, &s2, &s3];

        // "api" matches s2's exact id AND s1's exact title -> exact id wins (s2)
        assert_eq!(resolve_session(&list, "API", "test").unwrap().id, "api");
        // "API Tests" matches s2's exact title
        assert_eq!(
            resolve_session(&list, "api tests", "test").unwrap().id,
            "api"
        );
        // "unique" matches s3's title by unique prefix
        assert_eq!(
            resolve_session(&list, "unique", "test").unwrap().id,
            "gamma-1"
        );
    }

    #[tokio::test]
    async fn session_list_marks_caller_from_header_and_session_send_prefers_header() {
        let (mcp, team) = make_mcp();

        let list = mcp
            .handle_jsonrpc_with_session(
                json!({
                    "jsonrpc": "2.0",
                    "id": 20,
                    "method": "tools/call",
                    "params": { "name": "session_list", "arguments": {} }
                }),
                Some("local-1111-aaaa".to_string()),
            )
            .await
            .expect("session_list response");
        assert_eq!(list["result"]["isError"], false);
        let list_text = list["result"]["content"][0]["text"].as_str().unwrap();
        assert!(list_text.contains("[current session]"));
        assert_eq!(
            list["result"]["structuredContent"]["currentSessionId"],
            "local-1111-aaaa"
        );
        assert_eq!(
            list["result"]["structuredContent"]["sessions"][0]["isSelf"],
            true
        );
        assert_eq!(
            list["result"]["structuredContent"]["sessions"][1]["isSelf"],
            false
        );

        let send = mcp
            .handle_jsonrpc_with_session(
                json!({
                    "jsonrpc": "2.0",
                    "id": 21,
                    "method": "tools/call",
                    "params": {
                        "name": "session_send",
                        "arguments": {
                            "to": "Velox Build",
                            "text": "Ping build check",
                            "from_session": "Forged Name"
                        }
                    }
                }),
                Some("local-1111-aaaa".to_string()),
            )
            .await
            .expect("session_send response");
        assert_eq!(send["result"]["isError"], false);
        assert_eq!(
            send["result"]["structuredContent"]["fromSessionId"],
            "local-1111-aaaa"
        );
        assert_eq!(
            send["result"]["structuredContent"]["fromTitle"],
            "Velox Lider"
        );

        let sent = team.sent_local.lock().unwrap();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].from_session_id.as_deref(), Some("local-1111-aaaa"));
        assert_eq!(sent[0].from_session_query.as_deref(), Some("Forged Name"));
        assert_eq!(sent[0].to, "Velox Build");
        assert_eq!(sent[0].text, "Ping build check");
    }

    #[tokio::test]
    async fn session_send_validates_empty_and_oversized_text() {
        let (mcp, _) = make_mcp();

        let empty = mcp
            .handle_jsonrpc(json!({
                "jsonrpc": "2.0",
                "id": 30,
                "method": "tools/call",
                "params": {
                    "name": "session_send",
                    "arguments": { "to": "Velox Build", "text": "   \n " }
                }
            }))
            .await
            .unwrap();
        assert_eq!(empty["result"]["isError"], true);

        let oversized = "a".repeat(MAX_TASK_TEXT_BYTES + 1);
        let too_big = mcp
            .handle_jsonrpc(json!({
                "jsonrpc": "2.0",
                "id": 31,
                "method": "tools/call",
                "params": {
                    "name": "session_send",
                    "arguments": { "to": "Velox Build", "text": oversized }
                }
            }))
            .await
            .unwrap();
        assert_eq!(too_big["result"]["isError"], true);
    }
}
