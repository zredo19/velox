# Lane A — Velox Online backend core (hub, WebSocket server/client, session hooks, IPC)

**Read first:** `docs/online/PLAN.md` (whole file), `docs/ARCHITECTURE.md` (Rust standards),
`src-tauri/src/team/protocol.rs`, `src-tauri/src/team/api.rs`, `src-tauri/src/domain/config.rs`
(`TeamConfig`), `src-tauri/src/session/manager.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/mcp/gateway.rs`.
Skills: `.agents/skills/rust-best-practices/SKILL.md`, `.agents/skills/rust-async-patterns/SKILL.md`.

**Work ONLY in:** `D:\Workspace\velox-online\lane-a` (git worktree, branch `online/lane-a`).
Never edit `D:\Workspace\velox` (the main checkout) or the other lanes' worktrees.

## Owned files (edit only these)

- `src-tauri/src/team/hub.rs`, `team/server.rs`, `team/client.rs`, `team/tailscale.rs`
- `src-tauri/src/commands/team.rs` (new), `commands/mod.rs`, `commands/sessions.rs`, `commands/system.rs`
- `src-tauri/src/session/manager.rs` (additive hooks only, no refactors)
- `src-tauri/src/lib.rs`
- `src-tauri/tests/team_tests.rs` (new) and `#[cfg(test)]` modules inside your files

Do NOT edit: `team/protocol.rs`, `team/api.rs`, `team/mcp.rs`, `team/delivery.rs`, `team/summary.rs`,
`domain/*`, `mcp/*`, `pty/*`, anything under `src/` (frontend), `Cargo.toml`.
Lane B is implementing `team::mcp::TeamMcp`, `team::delivery::build_task_input` and
`team::summary::last_action_summary` in parallel; their stubs already compile with final signatures,
so just call them. Dependencies available: `axum` (with `ws`), `tokio-tungstenite`, `futures-util`,
`vt100`, `uuid`, `sha2`, `base64`, `chrono`, `regex`. Need another crate? Write it in your status
file under "Questions" and keep going with a workaround.

## Requirements

### 1. `team/tailscale.rs`
- `detect_tailscale_ipv4() -> Option<Ipv4Addr>`: run `tailscale ip -4` (try `tailscale` on PATH, then
  `C:\Program Files\Tailscale\tailscale.exe`), with `CREATE_NO_WINDOW` (0x0800_0000) on Windows and a
  3 s timeout. First line parsed as `Ipv4Addr`, accepted only inside `100.64.0.0/10`. Cache 60 s.
- Pure helper `parse_tailscale_ip(output: &str) -> Option<Ipv4Addr>` + unit tests (range check included).

### 2. `session/manager.rs` hooks (additive)
- `pub trait SessionTap: Send + Sync { fn on_output(&self, session_id: &str, data: &[u8], pos: StreamPos); fn on_resize(&self, session_id: &str, cols: u16, rows: u16); }`
  stored as `Arc<RwLock<Option<Arc<dyn SessionTap>>>>`, set with `pub fn set_tap(&self, tap: Arc<dyn SessionTap>)`.
  Call `on_output` from the existing output callback (after the Tauri emit) and `on_resize` from
  `resize_pty` and when a PTY spawns with its initial size. Must be cheap when nothing is shared.
- Track each live PTY size: `pub fn pty_size(&self, session_id: &str) -> Option<(u16, u16)>`.
- `pub fn set_session_shared(&self, app: &AppHandle, session_id: &str, shared: bool) -> VeloxResult<SessionRecord>`:
  persist (save_sessions), emit `session-updated`.
- `pub fn write_pty_bytes(&self, session_id: &str, data: &[u8]) -> VeloxResult<()>` (updates last activity like `write_pty`).
- Keep every existing behaviour and test green.

### 3. `team/hub.rs` — `TeamHub`
- `pub fn new(sessions: Arc<SessionManager>, config: TeamConfig) -> Arc<Self>`
- `pub fn start(self: &Arc<Self>, app: AppHandle)`: starts the server (if enabled, token non-empty and a
  bind address exists), dials every peer, and runs a 2 s tick that: rebuilds the local session list
  (`LocalSessionInfo` for live sessions; `RemoteSessionInfo.summary` from
  `team::summary::last_action_summary`, cached 5 s per session; `folder` = cwd basename;
  `cols/rows` from `pty_size`, default 120x30), pushes `ServerMsg::Sessions` to connected clients
  when the shared list changed, emits `team-updated` when the snapshot changed, expires `Pending`
  tasks older than 30 min (`Expired`, notify requester).
- `pub async fn apply_config(self: &Arc<Self>, config: TeamConfig)`: restart server/clients only if
  the relevant fields changed; always emit `team-updated`.
- `pub async fn shutdown(&self)`.
- `impl SessionTap for TeamHub`: forward `Output` / `Resize` only to connections subscribed to that
  session, and only if it is shared. Use bounded per-connection channels and `try_send`; if a
  viewer lags (channel full), drop that subscription and send it a fresh `Snapshot` once it drains.
- Sharing turned off / session slept / closed → `SessionGone` to its subscribers.
- Incoming `TaskRequest` (remote) → validate (session exists, live, shared; text ≤ `MAX_TASK_TEXT_BYTES`;
  rate limit 20/min/connection) → `TaskTicket { direction: Incoming, state: Pending }` → emit
  `team-tasks-updated`. Remember which connection asked so `TaskUpdate`s go back to it.
