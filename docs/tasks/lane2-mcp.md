# Lane 2 (`velox-mcp`) — Native Rust Scale-to-Zero Lazy MCP Gateway

**Model Tier:** FreeLLMAPI Auto #1 (High-Tier Rust Async Networking Lane)
**Skills to follow:** Read `C:\Users\Asus\.agents\skills\rust-best-practices\SKILL.md` and `C:\Users\Asus\.agents\skills\rust-async-patterns\SKILL.md` before coding.
**Architecture Spec:** Read `C:\Users\Asus\OneDrive\Escritorio\Proyectos\velox\docs\ARCHITECTURE.md` and `src-tauri/src/domain/*`.
**Reference Compatibility:** Read `C:\Users\Asus\OneDrive\Escritorio\Proyectos\lazy-mcp-gateway\backends.json` to ensure 100% config format compatibility.

## Owned Files (EDIT ONLY THESE FILES)
- `src-tauri/src/mcp/mod.rs`
- `src-tauri/src/mcp/config.rs`
- `src-tauri/src/mcp/backend.rs`
- `src-tauri/src/mcp/gateway.rs`

Do NOT modify `Cargo.toml`, `domain/*`, `pty/*`, `session/*`, `commands/*`, or `lib.rs`.
Note: Sibling Rust modules (`pty/`, `session/`) are being edited in parallel by other agents. Stay strictly inside `src-tauri/src/mcp/*` and never touch sibling files.
Note: You may import `crate::pty::job_object::ProcessJobObject` (its public API `new() / assign_pid(u32) / terminate()` is guaranteed by Lane 1).

## Exact Requirements

### 1. `src-tauri/src/mcp/config.rs`
- Deserialize `backends.json` with exact compatibility for:
  ```json
  {
    "backends": {
      "codegraph": {
        "command": "node.exe",
        "args": ["..."],
        "cwd": "...",
        "env": { "KEY": "VAL" },
        "idleMs": 120000,
        "timeoutMs": 120000
      }
    }
  }
  ```
- Structs: `McpBackendsFile { pub backends: BTreeMap<String, McpBackendConfig> }`, `McpBackendConfig { pub command: String, #[serde(default)] pub args: Vec<String>, pub cwd: Option<String>, #[serde(default)] pub env: HashMap<String, String>, #[serde(rename = "idleMs", default = "default_idle_ms")] pub idle_ms: u64, #[serde(rename = "timeoutMs", default = "default_timeout_ms")] pub timeout_ms: u64 }`.
- Defaults: `idle_ms = 120_000` (note: `0` means never auto-sleep!), `timeout_ms = 120_000`.
- Helper `load_backends_config(path: &Path) -> VeloxResult<McpBackendsFile>` (returns empty map if file does not exist instead of crashing).
- Helper `config_fingerprint(cfg: &McpBackendConfig) -> String` (SHA-256 hex prefix of command+args+cwd) for schema cache invalidation.

### 2. `src-tauri/src/mcp/backend.rs`
Implement `StdioMcpBackend` providing **on-demand spawn + schema caching + idle scale-to-zero**:
- Cached read-only MCP methods (`CACHED_METHODS`): `"initialize"`, `"tools/list"`, `"resources/list"`, `"resources/templates/list"`, `"prompts/list"`, `"server/discover"`.
- Persistent JSON schema cache stored at `%APPDATA%/velox/mcp-schema-cache.json` (keyed by backend name + `fingerprint`).
- When a JSON-RPC request arrives via `handle_jsonrpc(&self, body: serde_json::Value) -> VeloxResult<Option<serde_json::Value>>`:
  - Notifications (requests without `"id"` or `"notifications/initialized"`): return `Ok(None)` (caller responds `202 Accepted`) — do NOT wake a sleeping backend just for `notifications/initialized`!
  - If method is in `CACHED_METHODS` and cached response exists for current `fingerprint`, return cached result wrapped with the request's `"id"` immediately WITHOUT spawning the child process (`0 MB` RAM!).
  - Otherwise: ensure child process is spawned via `tokio::process::Command` (with `Stdio::piped()` for stdin/stdout, `Stdio::null()` for stderr, Windows `CREATE_NO_WINDOW = 0x08000000` via `creation_flags` on `#[cfg(windows)]` so no console popup ever flashes!).
  - Assign spawned child PID to `ProcessJobObject` so killing the backend terminates its entire process tree!
  - If freshly spawned and request method != `"initialize"`, perform a quick MCP `"initialize"` handshake + `"notifications/initialized"` first so stateful MCP servers accept the real tool call.
  - Serialize newline-delimited JSON-RPC over stdin, await matching response line on stdout with `timeout_ms` timeout (never hold an async mutex across long waits in a way that deadlocks; use an internal request channel or dedicated stdin writer + pending map of `oneshot::Sender<serde_json::Value>`!).
  - Cache response if method is in `CACHED_METHODS` and persist schema cache asynchronously/atomically.
  - Update `last_used_at` and schedule/refresh idle scale-to-zero check: when `in_flight == 0` and `idle_ms > 0` and elapsed >= `idle_ms`, kill the child process tree cleanly!
