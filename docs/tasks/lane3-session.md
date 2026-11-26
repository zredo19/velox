# Lane 3 (`velox-session`) — Session Lifecycle, Lazy-Restore, Hibernation, Telemetry & Tauri IPC

**Model Tier:** FreeLLMAPI Auto #2 (High-Tier Rust Backend Integration Lane)
**Skills to follow:** Read `C:\Users\Asus\.agents\skills\rust-best-practices\SKILL.md` and `C:\Users\Asus\.agents\skills\rust-async-patterns\SKILL.md` before coding.
**Architecture Spec:** Read `C:\Users\Asus\OneDrive\Escritorio\Proyectos\velox\docs\ARCHITECTURE.md` and `src-tauri/src/domain/*`.

## Owned Files (EDIT ONLY THESE FILES)
- `src-tauri/src/session/mod.rs`
- `src-tauri/src/session/store.rs`
- `src-tauri/src/session/detector.rs`
- `src-tauri/src/session/manager.rs`
- `src-tauri/src/telemetry/mod.rs`
- `src-tauri/src/telemetry/monitor.rs`
- `src-tauri/src/commands/mod.rs`
- `src-tauri/src/commands/sessions.rs`
- `src-tauri/src/commands/agents.rs`
- `src-tauri/src/commands/mcp.rs`
- `src-tauri/src/commands/system.rs`
- `src-tauri/src/lib.rs`
- `src-tauri/src/main.rs`

Do NOT modify `Cargo.toml`, `domain/*`, `pty/*`, `mcp/*`, or `src/*`.
Note: Sibling Rust modules (`pty/`, `mcp/`) are being edited in parallel by other agents. Stay strictly inside your owned files and never touch `pty/*` or `mcp/*`.
Use ONLY the documented public APIs of `crate::pty::manager::PtyManager` and `crate::mcp::gateway::McpGatewayManager` from `docs/ARCHITECTURE.md` (their stubs already compile with the exact signatures!).

## Exact Requirements

### 1. `src-tauri/src/session/store.rs`
Atomic JSON persistence for sessions at `%APPDATA%/velox/sessions.json`:
- `pub fn sessions_file_path() -> PathBuf`
- `pub fn load_sessions() -> Vec<SessionRecord>`:
  - Reads `sessions.json`.
  - **Lazy-Restore Rule**: Every restored session MUST have `status = SessionStatus::Sleeping`, `pid = None`, `memory_mb = 0.0`, `exit_code = None` so startup consumes `0 MB` of agent RAM regardless of how many sessions are saved!
- `pub fn save_sessions(sessions: &[SessionRecord]) -> VeloxResult<()>`:
  - Writes atomically via `.json.tmp` + `rename`.

### 2. `src-tauri/src/session/detector.rs`
Lightweight status & resume-ID detector for coding agents:
- `pub fn strip_ansi(input: &str) -> String` (strips CSI/OSC ANSI escape sequences cleanly using `OnceLock<Regex>`).
- `pub fn detect_status_from_output(kind: AgentKind, raw_chunk: &[u8]) -> Option<SessionStatus>`:
  - Detects `"blocked"` when prompt approval cues appear (e.g. `Allow once`, `Do you want to proceed`, `[Y/n]`, `Approve`, `Esc to cancel`).
  - Detects `"idle"` when prompt ready cues appear (e.g. `❯`, `Ask anything`, `> ` at line start).
  - Otherwise returns `Some(SessionStatus::Running)` when substantial non-control output arrives.
- `pub fn discover_latest_agent_session_id(kind: AgentKind, profile: Option<&str>, cwd: &str, started_after: DateTime<Utc>) -> Option<String>`:
  - Enables true `--resume` when waking a hibernated session!
  - For `AgentKind::Claude`: scans `~/.claude-personal/projects/*/*.jsonl` (or `~/.claude-trabajo`, or `~/.claude` based on `profile`) for the most recently modified UUID `.jsonl` file created/modified after `started_after` (falling back to latest modified UUID in the matching project dir).
  - For `AgentKind::Agy`: scans `~/.gemini/antigravity-cli/brain/*` for the most recently modified UUID directory modified after `started_after`.
  - Returns `None` gracefully if nothing matches (never errors).

