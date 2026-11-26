use crate::domain::config::velox_config_dir;
use crate::domain::error::{VeloxError, VeloxResult};
use crate::domain::session::McpBackendStatus;
use crate::mcp::config::{config_fingerprint, McpBackendConfig};
use crate::pty::job_object::ProcessJobObject;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;

const CACHED_METHODS: &[&str] = &[
    "initialize",
    "tools/list",
    "resources/list",
    "resources/templates/list",
    "prompts/list",
    "server/discover",
];

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct CachedBackendEntry {
    fingerprint: String,
    methods: HashMap<String, Value>,
}

type DiskSchemaCache = HashMap<String, CachedBackendEntry>;

fn schema_cache_path() -> PathBuf {
    velox_config_dir().join("mcp-schema-cache.json")
}

fn load_cached_methods(name: &str, fingerprint: &str) -> HashMap<String, Value> {
    let path = schema_cache_path();
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return HashMap::new();
    };
    let Ok(store) = serde_json::from_str::<DiskSchemaCache>(&raw) else {
        return HashMap::new();
    };
    match store.get(name) {
        Some(entry) if entry.fingerprint == fingerprint => entry.methods.clone(),
        _ => HashMap::new(),
    }
}

fn persist_cached_methods(name: &str, fingerprint: &str, methods: &HashMap<String, Value>) {
    let path = schema_cache_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut store: DiskSchemaCache = std::fs::read_to_string(&path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default();

    store.insert(
        name.to_string(),
        CachedBackendEntry {
            fingerprint: fingerprint.to_string(),
            methods: methods.clone(),
        },
    );

    if let Ok(serialized) = serde_json::to_string_pretty(&store) {
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, serialized).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

struct ActiveProcess {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    job: Option<ProcessJobObject>,
    pid: Option<u32>,
    initialized: bool,
}

impl ActiveProcess {
    async fn kill_cleanly(mut self) {
        if let Some(job) = self.job.take() {
            let _ = job.terminate();
        }
        let _ = self.child.kill().await;
    }
}

struct BackendState {
    proc: Option<ActiveProcess>,
    in_flight: usize,
    last_used_instant: Option<Instant>,
    last_used_at: Option<DateTime<Utc>>,
    schema_cache: HashMap<String, Value>,
    next_internal_id: u64,
}

/// On-demand stdio MCP backend with persistent schema caching and idle scale-to-zero.
pub struct StdioMcpBackend {
    name: String,
    config: McpBackendConfig,
    fingerprint: String,
    state: Mutex<BackendState>,
}

impl StdioMcpBackend {
    pub fn new(name: String, config: McpBackendConfig) -> Self {
        let fingerprint = config_fingerprint(&config);
        let schema_cache = load_cached_methods(&name, &fingerprint);
        Self {
            name,
            config,
            fingerprint,
            state: Mutex::new(BackendState {
                proc: None,
                in_flight: 0,
                last_used_instant: None,
                last_used_at: None,
                schema_cache,
                next_internal_id: 1,
            }),
        }
    }

    pub async fn pid(&self) -> Option<u32> {
        self.state.lock().await.proc.as_ref().and_then(|p| p.pid)
    }

    pub async fn status(&self, memory_mb: f64) -> McpBackendStatus {
        let state = self.state.lock().await;
        let running = state.proc.is_some();
        let pid = state.proc.as_ref().and_then(|p| p.pid);
        let remaining_idle_ms = if running && self.config.idle_ms > 0 {
            let elapsed = state
                .last_used_instant
                .map(|t| t.elapsed().as_millis() as u64)
                .unwrap_or(0);
            Some(self.config.idle_ms.saturating_sub(elapsed))
        } else {
            None
        };

        McpBackendStatus {
            name: self.name.clone(),
            running,
            pid,
            memory_mb: if running { memory_mb } else { 0.0 },
            idle_ms: self.config.idle_ms,
            remaining_idle_ms,
            in_flight: state.in_flight,
            cached_methods: state.schema_cache.len(),
            last_used_at: state.last_used_at.map(|t| t.to_rfc3339()),
        }
    }

    /// Terminate the active child process tree cleanly, returning backend to 0 MB sleeping state.
    pub async fn sleep(&self) {
        let active = {
            let mut state = self.state.lock().await;
            state.proc.take()
        };
        if let Some(proc_handle) = active {
            tracing::info!(backend = %self.name, pid = ?proc_handle.pid, "Scaling MCP backend to zero");
            proc_handle.kill_cleanly().await;
        }
    }

    /// Reap this backend if `idle_ms > 0`, `in_flight == 0`, and idle duration has elapsed.
    pub async fn reap_if_idle(&self) -> bool {
        if self.config.idle_ms == 0 {
            return false;
        }
        let should_sleep = {
            let state = self.state.lock().await;
            state.proc.is_some()
                && state.in_flight == 0
                && state
                    .last_used_instant
                    .is_some_and(|t| t.elapsed().as_millis() as u64 >= self.config.idle_ms)
        };
        if should_sleep {
            self.sleep().await;
            true
        } else {
            false
        }
    }

    /// Handle a single JSON-RPC 2.0 request using schema cache when sleeping, or waking on demand.
    pub async fn handle_jsonrpc(&self, body: Value) -> VeloxResult<Option<Value>> {
        let req_id = body.get("id").cloned();
        let method = body
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();

        // Notifications must not wake a sleeping backend
        if req_id.is_none() || method == "notifications/initialized" {
            return Ok(None);
        }

        let is_cacheable = CACHED_METHODS.contains(&method.as_str());

        let mut state = self.state.lock().await;
        if is_cacheable {
            if let Some(cached) = state.schema_cache.get(&method).cloned() {
                let mut resp = cached;
                if let Some(obj) = resp.as_object_mut() {
                    if let Some(id_val) = req_id {
                        obj.insert("id".to_string(), id_val);
                    }
                }
                return Ok(Some(resp));
            }
        }

        state.in_flight = state.in_flight.saturating_add(1);
        let result = self
            .forward_live_request(&mut state, &method, body, req_id, is_cacheable)
            .await;
        state.in_flight = state.in_flight.saturating_sub(1);
        state.last_used_instant = Some(Instant::now());
        state.last_used_at = Some(Utc::now());

        if result.is_err() {
            // Reset broken stdio process so next request respawns cleanly
            if let Some(broken) = state.proc.take() {
                broken.kill_cleanly().await;
            }
        }

        result
    }

    async fn forward_live_request(
        &self,
        state: &mut BackendState,
        method: &str,
        mut body: Value,
        orig_id: Option<Value>,
        is_cacheable: bool,
    ) -> VeloxResult<Option<Value>> {
        if state.proc.is_none() {
            state.proc = Some(self.spawn_process()?);
        }

        let timeout_dur = Duration::from_millis(self.config.timeout_ms.max(1_000));

        // Perform MCP initialize handshake first when waking for a non-initialize tool call
        if let Some(ref mut active) = state.proc {
            if !active.initialized && method != "initialize" {
                let init_id = state.next_internal_id;
                state.next_internal_id = state.next_internal_id.wrapping_add(1);
                let init_req = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": init_id,
                    "method": "initialize",
                    "params": {
                        "protocolVersion": "2024-11-05",
                        "capabilities": {},
                        "clientInfo": { "name": "velox-lazy-mcp", "version": "0.1.0" }
                    }
                });
                let _ = send_and_await_jsonrpc(active, &init_req, init_id, timeout_dur).await?;
                let notif = serde_json::json!({
                    "jsonrpc": "2.0",
                    "method": "notifications/initialized"
                });
                write_json_line(&mut active.stdin, &notif).await?;
                active.initialized = true;
            }
        }

        let wire_id = state.next_internal_id;
        state.next_internal_id = state.next_internal_id.wrapping_add(1);
        if let Some(obj) = body.as_object_mut() {
            obj.insert("id".to_string(), Value::from(wire_id));
        }

        let active = state
            .proc
            .as_mut()
            .ok_or_else(|| VeloxError::Mcp(format!("Backend {} not running", self.name)))?;

        let mut response = send_and_await_jsonrpc(active, &body, wire_id, timeout_dur).await?;
        if method == "initialize" {
            active.initialized = true;
            let notif = serde_json::json!({
                "jsonrpc": "2.0",
                "method": "notifications/initialized"
            });
            let _ = write_json_line(&mut active.stdin, &notif).await;
        }

        if let Some(obj) = response.as_object_mut() {
            if let Some(id_val) = orig_id {
                obj.insert("id".to_string(), id_val);
            }
        }

        if is_cacheable && response.get("error").is_none() {
            state
                .schema_cache
                .insert(method.to_string(), response.clone());
            persist_cached_methods(&self.name, &self.fingerprint, &state.schema_cache);
        }

        Ok(Some(response))
    }

    fn spawn_process(&self) -> VeloxResult<ActiveProcess> {
        let mut cmd = Command::new(&self.config.command);
        cmd.args(&self.config.args);
        if let Some(ref cwd) = self.config.cwd {
            cmd.current_dir(cwd);
        }
        for (k, v) in &self.config.env {
            cmd.env(k, v);
        }
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        #[cfg(windows)]
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW

        let mut child = cmd.spawn().map_err(|e| {
            VeloxError::Mcp(format!("Failed to spawn MCP backend '{}': {e}", self.name))
        })?;

        let pid = child.id();
        let job = ProcessJobObject::new().ok();
        if let (Some(ref job_obj), Some(p)) = (&job, pid) {
            let _ = job_obj.assign_pid(p);
        }

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| VeloxError::Mcp(format!("MCP backend '{}' missing stdin", self.name)))?;
        let stdout = child.stdout.take().ok_or_else(|| {
            VeloxError::Mcp(format!("MCP backend '{}' missing stdout", self.name))
        })?;

        tracing::info!(backend = %self.name, ?pid, "Woke on-demand stdio MCP backend");

        Ok(ActiveProcess {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            job,
            pid,
            initialized: false,
        })
    }
}

