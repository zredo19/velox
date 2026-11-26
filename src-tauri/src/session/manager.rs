use crate::domain::agent::{
    build_default_presets, prepare_agent_profile, resolve_launch_spec, AgentKind, AgentLaunch,
    AgentPreset, AgentSpawnSpec,
};
use crate::domain::config::VeloxConfig;
use crate::domain::error::{VeloxError, VeloxResult};
use crate::domain::session::{
    PtyOutputEvent, ScrollbackSnapshot, SessionRecord, SessionStatus, SessionUpdatedEvent,
};
use crate::pty::manager::{ExitCallback, OutputCallback, PtyManager, StreamPos};
use crate::session::detector::{
    agent_session_belongs_to_cwd, agent_session_is_resumable, detect_status_from_output,
    discover_agent_session_id, recover_agent_session_id,
};
use crate::session::store::{load_sessions, save_sessions};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use chrono::{DateTime, Utc};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

/// Coordinates persisted sessions, ConPTY instances, lazy-restore, and auto-hibernation.
pub struct SessionManager {
    sessions: Arc<RwLock<Vec<SessionRecord>>>,
    pty: Arc<PtyManager>,
    config: RwLock<VeloxConfig>,
    presets: Vec<AgentPreset>,
    last_activity: Arc<Mutex<HashMap<String, Instant>>>,
    started_at: Arc<Mutex<HashMap<String, DateTime<Utc>>>>,
    shutting_down: Arc<AtomicBool>,
    /// Size a pane asked for while its PTY was still being spawned (applied at spawn).
    pending_sizes: Arc<Mutex<HashMap<String, (u16, u16)>>>,
    /// Serializes the decision part of create/wake/sleep/close now that those commands run
    /// off the main thread (they used to be serialized by it).
    lifecycle: Mutex<()>,
}

impl SessionManager {
    pub fn new(config: VeloxConfig) -> Self {
        let initial_sessions = if config.lazy_restore_on_startup {
            let mut loaded = load_sessions().unwrap_or_default();
            repair_duplicate_agent_session_ids(
                &mut loaded,
                |s, id| agent_session_belongs_to_cwd(s.agent_kind, s.profile.as_deref(), &s.cwd, id),
                |s, claimed| {
                    recover_agent_session_id(s.agent_kind, s.profile.as_deref(), &s.cwd, claimed)
                },
            );
            loaded
        } else {
            Vec::new()
        };

        let scrollback_bytes = config.scrollback_bytes;
        Self {
            sessions: Arc::new(RwLock::new(initial_sessions)),
            pty: Arc::new(PtyManager::new(scrollback_bytes)),
            config: RwLock::new(config),
            presets: build_default_presets(),
            last_activity: Arc::new(Mutex::new(HashMap::new())),
            started_at: Arc::new(Mutex::new(HashMap::new())),
            shutting_down: Arc::new(AtomicBool::new(false)),
            pending_sizes: Arc::new(Mutex::new(HashMap::new())),
            lifecycle: Mutex::new(()),
        }
    }

