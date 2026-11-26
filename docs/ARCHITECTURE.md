# Velox — Architecture & Worker Contracts

Velox is a lightweight, memory-conscious Agentic Development Environment (ADE) desktop terminal built with **Rust (Tauri v2 + `portable-pty` ConPTY + Windows Job Objects + embedded Lazy MCP Gateway)** and **React + TypeScript + Tailwind v4 + `xterm.js` WebGL**.

## Core Architectural Pillars

1. **Zero Orphan Processes**: Every spawned ConPTY agent and on-demand stdio MCP backend is assigned to a Windows `ProcessJobObject` (`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`). Closing or hibernating a tab terminates its entire process tree immediately.
2. **Lazy-Restore & Hibernation (Sleep / Wake)**: Persisted sessions load as `sleeping` (`0 MB` RAM) on startup. Clicking a sleeping session wakes it in place using the agent's resume flag (`claude --resume <id>`, `agy --conversation <id>`, `opencode --session <id>`, `codex resume <id>`). Idle sessions auto-hibernate after `autoHibernateMinutes`.
3. **Embedded Scale-to-Zero Lazy MCP Gateway**: Native Rust HTTP JSON-RPC gateway compatible with `lazy-mcp-gateway/backends.json`, caching `initialize`/`tools/list`/`resources/list`/`prompts/list` so sleeping MCP backends consume `0 MB`, spawning stdio children on-demand and terminating them after `idleMs`.
4. **Bounded Scrollback**: Fixed-capacity ring buffer (default `2 MB` per active pane) prevents long sessions from leaking memory.

## Strict Lane Ownership (DO NOT EDIT OUTSIDE YOUR LANE)

