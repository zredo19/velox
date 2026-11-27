use crate::domain::error::{VeloxError, VeloxResult};
use crate::domain::session::{McpBackendStatus, McpGatewayMode, McpGatewaySnapshot};
use crate::mcp::backend::StdioMcpBackend;
use crate::mcp::config::load_backends_config;
use axum::extract::{Path as AxumPath, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use crate::mcp::builtin::BuiltinMcp;
use std::collections::BTreeMap;
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
use tokio::net::TcpListener;
use tokio::sync::{oneshot, Mutex, RwLock};
use tokio::task::JoinHandle;

const BYTES_PER_MB: f64 = 1_048_576.0;

struct GatewayInner {
    backends_path: PathBuf,
    port: u16,
    mode: RwLock<McpGatewayMode>,
    last_error: RwLock<Option<String>>,
    backends: RwLock<BTreeMap<String, Arc<StdioMcpBackend>>>,
    /// In-process MCP servers (e.g. `velox-team`), routed before stdio backends.
    builtins: RwLock<BTreeMap<String, Arc<dyn BuiltinMcp>>>,
    sys: std::sync::Mutex<System>,
    shutdown_tx: Mutex<Option<oneshot::Sender<()>>>,
    server_task: Mutex<Option<JoinHandle<()>>>,
    reaper_task: Mutex<Option<JoinHandle<()>>>,
}

/// Embedded Streamable HTTP Lazy MCP Gateway with schema caching and scale-to-zero reaping.
#[derive(Clone)]
pub struct McpGatewayManager {
    inner: Arc<GatewayInner>,
}

impl McpGatewayManager {
    pub fn new(backends_path: PathBuf, port: u16) -> Self {
        let initial_map = load_backends_config(&backends_path)
            .map(|file| {
                file.backends
                    .into_iter()
                    .map(|(name, cfg)| {
                        let backend = Arc::new(StdioMcpBackend::new(name.clone(), cfg));
                        (name, backend)
                    })
                    .collect()
            })
            .unwrap_or_default();

        Self {
            inner: Arc::new(GatewayInner {
                backends_path,
                port,
                mode: RwLock::new(McpGatewayMode::Disabled),
                last_error: RwLock::new(None),
                backends: RwLock::new(initial_map),
                builtins: RwLock::new(BTreeMap::new()),
                sys: std::sync::Mutex::new(System::new()),
                shutdown_tx: Mutex::new(None),
                server_task: Mutex::new(None),
                reaper_task: Mutex::new(None),
            }),
        }
    }

    /// Serve an in-process MCP server at `/mcp/{name}` (takes precedence over backends.json).
    pub async fn register_builtin(&self, name: impl Into<String>, handler: Arc<dyn BuiltinMcp>) {
        self.inner.builtins.write().await.insert(name.into(), handler);
    }

    pub async fn start(&self) -> VeloxResult<McpGatewaySnapshot> {
        // Reload backends.json cleanly on start
        if let Ok(file) = load_backends_config(&self.inner.backends_path) {
            let mut guard = self.inner.backends.write().await;
            if guard.is_empty() {
                for (name, cfg) in file.backends {
                    guard.insert(name.clone(), Arc::new(StdioMcpBackend::new(name, cfg)));
                }
            }
        }

        let port = self.inner.port;
        match TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await {
            Ok(listener) => {
                *self.inner.mode.write().await = McpGatewayMode::Embedded;
                *self.inner.last_error.write().await = None;

                let (tx, rx) = oneshot::channel::<()>();
                *self.inner.shutdown_tx.lock().await = Some(tx);

                let router = Router::new()
                    .route("/health", get(health_route))
                    .route("/status", get(status_route))
                    .route("/mcp/{name}", post(mcp_rpc_route))
                    .route("/mcp/{name}", delete(mcp_delete_route))
                    .route("/admin/sleep/{name}", post(sleep_backend_route))
                    .route("/admin/sleep-all", post(sleep_all_route))
                    .with_state(self.clone());

                let server_handle = tokio::spawn(async move {
                    let serve_res = axum::serve(listener, router)
                        .with_graceful_shutdown(async move {
                            let _ = rx.await;
                        })
                        .await;
                    if let Err(err) = serve_res {
                        tracing::warn!(%err, "Embedded Lazy MCP Gateway server exited with error");
                    }
                });
                *self.inner.server_task.lock().await = Some(server_handle);

                // Background idle scale-to-zero reaper (every 5 seconds)
                let mgr_reaper = self.clone();
                let reaper_handle = tokio::spawn(async move {
                    let mut interval = tokio::time::interval(Duration::from_secs(5));
                    interval.tick().await;
                    loop {
                        interval.tick().await;
                        mgr_reaper.reap_idle_backends().await;
                    }
                });
                *self.inner.reaper_task.lock().await = Some(reaper_handle);

                tracing::info!(port, "Started embedded Rust Lazy MCP Gateway");
                Ok(self.snapshot().await)
            }
            Err(err) if err.kind() == std::io::ErrorKind::AddrInUse => {
                tracing::info!(
                    port,
                    "Port already bound by external lazy-mcp-gateway; attaching in read-only ExternalAttached mode"
                );
                *self.inner.mode.write().await = McpGatewayMode::ExternalAttached;
                *self.inner.last_error.write().await = None;
                Ok(self.snapshot().await)
            }
            Err(err) => {
                let msg = format!("Failed to bind Lazy MCP Gateway on 127.0.0.1:{port}: {err}");
                *self.inner.mode.write().await = McpGatewayMode::Error;
                *self.inner.last_error.write().await = Some(msg.clone());
                Err(VeloxError::Mcp(msg))
            }
        }
    }

    async fn reap_idle_backends(&self) {
        let backends: Vec<Arc<StdioMcpBackend>> =
            self.inner.backends.read().await.values().cloned().collect();
        for backend in backends {
            let _ = backend.reap_if_idle().await;
        }
    }

    pub async fn snapshot(&self) -> McpGatewaySnapshot {
        let mode = *self.inner.mode.read().await;
        let error = self.inner.last_error.read().await.clone();
        let backends: Vec<Arc<StdioMcpBackend>> =
            self.inner.backends.read().await.values().cloned().collect();

        // Collect live PIDs once so we only refresh sysinfo when at least 1 backend is awake
        let mut pids = Vec::with_capacity(backends.len());
        for b in &backends {
            pids.push(b.pid().await);
        }

        let any_running = pids.iter().any(Option::is_some);
        let mem_by_pid: std::collections::HashMap<u32, f64> = if any_running {
            let mut sys = self.inner.sys.lock().unwrap_or_else(|e| e.into_inner());
            let target_pids: Vec<Pid> = pids.iter().flatten().map(|&p| Pid::from_u32(p)).collect();
            sys.refresh_processes_specifics(
                ProcessesToUpdate::Some(&target_pids),
                true,
                ProcessRefreshKind::nothing().with_memory(),
            );
            target_pids
                .into_iter()
                .map(|p| {
                    let mb = sys
                        .process(p)
                        .map(|proc_info| {
                            ((proc_info.memory() as f64 / BYTES_PER_MB) * 10.0).round() / 10.0
                        })
                        .unwrap_or(0.0);
                    (p.as_u32(), mb)
                })
                .collect()
        } else {
            std::collections::HashMap::new()
        };

        let mut statuses: Vec<McpBackendStatus> = Vec::with_capacity(backends.len());
        let mut active_backends = 0usize;
        let mut total_memory_mb = 0.0;

        for (backend, pid_opt) in backends.iter().zip(pids) {
            let mb = pid_opt
                .and_then(|p| mem_by_pid.get(&p).copied())
                .unwrap_or(0.0);
            let st = backend.status(mb).await;
            if st.running {
                active_backends += 1;
                total_memory_mb += st.memory_mb;
            }
            statuses.push(st);
        }

        McpGatewaySnapshot {
            mode,
            listen_addr: format!("127.0.0.1:{}", self.inner.port),
            total_backends: statuses.len(),
            active_backends,
            total_memory_mb: (total_memory_mb * 10.0).round() / 10.0,
            backends: statuses,
            error,
        }
    }

    pub async fn sleep_backend(&self, name: &str) -> VeloxResult<McpGatewaySnapshot> {
        let backend = self.inner.backends.read().await.get(name).cloned();
        if let Some(b) = backend {
            b.sleep().await;
            Ok(self.snapshot().await)
        } else {
            Err(VeloxError::Mcp(format!("Unknown MCP backend: {name}")))
        }
    }

    pub async fn sleep_all(&self) -> VeloxResult<McpGatewaySnapshot> {
        let backends: Vec<Arc<StdioMcpBackend>> =
            self.inner.backends.read().await.values().cloned().collect();
        for backend in backends {
            backend.sleep().await;
        }
        Ok(self.snapshot().await)
    }

    pub async fn shutdown(&self) {
        if let Some(tx) = self.inner.shutdown_tx.lock().await.take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.inner.reaper_task.lock().await.take() {
            handle.abort();
        }
        if let Some(handle) = self.inner.server_task.lock().await.take() {
            handle.abort();
        }
        let _ = self.sleep_all().await;
    }
}

async fn health_route(State(mgr): State<McpGatewayManager>) -> (StatusCode, Json<Value>) {
    let mode = *mgr.inner.mode.read().await;
    (StatusCode::OK, Json(json!({ "ok": true, "mode": mode })))
}

async fn status_route(
    State(mgr): State<McpGatewayManager>,
) -> (StatusCode, Json<McpGatewaySnapshot>) {
    (StatusCode::OK, Json(mgr.snapshot().await))
}

fn extract_velox_session_id(headers: &HeaderMap) -> Option<String> {
    let raw = headers.get("x-velox-session")?.to_str().ok()?.trim();
    if raw.is_empty() || raw.starts_with("${") {
        None
    } else {
        Some(raw.to_string())
    }
}

async fn mcp_rpc_route(
    AxumPath(name): AxumPath<String>,
    State(mgr): State<McpGatewayManager>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let session_id = extract_velox_session_id(&headers);
    let builtin = mgr.inner.builtins.read().await.get(&name).cloned();
    if let Some(handler) = builtin {
        return match handler.handle_jsonrpc_with_session(body, session_id).await {
            Some(response) => (StatusCode::OK, Json(response)),
            None => (StatusCode::ACCEPTED, Json(json!({ "ok": true }))),
        };
    }

    let backend = mgr.inner.backends.read().await.get(&name).cloned();
    let Some(backend) = backend else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": format!("Unknown MCP backend: {name}") })),
        );
    };

    match backend.handle_jsonrpc(body).await {
        Ok(Some(response)) => (StatusCode::OK, Json(response)),
        Ok(None) => (StatusCode::ACCEPTED, Json(json!({ "ok": true }))),
        Err(err) => (
            StatusCode::BAD_GATEWAY,
            Json(json!({
                "jsonrpc": "2.0",
                "error": { "code": -32000, "message": err.to_string() }
            })),
        ),
    }
}

