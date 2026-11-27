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
use velox_lib::domain::error::VeloxResult;
use velox_lib::domain::session::{SessionRecord, SessionStatus};
use velox_lib::pty::manager::StreamPos;
use velox_lib::pty::scrollback::RingScrollback;
use velox_lib::session::manager::SessionTap;
use velox_lib::team::api::{
    SendTaskRequest, SessionTarget, TaskDirection, TeamApi, TeamPtyOutputEvent,
    TeamPtySnapshotEvent, TeamSessionGoneEvent, EVT_TEAM_PTY_OUTPUT, EVT_TEAM_PTY_SNAPSHOT,
    EVT_TEAM_SESSION_GONE,
};
use velox_lib::team::delivery::build_task_input;
use velox_lib::team::hub::{EventSink, LocalSessions, TeamHub};
use velox_lib::team::protocol::{ClientMsg, ServerMsg, TaskState, PROTOCOL_VERSION, WS_PATH};

struct FakeLocalSessions {
    sessions: Mutex<Vec<SessionRecord>>,
    scrollbacks: Mutex<HashMap<String, RingScrollback>>,
    sizes: Mutex<HashMap<String, (u16, u16)>>,
    written: Mutex<HashMap<String, Vec<Vec<u8>>>>,
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
        }
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
        } => {
            assert_eq!(protocol, PROTOCOL_VERSION);
            assert_eq!(peer_name, "Rodolfo");
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
    })
    .unwrap();
    ws.send(Message::text(hello_cli)).await.unwrap();

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
