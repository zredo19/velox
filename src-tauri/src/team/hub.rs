//! `TeamHub`: central Velox Online state (config, local shared sessions, peers, tasks) and the
//! `TeamApi` implementation.

use crate::domain::config::{PeerConfig, TeamConfig};
use crate::domain::error::{VeloxError, VeloxResult};
use crate::domain::session::{SessionRecord, SessionStatus};
use crate::pty::manager::StreamPos;
use crate::session::manager::{SessionManager, SessionTap};
use crate::team::api::{
    BoxFuture, LocalSessionInfo, PeerStatus, SendTaskRequest, SessionTarget, TaskDirection,
    TaskTicket, TeamApi, TeamSnapshot, EVT_TEAM_TASKS_UPDATED, EVT_TEAM_UPDATED,
};
use crate::team::client::PeerClient;
use crate::team::delivery::{build_task_input, CHUNK_DELAY_MS};
use crate::team::protocol::{
    RemoteSessionInfo, ServerMsg, TaskState, MAX_READ_LINES, MAX_SNAPSHOT_BYTES,
    MAX_TASK_TEXT_BYTES,
};
use crate::team::server::spawn_server;
use crate::team::summary::last_action_summary;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use chrono::Utc;
use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet, VecDeque};
use std::net::{Ipv4Addr, SocketAddr};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use uuid::Uuid;

const DEFAULT_PTY_COLS: u16 = 120;
const DEFAULT_PTY_ROWS: u16 = 30;
const HUB_TICK_INTERVAL: Duration = Duration::from_secs(2);
const SUMMARY_CACHE_TTL: Duration = Duration::from_secs(5);
const TASK_EXPIRY: Duration = Duration::from_secs(30 * 60);
const RATE_LIMIT_WINDOW: Duration = Duration::from_secs(60);
const RATE_LIMIT_MAX_TASKS: usize = 20;
const MAX_TICKETS_RETAINED: usize = 200;
const MAX_TICKETS_RETURNED: usize = 50;
const MAX_READ_SCROLLBACK_BYTES: usize = 512 * 1024;
const VT100_SCROLLBACK_LINES: usize = 2000;

/// Minimal session backend interface so `TeamHub` can run against either `SessionManager` or a fake.
pub trait LocalSessions: Send + Sync {
    fn list_sessions(&self) -> Vec<SessionRecord>;
    fn pty_size(&self, session_id: &str) -> Option<(u16, u16)>;
    fn get_scrollback_with_pos(&self, session_id: &str) -> (Vec<u8>, StreamPos);
    fn write_pty_bytes(&self, session_id: &str, data: &[u8]) -> VeloxResult<()>;
    fn is_pty_alive(&self, session_id: &str) -> bool;
}

impl LocalSessions for SessionManager {
    fn list_sessions(&self) -> Vec<SessionRecord> {
        SessionManager::list_sessions(self)
    }

    fn pty_size(&self, session_id: &str) -> Option<(u16, u16)> {
        SessionManager::pty_size(self, session_id)
    }

    fn get_scrollback_with_pos(&self, session_id: &str) -> (Vec<u8>, StreamPos) {
        SessionManager::get_scrollback_with_pos(self, session_id)
    }

    fn write_pty_bytes(&self, session_id: &str, data: &[u8]) -> VeloxResult<()> {
        SessionManager::write_pty_bytes(self, session_id, data)
    }

    fn is_pty_alive(&self, session_id: &str) -> bool {
        SessionManager::is_pty_alive(self, session_id)
    }
}

/// Event emission sink so `TeamHub` can emit to Tauri in production or a test sink in unit/integration tests.
pub trait EventSink: Send + Sync {
    fn emit_json(&self, event: &str, payload: Value);
}

struct TauriEventSink(AppHandle);

impl EventSink for TauriEventSink {
    fn emit_json(&self, event: &str, payload: Value) {
        let _ = self.0.emit(event, payload);
    }
}

#[derive(Default)]
struct ServerRuntimeState {
    listen_addr: Option<SocketAddr>,
    server_error: Option<String>,
    tailscale_ip: Option<Ipv4Addr>,
}

#[derive(Default, Clone)]
struct PeerRuntimeState {
    online: bool,
    display_name: Option<String>,
    last_error: Option<String>,
    sessions: Vec<RemoteSessionInfo>,
}

struct ConnectionEntry {
    tx: mpsc::Sender<ServerMsg>,
    peer_name: Option<String>,
    task_timestamps: VecDeque<Instant>,
}

enum SubPhase {
    Buffering(Vec<(StreamPos, ServerMsg)>),
    Live {
        snap_generation: u64,
        snap_end_offset: u64,
    },
}

struct SubscriberEntry {
    tx: mpsc::Sender<ServerMsg>,
    phase: SubPhase,
}

#[derive(Default)]
struct SubscriberState {
    by_session: HashMap<String, HashMap<u64, SubscriberEntry>>,
    lagged: HashMap<u64, HashSet<String>>,
}

struct StoredTicket {
    seq: u64,
    ticket: TaskTicket,
    created_instant: Instant,
    requester_conn_id: Option<u64>,
}

struct SummaryCacheEntry {
    fetched_at: Instant,
    agent_session_id: Option<String>,
    summary: Option<String>,
}

struct ServerTaskHandle {
    shutdown_tx: oneshot::Sender<()>,
    task: JoinHandle<()>,
}

struct LifecycleHandles {
    server: Option<ServerTaskHandle>,
    peers: Vec<(PeerConfig, Arc<PeerClient>)>,
    tick_shutdown: Option<oneshot::Sender<()>>,
    tick_task: Option<JoinHandle<()>>,
}

/// Central Velox Online coordinator.
pub struct TeamHub {
    sessions: Arc<dyn LocalSessions>,
    event_sink: RwLock<Option<Arc<dyn EventSink>>>,
    config: RwLock<TeamConfig>,
    server_state: RwLock<ServerRuntimeState>,
    peer_states: RwLock<HashMap<String, PeerRuntimeState>>,
    local_sessions: RwLock<Vec<LocalSessionInfo>>,
    shared_live_ids: RwLock<HashSet<String>>,
    connections: Mutex<HashMap<u64, ConnectionEntry>>,
    next_conn_id: AtomicU64,
    subscribers: Mutex<SubscriberState>,
    tickets: Mutex<Vec<StoredTicket>>,
    next_ticket_seq: AtomicU64,
    summary_cache: Mutex<HashMap<String, SummaryCacheEntry>>,
    last_emitted_snapshot: Mutex<Value>,
    lifecycle: tokio::sync::Mutex<LifecycleHandles>,
}

