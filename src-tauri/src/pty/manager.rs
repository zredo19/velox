use crate::domain::agent::AgentSpawnSpec;
use crate::domain::error::VeloxResult;
use crate::pty::job_object::ProcessJobObject;
use crate::pty::scrollback::RingScrollback;
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtyPair, PtySize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

/// Position of an output chunk in a PTY's byte stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamPos {
    /// Increments on every spawn so a respawned PTY (same session id) starts a new stream.
    pub generation: u64,
    /// Stream offset just past this chunk.
    pub end_offset: u64,
}

pub type OutputCallback = Arc<dyn Fn(&str, &[u8], StreamPos) + Send + Sync>;

static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);
pub type ExitCallback = Arc<dyn Fn(&str, Option<i32>) + Send + Sync>;

struct SessionData {
    master: Option<Box<dyn MasterPty + Send>>,
    writer: Option<Box<dyn Write + Send>>,
    scrollback: Arc<Mutex<RingScrollback>>,
    generation: u64,
    job: Option<ProcessJobObject>,
    pid: Option<u32>,
    alive: Arc<AtomicBool>,
}

/// Manages active ConPTY terminals via `portable-pty`.
pub struct PtyManager {
    sessions: Arc<Mutex<HashMap<String, SessionData>>>,
    scrollback_bytes: usize,
}

