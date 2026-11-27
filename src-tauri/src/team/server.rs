//! Team WebSocket server (accepting side: publisher/executor).

use crate::team::hub::TeamHub;
use crate::team::protocol::{ClientMsg, ServerMsg, PROTOCOL_VERSION, WS_PATH};
use crate::team::tailscale::detect_tailscale_ipv4;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use futures_util::{SinkExt, StreamExt};
use sha2::{Digest, Sha256};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;

const MAX_CLIENT_FRAME_BYTES: usize = 64 * 1024;
const PING_INTERVAL: Duration = Duration::from_secs(20);
const SILENCE_TIMEOUT: Duration = Duration::from_secs(60);
const RETRY_DELAY: Duration = Duration::from_secs(30);
const CONN_CHANNEL_CAPACITY: usize = 256;

/// Spawns the team WebSocket server supervisor task until `shutdown_rx` resolves.
pub fn spawn_server(hub: Arc<TeamHub>, mut shutdown_rx: oneshot::Receiver<()>) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            let cfg = hub.config_snapshot();

            if !cfg.enabled {
                hub.set_server_state(None, None, None);
                return;
            }

            if cfg.token.trim().is_empty() {
                hub.set_server_state(None, Some("Team token is empty".to_string()), None);
                return;
            }

            let (target_ip, tailscale_ip): (IpAddr, Option<Ipv4Addr>) =
                if !cfg.bind_host.trim().is_empty() {
                    let raw = cfg.bind_host.trim();
                    let ip = match raw.parse::<IpAddr>() {
                        Ok(ip) if !ip.is_unspecified() => ip,
                        Ok(_) => {
                            hub.set_server_state(
                                None,
                                Some(
                                    "Refusing to bind unspecified address (0.0.0.0 / ::)"
                                        .to_string(),
                                ),
                                None,
                            );
                            let _ = shutdown_rx.await;
                            return;
                        }
                        Err(err) => {
                            hub.set_server_state(
                                None,
                                Some(format!("Invalid bindHost '{raw}': {err}")),
                                None,
                            );
                            let _ = shutdown_rx.await;
                            return;
                        }
                    };
                    (ip, None)
                } else {
                    let ts_ip = tokio::task::spawn_blocking(detect_tailscale_ipv4)
                        .await
                        .ok()
                        .flatten();
                    if let Some(ip) = ts_ip {
                        (IpAddr::V4(ip), Some(ip))
                    } else {
                        hub.set_server_state(
                            None,
                            Some("Tailscale not detected".to_string()),
                            None,
                        );
                        tokio::select! {
                            _ = &mut shutdown_rx => return,
                            _ = tokio::time::sleep(RETRY_DELAY) => continue,
                        }
                    }
                };

            let bind_addr = SocketAddr::new(target_ip, cfg.listen_port);
            let listener = match TcpListener::bind(bind_addr).await {
                Ok(l) => l,
                Err(err) => {
                    hub.set_server_state(
                        None,
                        Some(format!("Failed to bind {bind_addr}: {err}")),
                        tailscale_ip,
                    );
                    tokio::select! {
                        _ = &mut shutdown_rx => return,
                        _ = tokio::time::sleep(RETRY_DELAY) => continue,
                    }
                }
            };

            let local_addr = listener.local_addr().ok();
            tracing::info!(?local_addr, "Team WebSocket server listening");
            hub.set_server_state(local_addr, None, tailscale_ip);

            let (conn_shutdown_tx, conn_shutdown_rx) = watch::channel(false);
            let router = Router::new()
                .route(WS_PATH, get(ws_handler))
                .with_state(ServerState {
                    hub: Arc::clone(&hub),
                    conn_shutdown_rx,
                });

            let serve_res = axum::serve(listener, router)
                .with_graceful_shutdown(async move {
                    let _ = shutdown_rx.await;
                    let _ = conn_shutdown_tx.send(true);
                })
                .await;

            if let Err(err) = serve_res {
                tracing::warn!(%err, "Team WebSocket server exited with error");
            }

            hub.set_server_state(None, None, tailscale_ip);
            return;
        }
    })
}

#[derive(Clone)]
struct ServerState {
    hub: Arc<TeamHub>,
    conn_shutdown_rx: watch::Receiver<bool>,
}