impl TeamHub {
    pub fn new(sessions: Arc<SessionManager>, config: TeamConfig) -> Arc<Self> {
        Self::new_with_backends(sessions, None, config)
    }

    pub fn new_with_backends(
        sessions: Arc<dyn LocalSessions>,
        event_sink: Option<Arc<dyn EventSink>>,
        config: TeamConfig,
    ) -> Arc<Self> {
        Arc::new(Self {
            sessions,
            event_sink: RwLock::new(event_sink),
            config: RwLock::new(config),
            server_state: RwLock::new(ServerRuntimeState::default()),
            peer_states: RwLock::new(HashMap::new()),
            local_sessions: RwLock::new(Vec::new()),
            shared_live_ids: RwLock::new(HashSet::new()),
            connections: Mutex::new(HashMap::new()),
            next_conn_id: AtomicU64::new(1),
            subscribers: Mutex::new(SubscriberState::default()),
            tickets: Mutex::new(Vec::new()),
            next_ticket_seq: AtomicU64::new(1),
            summary_cache: Mutex::new(HashMap::new()),
            last_emitted_snapshot: Mutex::new(Value::Null),
            lifecycle: tokio::sync::Mutex::new(LifecycleHandles {
                server: None,
                peers: Vec::new(),
                tick_shutdown: None,
                tick_task: None,
            }),
        })
    }

    pub fn start(self: &Arc<Self>, app: AppHandle) {
        *self.event_sink.write().unwrap_or_else(|e| e.into_inner()) =
            Some(Arc::new(TauriEventSink(app)));
        self.start_headless();
    }

    /// Starts server, peer clients, and background tick using the configured `EventSink`.
    pub fn start_headless(self: &Arc<Self>) {
        let hub = Arc::clone(self);
        spawn_on_runtime(async move {
            hub.refresh_local_sessions().await;
            hub.start_network_tasks().await;
            hub.start_tick_task().await;
            hub.emit_snapshot_if_changed(true);
        });
    }

    pub async fn apply_config(self: &Arc<Self>, next: TeamConfig) {
        let prev = {
            let mut guard = self.config.write().unwrap_or_else(|e| e.into_inner());
            let prev = guard.clone();
            *guard = next.clone();
            prev
        };

        let server_changed = prev.enabled != next.enabled
            || prev.listen_port != next.listen_port
            || prev.bind_host != next.bind_host
            || prev.token != next.token
            || prev.display_name != next.display_name;

        let clients_changed = prev.enabled != next.enabled
            || prev.token != next.token
            || prev.display_name != next.display_name
            || prev.peers != next.peers;

        if server_changed {
            self.restart_server().await;
        }
        if clients_changed {
            self.restart_clients().await;
        }

        self.emit_snapshot_always();
    }

    pub async fn shutdown(&self) {
        let (server, peers, tick_tx, tick_task) = {
            let mut lc = self.lifecycle.lock().await;
            (
                lc.server.take(),
                std::mem::take(&mut lc.peers),
                lc.tick_shutdown.take(),
                lc.tick_task.take(),
            )
        };

        if let Some(tx) = tick_tx {
            let _ = tx.send(());
        }
        if let Some(h) = tick_task {
            h.abort();
            let _ = h.await;
        }
        if let Some(srv) = server {
            let _ = srv.shutdown_tx.send(());
            let _ = srv.task.await;
        }
        for (_, client) in peers {
            client.shutdown().await;
        }
    }

    pub fn server_addr(&self) -> Option<SocketAddr> {
        self.server_state
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .listen_addr
    }

    pub async fn subscribe(&self, peer: &str, session_id: &str) -> VeloxResult<()> {
        let client = self.find_peer_client(peer).await?;
        client.subscribe(session_id);
        Ok(())
    }

    pub async fn unsubscribe(&self, peer: &str, session_id: &str) -> VeloxResult<()> {
        let client = self.find_peer_client(peer).await?;
        client.unsubscribe(session_id);
        Ok(())
    }

    pub fn list_tasks(&self) -> Vec<TaskTicket> {
        let guard = self.tickets.lock().unwrap_or_else(|e| e.into_inner());
        Self::recent_tickets_from(&guard)
    }

    /// Triggers an immediate local session refresh (called after session mutations such as `set_session_shared`).
    pub fn sync_local_sessions_now(self: &Arc<Self>) {
        let hub = Arc::clone(self);
        spawn_on_runtime(async move {
            hub.refresh_local_sessions().await;
            hub.emit_snapshot_if_changed(false);
        });
    }

    pub async fn respond_task(&self, request_id: &str, accept: bool) -> VeloxResult<TaskTicket> {
        let (session_id, text, requester_conn_id) = {
            let mut guard = self.tickets.lock().unwrap_or_else(|e| e.into_inner());
            let entry = guard
                .iter_mut()
                .find(|t| t.ticket.request_id == request_id)
                .ok_or_else(|| {
                    VeloxError::InvalidInput(format!("Unknown task request: {request_id}"))
                })?;

            if entry.ticket.direction != TaskDirection::Incoming {
                return Err(VeloxError::InvalidInput(
                    "Cannot respond to an outgoing task".to_string(),
                ));
            }
            if entry.ticket.state != TaskState::Pending {
                return Err(VeloxError::InvalidInput(format!(
                    "Task '{}' is no longer pending",
                    request_id
                )));
            }

            if !accept {
                entry.ticket.state = TaskState::Rejected;
                entry.ticket.updated_at = Utc::now().to_rfc3339();
                let ticket = entry.ticket.clone();
                let conn_id = entry.requester_conn_id;
                drop(guard);

                self.notify_requester(conn_id, request_id, TaskState::Rejected, None);
                self.emit_tasks_updated();
                return Ok(ticket);
            }

            (
                entry.ticket.session_id.clone(),
                entry.ticket.text.clone(),
                entry.requester_conn_id,
            )
        };

        let target = self
            .sessions
            .list_sessions()
            .into_iter()
            .find(|s| s.id == session_id);
        let target_valid = match &target {
            Some(s) if is_live_status(s.status) => {
                requester_conn_id.is_none() || s.shared
            }
            _ => false,
        };

        if !target_valid {
            let msg = "Target session is unavailable or no longer shared".to_string();
            let ticket = self.update_ticket_state(request_id, TaskState::Failed, Some(msg.clone()));
            self.notify_requester(requester_conn_id, request_id, TaskState::Failed, Some(msg));
            self.emit_tasks_updated();
            return ticket.ok_or(VeloxError::SessionNotFound(session_id));
        }

        let kind = target
            .as_ref()
            .map(|s| s.active_agent.unwrap_or(s.agent_kind))
            .unwrap_or(crate::domain::agent::AgentKind::Shell);

        self.update_ticket_state(request_id, TaskState::Accepted, None);
        self.notify_requester(requester_conn_id, request_id, TaskState::Accepted, None);
        self.emit_tasks_updated();

        let chunks = build_task_input(kind, &text);
        let mut delivery_error: Option<String> = None;
        for (i, chunk) in chunks.iter().enumerate() {
            if i > 0 {
                tokio::time::sleep(Duration::from_millis(CHUNK_DELAY_MS)).await;
            }
            if let Err(err) = self.sessions.write_pty_bytes(&session_id, chunk) {
                delivery_error = Some(err.to_string());
                break;
            }
        }

        let (final_state, final_msg) = match delivery_error {
            None => (TaskState::Delivered, None),
            Some(err) => (TaskState::Failed, Some(err)),
        };

        let ticket = self
            .update_ticket_state(request_id, final_state, final_msg.clone())
            .ok_or_else(|| VeloxError::InvalidInput(format!("Task lost: {request_id}")))?;
        self.notify_requester(requester_conn_id, request_id, final_state, final_msg);
        self.emit_tasks_updated();
        Ok(ticket)
    }

