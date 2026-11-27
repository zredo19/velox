use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::Notify;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::AUTHORIZATION;
use tokio_tungstenite::tungstenite::http::{HeaderValue, StatusCode};
use tokio_tungstenite::tungstenite::Error as WsError;
use tokio_tungstenite::tungstenite::Message;
use velox_lib::domain::agent::AgentKind;
use velox_lib::domain::config::{PeerConfig, TeamConfig};
use velox_lib::domain::error::{VeloxError, VeloxResult};
use velox_lib::domain::session::{SessionRecord, SessionStatus};
use velox_lib::pty::manager::StreamPos;
use velox_lib::pty::scrollback::RingScrollback;
use velox_lib::session::manager::SessionTap;
use velox_lib::team::api::{
    LocalMessageStatus, SendLocalMessageRequest, SendTaskRequest, SessionTarget, TaskDirection,
    TeamApi, TeamPtyOutputEvent, TeamPtySnapshotEvent, TeamSessionGoneEvent,
    EVT_TEAM_LOCAL_MESSAGE, EVT_TEAM_PTY_OUTPUT, EVT_TEAM_PTY_SNAPSHOT, EVT_TEAM_SESSION_GONE,
};
use velox_lib::team::delivery::build_task_input;
use velox_lib::team::hub::{EventSink, LocalSessions, TeamHub};
use velox_lib::team::protocol::{
    ClientMsg, ServerMsg, TaskState, MAX_TASK_TEXT_BYTES, PROTOCOL_VERSION, WS_PATH,
};

struct FakeLocalSessions {
    sessions: Mutex<Vec<SessionRecord>>,
    scrollbacks: Mutex<HashMap<String, RingScrollback>>,
    sizes: Mutex<HashMap<String, (u16, u16)>>,
    written: Mutex<HashMap<String, Vec<Vec<u8>>>>,
    woken: Mutex<Vec<String>>,
}

impl FakeLocalSessions {
    fn new(initial: Vec<SessionRecord>) -> Self {
        let mut sizes = HashMap::new();
        let mut scrollbacks = HashMap::new();
        for s in &initial {
            sizes.insert(s.id.clone(), (100, 30));
            scrollbacks.insert(s.id.clone(), RingScrollback::new(256 * 1024));
        }
        Self {
            sessions: Mutex::new(initial),
            scrollbacks: Mutex::new(scrollbacks),
            sizes: Mutex::new(sizes),
            written: Mutex::new(HashMap::new()),
            woken: Mutex::new(Vec::new()),
        }
    }

    fn set_status(&self, session_id: &str, status: SessionStatus) {
        let mut guard = self.sessions.lock().unwrap();
        if let Some(s) = guard.iter_mut().find(|s| s.id == session_id) {
            s.status = status;
        }
    }

    fn clear_scrollback(&self, session_id: &str) {
        self.scrollbacks
            .lock()
            .unwrap()
            .insert(session_id.to_string(), RingScrollback::new(256 * 1024));
    }

    fn push_output(&self, hub: &TeamHub, session_id: &str, data: &[u8]) -> StreamPos {
        let end_offset = {
            let mut map = self.scrollbacks.lock().unwrap();
            let sb = map
                .entry(session_id.to_string())
                .or_insert_with(|| RingScrollback::new(256 * 1024));
            sb.push(data);
            sb.total()
        };
        let pos = StreamPos {
            generation: 1,
            end_offset,
        };
        hub.on_output(session_id, data, pos);
        pos
    }

    fn set_shared(&self, session_id: &str, shared: bool) {
        let mut guard = self.sessions.lock().unwrap();
        if let Some(s) = guard.iter_mut().find(|s| s.id == session_id) {
            s.shared = shared;
        }
    }

    fn written_bytes(&self, session_id: &str) -> Vec<u8> {
        self.written
            .lock()
            .unwrap()
            .get(session_id)
            .map(|chunks| chunks.concat())
            .unwrap_or_default()
    }
}

impl LocalSessions for FakeLocalSessions {
    fn list_sessions(&self) -> Vec<SessionRecord> {
        self.sessions.lock().unwrap().clone()
    }

    fn pty_size(&self, session_id: &str) -> Option<(u16, u16)> {
        self.sizes.lock().unwrap().get(session_id).copied()
    }

    fn get_scrollback_with_pos(&self, session_id: &str) -> (Vec<u8>, StreamPos) {
        let map = self.scrollbacks.lock().unwrap();
        match map.get(session_id) {
            Some(sb) => (
                sb.snapshot(),
                StreamPos {
                    generation: 1,
                    end_offset: sb.total(),
                },
            ),
            None => (
                Vec::new(),
                StreamPos {
                    generation: 0,
                    end_offset: 0,
                },
            ),
        }
    }

    fn write_pty_bytes(&self, session_id: &str, data: &[u8]) -> VeloxResult<()> {
        self.written
            .lock()
            .unwrap()
            .entry(session_id.to_string())
            .or_default()
            .push(data.to_vec());
        Ok(())
    }

    fn is_pty_alive(&self, session_id: &str) -> bool {
        self.sessions
            .lock()
            .unwrap()
            .iter()
            .any(|s| s.id == session_id && !matches!(s.status, SessionStatus::Sleeping | SessionStatus::Exited))
    }

    fn wake_session(
        &self,
        session_id: &str,
        _cols: u16,
        _rows: u16,
    ) -> VeloxResult<SessionRecord> {
        self.woken.lock().unwrap().push(session_id.to_string());
        let mut guard = self.sessions.lock().unwrap();
        let s = guard
            .iter_mut()
            .find(|s| s.id == session_id)
            .ok_or_else(|| VeloxError::SessionNotFound(session_id.to_string()))?;
        s.status = SessionStatus::Running;
        Ok(s.clone())
    }
}

struct FakeEventSink {
    events: Mutex<Vec<(String, Value)>>,
    notify: Notify,
}

impl FakeEventSink {
    fn new() -> Self {
        Self {
            events: Mutex::new(Vec::new()),
            notify: Notify::new(),
        }
    }

    async fn wait_for<F>(&self, timeout: Duration, mut predicate: F) -> Option<Value>
    where
        F: FnMut(&str, &Value) -> bool,
    {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            {
                let guard = self.events.lock().unwrap();
                for (name, val) in guard.iter() {
                    if predicate(name, val) {
                        return Some(val.clone());
                    }
                }
            }
            let now = tokio::time::Instant::now();
            if now >= deadline {
                return None;
            }
            let _ = tokio::time::timeout(deadline - now, self.notify.notified()).await;
        }
    }
}