async fn mcp_delete_route(AxumPath(_name): AxumPath<String>) -> StatusCode {
    StatusCode::NO_CONTENT
}

async fn sleep_backend_route(
    AxumPath(name): AxumPath<String>,
    State(mgr): State<McpGatewayManager>,
) -> (StatusCode, Json<Value>) {
    match mgr.sleep_backend(&name).await {
        Ok(snap) => (StatusCode::OK, Json(json!(snap))),
        Err(err) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": err.to_string() })),
        ),
    }
}

async fn sleep_all_route(State(mgr): State<McpGatewayManager>) -> (StatusCode, Json<Value>) {
    match mgr.sleep_all().await {
        Ok(snap) => (StatusCode::OK, Json(json!(snap))),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": err.to_string() })),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn gateway_snapshot_reports_sleeping_backends_at_zero_mb() {
        let dir = std::env::temp_dir().join(format!("velox-mcp-gw-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let file = dir.join("backends.json");
        std::fs::write(
            &file,
            r#"{"backends":{"demo":{"command":"node.exe","args":["--version"],"idleMs":60000}}}"#,
        )
        .expect("write backends.json");

        let mgr = McpGatewayManager::new(file, 0);
        let snap = mgr.snapshot().await;
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(snap.total_backends, 1);
        assert_eq!(snap.active_backends, 0);
        assert_eq!(snap.total_memory_mb, 0.0);
        assert!(!snap.backends[0].running);
        assert_eq!(snap.backends[0].memory_mb, 0.0);
    }

    struct CaptureSessionBuiltin {
        seen: std::sync::Mutex<Vec<Option<String>>>,
    }

    impl BuiltinMcp for CaptureSessionBuiltin {
        fn handle_jsonrpc(&self, _request: Value) -> crate::team::api::BoxFuture<'_, Option<Value>> {
            Box::pin(async { Some(json!({ "ok": true })) })
        }

        fn handle_jsonrpc_with_session(
            &self,
            _request: Value,
            session_id: Option<String>,
        ) -> crate::team::api::BoxFuture<'_, Option<Value>> {
            self.seen.lock().unwrap().push(session_id);
            Box::pin(async { Some(json!({ "ok": true })) })
        }
    }

    #[tokio::test]
    async fn mcp_rpc_route_forwards_valid_velox_session_header_only() {
        let dir = std::env::temp_dir().join(format!("velox-mcp-hdr-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let mgr = McpGatewayManager::new(dir.join("backends.json"), 0);
        let builtin = Arc::new(CaptureSessionBuiltin {
            seen: std::sync::Mutex::new(Vec::new()),
        });
        mgr.register_builtin("cap", builtin.clone()).await;

        let mut h1 = HeaderMap::new();
        h1.insert("X-Velox-Session", "  sess-123  ".parse().unwrap());
        let _ = mcp_rpc_route(
            AxumPath("cap".to_string()),
            State(mgr.clone()),
            h1,
            Json(json!({"jsonrpc": "2.0", "id": 1})),
        )
        .await;

        let mut h2 = HeaderMap::new();
        h2.insert("X-Velox-Session", "${VELOX_SESSION_ID}".parse().unwrap());
        let _ = mcp_rpc_route(
            AxumPath("cap".to_string()),
            State(mgr.clone()),
            h2,
            Json(json!({"jsonrpc": "2.0", "id": 2})),
        )
        .await;

        let mut h3 = HeaderMap::new();
        h3.insert("X-Velox-Session", "   ".parse().unwrap());
        let _ = mcp_rpc_route(
            AxumPath("cap".to_string()),
            State(mgr),
            h3,
            Json(json!({"jsonrpc": "2.0", "id": 3})),
        )
        .await;

        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(
            *builtin.seen.lock().unwrap(),
            vec![Some("sess-123".to_string()), None, None]
        );
    }
}