    // -----------------------------------------------------------------------
    // Internal & server/client coordination helpers
    // -----------------------------------------------------------------------

    async fn start_network_tasks(self: &Arc<Self>) {
        self.restart_server().await;
        self.restart_clients().await;
    }

    async fn restart_server(self: &Arc<Self>) {
        let old = {
            let mut lc = self.lifecycle.lock().await;
            lc.server.take()
        };
        if let Some(srv) = old {
            let _ = srv.shutdown_tx.send(());
            let _ = srv.task.await;
        }

        let cfg = self.config_snapshot();
        if !cfg.enabled {
            self.set_server_state(None, None, None);
            return;
        }
        let (tx, rx) = oneshot::channel();
        let task = spawn_server(Arc::clone(self), rx);
        let mut lc = self.lifecycle.lock().await;
        lc.server = Some(ServerTaskHandle {
            shutdown_tx: tx,
            task,
        });
    }

    async fn restart_clients(self: &Arc<Self>) {
        let old_peers = {
            let mut lc = self.lifecycle.lock().await;
            std::mem::take(&mut lc.peers)
        };
        for (_, client) in old_peers {
            client.shutdown().await;
        }

        let cfg = self.config_snapshot();
        {
            let mut ps = self.peer_states.write().unwrap_or_else(|e| e.into_inner());
            ps.clear();
            for peer in &cfg.peers {
                ps.insert(peer.name.clone(), PeerRuntimeState::default());
            }
        }

        if !cfg.enabled || cfg.token.trim().is_empty() {
            return;
        }

        let mut started = Vec::with_capacity(cfg.peers.len());
        for peer in cfg.peers {
            let client = PeerClient::start(
                peer.clone(),
                cfg.token.clone(),
                cfg.display_name.clone(),
                Arc::clone(self),
            );
            started.push((peer, client));
        }

        let mut lc = self.lifecycle.lock().await;
        lc.peers = started;
    }

    async fn start_tick_task(self: &Arc<Self>) {
        let (old_tx, old_task) = {
            let mut lc = self.lifecycle.lock().await;
            (lc.tick_shutdown.take(), lc.tick_task.take())
        };
        if let Some(tx) = old_tx {
            let _ = tx.send(());
        }
        if let Some(h) = old_task {
            h.abort();
            let _ = h.await;
        }

        let (tx, mut rx) = oneshot::channel();
        let hub = Arc::clone(self);
        let task = tokio::spawn(async move {
            let mut interval = tokio::time::interval(HUB_TICK_INTERVAL);
            interval.tick().await;
            loop {
                tokio::select! {
                    _ = &mut rx => return,
                    _ = interval.tick() => {
                        hub.refresh_local_sessions().await;
                        hub.expire_stale_tasks();
                        hub.emit_snapshot_if_changed(false);
                    }
                }
            }
        });

        let mut lc = self.lifecycle.lock().await;
        lc.tick_shutdown = Some(tx);
        lc.tick_task = Some(task);
    }

    async fn find_peer_client(&self, peer_name: &str) -> VeloxResult<Arc<PeerClient>> {
        let lc = self.lifecycle.lock().await;
        lc.peers
            .iter()
            .find(|(p, _)| p.name == peer_name)
            .map(|(_, c)| Arc::clone(c))
            .ok_or_else(|| VeloxError::InvalidInput(format!("Unknown peer: {peer_name}")))
    }

    async fn refresh_local_sessions(&self) {
        let raw_sessions = self.sessions.list_sessions();
        let live: Vec<SessionRecord> = raw_sessions
            .into_iter()
            .filter(|s| is_live_status(s.status))
            .collect();

        // Determine which sessions need summary recomputation (cached 5 s).
        let to_compute: Vec<(String, crate::domain::agent::AgentKind, Option<String>, Option<String>)> = {
            let cache = self.summary_cache.lock().unwrap_or_else(|e| e.into_inner());
            live.iter()
                .filter_map(|s| {
                    let hit = cache.get(&s.id).filter(|entry| {
                        entry.fetched_at.elapsed() < SUMMARY_CACHE_TTL
                            && entry.agent_session_id == s.agent_session_id
                    });
                    if hit.is_some() {
                        None
                    } else {
                        Some((
                            s.id.clone(),
                            s.agent_kind,
                            s.profile.clone(),
                            s.agent_session_id.clone(),
                        ))
                    }
                })
                .collect()
        };

        if !to_compute.is_empty() {
            let computed = tokio::task::spawn_blocking(move || {
                to_compute
                    .into_iter()
                    .map(|(id, kind, profile, agent_sid)| {
                        let summary =
                            last_action_summary(kind, profile.as_deref(), agent_sid.as_deref());
                        (id, agent_sid, summary)
                    })
                    .collect::<Vec<_>>()
            })
            .await
            .unwrap_or_default();

            let mut cache = self.summary_cache.lock().unwrap_or_else(|e| e.into_inner());
            let now = Instant::now();
            for (id, agent_sid, summary) in computed {
                cache.insert(
                    id,
                    SummaryCacheEntry {
                        fetched_at: now,
                        agent_session_id: agent_sid,
                        summary,
                    },
                );
            }
        }

        let live_ids: HashSet<String> = live.iter().map(|s| s.id.clone()).collect();
        let mut next_local = Vec::with_capacity(live.len());
        let mut next_shared_ids = HashSet::new();

        {
            let mut cache = self.summary_cache.lock().unwrap_or_else(|e| e.into_inner());
            cache.retain(|id, _| live_ids.contains(id));

            for s in live {
                let (cols, rows) = self
                    .sessions
                    .pty_size(&s.id)
                    .unwrap_or((DEFAULT_PTY_COLS, DEFAULT_PTY_ROWS));
                let summary = cache.get(&s.id).and_then(|e| e.summary.clone());
                if s.shared {
                    next_shared_ids.insert(s.id.clone());
                }
                next_local.push(LocalSessionInfo {
                    info: RemoteSessionInfo {
                        id: s.id,
                        title: s.title,
                        agent_kind: s.agent_kind,
                        active_agent: s.active_agent,
                        status: s.status,
                        folder: cwd_basename(&s.cwd),
                        last_active_at: s.last_active_at,
                        summary,
                        cols,
                        rows,
                    },
                    shared: s.shared,
                });
            }
        }

        let prev_shared: Vec<RemoteSessionInfo> = {
            self.local_sessions
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .filter(|s| s.shared)
                .map(|s| s.info.clone())
                .collect()
        };

        let next_shared: Vec<RemoteSessionInfo> = next_local
            .iter()
            .filter(|s| s.shared)
            .map(|s| s.info.clone())
            .collect();

        let prev_shared_ids: HashSet<String> = {
            self.shared_live_ids
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .clone()
        };

        *self
            .local_sessions
            .write()
            .unwrap_or_else(|e| e.into_inner()) = next_local;
        *self
            .shared_live_ids
            .write()
            .unwrap_or_else(|e| e.into_inner()) = next_shared_ids.clone();

        // Notify subscribers of sessions that were unshared / slept / closed.
        for gone_id in prev_shared_ids.difference(&next_shared_ids) {
            self.notify_session_gone(gone_id);
        }

        if prev_shared != next_shared {
            self.broadcast_to_connections(ServerMsg::Sessions {
                sessions: next_shared,
            });
        }
    }