impl EventSink for FakeEventSink {
    fn emit_json(&self, event: &str, payload: Value) {
        self.events
            .lock()
            .unwrap()
            .push((event.to_string(), payload));
        self.notify.notify_waiters();
    }
}

fn make_session(id: &str, title: &str, shared: bool, status: SessionStatus) -> SessionRecord {
    SessionRecord {
        id: id.to_string(),
        title: title.to_string(),
        agent_kind: AgentKind::Claude,
        active_agent: Some(AgentKind::Claude),
        preset_id: "claude".to_string(),
        profile: None,
        cwd: r"D:\Workspace\velox".to_string(),
        status,
        agent_session_id: None,
        previous_agent_session_ids: Vec::new(),
        wake_as_shell: false,
        pid: Some(1234),
        memory_mb: 120.0,
        created_at: "2026-11-26T00:00:00Z".to_string(),
        last_active_at: "2026-11-26T00:00:00Z".to_string(),
        exit_code: None,
        shared,
    }
}

fn server_config(name: &str, token: &str, port: u16) -> TeamConfig {
    TeamConfig {
        enabled: true,
        display_name: name.to_string(),
        listen_port: port,
        bind_host: "127.0.0.1".to_string(),
        token: token.to_string(),
        auto_approve_local: false,
        peers: Vec::new(),
    }
}

