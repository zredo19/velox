//! Team WebSocket client (dialing side: viewer/requester), one per configured peer.

use crate::domain::config::PeerConfig;
use crate::domain::error::{VeloxError, VeloxResult};
use crate::team::api::{
    TeamPtyOutputEvent, TeamPtyResizeEvent, TeamPtySnapshotEvent, TeamSessionGoneEvent,
    EVT_TEAM_PTY_OUTPUT, EVT_TEAM_PTY_RESIZE, EVT_TEAM_PTY_SNAPSHOT, EVT_TEAM_SESSION_GONE,
};
use crate::team::hub::TeamHub;
use crate::team::protocol::{ClientMsg, ServerMsg, PROTOCOL_VERSION, WS_PATH};
use futures_util::{SinkExt, StreamExt};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::{header::AUTHORIZATION, HeaderValue};
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

const INITIAL_BACKOFF: Duration = Duration::from_secs(2);
const MAX_BACKOFF: Duration = Duration::from_secs(30);
const SILENCE_TIMEOUT: Duration = Duration::from_secs(60);
const READ_TIMEOUT: Duration = Duration::from_secs(10);
const OUT_CHANNEL_CAPACITY: usize = 64;

/// Manages the WebSocket connection to a single configured teammate.
pub struct PeerClient {
    peer: PeerConfig,
    hub: Weak<TeamHub>,
    local_full_control: AtomicBool,
    tx: Mutex<Option<mpsc::Sender<ClientMsg>>>,
    active_mirrors: Mutex<HashSet<String>>,
    pending_reads: Mutex<HashMap<String, oneshot::Sender<Result<String, String>>>>,
    shutdown_tx: Mutex<Option<oneshot::Sender<()>>>,
    task: tokio::sync::Mutex<Option<JoinHandle<()>>>,
}

impl PeerClient {
    /// Spawns a background reconnect/reader loop for `peer`.
    pub fn start(
        peer: PeerConfig,
        token: String,
        display_name: String,
        hub: Arc<TeamHub>,
    ) -> Arc<Self> {
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let initial_fc = peer.full_control;
        let client = Arc::new(Self {
            peer: peer.clone(),
            hub: Arc::downgrade(&hub),
            local_full_control: AtomicBool::new(initial_fc),
            tx: Mutex::new(None),
            active_mirrors: Mutex::new(HashSet::new()),
            pending_reads: Mutex::new(HashMap::new()),
            shutdown_tx: Mutex::new(Some(shutdown_tx)),
            task: tokio::sync::Mutex::new(None),
        });

        let runner = Arc::clone(&client);
        let handle = tokio::spawn(async move {
            runner.run_loop(token, display_name, shutdown_rx).await;
        });
        if let Ok(mut guard) = client.task.try_lock() {
            *guard = Some(handle);
        }
        client
    }