- Methods on `StdioMcpBackend`:
  - `pub async fn sleep(&self)` — kills child process tree cleanly via `ProcessJobObject::terminate()` + `child.kill()`.
  - `pub async fn status(&self, memory_mb: f64) -> McpBackendStatus` — returns live status including `running`, `pid`, `memory_mb`, `idle_ms`, `remaining_idle_ms`, `in_flight`, `cached_methods`, `last_used_at`.
  - `pub async fn pid(&self) -> Option<u32>`.

### 3. `src-tauri/src/mcp/gateway.rs`
Implement `McpGatewayManager` (`#[derive(Clone)]`, `Arc`-backed) with EXACT public API from `docs/ARCHITECTURE.md`:
- `pub fn new(backends_path: PathBuf, port: u16) -> Self`
- `pub async fn start(&self) -> VeloxResult<McpGatewaySnapshot>`
- `pub async fn snapshot(&self) -> McpGatewaySnapshot`
- `pub async fn sleep_backend(&self, name: &str) -> VeloxResult<McpGatewaySnapshot>`
- `pub async fn sleep_all(&self) -> VeloxResult<McpGatewaySnapshot>`
- `pub async fn shutdown(&self)`
- Behavior of `start()`:
  - Loads backends from `backends_path` and schema cache.
  - Attempts `tokio::net::TcpListener::bind(("127.0.0.1", port))`.
  - If bind succeeds: starts Axum server in a background Tokio task (`McpGatewayMode::Embedded`) with routes:
    - `GET /health` -> `200 OK` JSON `{ "ok": true, "mode": "embedded" }`
    - `GET /status` -> `200 OK` JSON `McpGatewaySnapshot`
    - `POST /mcp/{name}` -> forwards JSON-RPC to named `StdioMcpBackend` (returns `202 Accepted` for notifications, `200 OK` JSON for responses, `404` if unknown backend)
    - `DELETE /mcp/{name}` -> `204 No Content`
    - `POST /admin/sleep/{name}` -> sleeps backend, returns `McpGatewaySnapshot`
    - `POST /admin/sleep-all` -> sleeps all backends, returns `McpGatewaySnapshot`
  - Also spawns a lightweight background reaper task (every 5 seconds) that scales-to-zero any backend whose `idle_ms > 0`, `in_flight == 0`, and idle duration has elapsed.
  - If `bind` returns `AddrInUse`: DO NOT fail! Set mode to `McpGatewayMode::ExternalAttached`, keep loaded backend configs visible in `snapshot()`, and log a clean info message (this allows Velox to coexist smoothly if the user's external `lazy-mcp-gateway` is already running on `:43111`!).
- `snapshot()` computes per-backend RAM (using `sysinfo::System` lightweight targeted PID refresh or `0.0` when sleeping) and fills `McpGatewaySnapshot`.
- `shutdown()` aborts the HTTP server + reaper task and calls `sleep_all()` so zero MCP child processes survive Velox exit.

### 4. Unit Tests
Add unit tests in `config.rs` and `gateway.rs`:
- `backends_config_parses_lazy_mcp_gateway_format`
- `config_fingerprint_changes_when_args_change`
- `missing_backends_file_returns_empty_without_error`
- `gateway_snapshot_reports_sleeping_backends_at_zero_mb`

## Verification
From `C:\Users\Asus\OneDrive\Escritorio\Proyectos\velox\src-tauri`, run:
```bash
cargo test --lib mcp
cargo clippy --lib -- -D warnings
```
Fix any errors or warnings in your files until green, then reply concisely with summary and test output.