async fn wait_for_server_addr(hub: &TeamHub) -> SocketAddr {
    for _ in 0..100 {
        if let Some(addr) = hub.server_addr() {
            return addr;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!(
        "server did not bind in time; snapshot={:?}",
        hub.snapshot()
    );
}

async fn ws_connect(
    addr: SocketAddr,
    token: Option<&str>,
) -> Result<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    WsError,
> {
    let url = format!("ws://{addr}{WS_PATH}");
    let mut req = url.into_client_request().unwrap();
    if let Some(tok) = token {
        req.headers_mut().insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {tok}")).unwrap(),
        );
    }
    let (stream, _) = connect_async(req).await?;
    Ok(stream)
}

async fn recv_server_msg(
    ws: &mut (impl StreamExt<Item = Result<Message, WsError>> + Unpin),
) -> ServerMsg {
    loop {
        let item = tokio::time::timeout(Duration::from_secs(5), ws.next())
            .await
            .expect("timed out waiting for ServerMsg")
            .expect("stream ended")
            .expect("ws frame ok");
        if let Message::Text(text) = item {
            return serde_json::from_str(text.as_str()).expect("valid ServerMsg");
        }
    }
}

#[tokio::test]
async fn test_1_wrong_or_missing_token_returns_401_no_hello() {
    let sessions = Arc::new(FakeLocalSessions::new(vec![make_session(
        "s1",
        "Shared",
        true,
        SessionStatus::Running,
    )]));
    let sink = Arc::new(FakeEventSink::new());
    let hub = TeamHub::new_with_backends(
        sessions,
        Some(sink),
        server_config("ServerA", "secret-token-123", 0),
    );
    hub.start_headless();
    let addr = wait_for_server_addr(&hub).await;

    // Missing token
    let err = ws_connect(addr, None).await.expect_err("must reject");
    match err {
        WsError::Http(resp) => assert_eq!(resp.status(), StatusCode::UNAUTHORIZED),
        other => panic!("expected HTTP 401, got {other:?}"),
    }

    // Wrong token
    let err = ws_connect(addr, Some("wrong-token"))
        .await
        .expect_err("must reject");
    match err {
        WsError::Http(resp) => assert_eq!(resp.status(), StatusCode::UNAUTHORIZED),
        other => panic!("expected HTTP 401, got {other:?}"),
    }

    hub.shutdown().await;
}

#[tokio::test]
async fn test_2_right_token_sends_hello_and_only_shared_sessions() {
    let sessions = Arc::new(FakeLocalSessions::new(vec![
        make_session("s1", "Public One", true, SessionStatus::Running),
        make_session("s2", "Private Two", false, SessionStatus::Running),
        make_session("s3", "Asleep Shared", true, SessionStatus::Sleeping),
    ]));
    let sink = Arc::new(FakeEventSink::new());
    let hub = TeamHub::new_with_backends(
        sessions,
        Some(sink),
        server_config("Rodolfo", "good-token", 0),
    );
    hub.start_headless();
    let addr = wait_for_server_addr(&hub).await;

    let mut ws = ws_connect(addr, Some("good-token"))
        .await
        .expect("authorized");

    match recv_server_msg(&mut ws).await {
        ServerMsg::Hello {
            protocol,
            peer_name,
            full_control,
        } => {
            assert_eq!(protocol, PROTOCOL_VERSION);
            assert_eq!(peer_name, "Rodolfo");
            assert!(!full_control);
        }
        other => panic!("expected Hello, got {other:?}"),
    }

    match recv_server_msg(&mut ws).await {
        ServerMsg::Sessions { sessions } => {
            assert_eq!(sessions.len(), 1);
            assert_eq!(sessions[0].id, "s1");
            assert_eq!(sessions[0].title, "Public One");
            assert_eq!(sessions[0].folder, "velox");
        }
        other => panic!("expected Sessions, got {other:?}"),
    }

    hub.shutdown().await;
}

#[tokio::test]
async fn test_3_subscribe_sends_snapshot_then_live_output_and_blocks_unshared() {
    let sessions = Arc::new(FakeLocalSessions::new(vec![
        make_session("s1", "Shared", true, SessionStatus::Running),
        make_session("s2", "Private", false, SessionStatus::Running),
    ]));
    let sink = Arc::new(FakeEventSink::new());
    let hub = TeamHub::new_with_backends(
        Arc::clone(&sessions) as Arc<dyn LocalSessions>,
        Some(sink),
        server_config("Rodolfo", "tok", 0),
    );
    hub.start_headless();
    let addr = wait_for_server_addr(&hub).await;

    sessions.push_output(&hub, "s1", b"initial scrollback\r\n");

    let mut ws = ws_connect(addr, Some("tok")).await.unwrap();
    let _hello = recv_server_msg(&mut ws).await;
    let _sessions = recv_server_msg(&mut ws).await;

    // Subscribe to unshared session -> SessionGone, no Snapshot/Output
    let sub_priv = serde_json::to_string(&ClientMsg::Subscribe {
        session_id: "s2".to_string(),
    })
    .unwrap();
    ws.send(Message::text(sub_priv)).await.unwrap();
    match recv_server_msg(&mut ws).await {
        ServerMsg::SessionGone { session_id } => assert_eq!(session_id, "s2"),
        other => panic!("expected SessionGone for private session, got {other:?}"),
    }
    sessions.push_output(&hub, "s2", b"secret bytes");

    // Subscribe to shared session -> Snapshot then live Output
    let sub_pub = serde_json::to_string(&ClientMsg::Subscribe {
        session_id: "s1".to_string(),
    })
    .unwrap();
    ws.send(Message::text(sub_pub)).await.unwrap();

    match recv_server_msg(&mut ws).await {
        ServerMsg::Snapshot {
            session_id,
            data_b64,
            cols,
            rows,
            ..
        } => {
            assert_eq!(session_id, "s1");
            assert_eq!(cols, 100);
            assert_eq!(rows, 30);
            let decoded = BASE64.decode(data_b64).unwrap();
            assert_eq!(decoded, b"initial scrollback\r\n");
        }
        other => panic!("expected Snapshot, got {other:?}"),
    }

    sessions.push_output(&hub, "s1", b"live-chunk-2");
    match recv_server_msg(&mut ws).await {
        ServerMsg::Output {
            session_id,
            data_b64,
            ..
        } => {
            assert_eq!(session_id, "s1");
            let decoded = BASE64.decode(data_b64).unwrap();
            assert_eq!(decoded, b"live-chunk-2");
        }
        other => panic!("expected Output, got {other:?}"),
    }

    hub.shutdown().await;
}

#[tokio::test]
async fn test_4_task_request_pending_accept_delivers_bytes_and_updates() {
    let sessions = Arc::new(FakeLocalSessions::new(vec![make_session(
        "s1",
        "Shared Claude",
        true,
        SessionStatus::Running,
    )]));
    let sink = Arc::new(FakeEventSink::new());
    let hub = TeamHub::new_with_backends(
        Arc::clone(&sessions) as Arc<dyn LocalSessions>,
        Some(sink),
        server_config("Rodolfo", "tok", 0),
    );
    hub.start_headless();
    let addr = wait_for_server_addr(&hub).await;

    let mut ws = ws_connect(addr, Some("tok")).await.unwrap();
    let _hello = recv_server_msg(&mut ws).await;
    let _sessions = recv_server_msg(&mut ws).await;

    let hello_cli = serde_json::to_string(&ClientMsg::Hello {
        protocol: PROTOCOL_VERSION,
        peer_name: "Benjamin".to_string(),
        full_control: false,
    })
    .unwrap();
    ws.send(Message::text(hello_cli)).await.unwrap();
    assert_eq!(
        recv_server_msg(&mut ws).await,
        ServerMsg::FullControl { enabled: false }
    );

    let req_msg = serde_json::to_string(&ClientMsg::TaskRequest {
        request_id: "req-42".to_string(),
        session_id: "s1".to_string(),
        from_session: Some("Ben Agent".to_string()),
        text: "run cargo test".to_string(),
    })
    .unwrap();
    ws.send(Message::text(req_msg)).await.unwrap();

    // Wait for Pending ticket on the server hub
    let mut found_pending = false;
    for _ in 0..50 {
        if let Some(ticket) = hub.task_status("req-42") {
            assert_eq!(ticket.state, TaskState::Pending);
            assert_eq!(ticket.direction, TaskDirection::Incoming);
            assert_eq!(ticket.from_name, "Benjamin");
            assert_eq!(ticket.from_session.as_deref(), Some("Ben Agent"));
            found_pending = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(found_pending, "Pending ticket was not created");

    // Accept the task
    let final_ticket = hub.respond_task("req-42", true).await.expect("accept task");
    assert_eq!(final_ticket.state, TaskState::Delivered);

    let expected_bytes: Vec<u8> = build_task_input(AgentKind::Claude, "run cargo test")
        .into_iter()
        .flatten()
        .collect();
    assert_eq!(sessions.written_bytes("s1"), expected_bytes);

    // Client must receive Accepted then Delivered TaskUpdate frames
    let mut seen_states = Vec::new();
    for _ in 0..2 {
        match recv_server_msg(&mut ws).await {
            ServerMsg::TaskUpdate {
                request_id, state, ..
            } => {
                assert_eq!(request_id, "req-42");
                seen_states.push(state);
            }
            other => panic!("expected TaskUpdate, got {other:?}"),
        }
    }
    assert_eq!(seen_states, vec![TaskState::Accepted, TaskState::Delivered]);

    hub.shutdown().await;
}

#[tokio::test]
async fn test_5_read_request_returns_rendered_ansi_stripped_text() {
    let sessions = Arc::new(FakeLocalSessions::new(vec![
        make_session("s1", "Shared", true, SessionStatus::Running),
        make_session("s2", "Private", false, SessionStatus::Running),
    ]));
    let sink = Arc::new(FakeEventSink::new());
    let hub = TeamHub::new_with_backends(
        Arc::clone(&sessions) as Arc<dyn LocalSessions>,
        Some(sink),
        server_config("Rodolfo", "tok", 0),
    );
    hub.start_headless();
    let addr = wait_for_server_addr(&hub).await;

    sessions.push_output(
        &hub,
        "s1",
        b"\x1b[32mline one\x1b[0m\r\n\x1b[1mline two\x1b[0m\r\nline three\r\n",
    );

    let mut ws = ws_connect(addr, Some("tok")).await.unwrap();
    let _hello = recv_server_msg(&mut ws).await;
    let _sessions = recv_server_msg(&mut ws).await;

    let read_req = serde_json::to_string(&ClientMsg::ReadRequest {
        request_id: "read-1".to_string(),
        session_id: "s1".to_string(),
        max_lines: 2,
    })
    .unwrap();
    ws.send(Message::text(read_req)).await.unwrap();

    match recv_server_msg(&mut ws).await {
        ServerMsg::ReadResult {
            request_id,
            text,
            error,
        } => {
            assert_eq!(request_id, "read-1");
            assert_eq!(error, None);
            assert_eq!(text.as_deref(), Some("line two\nline three"));
        }
        other => panic!("expected ReadResult, got {other:?}"),
    }

    // Private session read must fail
    let read_priv = serde_json::to_string(&ClientMsg::ReadRequest {
        request_id: "read-priv".to_string(),
        session_id: "s2".to_string(),
        max_lines: 10,
    })
    .unwrap();
    ws.send(Message::text(read_priv)).await.unwrap();
    match recv_server_msg(&mut ws).await {
        ServerMsg::ReadResult {
            request_id,
            text,
            error,
        } => {
            assert_eq!(request_id, "read-priv");
            assert_eq!(text, None);
            assert!(error.is_some());
        }
        other => panic!("expected ReadResult error, got {other:?}"),
    }

    hub.shutdown().await;
}

#[tokio::test]
async fn test_6_client_reconnects_after_server_restart_and_resubscribes() {
    // Bind a free port on 127.0.0.1, then release it so server A can bind it, restart on the same port.
    let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = probe.local_addr().unwrap().port();
    drop(probe);

    let srv_sessions = Arc::new(FakeLocalSessions::new(vec![make_session(
        "s1",
        "Shared",
        true,
        SessionStatus::Running,
    )]));
    let srv_sink = Arc::new(FakeEventSink::new());
    let srv_hub = TeamHub::new_with_backends(
        Arc::clone(&srv_sessions) as Arc<dyn LocalSessions>,
        Some(srv_sink),
        server_config("Benjamin", "shared-secret", port),
    );
    srv_hub.start_headless();
    let _ = wait_for_server_addr(&srv_hub).await;

    srv_sessions.push_output(&srv_hub, "s1", b"before-restart\r\n");

    let cli_sessions = Arc::new(FakeLocalSessions::new(vec![]));
    let cli_sink = Arc::new(FakeEventSink::new());
    let mut cli_cfg = server_config("Rodolfo", "shared-secret", 0);
    cli_cfg.bind_host = "127.0.0.1".to_string();
    cli_cfg.peers = vec![PeerConfig {
        name: "Benjamin".to_string(),
        host: "127.0.0.1".to_string(),
        port,
        full_control: false,
    }];
    let cli_hub = TeamHub::new_with_backends(cli_sessions, Some(cli_sink.clone()), cli_cfg);
    cli_hub.start_headless();

    // Wait until client sees Benjamin online with session s1
    let mut connected = false;
    for _ in 0..100 {
        let snap = cli_hub.snapshot();
        if snap.peers.first().is_some_and(|p| p.online && p.sessions.len() == 1) {
            connected = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    assert!(connected, "client did not connect initially");

    // Subscribe via client hub -> receives initial Snapshot
    cli_hub.subscribe("Benjamin", "s1").await.unwrap();
    let snap_val = cli_sink
        .wait_for(Duration::from_secs(3), |evt, _| evt == EVT_TEAM_PTY_SNAPSHOT)
        .await
        .expect("initial snapshot event");
    let snap_ev: TeamPtySnapshotEvent = serde_json::from_value(snap_val).unwrap();
    assert_eq!(
        BASE64.decode(snap_ev.data_b64).unwrap(),
        b"before-restart\r\n"
    );

    // Shutdown server and wait for client to go offline
    srv_hub.shutdown().await;
    let mut went_offline = false;
    for _ in 0..100 {
        if !cli_hub.snapshot().peers[0].online {
            went_offline = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    assert!(went_offline, "client did not mark peer offline");

    // Restart server on the same port with new output
    let srv_sessions_2 = Arc::new(FakeLocalSessions::new(vec![make_session(
        "s1",
        "Shared",
        true,
        SessionStatus::Running,
    )]));
    let srv_sink_2 = Arc::new(FakeEventSink::new());
    let srv_hub_2 = TeamHub::new_with_backends(
        Arc::clone(&srv_sessions_2) as Arc<dyn LocalSessions>,
        Some(srv_sink_2),
        server_config("Benjamin", "shared-secret", port),
    );
    srv_hub_2.start_headless();
    let _ = wait_for_server_addr(&srv_hub_2).await;
    srv_sessions_2.push_output(&srv_hub_2, "s1", b"after-restart\r\n");

    // Client reconnects (2s initial backoff) and re-subscribes automatically
    let mut reconnected = false;
    for _ in 0..150 {
        let snap = cli_hub.snapshot();
        if snap.peers[0].online && snap.peers[0].sessions.len() == 1 {
            reconnected = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(reconnected, "client did not reconnect after server restart");

    let snap_val_2 = cli_sink
        .wait_for(Duration::from_secs(4), |evt, val| {
            if evt != EVT_TEAM_PTY_SNAPSHOT {
                return false;
            }
            serde_json::from_value::<TeamPtySnapshotEvent>(val.clone())
                .ok()
                .and_then(|e| BASE64.decode(e.data_b64).ok())
                .is_some_and(|b| b == b"after-restart\r\n")
        })
        .await
        .expect("reconnected client should receive fresh Snapshot on active mirror");
    let snap_ev_2: TeamPtySnapshotEvent = serde_json::from_value(snap_val_2).unwrap();
    assert_eq!(snap_ev_2.peer, "Benjamin");

    // Live output after reconnect also arrives
    srv_sessions_2.push_output(&srv_hub_2, "s1", b"live-after-reconnect");
    let out_val = cli_sink
        .wait_for(Duration::from_secs(3), |evt, val| {
            if evt != EVT_TEAM_PTY_OUTPUT {
                return false;
            }
            serde_json::from_value::<TeamPtyOutputEvent>(val.clone())
                .ok()
                .and_then(|e| BASE64.decode(e.data_b64).ok())
                .is_some_and(|b| b == b"live-after-reconnect")
        })
        .await
        .expect("live output after reconnect");
    let out_ev: TeamPtyOutputEvent = serde_json::from_value(out_val).unwrap();
    assert_eq!(out_ev.session_id, "s1");

    // Test read_session and send_task end-to-end across client -> server
    let read_back = cli_hub
        .read_session(
            SessionTarget {
                peer: Some("Benjamin".to_string()),
                session_id: "s1".to_string(),
            },
            10,
        )
        .await
        .expect("remote read_session");
    assert!(read_back.contains("after-restart"));

    // Unsharing emits SessionGone
    srv_sessions_2.set_shared("s1", false);
    srv_hub_2.sync_local_sessions_now();
    let gone_val = cli_sink
        .wait_for(Duration::from_secs(3), |evt, _| evt == EVT_TEAM_SESSION_GONE)
        .await
        .expect("session gone event when unshared");
    let gone_ev: TeamSessionGoneEvent = serde_json::from_value(gone_val).unwrap();
    assert_eq!(gone_ev.session_id, "s1");

    cli_hub.shutdown().await;
    srv_hub_2.shutdown().await;
}

#[tokio::test]
async fn test_refuses_0_0_0_0_bind_host_and_local_auto_approve() {
    let sessions = Arc::new(FakeLocalSessions::new(vec![make_session(
        "s1",
        "Local",
        false,
        SessionStatus::Running,
    )]));
    let sink = Arc::new(FakeEventSink::new());
    let mut cfg = server_config("Rodolfo", "tok", 0);
    cfg.bind_host = "0.0.0.0".to_string();
    cfg.auto_approve_local = true;

    let hub = TeamHub::new_with_backends(
        Arc::clone(&sessions) as Arc<dyn LocalSessions>,
        Some(sink),
        cfg,
    );
    hub.start_headless();

    // Wait for server check to record refusal error
    let mut refused = false;
    for _ in 0..50 {
        let snap = hub.snapshot();
        if snap.server_error.is_some() {
            assert_eq!(snap.listen_addr, None);
            refused = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(refused, "0.0.0.0 must be refused with serverError");

    // Local send_task with autoApproveLocal = true delivers immediately without needing shared=true
    let ticket = hub
        .send_task(SendTaskRequest {
            peer: None,
            session_id: "s1".to_string(),
            text: "local hello".to_string(),
            from_session: Some("Other".to_string()),
        })
        .await
        .expect("local task");
    assert_eq!(ticket.state, TaskState::Delivered);
    assert!(!sessions.written_bytes("s1").is_empty());

    hub.shutdown().await;
}

#[tokio::test]
async fn test_task_reject_and_rate_limit_20_per_min() {
    let sessions = Arc::new(FakeLocalSessions::new(vec![make_session(
        "s1",
        "Shared",
        true,
        SessionStatus::Running,
    )]));
    let sink = Arc::new(FakeEventSink::new());
    let hub = TeamHub::new_with_backends(
        Arc::clone(&sessions) as Arc<dyn LocalSessions>,
        Some(sink),
        server_config("Rodolfo", "tok", 0),
    );
    hub.start_headless();
    let addr = wait_for_server_addr(&hub).await;

    let mut ws = ws_connect(addr, Some("tok")).await.unwrap();
    let _hello = recv_server_msg(&mut ws).await;
    let _sessions = recv_server_msg(&mut ws).await;

    // Send 20 tasks (all succeed into Pending)
    for i in 0..20 {
        let msg = serde_json::to_string(&ClientMsg::TaskRequest {
            request_id: format!("r-{i}"),
            session_id: "s1".to_string(),
            from_session: None,
            text: format!("task {i}"),
        })
        .unwrap();
        ws.send(Message::text(msg)).await.unwrap();
    }

    // 21st task within 60 s must fail with rate limit
    let over_msg = serde_json::to_string(&ClientMsg::TaskRequest {
        request_id: "r-over".to_string(),
        session_id: "s1".to_string(),
        from_session: None,
        text: "too many".to_string(),
    })
    .unwrap();
    ws.send(Message::text(over_msg)).await.unwrap();

    match recv_server_msg(&mut ws).await {
        ServerMsg::TaskUpdate {
            request_id,
            state,
            message,
        } => {
            assert_eq!(request_id, "r-over");
            assert_eq!(state, TaskState::Failed);
            assert!(message.unwrap_or_default().contains("Rate limit"));
        }
        other => panic!("expected TaskUpdate Failed, got {other:?}"),
    }

    // Reject r-0 -> Rejected + no bytes written
    let rejected = hub.respond_task("r-0", false).await.expect("reject task");
    assert_eq!(rejected.state, TaskState::Rejected);
    assert!(sessions.written_bytes("s1").is_empty());

    match recv_server_msg(&mut ws).await {
        ServerMsg::TaskUpdate {
            request_id, state, ..
        } => {
            assert_eq!(request_id, "r-0");
            assert_eq!(state, TaskState::Rejected);
        }
        other => panic!("expected TaskUpdate Rejected, got {other:?}"),
    }

    hub.shutdown().await;
}

#[tokio::test]
async fn local_messaging_delivers_directly_without_inbox_and_formats_prompt() {
    let sessions = Arc::new(FakeLocalSessions::new(vec![
        make_session("11111111-aaaa-bbbb-cccc-000000000001", "velox 1", false, SessionStatus::Idle),
        make_session("22222222-aaaa-bbbb-cccc-000000000002", "velox 2", false, SessionStatus::Running),
    ]));
    let sink = Arc::new(FakeEventSink::new());
    let hub = TeamHub::new_with_backends(
        Arc::clone(&sessions) as Arc<dyn LocalSessions>,
        Some(Arc::clone(&sink) as Arc<dyn EventSink>),
        TeamConfig::default(),
    );

    let receipt = hub
        .send_local_message(SendLocalMessageRequest {
            to: "velox 2".to_string(),
            text: "Revisa auth.rs".to_string(),
            from_session_id: Some("11111111-aaaa-bbbb-cccc-000000000001".to_string()),
            from_session_query: None,
        })
        .await
        .expect("direct local message");

    assert_eq!(receipt.status, LocalMessageStatus::Delivered);
    assert!(!receipt.woke_up);
    assert_eq!(receipt.from_title, "velox 1");
    assert_eq!(receipt.to_title, "velox 2");
    // No manual inbox approval entry created
    assert!(hub.list_tasks().is_empty());

    let expected_prompt = "[Velox · mensaje de \"velox 1\" (11111111)]\nRevisa auth.rs\n\n(Para responder: velox msg \"velox 1\" \"<tu respuesta>\")";
    let expected_bytes: Vec<u8> = build_task_input(AgentKind::Claude, expected_prompt).concat();
    assert_eq!(
        sessions.written_bytes("22222222-aaaa-bbbb-cccc-000000000002"),
        expected_bytes
    );

    let ev = sink
        .wait_for(Duration::from_secs(2), |name, val| {
            name == EVT_TEAM_LOCAL_MESSAGE && val["status"] == "delivered"
        })
        .await
        .expect("local message event");
    assert_eq!(ev["fromTitle"], "velox 1");
    assert_eq!(ev["toTitle"], "velox 2");
}

#[tokio::test]
async fn local_messaging_rejects_self_send_exited_and_oversized() {
    let sessions = Arc::new(FakeLocalSessions::new(vec![
        make_session("s1-aaaa", "Lider", false, SessionStatus::Idle),
        make_session("s2-exit", "Closed Tab", false, SessionStatus::Exited),
    ]));
    let hub = TeamHub::new_with_backends(
        Arc::clone(&sessions) as Arc<dyn LocalSessions>,
        None,
        TeamConfig::default(),
    );

    let self_err = hub
        .send_local_message(SendLocalMessageRequest {
            to: "Lider".to_string(),
            text: "hola yo".to_string(),
            from_session_id: Some("s1-aaaa".to_string()),
            from_session_query: None,
        })
        .await
        .expect_err("self-send should fail");
    assert!(self_err.to_string().contains("same session"));

    let exited_err = hub
        .send_local_message(SendLocalMessageRequest {
            to: "Closed Tab".to_string(),
            text: "ping".to_string(),
            from_session_id: Some("s1-aaaa".to_string()),
            from_session_query: None,
        })
        .await
        .expect_err("exited target should fail");
    assert!(exited_err.to_string().contains("Unknown session"));

    let big_err = hub
        .send_local_message(SendLocalMessageRequest {
            to: "s1-aaaa".to_string(),
            text: "x".repeat(MAX_TASK_TEXT_BYTES + 1),
            from_session_id: None,
            from_session_query: None,
        })
        .await
        .expect_err("oversized text should fail");
    assert!(big_err.to_string().contains("bytes"));
}

#[tokio::test]
async fn local_messaging_wakes_sleeping_target_and_delivers_when_ready() {
    let sessions = Arc::new(FakeLocalSessions::new(vec![
        make_session("sender-11", "velox 1", false, SessionStatus::Idle),
        make_session("sleep-22", "velox 2", false, SessionStatus::Sleeping),
    ]));
    let sink = Arc::new(FakeEventSink::new());
    let hub = TeamHub::new_with_backends(
        Arc::clone(&sessions) as Arc<dyn LocalSessions>,
        Some(Arc::clone(&sink) as Arc<dyn EventSink>),
        TeamConfig::default(),
    );

    let sessions_bg = Arc::clone(&sessions);
    let hub_bg = Arc::clone(&hub);
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(350)).await;
        // First emit early non-ready boot output, then full Claude prompt
        sessions_bg.push_output(&hub_bg, "sleep-22", b"Windows PowerShell\r\n");
        tokio::time::sleep(Duration::from_millis(250)).await;
        sessions_bg.set_status("sleep-22", SessionStatus::Idle);
        sessions_bg.push_output(
            &hub_bg,
            "sleep-22",
            "────────────\n❯ \n────────────\n".as_bytes(),
        );
    });

    let receipt = hub
        .send_local_message(SendLocalMessageRequest {
            to: "velox 2".to_string(),
            text: "Desperta!".to_string(),
            from_session_id: Some("sender-11".to_string()),
            from_session_query: None,
        })
        .await
        .expect("wake + deliver");

    assert_eq!(receipt.status, LocalMessageStatus::Delivered);
    assert!(receipt.woke_up);
    assert_eq!(*sessions.woken.lock().unwrap(), vec!["sleep-22".to_string()]);
    assert!(String::from_utf8_lossy(&sessions.written_bytes("sleep-22")).contains("Desperta!"));

    // Both queued and delivered events were emitted
    sink.wait_for(Duration::from_secs(2), |n, v| {
        n == EVT_TEAM_LOCAL_MESSAGE && v["status"] == "queued"
    })
    .await
    .expect("queued event");
    sink.wait_for(Duration::from_secs(2), |n, v| {
        n == EVT_TEAM_LOCAL_MESSAGE && v["status"] == "delivered"
    })
    .await
    .expect("delivered event");
    hub.shutdown().await;
}

#[tokio::test]
async fn local_messaging_queues_when_blocked_and_delivers_into_busy_agy() {
    let mut agy_busy = make_session("agy-1111", "AGY Busy", false, SessionStatus::Running);
    agy_busy.agent_kind = AgentKind::Agy;
    agy_busy.active_agent = Some(AgentKind::Agy);
    let mut agy_blocked = make_session("agy-2222", "AGY Blocked", false, SessionStatus::Blocked);
    agy_blocked.agent_kind = AgentKind::Agy;
    agy_blocked.active_agent = Some(AgentKind::Agy);

    let sessions = Arc::new(FakeLocalSessions::new(vec![agy_busy, agy_blocked]));
    let sink = Arc::new(FakeEventSink::new());
    let hub = TeamHub::new_with_backends(
        Arc::clone(&sessions) as Arc<dyn LocalSessions>,
        Some(Arc::clone(&sink) as Arc<dyn EventSink>),
        TeamConfig::default(),
    );

    // 1. Blocked AGY queues until trust/permission prompt clears
    sessions.push_output(
        &hub,
        "agy-2222",
        "Do you trust the contents of this project?\n> Yes\n  ↑/↓ Navigate · enter Confirm\n"
            .as_bytes(),
    );
    let sessions_bg = Arc::clone(&sessions);
    let hub_bg = Arc::clone(&hub);
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(350)).await;
        // Must still NOT have written while blocked
        assert!(sessions_bg.written_bytes("agy-2222").is_empty());
        sessions_bg.clear_scrollback("agy-2222");
        sessions_bg.set_status("agy-2222", SessionStatus::Idle);
        sessions_bg.push_output(
            &hub_bg,
            "agy-2222",
            "\x1b[2J\x1b[HAntigravity CLI 1.5.3\n────────────────────────\n>\n────────────────────────\n"
                .as_bytes(),
        );
    });

    let blocked_receipt = hub
        .send_local_message(SendLocalMessageRequest {
            to: "agy-2222".to_string(),
            text: "Listo el fix".to_string(),
            from_session_id: Some("agy-1111".to_string()),
            from_session_query: None,
        })
        .await
        .expect("blocked -> delivered");
    assert_eq!(blocked_receipt.status, LocalMessageStatus::Delivered);
    assert!(String::from_utf8_lossy(&sessions.written_bytes("agy-2222")).contains("Listo el fix"));

    // 2. Live busy AGY accepts input directly without dropping (matches ConPTY probe)
    sessions.push_output(
        &hub,
        "agy-1111",
        "Antigravity CLI 1.5.3\n────────────────────────\n⣷  Generating...\n────────────────────────\n".as_bytes(),
    );
    let busy_receipt = hub
        .send_local_message(SendLocalMessageRequest {
            to: "agy-1111".to_string(),
            text: "Queued turn".to_string(),
            from_session_id: Some("agy-2222".to_string()),
            from_session_query: None,
        })
        .await
        .expect("busy agy delivery");
    assert_eq!(busy_receipt.status, LocalMessageStatus::Delivered);
    assert!(String::from_utf8_lossy(&sessions.written_bytes("agy-1111")).contains("Queued turn"));

    hub.shutdown().await;
}

#[tokio::test]
async fn full_control_two_hubs_mutual_task_and_remote_input_and_hot_cutoff() {
    // Allocate two free localhost ports so Hub A (Rodolfo) and Hub B (Benjamin) can connect bidirectionally.
    let p_a = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port_a = p_a.local_addr().unwrap().port();
    drop(p_a);
    let p_b = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port_b = p_b.local_addr().unwrap().port();
    drop(p_b);

    let sessions_a = Arc::new(FakeLocalSessions::new(vec![
        make_session("s-live", "velox 1", true, SessionStatus::Idle),
        make_session("s-priv", "Privada", false, SessionStatus::Idle),
        make_session("s-sleep", "Dormida", true, SessionStatus::Sleeping),
    ]));
    let sink_a = Arc::new(FakeEventSink::new());
    let mut cfg_a = server_config("Rodolfo", "mutual-secret", port_a);
    cfg_a.peers = vec![PeerConfig {
        name: "Benjamin".to_string(),
        host: "127.0.0.1".to_string(),
        port: port_b,
        full_control: false,
    }];
    let hub_a = TeamHub::new_with_backends(
        Arc::clone(&sessions_a) as Arc<dyn LocalSessions>,
        Some(Arc::clone(&sink_a) as Arc<dyn EventSink>),
        cfg_a.clone(),
    );

    let sessions_b = Arc::new(FakeLocalSessions::new(vec![]));
    let sink_b = Arc::new(FakeEventSink::new());
    let mut cfg_b = server_config("Benjamin", "mutual-secret", port_b);
    cfg_b.peers = vec![PeerConfig {
        name: "Rodolfo".to_string(),
        host: "127.0.0.1".to_string(),
        port: port_a,
        full_control: false,
    }];
    let hub_b = TeamHub::new_with_backends(
        Arc::clone(&sessions_b) as Arc<dyn LocalSessions>,
        Some(Arc::clone(&sink_b) as Arc<dyn EventSink>),
        cfg_b.clone(),
    );

    hub_a.start_headless();
    hub_b.start_headless();
    let addr_a = wait_for_server_addr(&hub_a).await;
    let _addr_b = wait_for_server_addr(&hub_b).await;

    // Wait for both hubs to connect online
    let mut both_online = false;
    for _ in 0..100 {
        let sa = hub_a.snapshot();
        let sb = hub_b.snapshot();
        if sa.peers[0].online && sb.peers[0].online && sb.peers[0].sessions.len() == 1 {
            both_online = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    assert!(both_online, "both hubs must connect online");

    // 1. Both OFF: remote input rejected on client AND rejected on raw WS server; task stays Pending.
    assert!(hub_b
        .send_remote_input("Rodolfo", "s-live", "\x03")
        .await
        .is_err());

    let mut raw_ws = ws_connect(addr_a, Some("mutual-secret")).await.unwrap();
    let _ = recv_server_msg(&mut raw_ws).await; // Hello
    let _ = recv_server_msg(&mut raw_ws).await; // Sessions
    ws_send_json(
        &mut raw_ws,
        &ClientMsg::Hello {
            protocol: PROTOCOL_VERSION,
            peer_name: "Benjamin".to_string(),
            full_control: true, // Spoofing remote=true while A still has local=false!
        },
    )
    .await;
    assert_eq!(
        recv_server_msg(&mut raw_ws).await,
        ServerMsg::FullControl { enabled: false }
    );
    ws_send_json(
        &mut raw_ws,
        &ClientMsg::TerminalInput {
            session_id: "s-live".to_string(),
            data_b64: BASE64.encode(b"\x03"),
        },
    )
    .await;
    match recv_server_msg(&mut raw_ws).await {
        ServerMsg::Error { message } => {
            assert!(message.to_lowercase().contains("control total"));
        }
        other => panic!("expected Error rejecting unauthorized TerminalInput, got {other:?}"),
    }
    assert!(
        sessions_a.written_bytes("s-live").is_empty(),
        "unauthorized TerminalInput must never write to PTY"
    );

    // 2. Only B enables full_control (A stays OFF): still NOT active, task stays Pending.
    cfg_b.peers[0].full_control = true;
    hub_b.apply_config(cfg_b.clone()).await;
    tokio::time::sleep(Duration::from_millis(120)).await;
    assert!(!hub_b.snapshot().peers[0].full_control_active);
    assert!(hub_a.snapshot().full_control_peers.is_empty());
    assert!(hub_b
        .send_remote_input("Rodolfo", "s-live", "hello\r")
        .await
        .is_err());

    let pending_ticket = hub_b
        .send_task(SendTaskRequest {
            peer: Some("Rodolfo".to_string()),
            session_id: "s-live".to_string(),
            text: "needs approval".to_string(),
            from_session: None,
        })
        .await
        .expect("send pending task");
    let mut saw_pending = false;
    for _ in 0..50 {
        if let Some(t) = hub_a.task_status(&pending_ticket.request_id) {
            assert_eq!(t.state, TaskState::Pending);
            saw_pending = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(saw_pending, "one-sided full_control must still keep task Pending");
    assert!(sessions_a.written_bytes("s-live").is_empty());

    // 3. A ALSO enables full_control -> MUTUAL ACTIVE (hot, without dropping connections!).
    cfg_a.peers[0].full_control = true;
    hub_a.apply_config(cfg_a.clone()).await;

    let mut mutual_active = false;
    for _ in 0..100 {
        let sa = hub_a.snapshot();
        let sb = hub_b.snapshot();
        if sa.peers[0].full_control_active
            && sb.peers[0].full_control_active
            && sa.full_control_peers == vec!["Benjamin".to_string()]
            && sb.full_control_peers == vec!["Rodolfo".to_string()]
            && sb.peers[0].sessions.len() == 2 // s-live + s-sleep (s-priv still hidden!)
        {
            mutual_active = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    assert!(
        mutual_active,
        "mutual full_control must activate on both hubs; A={:?}, B={:?}",
        hub_a.snapshot(),
        hub_b.snapshot()
    );
    assert!(
        !hub_b.snapshot().peers[0]
            .sessions
            .iter()
            .any(|s| s.id == "s-priv"),
        "unshared session must stay invisible even under Control total"
    );

    // 3a. Task delivers automatically WITHOUT Accept/Reject card, stays in history, emits toast.
    let auto_ticket = hub_b
        .send_task(SendTaskRequest {
            peer: Some("Rodolfo".to_string()),
            session_id: "s-live".to_string(),
            text: "run cargo test".to_string(),
            from_session: Some("Ben Tab".to_string()),
        })
        .await
        .expect("send auto-approved task");

    let mut delivered_on_both = false;
    for _ in 0..100 {
        let ta = hub_a.task_status(&auto_ticket.request_id);
        let tb = hub_b.task_status(&auto_ticket.request_id);
        if ta.as_ref().is_some_and(|t| t.state == TaskState::Delivered)
            && tb.as_ref().is_some_and(|t| t.state == TaskState::Delivered)
        {
            delivered_on_both = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    assert!(delivered_on_both, "task must auto-deliver on both hubs");

    let expected_task_bytes: Vec<u8> = build_task_input(AgentKind::Claude, "run cargo test")
        .into_iter()
        .flatten()
        .collect();
    assert_eq!(sessions_a.written_bytes("s-live"), expected_task_bytes);

    let toast_ev = sink_a
        .wait_for(Duration::from_secs(2), |evt, val| {
            evt == EVT_TEAM_LOCAL_MESSAGE
                && val["isRemote"] == true
                && val["status"] == "delivered"
                && val["fromTitle"] == "Benjamin"
                && val["toTitle"] == "velox 1"
        })
        .await
        .expect("remote auto-delivery toast event on Hub A");
    assert_eq!(toast_ev["preview"], "run cargo test");

    // 3b. Direct raw keyboard input (Ctrl+C + Arrow Up + 'y' + Enter) arrives UNFILTERED in shared session!
    let raw_keys = "\x03\x1b[Ay\r";
    hub_b
        .send_remote_input("Rodolfo", "s-live", raw_keys)
        .await
        .expect("send remote raw input");
    let mut got_raw = false;
    for _ in 0..80 {
        if sessions_a
            .written_bytes("s-live")
            .ends_with(raw_keys.as_bytes())
        {
            got_raw = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(got_raw, "raw control keys must reach owner's shared PTY verbatim");

    // 3c. Unshared session stays completely inaccessible even with mutual full control ON!
    hub_b
        .send_remote_input("Rodolfo", "s-priv", "secret\r")
        .await
        .expect("frame dispatched to server");
    tokio::time::sleep(Duration::from_millis(120)).await;
    assert!(
        sessions_a.written_bytes("s-priv").is_empty(),
        "unshared session must NEVER receive remote input"
    );

    // 3d. Sleeping shared session wakes automatically when tasked under mutual full control!
    let sessions_a_bg = Arc::clone(&sessions_a);
    let hub_a_bg = Arc::clone(&hub_a);
    tokio::spawn(async move {
        for _ in 0..100 {
            if sessions_a_bg
                .woken
                .lock()
                .unwrap()
                .iter()
                .any(|id| id == "s-sleep")
            {
                tokio::time::sleep(Duration::from_millis(100)).await;
                sessions_a_bg.set_status("s-sleep", SessionStatus::Idle);
                sessions_a_bg.push_output(
                    &hub_a_bg,
                    "s-sleep",
                    "────────────\n❯ \n────────────\n".as_bytes(),
                );
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    });

    let wake_ticket = hub_b
        .send_task(SendTaskRequest {
            peer: Some("Rodolfo".to_string()),
            session_id: "s-sleep".to_string(),
            text: "despierta y revisa".to_string(),
            from_session: None,
        })
        .await
        .expect("send task to sleeping shared session");

    let mut wake_delivered = false;
    for _ in 0..150 {
        if hub_b
            .task_status(&wake_ticket.request_id)
            .is_some_and(|t| t.state == TaskState::Delivered)
        {
            wake_delivered = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(40)).await;
    }
    assert!(wake_delivered, "sleeping shared session must wake and receive task");
    assert!(sessions_a.woken.lock().unwrap().contains(&"s-sleep".to_string()));
    assert!(String::from_utf8_lossy(&sessions_a.written_bytes("s-sleep"))
        .contains("despierta y revisa"));

    // 4. Hot cutoff: A turns OFF full_control -> immediately stops control on both sides!
    let bytes_before_cutoff = sessions_a.written_bytes("s-live").len();
    cfg_a.peers[0].full_control = false;
    hub_a.apply_config(cfg_a).await;

    let mut cut_off = false;
    for _ in 0..100 {
        let sa = hub_a.snapshot();
        let sb = hub_b.snapshot();
        if !sa.peers[0].full_control_active
            && !sb.peers[0].full_control_active
            && sa.full_control_peers.is_empty()
            && sb.full_control_peers.is_empty()
        {
            cut_off = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    assert!(cut_off, "turning off full_control must cut off immediately on both hubs");

    assert!(hub_b
        .send_remote_input("Rodolfo", "s-live", "after-off\r")
        .await
        .is_err());

    // Even a raw WS frame on the already-open raw_ws connection is rejected on the server.
    ws_send_json(
        &mut raw_ws,
        &ClientMsg::TerminalInput {
            session_id: "s-live".to_string(),
            data_b64: BASE64.encode(b"rogue\r"),
        },
    )
    .await;
    tokio::time::sleep(Duration::from_millis(120)).await;
    assert_eq!(
        sessions_a.written_bytes("s-live").len(),
        bytes_before_cutoff,
        "no bytes may be written after full_control cutoff"
    );

    // Subsequent task goes back to Pending (manual approval).
    let after_off_ticket = hub_b
        .send_task(SendTaskRequest {
            peer: Some("Rodolfo".to_string()),
            session_id: "s-live".to_string(),
            text: "needs approval again".to_string(),
            from_session: None,
        })
        .await
        .expect("send task after cutoff");
    let mut back_to_pending = false;
    for _ in 0..50 {
        if let Some(t) = hub_a.task_status(&after_off_ticket.request_id) {
            assert_eq!(t.state, TaskState::Pending);
            back_to_pending = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(back_to_pending, "after cutoff, tasks must require manual approval again");

    hub_b.shutdown().await;
    hub_a.shutdown().await;
}

async fn ws_send_json(
    ws: &mut (impl SinkExt<Message, Error = WsError> + Unpin),
    msg: &ClientMsg,
) {
    let raw = serde_json::to_string(msg).unwrap();
    ws.send(Message::text(raw)).await.unwrap();
}