    pub fn list_sessions(&self) -> Vec<SessionRecord> {
        self.sessions
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn list_presets(&self) -> Vec<AgentPreset> {
        self.presets.clone()
    }

    pub fn list_recent_dirs(&self) -> Vec<String> {
        self.config
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .recent_dirs
            .clone()
    }

    pub fn get_config(&self) -> VeloxConfig {
        self.config
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn update_config(&self, next: VeloxConfig) -> VeloxResult<VeloxConfig> {
        next.save()?;
        let mut guard = self.config.write().unwrap_or_else(|e| e.into_inner());
        *guard = next.clone();
        Ok(next)
    }

    fn find_preset(&self, preset_id: &str) -> VeloxResult<&AgentPreset> {
        self.presets
            .iter()
            .find(|p| p.id == preset_id)
            .ok_or_else(|| VeloxError::InvalidInput(format!("Unknown agent preset: {preset_id}")))
    }

    fn build_callbacks(
        &self,
        app: AppHandle,
        agent_kind: crate::domain::agent::AgentKind,
    ) -> (OutputCallback, ExitCallback) {
        let sessions_out = Arc::clone(&self.sessions);
        let activity_out = Arc::clone(&self.last_activity);
        let app_out = app.clone();

        let on_output: OutputCallback =
            Arc::new(move |session_id: &str, data: &[u8], pos: StreamPos| {
                let data_b64 = BASE64.encode(data);
                let _ = app_out.emit(
                    "pty-output",
                    PtyOutputEvent {
                        session_id: session_id.to_string(),
                        data_b64,
                        generation: pos.generation,
                        end_offset: pos.end_offset,
                    },
                );

                activity_out
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(session_id.to_string(), Instant::now());

                if let Some(detected) = detect_status_from_output(agent_kind, data) {
                    let updated = {
                        let mut guard = sessions_out.write().unwrap_or_else(|e| e.into_inner());
                        if let Some(record) = guard.iter_mut().find(|s| s.id == session_id) {
                            if record.status != SessionStatus::Sleeping
                                && record.status != SessionStatus::Exited
                                && record.status != detected
                            {
                                record.status = detected;
                                record.last_active_at = Utc::now().to_rfc3339();
                                Some(record.clone())
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    };
                    if let Some(session) = updated {
                        let _ = app_out.emit("session-updated", SessionUpdatedEvent { session });
                    }
                }
            });

        let sessions_exit = Arc::clone(&self.sessions);
        let started_exit = Arc::clone(&self.started_at);
        let shutting_down = Arc::clone(&self.shutting_down);
        let app_exit = app;

        let on_exit: ExitCallback = Arc::new(move |session_id: &str, exit_code: Option<i32>| {
            // During app shutdown the IDs were already captured; don't let PTY teardown race it.
            if shutting_down.load(Ordering::SeqCst) {
                return;
            }
            let started = started_exit
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(session_id)
                .copied()
                .unwrap_or_else(|| Utc::now() - chrono::Duration::minutes(5));

            let (updated, snapshot_to_save) = {
                let mut guard = sessions_exit.write().unwrap_or_else(|e| e.into_inner());
                let claimed = claimed_agent_session_ids(&guard, session_id);
                let updated = if let Some(record) = guard.iter_mut().find(|s| s.id == session_id) {
                    if record.status == SessionStatus::Sleeping {
                        None
                    } else {
                        if let Some(found_id) = discover_agent_session_id(
                            record.agent_kind,
                            record.profile.as_deref(),
                            &record.cwd,
                            started,
                            &claimed,
                        ) {
                            record.agent_session_id = Some(found_id);
                        }
                        record.status = SessionStatus::Exited;
                        record.active_agent = None;
                        record.pid = None;
                        record.memory_mb = 0.0;
                        record.exit_code = exit_code;
                        record.last_active_at = Utc::now().to_rfc3339();
                        Some(record.clone())
                    }
                } else {
                    None
                };
                let snapshot = if updated.is_some() {
                    Some(guard.clone())
                } else {
                    None
                };
                (updated, snapshot)
            };

            if let Some(snapshot) = snapshot_to_save {
                let _ = save_sessions(&snapshot);
            }
            if let Some(session) = updated {
                let _ = app_exit.emit("session-updated", SessionUpdatedEvent { session });
            }
        });

        (on_output, on_exit)
    }

    pub fn create_session(
        &self,
        app: &AppHandle,
        preset_id: &str,
        cwd: Option<String>,
        title: Option<String>,
        cols: u16,
        rows: u16,
    ) -> VeloxResult<SessionRecord> {
        let _lifecycle = self.lifecycle.lock().unwrap_or_else(|e| e.into_inner());
        let preset = self.find_preset(preset_id)?.clone();

        // Resolve working directory and persist in recent_dirs without holding overlapping locks
        let resolved_cwd = {
            let mut cfg = self.config.write().unwrap_or_else(|e| e.into_inner());
            let dir = cwd
                .filter(|d| !d.trim().is_empty())
                .unwrap_or_else(|| cfg.default_cwd.clone());
            cfg.record_recent_dir(&dir);
            let _ = cfg.save();
            dir
        };

        let resolved_title = title.filter(|t| !t.trim().is_empty()).unwrap_or_else(|| {
            Path::new(&resolved_cwd)
                .file_name()
                .and_then(|n| n.to_str())
                .filter(|s| !s.is_empty())
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| preset.label.clone())
        });

        let session_id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now();
        let now_str = now.to_rfc3339();

        let record = SessionRecord {
            id: session_id.clone(),
            title: resolved_title,
            agent_kind: preset.kind,
            active_agent: None,
            preset_id: preset.id.clone(),
            profile: preset.profile.clone(),
            cwd: resolved_cwd.clone(),
            status: SessionStatus::Starting,
            // Claude lets us choose the conversation ID up-front, so this tab always knows
            // exactly which transcript to `--resume` later.
            agent_session_id: (preset.kind == AgentKind::Claude)
                .then(|| uuid::Uuid::new_v4().to_string()),
            wake_as_shell: preset.kind == crate::domain::agent::AgentKind::Shell,
            pid: None,
            memory_mb: 0.0,
            created_at: now_str.clone(),
            last_active_at: now_str,
            exit_code: None,
        };

        // Insert record before spawn so early PTY output callbacks find the session
        {
            let mut guard = self.sessions.write().unwrap_or_else(|e| e.into_inner());
            guard.push(record.clone());
        }
        self.started_at
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(session_id.clone(), now);
        self.last_activity
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(session_id.clone(), Instant::now());

        let launch = match record.agent_session_id.as_deref() {
            Some(id) => AgentLaunch::NewWithId(id),
            None => AgentLaunch::Fresh,
        };
        let spec = resolve_launch_spec(&preset, Path::new(&resolved_cwd), launch);

        // The tab is returned right away in `Starting`; the slow part (credential swap, ConPTY
        // spawn) finishes in the background and is announced with `session-updated`.
        self.spawn_in_background(
            app,
            &session_id,
            preset,
            true,
            SpawnFailure::RemoveTab,
            spec,
            cols,
            rows,
        );
        Ok(record)
    }

    /// Finish opening a tab off the IPC thread: swap the agent profile (AGY runs a PowerShell
    /// helper that takes seconds), spawn the ConPTY, then flip the record to `Running`.
    /// If the tab was closed or put to sleep meanwhile, the fresh PTY is torn down again.
    #[allow(clippy::too_many_arguments)]
    fn spawn_in_background(
        &self,
        app: &AppHandle,
        session_id: &str,
        preset: AgentPreset,
        prepare_profile: bool,
        on_failure: SpawnFailure,
        spec: AgentSpawnSpec,
        cols: u16,
        rows: u16,
    ) {
        let (on_output, on_exit) = self.build_callbacks(app.clone(), preset.kind);
        let pty = Arc::clone(&self.pty);
        let pending_sizes = Arc::clone(&self.pending_sizes);
        let sessions = Arc::clone(&self.sessions);
        let shutting_down = Arc::clone(&self.shutting_down);
        let app = app.clone();
        let session_id = session_id.to_string();

        let job = move || {
            if prepare_profile {
                prepare_agent_profile(&preset);
            }
            let wanted = |sessions: &RwLock<Vec<SessionRecord>>| {
                !shutting_down.load(Ordering::SeqCst)
                    && sessions
                        .read()
                        .unwrap_or_else(|e| e.into_inner())
                        .iter()
                        .any(|s| s.id == session_id && is_pending_or_live(s.status))
            };
            if !wanted(&sessions) {
                return;
            }

            // The pane usually measured itself while the PTY did not exist yet: use that size.
            let take_size = || {
                pending_sizes
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&session_id)
            };
            let (cols, rows) = take_size().unwrap_or((cols, rows));
            let result = pty.spawn(&session_id, &spec, cols, rows, on_output, on_exit);
            if result.is_ok() {
                // A resize that raced with the spawn was parked instead of applied.
                if let Some((c, r)) = take_size() {
                    let _ = pty.resize(&session_id, c, r);
                }
            }
            let outcome = {
                let mut guard = sessions.write().unwrap_or_else(|e| e.into_inner());
                let still_wanted = !shutting_down.load(Ordering::SeqCst);
                match guard.iter_mut().find(|s| s.id == session_id) {
                    Some(record) if still_wanted && is_pending_or_live(record.status) => {
                        match &result {
                            Ok(pid) => {
                                record.pid = Some(*pid);
                                record.exit_code = None;
                                // Early output may already have set a more precise status.
                                if record.status == SessionStatus::Starting {
                                    record.status = SessionStatus::Running;
                                }
                            }
                            Err(_) => {
                                // Wake failures go back to sleep with the conversation ID they
                                // had; create failures drop the tab, as the synchronous path did.
                                record.status = SessionStatus::Sleeping;
                                record.pid = None;
                                if let SpawnFailure::BackToSleep(previous) = &on_failure {
                                    record.agent_session_id = previous.clone();
                                }
                            }
                        }
                        let session = record.clone();
                        if result.is_err() && on_failure == SpawnFailure::RemoveTab {
                            guard.retain(|s| s.id != session_id);
                        }
                        Some((session, guard.clone()))
                    }
                    _ => None,
                }
            };

            match outcome {
                None => {
                    // Closed, hibernated or shutting down while we were spawning.
                    if result.is_ok() {
                        let _ = pty.kill(&session_id);
                    }
                }
                Some((session, snapshot)) => {
                    let _ = save_sessions(&snapshot);
                    if let Err(err) = &result {
                        tracing::warn!(session_id = %session_id, %err, "Failed to start session");
                        let _ = app.emit(
                            "session-spawn-failed",
                            SessionSpawnFailedEvent {
                                session_id: session_id.clone(),
                                message: err.to_string(),
                                removed: on_failure == SpawnFailure::RemoveTab,
                            },
                        );
                        if on_failure == SpawnFailure::RemoveTab {
                            return;
                        }
                    }
                    let _ = app.emit("session-updated", SessionUpdatedEvent { session });
                }
            }
        };

        let job = Arc::new(Mutex::new(Some(job)));
        let job_thread = Arc::clone(&job);
        let spawned = std::thread::Builder::new()
            .name("velox-session-spawn".to_string())
            .spawn(move || {
                if let Some(f) = job_thread.lock().unwrap_or_else(|e| e.into_inner()).take() {
                    f();
                }
            });
        if spawned.is_err() {
            // Could not get a thread: do it inline rather than leave the tab stuck in Starting.
            if let Some(f) = job.lock().unwrap_or_else(|e| e.into_inner()).take() {
                f();
            }
        }
    }

    pub fn wake_session(
        &self,
        app: &AppHandle,
        session_id: &str,
        cols: u16,
        rows: u16,
    ) -> VeloxResult<SessionRecord> {
        let _lifecycle = self.lifecycle.lock().unwrap_or_else(|e| e.into_inner());
        let existing = {
            let guard = self.sessions.read().unwrap_or_else(|e| e.into_inner());
            guard
                .iter()
                .find(|s| s.id == session_id)
                .cloned()
                .ok_or_else(|| VeloxError::SessionNotFound(session_id.to_string()))?
        };

        // Already running, or a wake/create for it is still spawning (double click).
        if existing.status == SessionStatus::Starting || self.pty.is_alive(session_id) {
            return Ok(existing);
        }

        let (preset, resume_id, is_shell_wake) =
            select_wake_preset_and_resume(&self.presets, &existing)?;

        // Only resume conversations that really exist and have content; otherwise the CLI opens an
        // empty (or unrelated) session. For Claude we then start fresh with a pinned ID instead.
        // A conversation already owned by another tab must never be resumed twice (that is what
        // duplicated tabs and left the other conversation orphaned): recover this folder's own.
        let claimed = {
            let guard = self.sessions.read().unwrap_or_else(|e| e.into_inner());
            claimed_agent_session_ids(&guard, session_id)
        };
        let profile = existing.profile.as_deref();
        let (next_agent_session_id, resume): (Option<String>, bool) = if is_shell_wake {
            (existing.agent_session_id.clone(), false)
        } else {
            let usable = match resume_id {
                Some(id) if claimed.contains(id) => {
                    tracing::warn!(session_id, id, "Conversation owned by another tab; recovering");
                    recover_agent_session_id(preset.kind, profile, &existing.cwd, &claimed)
                }
                Some(id) if agent_session_is_resumable(preset.kind, profile, id) => {
                    Some(id.to_string())
                }
                _ => None,
            };
            match usable {
                Some(id) => (Some(id), true),
                None if preset.kind == AgentKind::Claude => {
                    (Some(uuid::Uuid::new_v4().to_string()), false)
                }
                None => (None, false),
            }
        };
        let launch = match next_agent_session_id.as_deref() {
            Some(id) if resume => AgentLaunch::Resume(id),
            Some(id) if !is_shell_wake && preset.kind == AgentKind::Claude => {
                AgentLaunch::NewWithId(id)
            }
            _ => AgentLaunch::Fresh,
        };
        if let (Some(old), AgentLaunch::NewWithId(_) | AgentLaunch::Fresh) = (resume_id, launch) {
            if !is_shell_wake {
                tracing::warn!(
                    session_id,
                    old,
                    "Stored conversation is not resumable; starting fresh"
                );
            }
        }

        let _ = self.pty.kill(session_id);

        let now = Utc::now();
        self.started_at
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(session_id.to_string(), now);
        self.last_activity
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(session_id.to_string(), Instant::now());

        let spec = resolve_launch_spec(&preset, Path::new(&existing.cwd), launch);

        // Claim the conversation ID now (so a parallel wake sees it) and show the tab as
        // Starting; the spawn itself finishes in the background.
        let updated = {
            let mut guard = self.sessions.write().unwrap_or_else(|e| e.into_inner());
            let record = guard
                .iter_mut()
                .find(|s| s.id == session_id)
                .ok_or_else(|| VeloxError::SessionNotFound(session_id.to_string()))?;
            record.status = SessionStatus::Starting;
            record.active_agent = None;
            record.agent_session_id = next_agent_session_id;
            record.pid = None;
            record.exit_code = None;
            record.last_active_at = now.to_rfc3339();
            record.clone()
        };
        let _ = app.emit(
            "session-updated",
            SessionUpdatedEvent {
                session: updated.clone(),
            },
        );

        self.spawn_in_background(
            app,
            session_id,
            preset,
            !is_shell_wake,
            SpawnFailure::BackToSleep(existing.agent_session_id.clone()),
            spec,
            cols,
            rows,
        );
        Ok(updated)
    }

    pub fn sleep_session(&self, app: &AppHandle, session_id: &str) -> VeloxResult<SessionRecord> {
        let _lifecycle = self.lifecycle.lock().unwrap_or_else(|e| e.into_inner());
        let started = self
            .started_at
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(session_id)
            .copied()
            .unwrap_or_else(|| Utc::now() - chrono::Duration::hours(1));

        // Capture resume session ID before terminating the PTY tree
        let (pre_info, claimed) = {
            let guard = self.sessions.read().unwrap_or_else(|e| e.into_inner());
            let info = guard
                .iter()
                .find(|s| s.id == session_id)
                .map(|s| (s.agent_kind, s.profile.clone(), s.cwd.clone()))
                .ok_or_else(|| VeloxError::SessionNotFound(session_id.to_string()))?;
            (info, claimed_agent_session_ids(&guard, session_id))
        };

        let discovered_id = discover_agent_session_id(
            pre_info.0,
            pre_info.1.as_deref(),
            &pre_info.2,
            started,
            &claimed,
        );

        // Mark sleeping BEFORE kill so the reader thread's on_exit does not flip status to Exited
        let (updated, snapshot) = {
            let mut guard = self.sessions.write().unwrap_or_else(|e| e.into_inner());
            let record = guard
                .iter_mut()
                .find(|s| s.id == session_id)
                .ok_or_else(|| VeloxError::SessionNotFound(session_id.to_string()))?;
            if let Some(id) = discovered_id {
                record.agent_session_id = Some(id);
            }
            if record.active_agent == Some(crate::domain::agent::AgentKind::Shell) {
                record.wake_as_shell = true;
            } else if record.active_agent.is_some() {
                record.wake_as_shell = false;
            }
            record.active_agent = None;
            record.status = SessionStatus::Sleeping;
            record.pid = None;
            record.memory_mb = 0.0;
            record.exit_code = None;
            record.last_active_at = Utc::now().to_rfc3339();
            let cloned = record.clone();
            (cloned, guard.clone())
        };

        let _ = self.pty.kill(session_id);
        let _ = save_sessions(&snapshot);
        let _ = app.emit(
            "session-updated",
            SessionUpdatedEvent {
                session: updated.clone(),
            },
        );

        Ok(updated)
    }

    pub fn sleep_idle_sessions(
        &self,
        app: &AppHandle,
        exclude_session_id: Option<&str>,
    ) -> VeloxResult<Vec<SessionRecord>> {
        let targets: Vec<String> = {
            let guard = self.sessions.read().unwrap_or_else(|e| e.into_inner());
            guard
                .iter()
                .filter(|s| {
                    matches!(
                        s.status,
                        SessionStatus::Running | SessionStatus::Idle | SessionStatus::Blocked
                    ) && exclude_session_id != Some(s.id.as_str())
                })
                .map(|s| s.id.clone())
                .collect()
        };

        for id in targets {
            let _ = self.sleep_session(app, &id);
        }

        Ok(self.list_sessions())
    }

    pub fn close_session(&self, _app: &AppHandle, session_id: &str) -> VeloxResult<()> {
        let _lifecycle = self.lifecycle.lock().unwrap_or_else(|e| e.into_inner());
        self.pending_sizes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(session_id);
        let _ = self.pty.kill(session_id);
        let snapshot = {
            let mut guard = self.sessions.write().unwrap_or_else(|e| e.into_inner());
            guard.retain(|s| s.id != session_id);
            guard.clone()
        };
        self.last_activity
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(session_id);
        self.started_at
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(session_id);
        let _ = save_sessions(&snapshot);
        Ok(())
    }

    pub fn rename_session(
        &self,
        app: &AppHandle,
        session_id: &str,
        title: String,
    ) -> VeloxResult<SessionRecord> {
        let clean = title.trim();
        if clean.is_empty() {
            return Err(VeloxError::InvalidInput(
                "Session title cannot be empty".to_string(),
            ));
        }

        let (updated, snapshot) = {
            let mut guard = self.sessions.write().unwrap_or_else(|e| e.into_inner());
            let record = guard
                .iter_mut()
                .find(|s| s.id == session_id)
                .ok_or_else(|| VeloxError::SessionNotFound(session_id.to_string()))?;
            record.title = clean.to_string();
            let cloned = record.clone();
            (cloned, guard.clone())
        };

        let _ = save_sessions(&snapshot);
        let _ = app.emit(
            "session-updated",
            SessionUpdatedEvent {
                session: updated.clone(),
            },
        );
        Ok(updated)
    }

    pub fn reorder_sessions(&self, session_ids: &[String]) -> Vec<SessionRecord> {
        let snapshot = {
            let mut guard = self.sessions.write().unwrap_or_else(|e| e.into_inner());
            apply_session_order(&mut guard, session_ids);
            guard.clone()
        };
        let _ = save_sessions(&snapshot);
        snapshot
    }

    pub fn write_pty(&self, session_id: &str, data: &str) -> VeloxResult<()> {
        self.last_activity
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(session_id.to_string(), Instant::now());
        self.pty.write(session_id, data.as_bytes())
    }

    pub fn resize_pty(&self, session_id: &str, cols: u16, rows: u16) -> VeloxResult<()> {
        if !self.pty.is_alive(session_id) {
            // Still spawning: remember the size so the agent starts with the real pane size.
            let starting = self
                .sessions
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .any(|s| s.id == session_id && s.status == SessionStatus::Starting);
            if starting {
                self.pending_sizes
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(session_id.to_string(), (cols, rows));
            }
            return Ok(());
        }
        self.pty.resize(session_id, cols, rows)
    }

    pub fn get_scrollback_snapshot(&self, session_id: &str) -> ScrollbackSnapshot {
        let (bytes, pos) = self.pty.get_scrollback_with_pos(session_id);
        ScrollbackSnapshot {
            data_b64: BASE64.encode(bytes),
            generation: pos.generation,
            end_offset: pos.end_offset,
        }
    }

    pub fn apply_ram_telemetry(&self, session_ram: &HashMap<String, f64>) {
        let mut guard = self.sessions.write().unwrap_or_else(|e| e.into_inner());
        for s in guard.iter_mut() {
            if s.status == SessionStatus::Sleeping || s.status == SessionStatus::Exited {
                s.memory_mb = 0.0;
            } else if let Some(&mb) = session_ram.get(&s.id) {
                s.memory_mb = mb;
            }
        }
    }

    pub fn apply_telemetry(
        &self,
        app: &AppHandle,
        session_ram: &HashMap<String, f64>,
        active_agents: &HashMap<String, Option<AgentKind>>,
    ) {
        let now = Utc::now();
        let started_snapshot = self
            .started_at
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();

        let changed = {
            let mut guard = self.sessions.write().unwrap_or_else(|e| e.into_inner());
            let mut changed = Vec::new();
            for s in guard.iter_mut() {
                if s.status == SessionStatus::Sleeping || s.status == SessionStatus::Exited {
                    s.memory_mb = 0.0;
                    continue;
                }
                if let Some(&mb) = session_ram.get(&s.id) {
                    s.memory_mb = mb;
                }
                if let Some(&detected) = active_agents.get(&s.id) {
                    let elapsed_secs = started_snapshot
                        .get(&s.id)
                        .map(|started| now.signed_duration_since(*started).num_seconds())
                        .unwrap_or(SPAWN_GRACE_SECS);
                    let next = resolve_next_active_agent(s, detected, elapsed_secs);
                    if let Some(kind) = next {
                        s.wake_as_shell = kind == AgentKind::Shell;
                    }
                    if s.active_agent != next {
                        s.active_agent = next;
                        changed.push(s.clone());
                    }
                }
            }
            changed
        };

        for session in changed {
            let _ = app.emit("session-updated", SessionUpdatedEvent { session });
        }
    }

    pub fn hibernate_stale_sessions(&self, app: &AppHandle) {
        let minutes = self
            .config
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .auto_hibernate_minutes;
        if minutes == 0 {
            return;
        }
        let threshold = Duration::from_secs(minutes.saturating_mul(60));
        let now = Instant::now();

        let activity = self.last_activity.lock().unwrap_or_else(|e| e.into_inner());
        let stale_ids: Vec<String> = {
            let guard = self.sessions.read().unwrap_or_else(|e| e.into_inner());
            guard
                .iter()
                .filter(|s| matches!(s.status, SessionStatus::Idle | SessionStatus::Running))
                .filter(|s| {
                    activity
                        .get(&s.id)
                        .is_some_and(|last| now.duration_since(*last) >= threshold)
                })
                .map(|s| s.id.clone())
                .collect()
        };
        drop(activity);

        for id in stale_ids {
            tracing::info!(session_id = %id, minutes, "Auto-hibernating idle session");
            let _ = self.sleep_session(app, &id);
        }
    }

    /// Re-discover which conversation each live agent tab is driving (handles `/clear`,
    /// `/resume` inside the CLI, and AGY where the ID is only known after the first prompt).
    /// Returns the sessions whose ID changed.
    fn refresh_live_agent_session_ids(&self) -> (Vec<SessionRecord>, Vec<SessionRecord>) {
        let started_snapshot = self
            .started_at
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();

        let targets: Vec<RefreshTarget> = {
            let guard = self.sessions.read().unwrap_or_else(|e| e.into_inner());
            guard
                .iter()
                .filter(|s| {
                    !matches!(s.status, SessionStatus::Sleeping | SessionStatus::Exited)
                        && matches!(s.agent_kind, AgentKind::Claude | AgentKind::Agy)
                })
                .filter_map(|s| {
                    let started = started_snapshot.get(&s.id).copied()?;
                    Some((
                        s.id.clone(),
                        s.agent_kind,
                        s.profile.clone(),
                        s.cwd.clone(),
                        started,
                    ))
                })
                .collect()
        };

        let mut changed = Vec::new();
        let mut guard = self.sessions.write().unwrap_or_else(|e| e.into_inner());
        for (id, kind, profile, cwd, started) in targets {
            let claimed = claimed_agent_session_ids(&guard, &id);
            let Some(found) =
                discover_agent_session_id(kind, profile.as_deref(), &cwd, started, &claimed)
            else {
                continue;
            };
            if let Some(record) = guard.iter_mut().find(|s| s.id == id) {
                if record.agent_session_id.as_deref() != Some(found.as_str()) {
                    record.agent_session_id = Some(found);
                    changed.push(record.clone());
                }
            }
        }
        (changed, guard.clone())
    }

    /// Periodically persist the current conversation IDs so a crash or forced close still
    /// resumes the right conversations.
    pub fn refresh_agent_session_ids(&self, app: &AppHandle) {
        let (changed, snapshot) = self.refresh_live_agent_session_ids();
        if changed.is_empty() {
            return;
        }
        let _ = save_sessions(&snapshot);
        for session in changed {
            let _ = app.emit("session-updated", SessionUpdatedEvent { session });
        }
    }

    pub fn shutdown(&self) {
        if self.shutting_down.swap(true, Ordering::SeqCst) {
            return;
        }
        let (_, snapshot) = self.refresh_live_agent_session_ids();
        let _ = save_sessions(&snapshot);
        self.pty.kill_all();
    }
}

const SPAWN_GRACE_SECS: i64 = 6;

/// What to do with a tab whose background spawn failed.
#[derive(Debug, Clone, PartialEq, Eq)]
enum SpawnFailure {
    /// A brand-new tab: drop it (the frontend is told via `session-spawn-failed`).
    RemoveTab,
    /// A woken tab: put it back to sleep with this conversation ID.
    BackToSleep(Option<String>),
}

/// Emitted when a tab could not be started in the background.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionSpawnFailedEvent {
    session_id: String,
    message: String,
    /// The tab was removed (failed create) instead of put back to sleep (failed wake).
    removed: bool,
}

/// A background spawn still applies: the tab was not closed or hibernated meanwhile.
fn is_pending_or_live(status: SessionStatus) -> bool {
    !matches!(status, SessionStatus::Sleeping | SessionStatus::Exited)
}

/// (velox session id, agent kind, profile, cwd, spawn time)
type RefreshTarget = (String, AgentKind, Option<String>, String, DateTime<Utc>);

/// Older builds could store the same conversation on several tabs, which made them resume the same
/// conversation (duplicated tab) and orphaned the real one. Keep each ID on a single tab, preferring
/// the tab whose folder the conversation belongs to, and re-discover the others' own conversations.
pub fn repair_duplicate_agent_session_ids(
    sessions: &mut [SessionRecord],
    belongs: impl Fn(&SessionRecord, &str) -> bool,
    mut recover: impl FnMut(&SessionRecord, &HashSet<String>) -> Option<String>,
) {
    let mut owners: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, s) in sessions.iter().enumerate() {
        if let Some(id) = &s.agent_session_id {
            owners.entry(id.clone()).or_default().push(i);
        }
    }