async fn write_json_line(stdin: &mut ChildStdin, payload: &Value) -> VeloxResult<()> {
    let mut line = serde_json::to_string(payload)?;
    line.push('\n');
    stdin
        .write_all(line.as_bytes())
        .await
        .map_err(|e| VeloxError::Mcp(format!("MCP stdin write failed: {e}")))?;
    stdin
        .flush()
        .await
        .map_err(|e| VeloxError::Mcp(format!("MCP stdin flush failed: {e}")))?;
    Ok(())
}

async fn send_and_await_jsonrpc(
    active: &mut ActiveProcess,
    payload: &Value,
    expected_id: u64,
    timeout_dur: Duration,
) -> VeloxResult<Value> {
    write_json_line(&mut active.stdin, payload).await?;

    let read_fut = async {
        let mut line = String::new();
        loop {
            line.clear();
            let n = active
                .stdout
                .read_line(&mut line)
                .await
                .map_err(|e| VeloxError::Mcp(format!("MCP stdout read failed: {e}")))?;
            if n == 0 {
                return Err(VeloxError::Mcp(
                    "MCP backend closed stdout before responding".to_string(),
                ));
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Ok(val) = serde_json::from_str::<Value>(trimmed) {
                if val.get("id").and_then(Value::as_u64) == Some(expected_id) {
                    return Ok(val);
                }
            }
        }
    };

    tokio::time::timeout(timeout_dur, read_fut)
        .await
        .map_err(|_| VeloxError::Mcp("MCP backend request timed out".to_string()))?
}