- `pub async fn respond_task(&self, request_id: &str, accept: bool) -> VeloxResult<TaskTicket>`:
  Reject → `Rejected`. Accept → `Accepted`, then type it: for each chunk of
  `team::delivery::build_task_input(kind, &text)` call `write_pty_bytes`, sleeping
  `team::delivery::CHUNK_DELAY_MS` between chunks (tokio sleep, never block) → `Delivered`
  (or `Failed` with message). Notify requester with `TaskUpdate` at each state change.
- `impl TeamApi for TeamHub` (see `team/api.rs` docs): `send_task` with `peer: None` creates a local
  Incoming ticket (`from_name` = own display name, `peer: None`) and auto-delivers when
  `auto_approve_local`; with a peer it sends `ClientMsg::TaskRequest` through that peer's client and
  stores an Outgoing ticket updated by `TaskUpdate`s. Unknown/offline peer → error.
  `read_session`: local → last `max_lines` (≤ `MAX_READ_LINES`) lines of the rendered screen:
  feed the scrollback tail (≤ 512 KB) into `vt100::Parser::new(rows, cols, 2000)` and take the
  scrollback + visible screen text, trimmed of trailing blank lines. Remote → `ClientMsg::ReadRequest`
  with a 10 s timeout. Server side answers `ReadRequest` the same way (shared sessions only).
- Keep at most 200 tickets (drop oldest finished ones). `team-tasks-updated` sends newest 50.

### 4. `team/server.rs`
- axum router, single route `WS_PATH`. Bind `bindHost` if non-empty, else the Tailscale IP. Never
  `0.0.0.0`. No address → `serverError = "Tailscale not detected"`, retry every 30 s.
- Check `Authorization: Bearer <token>` (constant-time compare) BEFORE upgrading; else 401.
- Per connection: send `Hello` then `Sessions`; read `ClientMsg`s; writer task fed by a bounded
  mpsc; WebSocket ping every 20 s, close after 60 s of silence; frames > 64 KB → close.
  Record the client's `peer_name` from its `Hello` in `watchers` while connected.
- `Subscribe` → `Snapshot` (scrollback tail ≤ `MAX_SNAPSHOT_BYTES`, plus generation/end_offset from
  `get_scrollback_snapshot`, cols/rows) then live `Output`s. Make sure no chunk is lost or duplicated
  between snapshot and live stream (register the subscriber first, then snapshot, and drop live
  chunks whose `end_offset` ≤ snapshot `end_offset`, same idea as `TerminalPane.tsx`).
- Expose the bound address so tests can use port 0.

### 5. `team/client.rs`
- One task per `PeerConfig`: connect `ws://{host}:{port}{WS_PATH}` with the Bearer header
  (`tokio_tungstenite::connect_async` + `IntoClientRequest`), send `Hello`, then handle `ServerMsg`s:
  `Hello` → peer `display_name`; `Sessions` → peer sessions + `team-updated`;
  `Snapshot`/`Output`/`Resize`/`SessionGone` → emit `team-pty-snapshot`/`-output`/`-resize`/`team-session-gone`
  with `peer = PeerConfig.name`; `TaskUpdate` → outgoing ticket; `ReadResult` → pending oneshot.
- Reconnect with backoff 2 s → 30 s; on reconnect re-send `Subscribe` for active mirrors.
  Offline → `online = false`, sessions cleared, `last_error` set.
- API used by the hub: `subscribe`, `unsubscribe`, `send_task`, `read_session`.

### 6. IPC — `commands/team.rs` (+ `AppState.team: Arc<TeamHub>` in `lib.rs`)
Implement exactly the commands in PLAN.md "Tauri IPC" with those names and argument names
(`session_id`, `request_id`, `peer`, `shared`, `accept`, `text`). `set_session_shared` may live in
`commands/sessions.rs`. `generate_team_token` = 64 lowercase hex chars from two `Uuid::new_v4()`.
`update_config` (in `commands/system.rs`) must also call `team.apply_config(config.team)`.

### 7. `lib.rs` wiring
- Build `TeamHub::new(sessions.clone(), config.team.clone())` in `AppState::new`.
- In `setup`: `sessions.set_tap(hub.clone())`; `hub.start(handle)`; register the MCP builtin:
  `mcp.register_builtin(team::mcp::TEAM_MCP_NAME, Arc::new(team::mcp::TeamMcp::new(hub.clone())))`
  (await it inside the spawned task BEFORE `mcp.start()`).
- On exit: `hub.shutdown()`.

### 8. Tests (must pass)
Write the server/client core so it can be tested without Tauri or real PTYs (e.g. a small trait for
"local sessions" and one for "event sink", with fakes in tests). Required tests:
1. wrong / missing token → 401, no `Hello`;
2. right token → `Hello` + `Sessions` containing only shared sessions;
3. `Subscribe` → `Snapshot` then `Output` for new bytes, nothing for an unshared session;
4. `TaskRequest` → pending ticket → accept → delivery bytes written → `TaskUpdate Delivered` received;
5. `ReadRequest` → text; 6. client reconnects after the server restarts.
Use `127.0.0.1` and port 0.

## Validation (run from `src-tauri`)
`cargo check --all-targets`, `cargo test`, `cargo clippy --all-targets -- -D warnings` — all green.
No `.unwrap()`/`.expect()` outside tests. Never hold a lock across `.await`.

## Working rules
- Commit early and often on `online/lane-a` (small, clear messages). Do not push. Do not merge.
- After each milestone, update `D:\Workspace\velox-online\status\lane-a.md` (see template there).
- Blocked on a contract? Write it under "Questions" in the status file, pick the safest workaround, continue.
- Do not launch `velox.exe` or `pnpm tauri dev`; the coordinator runs the end-to-end test.
- When everything is green: set status to `DONE`, list what was built and anything left undone.