    let mut losers = Vec::new();
    for (id, idxs) in owners.iter().filter(|(_, v)| v.len() > 1) {
        let winner = idxs.iter().copied().find(|&i| belongs(&sessions[i], id));
        for &i in idxs {
            if Some(i) != winner {
                losers.push(i);
            }
        }
        tracing::warn!(id, tabs = idxs.len(), ?winner, "Repairing duplicated conversation ID");
    }
    losers.sort_unstable();

    for &i in &losers {
        sessions[i].agent_session_id = None;
    }
    for i in losers {
        let claimed: HashSet<String> = sessions
            .iter()
            .filter_map(|s| s.agent_session_id.clone())
            .collect();
        sessions[i].agent_session_id = recover(&sessions[i], &claimed);
    }
}

/// Conversation IDs owned by every Velox tab except `exclude_session_id`, so two tabs in the same
/// folder never end up resuming the same conversation.
fn claimed_agent_session_ids(
    sessions: &[SessionRecord],
    exclude_session_id: &str,
) -> HashSet<String> {
    sessions
        .iter()
        .filter(|s| s.id != exclude_session_id)
        .filter_map(|s| s.agent_session_id.clone())
        .collect()
}

/// Choose the preset and optional resume ID when waking a hibernated session.
/// If the session was at the shell when it went to sleep (`wake_as_shell`), wake it as a plain shell.
pub fn select_wake_preset_and_resume<'a>(
    presets: &[AgentPreset],
    existing: &'a SessionRecord,
) -> VeloxResult<(AgentPreset, Option<&'a str>, bool)> {
    if existing.wake_as_shell {
        let shell_preset = presets
            .iter()
            .find(|p| p.kind == AgentKind::Shell)
            .cloned()
            .ok_or_else(|| VeloxError::InvalidInput("No shell preset found".to_string()))?;
        return Ok((shell_preset, None, true));
    }

    let preset = presets
        .iter()
        .find(|p| p.id == existing.preset_id)
        .or_else(|| presets.iter().find(|p| p.kind == existing.agent_kind))
        .cloned()
        .ok_or_else(|| {
            VeloxError::InvalidInput(format!("No preset found for {}", existing.preset_id))
        })?;
    Ok((preset, existing.agent_session_id.as_deref(), false))
}