    fn expire_stale_tasks(&self) {
        let mut expired_notifications: Vec<(Option<u64>, String)> = Vec::new();
        {
            let mut guard = self.tickets.lock().unwrap_or_else(|e| e.into_inner());
            let now_str = Utc::now().to_rfc3339();
            for entry in guard.iter_mut() {
                if entry.ticket.state == TaskState::Pending
                    && entry.created_instant.elapsed() >= TASK_EXPIRY
                {
                    entry.ticket.state = TaskState::Expired;
                    entry.ticket.message = Some("Expired after 30 minutes".to_string());
                    entry.ticket.updated_at = now_str.clone();
                    expired_notifications
                        .push((entry.requester_conn_id, entry.ticket.request_id.clone()));
                }
            }
        }

        if !expired_notifications.is_empty() {
            let msg = Some("Expired after 30 minutes".to_string());
            for (conn_id, req_id) in expired_notifications {
                self.notify_requester(conn_id, &req_id, TaskState::Expired, msg.clone());
            }
            self.emit_tasks_updated();
        }
    }

    pub(crate) fn config_snapshot(&self) -> TeamConfig {
        self.config
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub(crate) fn token_snapshot(&self) -> String {
        self.config
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .token
            .clone()
    }

    pub(crate) fn display_name(&self) -> String {
        self.config
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .display_name
            .clone()
    }

    pub(crate) fn shared_sessions(&self) -> Vec<RemoteSessionInfo> {
        let cached = self
            .local_sessions
            .read()
            .unwrap_or_else(|e| e.into_inner());
        if !cached.is_empty() {
            return cached
                .iter()
                .filter(|s| s.shared)
                .map(|s| s.info.clone())
                .collect();
        }
        drop(cached);

        self.sessions
            .list_sessions()
            .into_iter()
            .filter(|s| is_live_status(s.status) && s.shared)
            .map(|s| {
                let (cols, rows) = self
                    .sessions
                    .pty_size(&s.id)
                    .unwrap_or((DEFAULT_PTY_COLS, DEFAULT_PTY_ROWS));
                RemoteSessionInfo {
                    id: s.id,
                    title: s.title,
                    agent_kind: s.agent_kind,
                    active_agent: s.active_agent,
                    status: s.status,
                    folder: cwd_basename(&s.cwd),
                    last_active_at: s.last_active_at,
                    summary: None,
                    cols,
                    rows,
                }
            })
            .collect()
    }

    pub(crate) fn register_connection(&self, tx: mpsc::Sender<ServerMsg>) -> u64 {
        let conn_id = self.next_conn_id.fetch_add(1, Ordering::Relaxed);
        self.connections
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                conn_id,
                ConnectionEntry {
                    tx,
                    peer_name: None,
                    task_timestamps: VecDeque::new(),
                },
            );
        conn_id
    }

    pub(crate) fn unregister_connection(&self, conn_id: u64) {
        let had_name = self
            .connections
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&conn_id)
            .and_then(|c| c.peer_name)
            .is_some();

        {
            let mut subs = self.subscribers.lock().unwrap_or_else(|e| e.into_inner());
            subs.lagged.remove(&conn_id);
            subs.by_session.retain(|_, map| {
                map.remove(&conn_id);
                !map.is_empty()
            });
        }