Shared Read-Only Contracts (owned by Coordinator):
- `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, `package.json`
- `src-tauri/src/domain/*`
- `src/types/ipc.ts`, `src/lib/tauri.ts`, `src/main.tsx`

| Lane | Owner Files | Responsibility |
|------|-------------|----------------|
| **Lane 1 (`velox-pty`)** | `src-tauri/src/pty/*` | Windows `ProcessJobObject` RAII, `RingScrollback`, `PtyManager` (`portable-pty` ConPTY spawn/read/write/resize/kill) + unit tests |
| **Lane 2 (`velox-mcp`)** | `src-tauri/src/mcp/*` | `backends.json` config loader, on-demand stdio `McpBackend` with schema cache + idle scale-to-zero, Axum `McpGatewayManager` + unit tests |
| **Lane 3 (`velox-session`)** | `src-tauri/src/session/*`, `src-tauri/src/telemetry/*`, `src-tauri/src/commands/*`, `src-tauri/src/lib.rs`, `src-tauri/src/main.rs` | Session persistence (`store.rs`), status & resume-ID detector (`detector.rs`), `SessionManager` (lazy-restore + auto-hibernate + sleep/wake), process-tree RAM telemetry (`monitor.rs`), Tauri IPC commands & `lib.rs` wiring |
| **Lane 4 (`velox-ui-core`)** | `src/hooks/useSessions.ts`, `src/components/TerminalPane.tsx`, `src/components/Sidebar.tsx`, `src/components/AgentLauncherModal.tsx`, `src/App.tsx` | `xterm.js` WebGL terminal pane, Sleep/Wake overlay, Sidebar with `+` button & agent status/RAM badges, Command Launcher modal, global shortcuts |
| **Lane 5 (`velox-ui-chrome`)** | `src/index.css`, `src/hooks/useSystemTelemetry.ts`, `src/components/TitleBar.tsx`, `src/components/McpDrawer.tsx`, `src/components/EmptyState.tsx`, `src/components/icons/AgentIcons.tsx` | Custom frameless TitleBar, Lazy MCP Drawer, EmptyState welcome view, crisp bespoke SVG agent icons, design tokens & CSS |

## Rust Public Interfaces Between Lanes

### Lane 1 (`crate::pty`)
- `crate::pty::job_object::ProcessJobObject`:
  - `pub fn new() -> VeloxResult<Self>`
  - `pub fn assign_pid(&self, pid: u32) -> VeloxResult<()>`
  - `pub fn terminate(&self) -> VeloxResult<()>`
  - Implements `Send + Sync` and `Drop` (closes handle; `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` kills entire tree). Non-Windows compiles cleanly as a no-op stub.
- `crate::pty::scrollback::RingScrollback`:
  - `pub fn new(max_bytes: usize) -> Self`
  - `pub fn push(&mut self, chunk: &[u8])`
  - `pub fn snapshot(&self) -> Vec<u8>`
  - `pub fn clear(&mut self)`
  - `pub fn len(&self) -> usize`
  - `pub fn is_empty(&self) -> bool`
- `crate::pty::manager::{PtyManager, OutputCallback, ExitCallback}`:
  - `pub type OutputCallback = Arc<dyn Fn(&str, &[u8]) + Send + Sync>;`
  - `pub type ExitCallback = Arc<dyn Fn(&str, Option<i32>) + Send + Sync>;`
  - `pub fn new(scrollback_bytes: usize) -> Self`
  - `pub fn spawn(&self, session_id: &str, spec: &AgentSpawnSpec, cols: u16, rows: u16, on_output: OutputCallback, on_exit: ExitCallback) -> VeloxResult<u32>`
  - `pub fn write(&self, session_id: &str, data: &[u8]) -> VeloxResult<()>`
  - `pub fn resize(&self, session_id: &str, cols: u16, rows: u16) -> VeloxResult<()>`
  - `pub fn kill(&self, session_id: &str) -> VeloxResult<()>`
  - `pub fn kill_all(&self)`
  - `pub fn get_scrollback(&self, session_id: &str) -> Vec<u8>`
  - `pub fn get_pid(&self, session_id: &str) -> Option<u32>`
  - `pub fn is_alive(&self, session_id: &str) -> bool`

### Lane 2 (`crate::mcp`)
- `crate::mcp::gateway::McpGatewayManager` (`Clone` via `Arc`):
  - `pub fn new(backends_path: PathBuf, port: u16) -> Self`
  - `pub async fn start(&self) -> VeloxResult<McpGatewaySnapshot>`
  - `pub async fn snapshot(&self) -> McpGatewaySnapshot`
  - `pub async fn sleep_backend(&self, name: &str) -> VeloxResult<McpGatewaySnapshot>`
  - `pub async fn sleep_all(&self) -> VeloxResult<McpGatewaySnapshot>`
  - `pub async fn shutdown(&self)`

## Tauri IPC Command Contract (1:1 with `src/lib/tauri.ts`)

- `list_sessions() -> VeloxResult<Vec<SessionRecord>>`
- `create_session(preset_id: String, cwd: Option<String>, title: Option<String>, cols: u16, rows: u16) -> VeloxResult<SessionRecord>`
- `wake_session(session_id: String, cols: u16, rows: u16) -> VeloxResult<SessionRecord>`
- `sleep_session(session_id: String) -> VeloxResult<SessionRecord>`
- `sleep_idle_sessions(exclude_session_id: Option<String>) -> VeloxResult<Vec<SessionRecord>>`
- `close_session(session_id: String) -> VeloxResult<()>`
- `rename_session(session_id: String, title: String) -> VeloxResult<SessionRecord>`
- `write_pty(session_id: String, data: String) -> VeloxResult<()>`
- `resize_pty(session_id: String, cols: u16, rows: u16) -> VeloxResult<()>`
- `get_scrollback(session_id: String) -> VeloxResult<String>` (returns base64-encoded bytes)
- `list_agent_presets() -> VeloxResult<Vec<AgentPreset>>`
- `list_recent_dirs() -> VeloxResult<Vec<String>>`
- `get_mcp_status() -> VeloxResult<McpGatewaySnapshot>`
- `sleep_mcp_backend(name: String) -> VeloxResult<McpGatewaySnapshot>`
- `sleep_all_mcps() -> VeloxResult<McpGatewaySnapshot>`
- `get_telemetry() -> VeloxResult<SystemTelemetry>`
- `get_config() -> VeloxResult<VeloxConfig>`
- `update_config(config: VeloxConfig) -> VeloxResult<VeloxConfig>`

Tauri Events:
- `"pty-output"`: `PtyOutputEvent { sessionId, dataB64 }`
- `"session-updated"`: `SessionUpdatedEvent { session }`
- `"telemetry-updated"`: `SystemTelemetry`
- `"mcp-updated"`: `McpGatewaySnapshot`

## Design System Tokens (Intentional Dark Industrial Minimalism)

- **Canvas Void** (terminal background): `#0B0D12`
- **Chrome Surface** (sidebar, titlebar): `#11141C`
- **Elevated Popover** (launcher modal, MCP drawer): `#171B26`
- **Interactive Hover**: `#1C2130`
- **Active Row**: `#1F2638`
- **Hairline Divider**: `rgba(255, 255, 255, 0.07)`
- **Primary Ink**: `#E6EAF2`
- **Secondary Ink**: `#98A2B8`
- **Muted Ink**: `#64708A`
- **Semantic Agent Accents** (strictly for agent identity):
  - Claude: `#D97757` (profile badges: personal `#E8B4D9`, trabajo `#7DD3C8`)
  - AGY: `#4E8DF5`
  - Codex: `#10A37F`
  - OpenCode: `#C4B5FD`
  - Shell: `#94A3B8`
  - Blocked: `#F43F5E`
  - Sleeping: `#52607A`
- **Typography**:
  - UI Sans: `'Inter', 'Segoe UI Variable', system-ui, -apple-system, sans-serif`
  - Mono / Numerals: `'JetBrains Mono', 'Cascadia Code', Consolas, monospace` with `tabular-nums` for RAM/timers
- **Discipline**:
  - Clean sentence case for UI labels (avoid tracked-out ALL-CAPS eyebrows).
  - No generic gradient-card SaaS clutter; crisp 1px borders, tight spacing, fast tactile transitions (`120ms ease-out`).
  - All generated UI copy, code, and comments must be in clean English.

## Rust Standards (Mandatory)
- Toolchain: `rust-toolchain.toml` pins `stable-x86_64-pc-windows-gnu` (WinLibs MinGW64 UCRT). Run `cargo check` / `cargo test` from `velox/src-tauri`.
- Follow `rust-best-practices` and `rust-async-patterns` skills.
- No `.unwrap()` or `.expect()` in production code (tests only).
- Never hold a `std::sync::Mutex` / `tokio::sync::Mutex` across `.await` when blocking I/O or external calls occur.
- Propagate errors via `VeloxResult<T>` and `?`.
- Keep modules focused, small, and clean under `cargo clippy -- -D warnings`.