### 3. `src-tauri/src/session/manager.rs`
Implement `SessionManager` (`Arc`-shareable across Tauri state and background tasks):
- Holds: `sessions: RwLock<Vec<SessionRecord>>`, `pty: Arc<PtyManager>`, `config: RwLock<VeloxConfig>`, `presets: Vec<AgentPreset>`, `last_output_at: Mutex<HashMap<String, Instant>>`.
- Methods:
  - `pub fn new(config: VeloxConfig) -> Self` (loads persisted sessions in `Sleeping` state if `config.lazy_restore_on_startup`, initializes `PtyManager::new(config.scrollback_bytes)`, builds presets via `build_default_presets()`).
  - `pub fn list_sessions(&self) -> Vec<SessionRecord>`
  - `pub fn list_presets(&self) -> Vec<AgentPreset>`
  - `pub fn list_recent_dirs(&self) -> Vec<String>`
  - `pub fn get_config(&self) -> VeloxConfig`
  - `pub fn update_config(&self, cfg: VeloxConfig) -> VeloxResult<VeloxConfig>`
  - `pub fn create_session(&self, app: &AppHandle, preset_id: &str, cwd: Option<String>, title: Option<String>, cols: u16, rows: u16) -> VeloxResult<SessionRecord>`:
    - Finds preset by `preset_id`, resolves `cwd` (defaults to `config.default_cwd`), records `cwd` in `config.recent_dirs` and saves config.
    - Generates default `title` from folder basename (e.g. `"velox"` or `"Proyectos"`) if not provided.
    - Creates `SessionRecord`, spawns PTY via `resolve_spawn_spec` + `pty.spawn(...)` with callbacks that:
      - Encode output bytes as base64 (`base64::engine::general_purpose::STANDARD`) and emit `"pty-output"` (`PtyOutputEvent`).
      - Update `last_output_at` & lightweight status via `detector::detect_status_from_output` and emit `"session-updated"` when status changes.
      - On exit: update status to `SessionStatus::Exited` (unless already `Sleeping`), store `exit_code`, capture `agent_session_id` via `discover_latest_agent_session_id`, clear `pid`/`memory_mb`, persist, and emit `"session-updated"`.
    - Persists sessions and emits `"session-updated"`.
  - `pub fn wake_session(&self, app: &AppHandle, session_id: &str, cols: u16, rows: u16) -> VeloxResult<SessionRecord>`:
    - If already alive (`pty.is_alive`), returns current record.
    - Resolves spawn spec WITH `record.agent_session_id.as_deref()` so Claude/AGY/OpenCode/Codex resume their conversation automatically!
    - Spawns PTY, updates `status = Running`, `pid`, `last_active_at`, persists, emits `"session-updated"`.
  - `pub fn sleep_session(&self, app: &AppHandle, session_id: &str) -> VeloxResult<SessionRecord>`:
    - Before killing, tries to capture/refresh `agent_session_id` via `discover_latest_agent_session_id` so waking later resumes cleanly!
    - Kills PTY tree via `pty.kill(session_id)` (0 MB RAM!).
    - Sets `status = SessionStatus::Sleeping`, `pid = None`, `memory_mb = 0.0`, persists, emits `"session-updated"`.
  - `pub fn sleep_idle_sessions(&self, app: &AppHandle, exclude_session_id: Option<&str>) -> VeloxResult<Vec<SessionRecord>>`:
    - Hibernates all currently awake sessions except `exclude_session_id` (and skips `Starting`), returns updated session list.
  - `pub fn close_session(&self, app: &AppHandle, session_id: &str) -> VeloxResult<()>`:
    - Kills PTY tree, removes from sessions list, persists.
  - `pub fn rename_session(&self, app: &AppHandle, session_id: &str, title: String) -> VeloxResult<SessionRecord>`
  - `pub fn write_pty(&self, session_id: &str, data: &str) -> VeloxResult<()>` (updates `last_active_at`)
  - `pub fn resize_pty(&self, session_id: &str, cols: u16, rows: u16) -> VeloxResult<()>`
  - `pub fn get_scrollback_b64(&self, session_id: &str) -> String`
  - `pub fn apply_ram_telemetry(&self, session_ram: &HashMap<String, f64>)`
  - `pub fn hibernate_stale_sessions(&self, app: &AppHandle)`: called periodically by background timer; sleeps sessions idle longer than `config.auto_hibernate_minutes` (when > 0).
  - `pub fn shutdown(&self)`: calls `pty.kill_all()`.