impl PtyManager {
    /// Creates a new `PtyManager` with the specified scrollback capacity per session.
    pub fn new(scrollback_bytes: usize) -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            scrollback_bytes,
        }
    }

    /// Spawns a new PTY session with the given spec and callbacks.
    ///
    /// Returns the process ID (PID) of the spawned child process.
    pub fn spawn(
        &self,
        session_id: &str,
        spec: &AgentSpawnSpec,
        cols: u16,
        rows: u16,
        on_output: OutputCallback,
        on_exit: ExitCallback,
    ) -> VeloxResult<u32> {
        let pty_system = native_pty_system();

        let size = PtySize {
            rows: rows.max(2),
            cols: cols.max(10),
            pixel_width: 0,
            pixel_height: 0,
        };

        let pair: PtyPair = pty_system
            .openpty(size)
            .map_err(|e| crate::domain::error::VeloxError::Pty(format!("openpty failed: {e}")))?;

        let mut cmd = CommandBuilder::new(&spec.program);
        cmd.args(&spec.args);
        cmd.cwd(&spec.cwd);
        for (k, v) in &spec.env {
            cmd.env(k, v);
        }
        if !spec.env.contains_key("VELOX_SESSION_ID") {
            cmd.env("VELOX_SESSION_ID", session_id);
        }

        // Ensure rich terminal capabilities for ConPTY agents
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");

        let mut child = pair.slave.spawn_command(cmd).map_err(|e| {
            crate::domain::error::VeloxError::Pty(format!("spawn_command failed: {e}"))
        })?;

        // CRITICAL: Drop slave immediately so EOF fires cleanly when child exits on Windows ConPTY
        drop(pair.slave);

        let pid = child.process_id().ok_or_else(|| {
            crate::domain::error::VeloxError::Pty("child process_id unavailable".to_string())
        })?;

        // Create job object and assign PID
        let job = ProcessJobObject::new().ok();
        if let Some(ref job_obj) = job {
            if let Err(e) = job_obj.assign_pid(pid) {
                tracing::warn!(pid, %e, "Failed to assign PID to job object");
            }
        }

        let scrollback = Arc::new(Mutex::new(RingScrollback::new(self.scrollback_bytes)));
        let generation = NEXT_GENERATION.fetch_add(1, Ordering::SeqCst);
        let alive = Arc::new(AtomicBool::new(true));

        // Clone for reader thread
        let session_id_reader = session_id.to_string();
        let scrollback_clone = scrollback.clone();
        let on_output_clone = on_output.clone();

        // Spawn dedicated OS reader thread
        let mut reader = pair.master.try_clone_reader().map_err(|e| {
            crate::domain::error::VeloxError::Pty(format!("try_clone_reader failed: {e}"))
        })?;

        let reader_handle = thread::Builder::new()
            .name(format!("pty-reader-{}", session_id))
            .spawn(move || {
                // 64 KiB: a burst of agent output arrives in a few large chunks instead of many
                // 8 KiB ones, and every chunk costs an IPC event + base64 + JS dispatch.
                let mut buf = vec![0u8; 64 * 1024];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => break, // EOF
                        Ok(n) => {
                            let data = &buf[..n];
                            let end_offset = {
                                let mut sb =
                                    scrollback_clone.lock().unwrap_or_else(|e| e.into_inner());
                                sb.push(data);
                                sb.total()
                            };
                            on_output_clone(
                                &session_id_reader,
                                data,
                                StreamPos {
                                    generation,
                                    end_offset,
                                },
                            );
                        }
                        Err(e) => {
                            tracing::warn!(session_id = %session_id_reader, %e, "PTY read error");
                            break;
                        }
                    }
                }
            })
            .map_err(|e| {
                crate::domain::error::VeloxError::Pty(format!("spawn reader thread failed: {e}"))
            })?;

        // Store writer and master for later operations BEFORE spawning waiter thread
        let writer = pair.master.take_writer().map_err(|e| {
            crate::domain::error::VeloxError::Pty(format!("take_writer failed: {e}"))
        })?;

        let session_data = SessionData {
            master: Some(pair.master),
            writer: Some(writer),
            scrollback,
            generation,
            job,
            pid: Some(pid),
            alive: alive.clone(),
        };

        self.sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(session_id.to_string(), session_data);

        // Wait on child exit separately: on Windows ConPTY, reader EOF does not fire until
        // master (HPCON) is closed via ClosePseudoConsole, so waiter drops master after exit.
        let sessions_waiter = Arc::clone(&self.sessions);
        let session_id_waiter = session_id.to_string();
        thread::Builder::new()
            .name(format!("pty-waiter-{}", session_id))
            .spawn(move || {
                let exit_code = child.wait().ok().map(|s| s.exit_code() as i32);

                let (writer_opt, master_opt, is_current_spawn) = {
                    let mut guard = sessions_waiter.lock().unwrap_or_else(|e| e.into_inner());
                    match guard.get_mut(&session_id_waiter) {
                        Some(s) if Arc::ptr_eq(&s.alive, &alive) => {
                            s.alive.store(false, Ordering::SeqCst);
                            (s.writer.take(), s.master.take(), true)
                        }
                        _ => (None, None, false),
                    }
                };
                // Drop handles outside lock while reader thread is still draining ConPTY output.
                drop(writer_opt);
                drop(master_opt);

                let _ = reader_handle.join();
                alive.store(false, Ordering::SeqCst);
                if is_current_spawn {
                    on_exit(&session_id_waiter, exit_code);
                }
            })
            .map_err(|e| {
                crate::domain::error::VeloxError::Pty(format!("spawn waiter thread failed: {e}"))
            })?;

        Ok(pid)
    }

    /// Writes data to the PTY session.
    pub fn write(&self, session_id: &str, data: &[u8]) -> VeloxResult<()> {
        let mut sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(session) = sessions.get_mut(session_id) {
            let writer = session.writer.as_mut().ok_or_else(|| {
                crate::domain::error::VeloxError::Pty(format!(
                    "Session already exited: {session_id}"
                ))
            })?;
            writer
                .write_all(data)
                .and_then(|()| writer.flush())
                .map_err(|e| crate::domain::error::VeloxError::Pty(format!("write failed: {e}")))?;
            Ok(())
        } else {
            Err(crate::domain::error::VeloxError::Pty(format!(
                "Session not found: {session_id}"
            )))
        }
    }

    /// Resizes the PTY session.
    pub fn resize(&self, session_id: &str, cols: u16, rows: u16) -> VeloxResult<()> {
        let mut sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(session) = sessions.get_mut(session_id) {
            let master = session.master.as_ref().ok_or_else(|| {
                crate::domain::error::VeloxError::Pty(format!(
                    "Session already exited: {session_id}"
                ))
            })?;
            let size = PtySize {
                rows: rows.max(2),
                cols: cols.max(10),
                pixel_width: 0,
                pixel_height: 0,
            };
            master.resize(size).map_err(|e| {
                crate::domain::error::VeloxError::Pty(format!("resize failed: {e}"))
            })?;
            Ok(())
        } else {
            Err(crate::domain::error::VeloxError::Pty(format!(
                "Session not found: {session_id}"
            )))
        }
    }

    /// Kills a specific PTY session.
    pub fn kill(&self, session_id: &str) -> VeloxResult<()> {
        let removed = {
            let mut sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
            sessions.remove(session_id)
        };
        if let Some(mut session) = removed {
            // Terminate job object (kills entire process tree on Windows)
            if let Some(job) = session.job.take() {
                if let Err(e) = job.terminate() {
                    tracing::warn!(session_id, %e, "Failed to terminate job object");
                }
            }
            // Drop writer and master outside lock so reader drains cleanly
            drop(session.writer);
            drop(session.master);
            Ok(())
        } else {
            Err(crate::domain::error::VeloxError::Pty(format!(
                "Session not found: {session_id}"
            )))
        }
    }

    /// Kills all active PTY sessions.
    pub fn kill_all(&self) {
        let drained: Vec<(String, SessionData)> = {
            let mut sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
            sessions.drain().collect()
        };
        for (session_id, mut session) in drained {
            if let Some(job) = session.job.take() {
                if let Err(e) = job.terminate() {
                    tracing::warn!(%session_id, %e, "Failed to terminate job object");
                }
            }
            drop(session.writer);
            drop(session.master);
        }
    }

    /// Returns the scrollback buffer plus the stream position it covers, read atomically so the
    /// frontend can merge it with live output without gaps or duplicates.
    pub fn get_scrollback_with_pos(&self, session_id: &str) -> (Vec<u8>, StreamPos) {
        self.sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(session_id)
            .map(|s| {
                let sb = s.scrollback.lock().unwrap_or_else(|e| e.into_inner());
                (
                    sb.snapshot(),
                    StreamPos {
                        generation: s.generation,
                        end_offset: sb.total(),
                    },
                )
            })
            .unwrap_or((
                Vec::new(),
                StreamPos {
                    generation: 0,
                    end_offset: 0,
                },
            ))
    }

    /// Returns the scrollback buffer for a session.
    pub fn get_scrollback(&self, session_id: &str) -> Vec<u8> {
        self.sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(session_id)
            .map(|s| {
                s.scrollback
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .snapshot()
            })
            .unwrap_or_default()
    }

    /// Returns the PID of a session if it exists.
    pub fn get_pid(&self, session_id: &str) -> Option<u32> {
        self.sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(session_id)
            .and_then(|s| s.pid)
    }

    /// Returns true if the session is alive.
    pub fn is_alive(&self, session_id: &str) -> bool {
        self.sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(session_id)
            .map(|s| s.alive.load(Ordering::SeqCst))
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pty_manager_new_creates_empty_manager() {
        let manager = PtyManager::new(1024);
        assert!(!manager.is_alive("nonexistent"));
        assert_eq!(manager.get_pid("nonexistent"), None);
        assert_eq!(manager.get_scrollback("nonexistent"), Vec::<u8>::new());
    }

    #[test]
    fn pty_manager_kill_all_on_empty() {
        let manager = PtyManager::new(1024);
        // Should not panic
        manager.kill_all();
    }

    #[test]
    fn pty_manager_kill_nonexistent_returns_error() {
        let manager = PtyManager::new(1024);
        let result = manager.kill("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn pty_manager_write_nonexistent_returns_error() {
        let manager = PtyManager::new(1024);
        let result = manager.write("nonexistent", b"test");
        assert!(result.is_err());
    }

    #[test]
    fn pty_manager_resize_nonexistent_returns_error() {
        let manager = PtyManager::new(1024);
        let result = manager.resize("nonexistent", 80, 24);
        assert!(result.is_err());
    }

    #[test]
    fn pty_manager_get_scrollback_empty_session() {
        let manager = PtyManager::new(1024);
        let scrollback = manager.get_scrollback("empty");
        assert!(scrollback.is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn pty_manager_kill_and_immediate_respawn_same_id_keeps_latest_alive() {
        use std::collections::HashMap;
        use std::path::PathBuf;
        use std::time::{Duration, Instant};

        let manager = PtyManager::new(32 * 1024);
        let sid = "respawn-race-test";
        let exits = Arc::new(Mutex::new(Vec::<(String, Option<i32>)>::new()));
        let output = Arc::new(Mutex::new(String::new()));

        let spec = AgentSpawnSpec {
            program: "cmd.exe".to_string(),
            args: vec!["/d".to_string(), "/k".to_string()],
            env: HashMap::new(),
            cwd: PathBuf::from("."),
        };

        for i in 0..5 {
            if i > 0 {
                let _ = manager.kill(sid);
                output.lock().unwrap().clear();
            }
            let out_buf = Arc::clone(&output);
            let on_output: OutputCallback = Arc::new(move |_, bytes, _| {
                out_buf
                    .lock()
                    .unwrap()
                    .push_str(&String::from_utf8_lossy(bytes));
            });
            let exit_buf = Arc::clone(&exits);
            let on_exit: ExitCallback = Arc::new(move |id, code| {
                exit_buf.lock().unwrap().push((id.to_string(), code));
            });

            manager
                .spawn(sid, &spec, 80, 24, on_output, on_exit)
                .expect("spawn cmd.exe");
        }

        // Let any old waiter threads settle and answer any ConPTY DSR (\x1b[6n)
        let mut dsr_sent = 0usize;
        let settle_start = Instant::now();
        while settle_start.elapsed() < Duration::from_millis(600) {
            let snap = output.lock().unwrap().clone();
            let dsr_count = snap.matches("\x1b[6n").count();
            while dsr_sent < dsr_count {
                let _ = manager.write(sid, b"\x1b[1;1R");
                dsr_sent += 1;
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        assert!(
            exits.lock().unwrap().is_empty(),
            "old waiter thread fired on_exit onto respawned session: {:?}",
            *exits.lock().unwrap()
        );
        assert!(
            manager.is_alive(sid),
            "latest respawned session was marked dead by old waiter"
        );

        manager
            .write(sid, b"echo RESPAWN_OK\r")
            .expect("latest respawned session must remain writable");

        let start = Instant::now();
        let mut saw_marker = false;
        while start.elapsed() < Duration::from_secs(10) {
            let snap = output.lock().unwrap().clone();
            let dsr_count = snap.matches("\x1b[6n").count();
            while dsr_sent < dsr_count {
                let _ = manager.write(sid, b"\x1b[1;1R");
                dsr_sent += 1;
            }
            if snap.contains("RESPAWN_OK") {
                saw_marker = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        if !saw_marker {
            let snap = output.lock().unwrap().clone();
            let _ = manager.kill(sid);
            panic!("did not observe RESPAWN_OK from latest session; output:\n{snap:?}");
        }

        let _ = manager.kill(sid);
    }
}