        if had_name {
            self.emit_snapshot_if_changed(false);
        }
    }

    pub(crate) fn set_connection_peer_name(&self, conn_id: u64, peer_name: String) {
        {
            let mut conns = self.connections.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(entry) = conns.get_mut(&conn_id) {
                entry.peer_name = Some(peer_name);
            }
        }
        self.emit_snapshot_if_changed(false);
    }

    pub(crate) fn handle_subscribe(&self, conn_id: u64, session_id: &str) {
        let tx = {
            let conns = self.connections.lock().unwrap_or_else(|e| e.into_inner());
            match conns.get(&conn_id) {
                Some(c) => c.tx.clone(),
                None => return,
            }
        };

        let is_shared_live = self
            .sessions
            .list_sessions()
            .into_iter()
            .any(|s| s.id == session_id && is_live_status(s.status) && s.shared);

        if !is_shared_live {
            let _ = tx.try_send(ServerMsg::SessionGone {
                session_id: session_id.to_string(),
            });
            return;
        }

        // Register buffering subscriber BEFORE reading scrollback so no chunk is lost or duplicated.
        {
            let mut subs = self.subscribers.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(set) = subs.lagged.get_mut(&conn_id) {
                set.remove(session_id);
            }
            subs.by_session
                .entry(session_id.to_string())
                .or_default()
                .insert(
                    conn_id,
                    SubscriberEntry {
                        tx: tx.clone(),
                        phase: SubPhase::Buffering(Vec::new()),
                    },
                );
        }

        let (bytes, pos) = self.sessions.get_scrollback_with_pos(session_id);
        let (cols, rows) = self
            .sessions
            .pty_size(session_id)
            .unwrap_or((DEFAULT_PTY_COLS, DEFAULT_PTY_ROWS));
        let tail = if bytes.len() > MAX_SNAPSHOT_BYTES {
            &bytes[bytes.len() - MAX_SNAPSHOT_BYTES..]
        } else {
            &bytes
        };
        let snapshot_msg = ServerMsg::Snapshot {
            session_id: session_id.to_string(),
            generation: pos.generation,
            end_offset: pos.end_offset,
            data_b64: BASE64.encode(tail),
            cols,
            rows,
        };

        let mut subs = self.subscribers.lock().unwrap_or_else(|e| e.into_inner());
        let Some(entry) = subs
            .by_session
            .get_mut(session_id)
            .and_then(|m| m.get_mut(&conn_id))
        else {
            return;
        };

        let buffered = match &mut entry.phase {
            SubPhase::Buffering(buf) => std::mem::take(buf),
            SubPhase::Live { .. } => return,
        };

        match entry.tx.try_send(snapshot_msg) {
            Ok(()) => {
                entry.phase = SubPhase::Live {
                    snap_generation: pos.generation,
                    snap_end_offset: pos.end_offset,
                };
                for (chunk_pos, msg) in buffered {
                    if chunk_pos.generation != pos.generation
                        || chunk_pos.end_offset > pos.end_offset
                    {
                        if let Err(mpsc::error::TrySendError::Full(_)) = entry.tx.try_send(msg) {
                            if let Some(m) = subs.by_session.get_mut(session_id) {
                                m.remove(&conn_id);
                            }
                            subs.lagged
                                .entry(conn_id)
                                .or_default()
                                .insert(session_id.to_string());
                            return;
                        }
                    }
                }
            }
            Err(mpsc::error::TrySendError::Full(_)) => {
                if let Some(m) = subs.by_session.get_mut(session_id) {
                    m.remove(&conn_id);
                }
                subs.lagged
                    .entry(conn_id)
                    .or_default()
                    .insert(session_id.to_string());
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                if let Some(m) = subs.by_session.get_mut(session_id) {
                    m.remove(&conn_id);
                }
            }
        }
    }

    pub(crate) fn handle_unsubscribe(&self, conn_id: u64, session_id: &str) {
        let mut subs = self.subscribers.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(set) = subs.lagged.get_mut(&conn_id) {
            set.remove(session_id);
        }
        if let Some(m) = subs.by_session.get_mut(session_id) {
            m.remove(&conn_id);
            if m.is_empty() {
                subs.by_session.remove(session_id);
            }
        }
    }

    pub(crate) fn flush_lagged_resubscribes(&self, conn_id: u64) {
        let sessions_to_resub: Vec<String> = {
            let mut subs = self.subscribers.lock().unwrap_or_else(|e| e.into_inner());
            subs.lagged
                .remove(&conn_id)
                .map(|set| set.into_iter().collect())
                .unwrap_or_default()
        };
        for session_id in sessions_to_resub {
            self.handle_subscribe(conn_id, &session_id);
        }
    }

    pub(crate) fn handle_incoming_task(
        &self,
        conn_id: u64,
        request_id: String,
        session_id: String,
        from_session: Option<String>,
        text: String,
    ) {
        let (tx, from_name, rate_limited) = {
            let mut conns = self.connections.lock().unwrap_or_else(|e| e.into_inner());
            let Some(conn) = conns.get_mut(&conn_id) else {
                return;
            };
            let now = Instant::now();
            while conn
                .task_timestamps
                .front()
                .is_some_and(|t| now.duration_since(*t) >= RATE_LIMIT_WINDOW)
            {
                conn.task_timestamps.pop_front();
            }
            let limited = conn.task_timestamps.len() >= RATE_LIMIT_MAX_TASKS;
            if !limited {
                conn.task_timestamps.push_back(now);
            }
            (
                conn.tx.clone(),
                conn.peer_name.clone().unwrap_or_else(|| "Peer".to_string()),
                limited,
            )
        };

        if rate_limited {
            let _ = tx.try_send(ServerMsg::TaskUpdate {
                request_id,
                state: TaskState::Failed,
                message: Some("Rate limit exceeded (max 20 requests/min)".to_string()),
            });
            return;
        }

        if text.is_empty() || text.len() > MAX_TASK_TEXT_BYTES {
            let _ = tx.try_send(ServerMsg::TaskUpdate {
                request_id,
                state: TaskState::Failed,
                message: Some(format!(
                    "Task text must be 1..={MAX_TASK_TEXT_BYTES} bytes"
                )),
            });
            return;
        }

        let target = self
            .sessions
            .list_sessions()
            .into_iter()
            .find(|s| s.id == session_id && is_live_status(s.status) && s.shared);

        let Some(session) = target else {
            let _ = tx.try_send(ServerMsg::TaskUpdate {
                request_id,
                state: TaskState::Failed,
                message: Some("Session is unavailable or not shared".to_string()),
            });
            return;
        };

        let now_str = Utc::now().to_rfc3339();
        let ticket = TaskTicket {
            request_id: request_id.clone(),
            direction: TaskDirection::Incoming,
            peer: Some(from_name.clone()),
            session_id,
            session_title: Some(session.title),
            from_name,
            from_session,
            text,
            state: TaskState::Pending,
            message: None,
            created_at: now_str.clone(),
            updated_at: now_str,
        };

        self.insert_ticket(ticket, Some(conn_id));
        self.emit_tasks_updated();
    }

    pub(crate) fn handle_read_request(
        &self,
        conn_id: u64,
        request_id: String,
        session_id: String,
        max_lines: u32,
    ) {
        let tx = {
            let conns = self.connections.lock().unwrap_or_else(|e| e.into_inner());
            match conns.get(&conn_id) {
                Some(c) => c.tx.clone(),
                None => return,
            }
        };

        let is_shared_live = self
            .sessions
            .list_sessions()
            .into_iter()
            .any(|s| s.id == session_id && is_live_status(s.status) && s.shared);

        if !is_shared_live {
            let _ = tx.try_send(ServerMsg::ReadResult {
                request_id,
                text: None,
                error: Some("Session is unavailable or not shared".to_string()),
            });
            return;
        }

        let text = render_session_text(self.sessions.as_ref(), &session_id, max_lines);
        let _ = tx.try_send(ServerMsg::ReadResult {
            request_id,
            text: Some(text),
            error: None,
        });
    }

    pub(crate) fn set_server_state(
        &self,
        listen_addr: Option<SocketAddr>,
        server_error: Option<String>,
        tailscale_ip: Option<Ipv4Addr>,
    ) {
        {
            let mut st = self.server_state.write().unwrap_or_else(|e| e.into_inner());
            st.listen_addr = listen_addr;
            st.server_error = server_error;
            if tailscale_ip.is_some() || listen_addr.is_none() {
                st.tailscale_ip = tailscale_ip;
            }
        }
        self.emit_snapshot_if_changed(false);
    }

    pub(crate) fn set_peer_hello(&self, peer_name: &str, display_name: String) {
        {
            let mut ps = self.peer_states.write().unwrap_or_else(|e| e.into_inner());
            let entry = ps.entry(peer_name.to_string()).or_default();
            entry.online = true;
            entry.display_name = Some(display_name);
            entry.last_error = None;
        }
        self.emit_snapshot_if_changed(false);
    }

    pub(crate) fn set_peer_sessions(&self, peer_name: &str, sessions: Vec<RemoteSessionInfo>) {
        {
            let mut ps = self.peer_states.write().unwrap_or_else(|e| e.into_inner());
            let entry = ps.entry(peer_name.to_string()).or_default();
            entry.online = true;
            entry.sessions = sessions;
        }
        self.emit_snapshot_if_changed(false);
    }

    pub(crate) fn set_peer_offline(&self, peer_name: &str, error: Option<String>) {
        {
            let mut ps = self.peer_states.write().unwrap_or_else(|e| e.into_inner());
            let entry = ps.entry(peer_name.to_string()).or_default();
            entry.online = false;
            entry.sessions.clear();
            entry.last_error = error;
        }
        self.emit_snapshot_if_changed(false);
    }

    pub(crate) fn set_peer_error(&self, peer_name: &str, error: String) {
        {
            let mut ps = self.peer_states.write().unwrap_or_else(|e| e.into_inner());
            let entry = ps.entry(peer_name.to_string()).or_default();
            entry.last_error = Some(error);
        }
        self.emit_snapshot_if_changed(false);
    }

    pub(crate) fn handle_remote_task_update(
        &self,
        _peer_name: &str,
        request_id: String,
        state: TaskState,
        message: Option<String>,
    ) {
        if self
            .update_ticket_state(&request_id, state, message)
            .is_some()
        {
            self.emit_tasks_updated();
        }
    }

    pub(crate) fn emit_event<T: Serialize>(&self, event: &str, payload: &T) {
        let sink = self
            .event_sink
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if let Some(sink) = sink {
            if let Ok(json) = serde_json::to_value(payload) {
                sink.emit_json(event, json);
            }
        }
    }

    fn emit_snapshot_if_changed(&self, force: bool) {
        let snap = self.snapshot();
        let Ok(json) = serde_json::to_value(&snap) else {
            return;
        };
        let should_emit = {
            let mut last = self
                .last_emitted_snapshot
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if force || *last != json {
                *last = json.clone();
                true
            } else {
                false
            }
        };
        if should_emit {
            let sink = self
                .event_sink
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            if let Some(sink) = sink {
                sink.emit_json(EVT_TEAM_UPDATED, json);
            }
        }
    }

    fn emit_snapshot_always(&self) {
        self.emit_snapshot_if_changed(true);
    }

    fn emit_tasks_updated(&self) {
        let tasks = self.list_tasks();
        self.emit_event(EVT_TEAM_TASKS_UPDATED, &tasks);
    }

    fn insert_ticket(&self, ticket: TaskTicket, requester_conn_id: Option<u64>) {
        let seq = self.next_ticket_seq.fetch_add(1, Ordering::Relaxed);
        let mut guard = self.tickets.lock().unwrap_or_else(|e| e.into_inner());
        guard.push(StoredTicket {
            seq,
            ticket,
            created_instant: Instant::now(),
            requester_conn_id,
        });
        Self::prune_tickets(&mut guard);
    }

    fn remove_ticket(&self, request_id: &str) {
        let mut guard = self.tickets.lock().unwrap_or_else(|e| e.into_inner());
        guard.retain(|t| t.ticket.request_id != request_id);
    }

    fn prune_tickets(tickets: &mut Vec<StoredTicket>) {
        while tickets.len() > MAX_TICKETS_RETAINED {
            let drop_idx = tickets
                .iter()
                .enumerate()
                .filter(|(_, t)| {
                    !matches!(t.ticket.state, TaskState::Pending | TaskState::Accepted)
                })
                .min_by_key(|(_, t)| t.seq)
                .map(|(idx, _)| idx)
                .or_else(|| tickets.iter().enumerate().min_by_key(|(_, t)| t.seq).map(|(i, _)| i));
            match drop_idx {
                Some(idx) => {
                    tickets.remove(idx);
                }
                None => break,
            }
        }
    }

    fn recent_tickets_from(tickets: &[StoredTicket]) -> Vec<TaskTicket> {
        let mut sorted: Vec<&StoredTicket> = tickets.iter().collect();
        sorted.sort_by_key(|a| std::cmp::Reverse(a.seq));
        sorted
            .into_iter()
            .take(MAX_TICKETS_RETURNED)
            .map(|s| s.ticket.clone())
            .collect()
    }

    fn update_ticket_state(
        &self,
        request_id: &str,
        state: TaskState,
        message: Option<String>,
    ) -> Option<TaskTicket> {
        let mut guard = self.tickets.lock().unwrap_or_else(|e| e.into_inner());
        let entry = guard
            .iter_mut()
            .find(|t| t.ticket.request_id == request_id)?;
        entry.ticket.state = state;
        entry.ticket.message = message;
        entry.ticket.updated_at = Utc::now().to_rfc3339();
        Some(entry.ticket.clone())
    }

    fn notify_requester(
        &self,
        conn_id: Option<u64>,
        request_id: &str,
        state: TaskState,
        message: Option<String>,
    ) {
        let Some(cid) = conn_id else {
            return;
        };
        let tx = self
            .connections
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&cid)
            .map(|c| c.tx.clone());
        if let Some(tx) = tx {
            let _ = tx.try_send(ServerMsg::TaskUpdate {
                request_id: request_id.to_string(),
                state,
                message,
            });
        }
    }

    fn notify_session_gone(&self, session_id: &str) {
        let removed_subscribers: Vec<mpsc::Sender<ServerMsg>> = {
            let mut subs = self.subscribers.lock().unwrap_or_else(|e| e.into_inner());
            for set in subs.lagged.values_mut() {
                set.remove(session_id);
            }
            subs.by_session
                .remove(session_id)
                .map(|m| m.into_values().map(|e| e.tx).collect())
                .unwrap_or_default()
        };

        for tx in removed_subscribers {
            let _ = tx.try_send(ServerMsg::SessionGone {
                session_id: session_id.to_string(),
            });
        }
    }

    fn broadcast_to_connections(&self, msg: ServerMsg) {
        let txs: Vec<mpsc::Sender<ServerMsg>> = {
            self.connections
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .values()
                .map(|c| c.tx.clone())
                .collect()
        };
        for tx in txs {
            let _ = tx.try_send(msg.clone());
        }
    }

    fn is_session_shared_and_live(&self, session_id: &str) -> bool {
        if self
            .shared_live_ids
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .contains(session_id)
        {
            return true;
        }
        self.sessions
            .list_sessions()
            .into_iter()
            .any(|s| s.id == session_id && is_live_status(s.status) && s.shared)
    }
}