async fn ws_handler(
    headers: HeaderMap,
    State(state): State<ServerState>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    let expected = state.hub.token_snapshot();
    if expected.trim().is_empty() || !verify_bearer(&headers, &expected) {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let hub = state.hub;
    let shutdown_rx = state.conn_shutdown_rx;
    ws.max_frame_size(MAX_CLIENT_FRAME_BYTES)
        .max_message_size(MAX_CLIENT_FRAME_BYTES)
        .on_upgrade(move |socket| handle_socket(socket, hub, shutdown_rx))
}

fn verify_bearer(headers: &HeaderMap, expected: &str) -> bool {
    let Some(raw) = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    else {
        return false;
    };
    let Some((scheme, presented)) = raw.split_once(' ') else {
        return false;
    };
    if !scheme.eq_ignore_ascii_case("bearer") {
        return false;
    }
    constant_time_token_eq(presented.trim(), expected)
}

fn constant_time_token_eq(presented: &str, expected: &str) -> bool {
    let digest_a = Sha256::digest(presented.as_bytes());
    let digest_b = Sha256::digest(expected.as_bytes());
    let mut diff: u8 = 0;
    for (a, b) in digest_a.iter().zip(digest_b.iter()) {
        diff |= a ^ b;
    }
    diff == 0 && presented.len() == expected.len()
}

async fn handle_socket(
    socket: WebSocket,
    hub: Arc<TeamHub>,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    let (mut ws_tx, mut ws_rx) = socket.split();
    let (tx, mut rx) = mpsc::channel::<ServerMsg>(CONN_CHANNEL_CAPACITY);
    let conn_id = hub.register_connection(tx.clone());

    let _ = tx.try_send(ServerMsg::Hello {
        protocol: PROTOCOL_VERSION,
        peer_name: hub.display_name(),
        full_control: false,
    });
    let _ = tx.try_send(ServerMsg::Sessions {
        sessions: hub.shared_sessions(),
    });

    let last_activity = Arc::new(Mutex::new(Instant::now()));
    let last_activity_writer = Arc::clone(&last_activity);
    let hub_writer = Arc::clone(&hub);
    let mut writer_shutdown = shutdown_rx.clone();

    let writer_task = tokio::spawn(async move {
        let mut ping_interval = tokio::time::interval(PING_INTERVAL);
        ping_interval.tick().await;

        loop {
            tokio::select! {
                _ = writer_shutdown.changed() => {
                    let _ = ws_tx.send(Message::Close(None)).await;
                    break;
                }
                maybe_msg = rx.recv() => {
                    let Some(msg) = maybe_msg else {
                        break;
                    };
                    if send_server_msg(&mut ws_tx, &msg).await.is_err() {
                        break;
                    }
                    while let Ok(next_msg) = rx.try_recv() {
                        if send_server_msg(&mut ws_tx, &next_msg).await.is_err() {
                            return;
                        }
                    }
                    if rx.is_empty() {
                        hub_writer.flush_lagged_resubscribes(conn_id);
                    }
                }
                _ = ping_interval.tick() => {
                    let silent = last_activity_writer
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .elapsed();
                    if silent >= SILENCE_TIMEOUT {
                        let _ = ws_tx.send(Message::Close(None)).await;
                        break;
                    }
                    if ws_tx.send(Message::Ping(Default::default())).await.is_err() {
                        break;
                    }
                }
            }
        }
    });

    loop {
        if *shutdown_rx.borrow() {
            break;
        }
        let frame = tokio::select! {
            _ = shutdown_rx.changed() => break,
            next = tokio::time::timeout(SILENCE_TIMEOUT, ws_rx.next()) => {
                match next {
                    Ok(Some(Ok(frame))) => frame,
                    Ok(Some(Err(err))) => {
                        tracing::debug!(conn_id, %err, "Team WS client read error");
                        break;
                    }
                    Ok(None) | Err(_) => break,
                }
            }
        };

        *last_activity.lock().unwrap_or_else(|e| e.into_inner()) = Instant::now();

        match frame {
            Message::Text(text) => {
                if text.len() > MAX_CLIENT_FRAME_BYTES {
                    let _ = tx.try_send(ServerMsg::Error {
                        message: "Frame exceeds 64 KB limit".to_string(),
                    });
                    break;
                }
                match serde_json::from_str::<ClientMsg>(text.as_str()) {
                    Ok(ClientMsg::Hello {
                        protocol,
                        peer_name,
                        full_control,
                    }) => {
                        if protocol != PROTOCOL_VERSION {
                            let _ = tx.try_send(ServerMsg::Error {
                                message: format!(
                                    "Protocol mismatch: expected {PROTOCOL_VERSION}, got {protocol}"
                                ),
                            });
                            break;
                        }
                        hub.handle_client_hello(conn_id, peer_name, full_control);
                    }
                    Ok(ClientMsg::FullControl { enabled }) => {
                        hub.handle_client_full_control(conn_id, enabled);
                    }
                    Ok(ClientMsg::Subscribe { session_id }) => {
                        hub.handle_subscribe(conn_id, &session_id);
                    }
                    Ok(ClientMsg::Unsubscribe { session_id }) => {
                        hub.handle_unsubscribe(conn_id, &session_id);
                    }
                    Ok(ClientMsg::TerminalInput {
                        session_id,
                        data_b64,
                    }) => {
                        hub.handle_terminal_input(conn_id, session_id, data_b64);
                    }
                    Ok(ClientMsg::TaskRequest {
                        request_id,
                        session_id,
                        from_session,
                        text,
                    }) => {
                        hub.handle_incoming_task(
                            conn_id,
                            request_id,
                            session_id,
                            from_session,
                            text,
                        );
                    }
                    Ok(ClientMsg::ReadRequest {
                        request_id,
                        session_id,
                        max_lines,
                    }) => {
                        hub.handle_read_request(conn_id, request_id, session_id, max_lines);
                    }
                    Err(err) => {
                        let _ = tx.try_send(ServerMsg::Error {
                            message: format!("Invalid client message: {err}"),
                        });
                    }
                }
            }
            Message::Binary(bytes) => {
                if bytes.len() > MAX_CLIENT_FRAME_BYTES {
                    break;
                }
            }
            Message::Ping(_) | Message::Pong(_) => {}
            Message::Close(_) => break,
        }
    }

    hub.unregister_connection(conn_id);
    drop(tx);
    let _ = writer_task.await;
}

async fn send_server_msg(
    ws_tx: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    msg: &ServerMsg,
) -> Result<(), ()> {
    let Ok(payload) = serde_json::to_string(msg) else {
        return Ok(());
    };
    ws_tx
        .send(Message::Text(payload.into()))
        .await
        .map_err(|_| ())
}