    pub async fn shutdown(&self) {
        if let Some(tx) = self
            .shutdown_tx
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            let _ = tx.send(());
        }
        let handle = self.task.lock().await.take();
        if let Some(h) = handle {
            h.abort();
            let _ = h.await;
        }
        *self.tx.lock().unwrap_or_else(|e| e.into_inner()) = None;
        self.fail_all_pending_reads("Peer client shut down");
    }

    pub fn subscribe(&self, session_id: &str) {
        self.active_mirrors
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(session_id.to_string());
        if let Some(tx) = self.tx.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            let _ = tx.try_send(ClientMsg::Subscribe {
                session_id: session_id.to_string(),
            });
        }
    }

    pub fn unsubscribe(&self, session_id: &str) {
        self.active_mirrors
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(session_id);
        if let Some(tx) = self.tx.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            let _ = tx.try_send(ClientMsg::Unsubscribe {
                session_id: session_id.to_string(),
            });
        }
    }

    pub fn set_full_control(&self, enabled: bool) {
        self.local_full_control.store(enabled, Ordering::SeqCst);
        if let Some(tx) = self.tx.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            let _ = tx.try_send(ClientMsg::FullControl { enabled });
        }
    }

    pub fn send_terminal_input(&self, session_id: String, data_b64: String) -> VeloxResult<()> {
        let tx = self
            .tx
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .ok_or_else(|| {
                VeloxError::InvalidInput(format!("Peer '{}' is offline", self.peer.name))
            })?;
        tx.try_send(ClientMsg::TerminalInput {
            session_id,
            data_b64,
        })
        .map_err(|_| {
            VeloxError::InvalidInput(format!(
                "Peer '{}' connection is congested or closed",
                self.peer.name
            ))
        })
    }

    pub fn send_task(
        &self,
        request_id: String,
        session_id: String,
        from_session: Option<String>,
        text: String,
    ) -> VeloxResult<()> {
        let tx = self
            .tx
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .ok_or_else(|| {
                VeloxError::InvalidInput(format!("Peer '{}' is offline", self.peer.name))
            })?;
        tx.try_send(ClientMsg::TaskRequest {
            request_id,
            session_id,
            from_session,
            text,
        })
        .map_err(|_| {
            VeloxError::InvalidInput(format!(
                "Peer '{}' connection is congested or closed",
                self.peer.name
            ))
        })
    }

    pub async fn read_session(&self, session_id: &str, max_lines: u32) -> VeloxResult<String> {
        let request_id = Uuid::new_v4().to_string();
        let (otx, orx) = oneshot::channel();
        {
            let mut pending = self.pending_reads.lock().unwrap_or_else(|e| e.into_inner());
            pending.insert(request_id.clone(), otx);
        }

        let send_res = {
            let tx_opt = self.tx.lock().unwrap_or_else(|e| e.into_inner()).clone();
            match tx_opt {
                Some(tx) => tx
                    .try_send(ClientMsg::ReadRequest {
                        request_id: request_id.clone(),
                        session_id: session_id.to_string(),
                        max_lines,
                    })
                    .map_err(|_| {
                        VeloxError::InvalidInput(format!("Peer '{}' is congested", self.peer.name))
                    }),
                None => Err(VeloxError::InvalidInput(format!(
                    "Peer '{}' is offline",
                    self.peer.name
                ))),
            }
        };

        if let Err(err) = send_res {
            self.pending_reads
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&request_id);
            return Err(err);
        }

        match tokio::time::timeout(READ_TIMEOUT, orx).await {
            Ok(Ok(Ok(text))) => Ok(text),
            Ok(Ok(Err(err))) => Err(VeloxError::InvalidInput(err)),
            Ok(Err(_)) => Err(VeloxError::InvalidInput(format!(
                "Peer '{}' disconnected before answering ReadRequest",
                self.peer.name
            ))),
            Err(_) => {
                self.pending_reads
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&request_id);
                Err(VeloxError::InvalidInput(format!(
                    "Timed out reading session from peer '{}'",
                    self.peer.name
                )))
            }
        }
    }

    async fn run_loop(
        self: Arc<Self>,
        token: String,
        display_name: String,
        mut shutdown_rx: oneshot::Receiver<()>,
    ) {
        let mut backoff = INITIAL_BACKOFF;

        loop {
            if shutdown_rx.try_recv().is_ok() {
                return;
            }

            let url = format!("ws://{}:{}{}", self.peer.host, self.peer.port, WS_PATH);
            let req = match build_ws_request(&url, &token) {
                Ok(r) => r,
                Err(err) => {
                    if let Some(hub) = self.hub.upgrade() {
                        hub.set_peer_offline(&self.peer.name, Some(err));
                    }
                    tokio::select! {
                        _ = &mut shutdown_rx => return,
                        _ = tokio::time::sleep(backoff) => {
                            backoff = (backoff * 2).min(MAX_BACKOFF);
                            continue;
                        }
                    }
                }
            };

            let ws_stream = tokio::select! {
                _ = &mut shutdown_rx => return,
                res = connect_async(req) => match res {
                    Ok((stream, _)) => stream,
                    Err(err) => {
                        if let Some(hub) = self.hub.upgrade() {
                            hub.set_peer_offline(&self.peer.name, Some(err.to_string()));
                        }
                        tokio::select! {
                            _ = &mut shutdown_rx => return,
                            _ = tokio::time::sleep(backoff) => {
                                backoff = (backoff * 2).min(MAX_BACKOFF);
                                continue;
                            }
                        }
                    }
                }
            };

            let (mut ws_tx, mut ws_rx) = ws_stream.split();
            let (out_tx, mut out_rx) = mpsc::channel::<ClientMsg>(OUT_CHANNEL_CAPACITY);
            *self.tx.lock().unwrap_or_else(|e| e.into_inner()) = Some(out_tx.clone());

            let _ = out_tx.try_send(ClientMsg::Hello {
                protocol: PROTOCOL_VERSION,
                peer_name: display_name.clone(),
                full_control: self.local_full_control.load(Ordering::SeqCst),
            });
            let mirrors: Vec<String> = self
                .active_mirrors
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .cloned()
                .collect();
            for session_id in mirrors {
                let _ = out_tx.try_send(ClientMsg::Subscribe { session_id });
            }

            let mut disconnect_reason: Option<String> = None;

            loop {
                tokio::select! {
                    _ = &mut shutdown_rx => {
                        let _ = ws_tx.send(Message::Close(None)).await;
                        *self.tx.lock().unwrap_or_else(|e| e.into_inner()) = None;
                        self.fail_all_pending_reads("Peer client shut down");
                        return;
                    }
                    maybe_out = out_rx.recv() => {
                        let Some(msg) = maybe_out else {
                            break;
                        };
                        if let Ok(json) = serde_json::to_string(&msg) {
                            if let Err(err) = ws_tx.send(Message::text(json)).await {
                                disconnect_reason = Some(err.to_string());
                                break;
                            }
                        }
                    }
                    incoming = tokio::time::timeout(SILENCE_TIMEOUT, ws_rx.next()) => {
                        match incoming {
                            Ok(Some(Ok(Message::Text(text)))) => {
                                match serde_json::from_str::<ServerMsg>(text.as_str()) {
                                    Ok(msg) => {
                                        if matches!(msg, ServerMsg::Hello { protocol, .. } if protocol == PROTOCOL_VERSION) {
                                            backoff = INITIAL_BACKOFF;
                                        }
                                        if let Err(err) = self.handle_server_msg(msg) {
                                            disconnect_reason = Some(err);
                                            break;
                                        }
                                    }
                                    Err(err) => {
                                        tracing::warn!(peer = %self.peer.name, %err, "Failed to parse ServerMsg");
                                    }
                                }
                            }
                            Ok(Some(Ok(Message::Ping(payload)))) => {
                                let _ = ws_tx.send(Message::Pong(payload)).await;
                            }
                            Ok(Some(Ok(Message::Pong(_)))) | Ok(Some(Ok(Message::Binary(_)))) => {}
                            Ok(Some(Ok(Message::Close(_)))) => {
                                disconnect_reason = Some("Connection closed by peer".to_string());
                                break;
                            }
                            Ok(Some(Err(err))) => {
                                disconnect_reason = Some(err.to_string());
                                break;
                            }
                            Ok(None) => {
                                disconnect_reason = Some("Connection closed".to_string());
                                break;
                            }
                            Err(_) => {
                                disconnect_reason = Some("Peer silent for 60 s".to_string());
                                break;
                            }
                            Ok(Some(Ok(Message::Frame(_)))) => {}
                        }
                    }
                }
            }

            *self.tx.lock().unwrap_or_else(|e| e.into_inner()) = None;
            self.fail_all_pending_reads(
                disconnect_reason
                    .as_deref()
                    .unwrap_or("Disconnected from peer"),
            );
            if let Some(hub) = self.hub.upgrade() {
                hub.set_peer_offline(&self.peer.name, disconnect_reason);
            }

            tokio::select! {
                _ = &mut shutdown_rx => return,
                _ = tokio::time::sleep(backoff) => {
                    backoff = (backoff * 2).min(MAX_BACKOFF);
                }
            }
        }
    }

    fn handle_server_msg(&self, msg: ServerMsg) -> Result<(), String> {
        let Some(hub) = self.hub.upgrade() else {
            return Err("TeamHub dropped".to_string());
        };
        match msg {
            ServerMsg::Hello {
                protocol,
                peer_name,
                full_control,
            } => {
                if protocol != PROTOCOL_VERSION {
                    return Err(format!(
                        "Protocol mismatch: expected {PROTOCOL_VERSION}, got {protocol}"
                    ));
                }
                hub.set_peer_hello(&self.peer.name, peer_name, full_control);
            }
            ServerMsg::FullControl { enabled } => {
                hub.set_peer_remote_full_control(&self.peer.name, enabled);
            }
            ServerMsg::Sessions { sessions } => {
                hub.set_peer_sessions(&self.peer.name, sessions);
            }
            ServerMsg::Snapshot {
                session_id,
                generation,
                end_offset,
                data_b64,
                cols,
                rows,
            } => {
                hub.emit_event(
                    EVT_TEAM_PTY_SNAPSHOT,
                    &TeamPtySnapshotEvent {
                        peer: self.peer.name.clone(),
                        session_id,
                        generation,
                        end_offset,
                        data_b64,
                        cols,
                        rows,
                    },
                );
            }
            ServerMsg::Output {
                session_id,
                generation,
                end_offset,
                data_b64,
            } => {
                hub.emit_event(
                    EVT_TEAM_PTY_OUTPUT,
                    &TeamPtyOutputEvent {
                        peer: self.peer.name.clone(),
                        session_id,
                        generation,
                        end_offset,
                        data_b64,
                    },
                );
            }
            ServerMsg::Resize {
                session_id,
                cols,
                rows,
            } => {
                hub.emit_event(
                    EVT_TEAM_PTY_RESIZE,
                    &TeamPtyResizeEvent {
                        peer: self.peer.name.clone(),
                        session_id,
                        cols,
                        rows,
                    },
                );
            }
            ServerMsg::SessionGone { session_id } => {
                self.active_mirrors
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&session_id);
                hub.emit_event(
                    EVT_TEAM_SESSION_GONE,
                    &TeamSessionGoneEvent {
                        peer: self.peer.name.clone(),
                        session_id,
                    },
                );
            }
            ServerMsg::TaskUpdate {
                request_id,
                state,
                message,
            } => {
                hub.handle_remote_task_update(&self.peer.name, request_id, state, message);
            }
            ServerMsg::ReadResult {
                request_id,
                text,
                error,
            } => {
                if let Some(tx) = self
                    .pending_reads
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&request_id)
                {
                    let result = match (text, error) {
                        (Some(t), _) => Ok(t),
                        (None, Some(e)) => Err(e),
                        (None, None) => Err("Empty ReadResult from peer".to_string()),
                    };
                    let _ = tx.send(result);
                }
            }
            ServerMsg::Error { message } => {
                tracing::warn!(peer = %self.peer.name, %message, "Peer reported error");
                hub.set_peer_error(&self.peer.name, message);
            }
        }
        Ok(())
    }

    fn fail_all_pending_reads(&self, reason: &str) {
        let drained: Vec<_> = self
            .pending_reads
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .drain()
            .collect();
        for (_, tx) in drained {
            let _ = tx.send(Err(reason.to_string()));
        }
    }
}

fn build_ws_request(
    url: &str,
    token: &str,
) -> Result<tokio_tungstenite::tungstenite::handshake::client::Request, String> {
    let mut req = url
        .into_client_request()
        .map_err(|e| format!("Invalid peer URL '{url}': {e}"))?;
    let auth = HeaderValue::from_str(&format!("Bearer {token}"))
        .map_err(|e| format!("Invalid Bearer token header: {e}"))?;
    req.headers_mut().insert(AUTHORIZATION, auth);
    Ok(req)
}