impl SessionTap for TeamHub {
    fn on_output(&self, session_id: &str, data: &[u8], pos: StreamPos) {
        // Cheap fast-path: skip lock/encoding completely if no one is subscribed to this session.
        {
            let subs = self.subscribers.lock().unwrap_or_else(|e| e.into_inner());
            if !subs
                .by_session
                .get(session_id)
                .is_some_and(|m| !m.is_empty())
            {
                return;
            }
        }

        if !self.is_session_shared_and_live(session_id) {
            self.notify_session_gone(session_id);
            return;
        }

        let msg = ServerMsg::Output {
            session_id: session_id.to_string(),
            generation: pos.generation,
            end_offset: pos.end_offset,
            data_b64: BASE64.encode(data),
        };

        let mut subs = self.subscribers.lock().unwrap_or_else(|e| e.into_inner());
        let mut lagged_conns = Vec::new();
        let mut dead_conns = Vec::new();

        if let Some(map) = subs.by_session.get_mut(session_id) {
            for (&conn_id, entry) in map.iter_mut() {
                match &mut entry.phase {
                    SubPhase::Buffering(buf) => {
                        buf.push((pos, msg.clone()));
                    }
                    SubPhase::Live {
                        snap_generation,
                        snap_end_offset,
                    } => {
                        if pos.generation == *snap_generation && pos.end_offset <= *snap_end_offset
                        {
                            continue;
                        }
                        match entry.tx.try_send(msg.clone()) {
                            Ok(()) => {}
                            Err(mpsc::error::TrySendError::Full(_)) => {
                                lagged_conns.push(conn_id);
                            }
                            Err(mpsc::error::TrySendError::Closed(_)) => {
                                dead_conns.push(conn_id);
                            }
                        }
                    }
                }
            }

            for conn_id in &lagged_conns {
                map.remove(conn_id);
            }
            for conn_id in &dead_conns {
                map.remove(conn_id);
            }
            if map.is_empty() {
                subs.by_session.remove(session_id);
            }
        }

        for conn_id in lagged_conns {
            subs.lagged
                .entry(conn_id)
                .or_default()
                .insert(session_id.to_string());
        }
    }