/// Compute the next `active_agent` value, suppressing `"shell"` during the initial spawn grace
/// period for agent sessions before the CLI child process appears.
pub fn resolve_next_active_agent(
    session: &SessionRecord,
    detected: Option<AgentKind>,
    elapsed_since_spawn_secs: i64,
) -> Option<AgentKind> {
    match detected {
        Some(AgentKind::Shell)
            if session.agent_kind != AgentKind::Shell
                && !session.wake_as_shell
                && session.active_agent.is_none()
                && elapsed_since_spawn_secs < SPAWN_GRACE_SECS =>
        {
            None
        }
        other => other,
    }
}

/// Reorder `sessions` in place to match `session_ids`.
/// Unknown IDs and duplicates are ignored; unlisted sessions keep their relative order at the end.
pub fn apply_session_order(sessions: &mut Vec<SessionRecord>, session_ids: &[String]) {
    let mut reordered = Vec::with_capacity(sessions.len());
    let mut seen = HashSet::with_capacity(session_ids.len());

    for id in session_ids {
        if !seen.insert(id.as_str()) {
            continue;
        }
        if let Some(pos) = sessions.iter().position(|s| s.id == *id) {
            reordered.push(sessions.remove(pos));
        }
    }

    reordered.append(sessions);
    *sessions = reordered;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::agent::AgentKind;

    fn dummy_session(id: &str) -> SessionRecord {
        SessionRecord {
            id: id.to_string(),
            title: id.to_string(),
            agent_kind: AgentKind::Shell,
            active_agent: None,
            preset_id: "shell".to_string(),
            profile: None,
            cwd: ".".to_string(),
            status: SessionStatus::Sleeping,
            agent_session_id: None,
            wake_as_shell: false,
            pid: None,
            memory_mb: 0.0,
            created_at: "2026-11-24T00:00:00Z".to_string(),
            last_active_at: "2026-11-24T00:00:00Z".to_string(),
            exit_code: None,
        }
    }

    #[test]
    fn apply_session_order_handles_partial_unknown_and_duplicate_ids() {
        let mut sessions = vec![
            dummy_session("a"),
            dummy_session("b"),
            dummy_session("c"),
            dummy_session("d"),
        ];

        let order = vec![
            "c".to_string(),
            "missing".to_string(),
            "a".to_string(),
            "c".to_string(),
        ];
        apply_session_order(&mut sessions, &order);

        let ids: Vec<&str> = sessions.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec!["c", "a", "b", "d"]);
    }

    #[test]
    fn repair_duplicate_ids_keeps_folder_owner_and_recovers_others() {
        let mut a = dummy_session("a");
        a.cwd = "D:/Digycrypt".to_string();
        a.agent_session_id = Some("dup".to_string());
        let mut b = dummy_session("b");
        b.cwd = "D:/Workspace".to_string();
        b.agent_session_id = Some("dup".to_string());
        let mut c = dummy_session("c");
        c.agent_session_id = Some("solo".to_string());
        let mut sessions = vec![a, b, c];

        repair_duplicate_agent_session_ids(
            &mut sessions,
            |s, _| s.cwd == "D:/Workspace",
            |s, claimed| {
                assert!(claimed.contains("dup") && claimed.contains("solo"));
                Some(format!("recovered-{}", s.id))
            },
        );
        let ids: Vec<_> = sessions.iter().map(|s| s.agent_session_id.as_deref()).collect();
        assert_eq!(ids, vec![Some("recovered-a"), Some("dup"), Some("solo")]);

        // Nobody owns it by folder: every duplicate is recovered (or cleared).
        let mut x = dummy_session("x");
        x.agent_session_id = Some("dup".to_string());
        let mut y = dummy_session("y");
        y.agent_session_id = Some("dup".to_string());
        let mut sessions = vec![x, y];
        repair_duplicate_agent_session_ids(&mut sessions, |_, _| false, |_, _| None);
        assert!(sessions.iter().all(|s| s.agent_session_id.is_none()));
    }

    #[test]
    fn apply_session_order_empty_input_preserves_order() {
        let mut sessions = vec![dummy_session("a"), dummy_session("b")];
        apply_session_order(&mut sessions, &[]);
        let ids: Vec<&str> = sessions.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec!["a", "b"]);
    }

    #[test]
    fn select_wake_preset_uses_shell_when_wake_as_shell_is_set() {
        let presets = build_default_presets();
        let mut session = dummy_session("s1");
        session.agent_kind = AgentKind::Agy;
        session.preset_id = "agy".to_string();
        session.agent_session_id = Some("conv-1".to_string());
        session.wake_as_shell = true;

        let (preset, resume, is_shell) =
            select_wake_preset_and_resume(&presets, &session).expect("wake preset");
        assert_eq!(preset.kind, AgentKind::Shell);
        assert_eq!(resume, None);
        assert!(is_shell);

        session.wake_as_shell = false;
        let (preset, resume, is_shell) =
            select_wake_preset_and_resume(&presets, &session).expect("wake preset");
        assert_eq!(preset.kind, AgentKind::Agy);
        assert_eq!(resume, Some("conv-1"));
        assert!(!is_shell);
    }

    #[test]
    fn resolve_next_active_agent_suppresses_shell_during_spawn_grace_window() {
        let mut session = dummy_session("s1");
        session.agent_kind = AgentKind::Claude;
        session.preset_id = "claude".to_string();
        session.wake_as_shell = false;
        session.active_agent = None;

        // First few seconds after spawning an agent: suppress "shell"
        assert_eq!(
            resolve_next_active_agent(&session, Some(AgentKind::Shell), 2),
            None
        );
        // Once the agent appears, report it immediately even inside grace window
        assert_eq!(
            resolve_next_active_agent(&session, Some(AgentKind::Claude), 2),
            Some(AgentKind::Claude)
        );
        // After grace window expires, report "shell"
        assert_eq!(
            resolve_next_active_agent(&session, Some(AgentKind::Shell), 7),
            Some(AgentKind::Shell)
        );
        // If agent was already observed and then exited early, report "shell" immediately
        session.active_agent = Some(AgentKind::Claude);
        assert_eq!(
            resolve_next_active_agent(&session, Some(AgentKind::Shell), 3),
            Some(AgentKind::Shell)
        );
    }
}
