# Lane 1 (`velox-pty`) — ConPTY, Windows Job Objects & Bounded Scrollback

**Model Tier:** DeepSeek V4.1 Flash (Hardest Core Systems Rust Lane)
**Skills to follow:** Read `C:\Users\Asus\.agents\skills\rust-best-practices\SKILL.md` and `C:\Users\Asus\.agents\skills\rust-async-patterns\SKILL.md` before coding.
**Architecture Spec:** Read `C:\Users\Asus\OneDrive\Escritorio\Proyectos\velox\docs\ARCHITECTURE.md` and `src-tauri/src/domain/*`.

## Owned Files (EDIT ONLY THESE FILES)
- `src-tauri/src/pty/mod.rs`
- `src-tauri/src/pty/job_object.rs`
- `src-tauri/src/pty/scrollback.rs`
- `src-tauri/src/pty/manager.rs`

Do NOT modify `Cargo.toml`, `domain/*`, `mcp/*`, `session/*`, `commands/*`, or `lib.rs`.
Note: Sibling Rust modules (`mcp/`, `session/`) are being edited in parallel by other agents. Stay strictly inside `src-tauri/src/pty/*` and never touch sibling files.

## Exact Requirements

### 1. `src-tauri/src/pty/job_object.rs`
Implement a production-grade RAII Windows Job Object wrapper `ProcessJobObject`:
- Public API:
  - `pub fn new() -> VeloxResult<Self>`
  - `pub fn assign_pid(&self, pid: u32) -> VeloxResult<()>`
  - `pub fn terminate(&self) -> VeloxResult<()>`
- Must be `Send + Sync`.
- On `#[cfg(windows)]` using `windows_sys` (`0.61`):
  - `CreateJobObjectW(std::ptr::null(), std::ptr::null())`
  - Configure `JOBOBJECT_EXTENDED_LIMIT_INFORMATION` with `BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_BREAKAWAY_OK` via `SetInformationJobObject(..., JobObjectExtendedLimitInformation, ...)`.
  - `assign_pid(pid)`: `OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid)`, call `AssignProcessToJobObject`, always close the process handle with `CloseHandle`.
  - `terminate()`: `TerminateJobObject(handle, 1)`.
  - `Drop`: safely calls `CloseHandle` if handle is non-null (wrapped in `AtomicBool` or `Mutex<Option<usize>>` so `terminate()` + `Drop` never double-close!).
- On `#[cfg(not(windows))]`: clean no-op implementation so code compiles cross-platform.
- Zero `.unwrap()` / `.expect()` in production code. Document `// SAFETY:` invariants on every `unsafe` block.

### 2. `src-tauri/src/pty/scrollback.rs`
Implement a fixed-capacity byte ring buffer `RingScrollback` (backed by `std::collections::VecDeque<u8>` or contiguous ring slice):
- Public API:
  - `pub fn new(max_bytes: usize) -> Self`
  - `pub fn push(&mut self, chunk: &[u8])`
  - `pub fn snapshot(&self) -> Vec<u8>`
  - `pub fn clear(&mut self)`
  - `pub fn len(&self) -> usize`
  - `pub fn is_empty(&self) -> bool`
- Behavior:
  - Never exceeds `max_bytes` bytes (even if a single `chunk.len() > max_bytes`, keep only the last `max_bytes` bytes of `chunk`!).
  - If `max_bytes == 0`, silently discards everything.
  - `snapshot()` returns the bytes in exact chronological order.

### 3. `src-tauri/src/pty/manager.rs`
Implement `PtyManager` managing active ConPTY terminals via `portable-pty = "0.9"`:
- Public types & methods (EXACT signatures from `docs/ARCHITECTURE.md`):
  - `pub type OutputCallback = Arc<dyn Fn(&str, &[u8]) + Send + Sync>;`
  - `pub type ExitCallback = Arc<dyn Fn(&str, Option<i32>) + Send + Sync>;`
  - `pub struct PtyManager` (`Send + Sync`)
  - `pub fn new(scrollback_bytes: usize) -> Self`
  - `pub fn spawn(&self, session_id: &str, spec: &AgentSpawnSpec, cols: u16, rows: u16, on_output: OutputCallback, on_exit: ExitCallback) -> VeloxResult<u32>`
  - `pub fn write(&self, session_id: &str, data: &[u8]) -> VeloxResult<()>`
  - `pub fn resize(&self, session_id: &str, cols: u16, rows: u16) -> VeloxResult<()>`
  - `pub fn kill(&self, session_id: &str) -> VeloxResult<()>`
  - `pub fn kill_all(&self)`
  - `pub fn get_scrollback(&self, session_id: &str) -> Vec<u8>`
  - `pub fn get_pid(&self, session_id: &str) -> Option<u32>`
  - `pub fn is_alive(&self, session_id: &str) -> bool`
- Implementation details:
  - Use `portable_pty::native_pty_system()` to open a `PtyPair` with `PtySize { rows: rows.max(2), cols: cols.max(10), pixel_width: 0, pixel_height: 0 }`.
  - Build `CommandBuilder::new(&spec.program)`, `.args(&spec.args)`, `.cwd(&spec.cwd)`, and apply every `(k, v)` from `&spec.env`.
  - Spawn the child via `pair.slave.spawn_command(cmd)`, then immediately drop `pair.slave` (CRITICAL on Windows ConPTY so EOF fires cleanly when the child exits!).
  - Capture `child.process_id()` (`u32`). Create a `ProcessJobObject::new()` and call `job.assign_pid(pid)` (log a `tracing::warn!` if Windows rejects job assignment, e.g. nested job restriction, without crashing spawn).
  - Store each active session's `master` (`Box<dyn MasterPty + Send>`), `writer` (`Box<dyn Write + Send>`), `scrollback` (`Arc<Mutex<RingScrollback>>`), `job` (`Option<ProcessJobObject>`), `pid` (`Option<u32>`), and `alive` (`Arc<AtomicBool>`).
  - Spawn a dedicated OS reader thread (`std::thread::Builder::new().name(...)`) reading 8192-byte chunks from `pair.master.try_clone_reader()`. For each non-empty read: append to `RingScrollback` and invoke `on_output(session_id, &buf[..n])`. On EOF/error, mark `alive = false`, wait/poll `child.wait()` for exit code, and invoke `on_exit(session_id, exit_code)`.
  - `kill(session_id)`: remove session from map, call `job.terminate()`, drop `writer` and `master` cleanly (guaranteed zero orphan processes!).
  - `kill_all()`: kills every active session cleanly.

### 4. Unit Tests
Add thorough unit tests in `scrollback.rs` and `job_object.rs`:
- `ring_scrollback_preserves_order_within_capacity`
- `ring_scrollback_evicts_oldest_bytes_when_overflowing`
- `ring_scrollback_handles_chunk_larger_than_capacity`
- `ring_scrollback_zero_capacity_discards_all`
- `job_object_creates_and_drops_cleanly`

## Verification
From `C:\Users\Asus\OneDrive\Escritorio\Proyectos\velox\src-tauri`, run:
```bash
cargo test --lib pty
cargo clippy --lib -- -D warnings
```
Fix any errors or clippy warnings in your files until green, then reply concisely with summary and test output.