    fn on_resize(&self, session_id: &str, cols: u16, rows: u16) {
        {
            let subs = self.subscribers.lock().unwrap_or_else(|e| e.into_inner());
            if !subs
                .by_session
                .get(session_id)
                .is_some_and(|m| !m.is_empty())
            {
                return;
            }
        }

        if !self.is_session_shared_and_live(session_id) {
            self.notify_session_gone(session_id);
            return;
        }

        let msg = ServerMsg::Resize {
            session_id: session_id.to_string(),
            cols,
            rows,
        };

        let mut subs = self.subscribers.lock().unwrap_or_else(|e| e.into_inner());
        let mut lagged_conns = Vec::new();
        let mut dead_conns = Vec::new();

        if let Some(map) = subs.by_session.get_mut(session_id) {
            for (&conn_id, entry) in map.iter_mut() {
                if let SubPhase::Live { .. } = entry.phase {
                    match entry.tx.try_send(msg.clone()) {
                        Ok(()) => {}
                        Err(mpsc::error::TrySendError::Full(_)) => lagged_conns.push(conn_id),
                        Err(mpsc::error::TrySendError::Closed(_)) => dead_conns.push(conn_id),
                    }
                }
            }
            for conn_id in &lagged_conns {
                map.remove(conn_id);
            }
            for conn_id in &dead_conns {
                map.remove(conn_id);
            }
            if map.is_empty() {
                subs.by_session.remove(session_id);
            }
        }

        for conn_id in lagged_conns {
            subs.lagged
                .entry(conn_id)
                .or_default()
                .insert(session_id.to_string());
        }
    }
}

impl TeamApi for TeamHub {
    fn snapshot(&self) -> TeamSnapshot {
        let cfg = self.config_snapshot();
        let srv = self
            .server_state
            .read()
            .unwrap_or_else(|e| e.into_inner());
        let peer_states = self
            .peer_states
            .read()
            .unwrap_or_else(|e| e.into_inner());

        let peers = cfg
            .peers
            .iter()
            .map(|p| {
                let st = peer_states.get(&p.name).cloned().unwrap_or_default();
                PeerStatus {
                    name: p.name.clone(),
                    host: p.host.clone(),
                    port: p.port,
                    online: st.online,
                    display_name: st.display_name,
                    last_error: st.last_error,
                    sessions: st.sessions,
                }
            })
            .collect();

        let local_sessions = {
            let cached = self
                .local_sessions
                .read()
                .unwrap_or_else(|e| e.into_inner());
            if !cached.is_empty() {
                cached.clone()
            } else {
                drop(cached);
                self.sessions
                    .list_sessions()
                    .into_iter()
                    .filter(|s| is_live_status(s.status))
                    .map(|s| {
                        let (cols, rows) = self
                            .sessions
                            .pty_size(&s.id)
                            .unwrap_or((DEFAULT_PTY_COLS, DEFAULT_PTY_ROWS));
                        LocalSessionInfo {
                            info: RemoteSessionInfo {
                                id: s.id,
                                title: s.title,
                                agent_kind: s.agent_kind,
                                active_agent: s.active_agent,
                                status: s.status,
                                folder: cwd_basename(&s.cwd),
                                last_active_at: s.last_active_at,
                                summary: None,
                                cols,
                                rows,
                            },
                            shared: s.shared,
                        }
                    })
                    .collect()
            }
        };

        let watchers = self
            .connections
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .filter_map(|c| c.peer_name.clone())
            .collect();

        TeamSnapshot {
            enabled: cfg.enabled,
            display_name: cfg.display_name,
            listen_addr: srv.listen_addr.map(|a| a.to_string()),
            server_error: srv.server_error.clone(),
            tailscale_ip: srv.tailscale_ip.map(|ip| ip.to_string()),
            peers,
            local_sessions,
            watchers,
        }
    }