### 4. `src-tauri/src/telemetry/monitor.rs`
Lightweight process-tree RAM monitor using `sysinfo = "0.35"`:
- `pub struct TelemetryMonitor` wrapping `Mutex<sysinfo::System>`.
- `pub fn sample(&self, sessions: &[SessionRecord], mcp_memory_mb: f64) -> SystemTelemetry`:
  - Refreshes ONLY processes (`ProcessesToUpdate::All`, `ProcessRefreshKind::nothing().with_memory()` — fast, zero CPU/disk overhead!).
  - Builds parent->children PID map once per sample, then for each awake session with `pid: Some(root_pid)`, sums memory (RSS in bytes / `1_048_576.0`) of `root_pid` + ALL descendants (catching every spawned `node`, `python`, `claude`, `agy`, `cmd`!).
  - Computes `app_memory_mb` (current Velox process RSS), `sessions_memory_mb`, `mcp_memory_mb`, `total_memory_mb`, `active_sessions`, `sleeping_sessions`, and `session_ram` map.

### 5. `src-tauri/src/commands/*` & `src-tauri/src/lib.rs`
- Define `pub struct AppState { pub sessions: Arc<SessionManager>, pub mcp: McpGatewayManager, pub telemetry: Arc<TelemetryMonitor> }`.
- Implement ALL 18 Tauri IPC commands listed in `docs/ARCHITECTURE.md` with exact names and parameter names (remember Tauri converts Rust `snake_case` args to JS `camelCase`, e.g. `preset_id` -> `presetId`, `session_id` -> `sessionId`, `exclude_session_id` -> `excludeSessionId`).
- Wire `src-tauri/src/lib.rs`:
  - Initialize `tracing_subscriber`.
  - Load `VeloxConfig::load_or_default()`, construct `AppState`.
  - In `.setup(|app| ...)`:
    - Start `McpGatewayManager` if `config.mcp_gateway_enabled` and emit `"mcp-updated"`.
    - Spawn a single lightweight background Tokio task ticking every 4 seconds:
      - Samples `TelemetryMonitor` + `McpGatewayManager::snapshot()`, updates session RAM map, emits `"telemetry-updated"` and `"mcp-updated"`.
      - Every 60 seconds, calls `sessions.hibernate_stale_sessions(&handle)`.
  - In `.on_window_event` (when main window `Destroyed` or `CloseRequested`) or `RunEvent::Exit`: cleanly call `sessions.shutdown()` and `tauri::async_runtime::block_on(mcp.shutdown())` so ZERO processes linger after closing Velox!

### 6. Unit Tests
Add unit tests in `session/detector.rs` and `session/store.rs`:
- `strip_ansi_removes_csi_and_osc_sequences`
- `detect_status_identifies_blocked_approval_prompts`
- `lazy_restore_forces_all_loaded_sessions_to_sleeping_zero_mb`

## Verification
From `C:\Users\Asus\OneDrive\Escritorio\Proyectos\velox\src-tauri`, run:
```bash
cargo test --lib session
cargo check
cargo clippy --lib -- -D warnings
```
Fix any errors or warnings in your files until green, then reply concisely with summary and test output.