    fn send_task(&self, req: SendTaskRequest) -> BoxFuture<'_, VeloxResult<TaskTicket>> {
        Box::pin(async move {
            if req.text.is_empty() || req.text.len() > MAX_TASK_TEXT_BYTES {
                return Err(VeloxError::InvalidInput(format!(
                    "Task text must be 1..={MAX_TASK_TEXT_BYTES} bytes"
                )));
            }

            let cfg = self.config_snapshot();
            let request_id = Uuid::new_v4().to_string();
            let now_str = Utc::now().to_rfc3339();
            let target_peer = req.peer.as_deref().filter(|p| !p.trim().is_empty());

            match target_peer {
                None => {
                    let target = self
                        .sessions
                        .list_sessions()
                        .into_iter()
                        .find(|s| s.id == req.session_id && is_live_status(s.status))
                        .ok_or_else(|| VeloxError::SessionNotFound(req.session_id.clone()))?;

                    let ticket = TaskTicket {
                        request_id: request_id.clone(),
                        direction: TaskDirection::Incoming,
                        peer: None,
                        session_id: req.session_id,
                        session_title: Some(target.title),
                        from_name: cfg.display_name,
                        from_session: req.from_session,
                        text: req.text,
                        state: TaskState::Pending,
                        message: None,
                        created_at: now_str.clone(),
                        updated_at: now_str,
                    };

                    self.insert_ticket(ticket.clone(), None);
                    self.emit_tasks_updated();

                    if cfg.auto_approve_local {
                        return self.respond_task(&request_id, true).await;
                    }
                    Ok(ticket)
                }
                Some(peer_name) => {
                    if !cfg.enabled || cfg.token.trim().is_empty() {
                        return Err(VeloxError::InvalidInput(
                            "Team mode is disabled or token is empty".to_string(),
                        ));
                    }

                    let client = self.find_peer_client(peer_name).await?;
                    let session_title = self
                        .peer_states
                        .read()
                        .unwrap_or_else(|e| e.into_inner())
                        .get(peer_name)
                        .and_then(|st| st.sessions.iter().find(|s| s.id == req.session_id))
                        .map(|s| s.title.clone());

                    let ticket = TaskTicket {
                        request_id: request_id.clone(),
                        direction: TaskDirection::Outgoing,
                        peer: Some(peer_name.to_string()),
                        session_id: req.session_id.clone(),
                        session_title,
                        from_name: cfg.display_name,
                        from_session: req.from_session.clone(),
                        text: req.text.clone(),
                        state: TaskState::Pending,
                        message: None,
                        created_at: now_str.clone(),
                        updated_at: now_str,
                    };

                    self.insert_ticket(ticket.clone(), None);
                    if let Err(err) = client.send_task(
                        request_id.clone(),
                        req.session_id,
                        req.from_session,
                        req.text,
                    ) {
                        self.remove_ticket(&request_id);
                        return Err(err);
                    }

                    self.emit_tasks_updated();
                    Ok(ticket)
                }
            }
        })
    }

    fn task_status(&self, request_id: &str) -> Option<TaskTicket> {
        self.tickets
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .find(|t| t.ticket.request_id == request_id)
            .map(|t| t.ticket.clone())
    }

    fn read_session(
        &self,
        target: SessionTarget,
        max_lines: u32,
    ) -> BoxFuture<'_, VeloxResult<String>> {
        Box::pin(async move {
            let lines = max_lines.clamp(1, MAX_READ_LINES);
            let target_peer = target.peer.as_deref().filter(|p| !p.trim().is_empty());

            match target_peer {
                None => {
                    let exists = self
                        .sessions
                        .list_sessions()
                        .into_iter()
                        .any(|s| s.id == target.session_id && is_live_status(s.status));
                    if !exists {
                        return Err(VeloxError::SessionNotFound(target.session_id));
                    }
                    Ok(render_session_text(
                        self.sessions.as_ref(),
                        &target.session_id,
                        lines,
                    ))
                }
                Some(peer_name) => {
                    let client = self.find_peer_client(peer_name).await?;
                    client.read_session(&target.session_id, lines).await
                }
            }
        })
    }
}

fn spawn_on_runtime<F>(fut: F)
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        handle.spawn(fut);
    } else {
        tauri::async_runtime::spawn(fut);
    }
}

fn is_live_status(status: SessionStatus) -> bool {
    !matches!(status, SessionStatus::Sleeping | SessionStatus::Exited)
}

fn cwd_basename(cwd: &str) -> String {
    Path::new(cwd)
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("workspace")
        .to_string()
}

/// Renders the last `max_lines` lines of scrollback + visible screen for `session_id` via `vt100`.
pub fn render_session_text(
    sessions: &dyn LocalSessions,
    session_id: &str,
    max_lines: u32,
) -> String {
    let (bytes, _) = sessions.get_scrollback_with_pos(session_id);
    let tail = if bytes.len() > MAX_READ_SCROLLBACK_BYTES {
        &bytes[bytes.len() - MAX_READ_SCROLLBACK_BYTES..]
    } else {
        &bytes
    };

    let (raw_cols, raw_rows) = sessions
        .pty_size(session_id)
        .unwrap_or((DEFAULT_PTY_COLS, DEFAULT_PTY_ROWS));
    let cols = raw_cols.max(10);
    let rows = raw_rows.max(2);

    let mut parser = vt100::Parser::new(rows, cols, VT100_SCROLLBACK_LINES);
    parser.process(tail);

    parser.screen_mut().set_scrollback(usize::MAX);
    let actual_sb = parser.screen().scrollback();

    let mut lines = Vec::new();
    let mut remaining_sb = actual_sb;
    while remaining_sb > 0 {
        parser.screen_mut().set_scrollback(remaining_sb);
        let take = remaining_sb.min(usize::from(rows));
        for line in parser.screen().rows(0, cols).take(take) {
            lines.push(line.trim_end().to_string());
        }
        remaining_sb -= take;
    }

    parser.screen_mut().set_scrollback(0);
    for line in parser.screen().rows(0, cols) {
        lines.push(line.trim_end().to_string());
    }

    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }

    let keep = (max_lines as usize).clamp(1, MAX_READ_LINES as usize);
    let start = lines.len().saturating_sub(keep);
    lines[start..].join("\n")
}
