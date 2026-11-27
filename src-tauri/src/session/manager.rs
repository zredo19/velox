use crate::domain::agent::{
    build_default_presets, prepare_agent_profile, resolve_launch_spec_with_options, AgentKind,
    AgentLaunch, AgentPreset, AgentSpawnSpec, SessionLaunchContext,
};
use crate::domain::config::VeloxConfig;
use crate::domain::error::{VeloxError, VeloxResult};
use crate::domain::session::{
    PtyOutputEvent, ScrollbackSnapshot, SessionRecord, SessionStatus, SessionUpdatedEvent,
};
use crate::pty::manager::{ExitCallback, OutputCallback, PtyManager, StreamPos};
use crate::session::bootstrap::velox_cli_bin_dir;
use crate::session::detector::{
    agent_session_belongs_to_cwd, agy_conversation_exists, check_agent_session_resumable,
    detect_status_from_output, is_uuid, match_agy_history_entries, read_agy_history,
    AgyHistoryEntry, AgyMatchCandidate, Resumability,
};
use crate::session::links::{
    agent_links_dir, ensure_agent_hooks, read_agent_link_from, remove_agent_link, AgentLinkRecord,
};
use crate::session::store::{load_sessions, save_sessions};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use chrono::{DateTime, Utc};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

const MAX_ENTER_TIMESTAMPS_PER_SESSION: usize = 16;

/// Hook receiving live PTY output and resize notifications (used by `TeamHub`).
pub trait SessionTap: Send + Sync {
    fn on_output(&self, session_id: &str, data: &[u8], pos: StreamPos);
    fn on_resize(&self, session_id: &str, cols: u16, rows: u16);
}

/// Coordinates persisted sessions, ConPTY instances, lazy-restore, and auto-hibernation.
pub struct SessionManager {
    sessions: Arc<RwLock<Vec<SessionRecord>>>,
    pty: Arc<PtyManager>,
    config: RwLock<VeloxConfig>,
    presets: Vec<AgentPreset>,
    last_activity: Arc<Mutex<HashMap<String, Instant>>>,
    started_at: Arc<Mutex<HashMap<String, DateTime<Utc>>>>,
    enter_timestamps: Arc<Mutex<HashMap<String, VecDeque<i64>>>>,
    shutting_down: Arc<AtomicBool>,
    /// Guards against overwriting disk state with an empty list if startup load failed.
    persistence_blocked: Arc<AtomicBool>,
    claude_settings_path: RwLock<Option<PathBuf>>,
    /// Size a pane asked for while its PTY was still being spawned (applied at spawn).
    pending_sizes: Arc<Mutex<HashMap<String, (u16, u16)>>>,
    /// Current live PTY dimensions per session (`cols, rows`).
    pty_sizes: Arc<Mutex<HashMap<String, (u16, u16)>>>,
    /// Optional tap receiving live PTY output and resize events.
    tap: Arc<RwLock<Option<Arc<dyn SessionTap>>>>,
    /// Tauri application handle used by headless subsystems (e.g. direct local messaging wake).
    app_handle: RwLock<Option<AppHandle>>,
    /// Serializes the decision part of create/wake/sleep/close now that those commands run
    /// off the main thread (they used to be serialized by it).
    lifecycle: Mutex<()>,
}

impl SessionManager {
    pub fn new(config: VeloxConfig) -> Self {
        let claude_settings = ensure_agent_hooks();
        let links_dir = agent_links_dir();
        let mut persistence_blocked = false;

        let initial_sessions = if config.lazy_restore_on_startup {
            match load_sessions() {
                Ok(mut loaded) => {
                    adopt_newer_agent_links(&mut loaded, &links_dir);
                    repair_duplicate_agent_session_ids(
                        &mut loaded,
                        |s, id| {
                            agent_session_belongs_to_cwd(
                                s.agent_kind,
                                s.profile.as_deref(),
                                &s.cwd,
                                id,
                            )
                        },
                        |s, claimed| recover_from_agent_link(&links_dir, s, claimed),
                    );
                    loaded
                }
                Err(err) => {
                    tracing::error!(
                        %err,
                        "Failed to load persisted sessions; protecting disk files from empty overwrite"
                    );
                    persistence_blocked = true;
                    Vec::new()
                }
            }
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
            enter_timestamps: Arc::new(Mutex::new(HashMap::new())),
            shutting_down: Arc::new(AtomicBool::new(false)),
            persistence_blocked: Arc::new(AtomicBool::new(persistence_blocked)),
            claude_settings_path: RwLock::new(claude_settings),
            pending_sizes: Arc::new(Mutex::new(HashMap::new())),
            pty_sizes: Arc::new(Mutex::new(HashMap::new())),
            tap: Arc::new(RwLock::new(None)),
            app_handle: RwLock::new(None),
            lifecycle: Mutex::new(()),
        }
    }

    /// Registers the Tauri app handle so non-IPC subsystems can wake sleeping sessions.
    pub fn set_app_handle(&self, app: AppHandle) {
        *self.app_handle.write().unwrap_or_else(|e| e.into_inner()) = Some(app);
    }

    /// Wakes a sleeping or exited session using the stored `AppHandle`.
    pub fn wake_session_headless(
        &self,
        session_id: &str,
        cols: u16,
        rows: u16,
    ) -> VeloxResult<SessionRecord> {
        let app = self
            .app_handle
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .ok_or_else(|| {
                VeloxError::InvalidInput("Application handle is not initialized yet".to_string())
            })?;
        self.wake_session(&app, session_id, cols, rows)
    }

    fn persist_snapshot_slice(&self, snapshot: &[SessionRecord]) {
        save_if_allowed(&self.persistence_blocked, snapshot);
    }

    fn ensure_claude_settings_path(&self) -> Option<PathBuf> {
        {
            let guard = self
                .claude_settings_path
                .read()
                .unwrap_or_else(|e| e.into_inner());
            if let Some(p) = guard.as_ref() {
                if p.is_file() {
                    return Some(p.clone());
                }
            }
        }
        let refreshed = ensure_agent_hooks();
        *self
            .claude_settings_path
            .write()
            .unwrap_or_else(|e| e.into_inner()) = refreshed.clone();
        refreshed
    }

    fn prepare_spawn_spec(
        &self,
        preset: &AgentPreset,
        cwd: &Path,
        launch: AgentLaunch<'_>,
        velox_session_id: &str,
        session_title: &str,
    ) -> AgentSpawnSpec {
        let gateway_url = {
            let cfg = self.config.read().unwrap_or_else(|e| e.into_inner());
            (cfg.mcp_gateway_enabled && cfg.mcp_gateway_port > 0)
                .then(|| format!("http://127.0.0.1:{}", cfg.mcp_gateway_port))
        };
        let cli_bin_dir = velox_cli_bin_dir();
        let ctx = SessionLaunchContext {
            session_id: velox_session_id,
            session_title,
            gateway_url: gateway_url.as_deref(),
            cli_bin_dir: Some(&cli_bin_dir),
        };
        let claude_settings = if preset.kind == AgentKind::Claude {
            self.ensure_claude_settings_path()
        } else {
            if preset.kind == AgentKind::Agy {
                let _ = self.ensure_claude_settings_path();
            }
            None
        };
        let mut spec = resolve_launch_spec_with_options(
            preset,
            cwd,
            launch,
            Some(&ctx),
            claude_settings.as_deref(),
        );
        spec.env
            .insert("VELOX_SESSION_ID".to_string(), velox_session_id.to_string());
        spec.env.insert(
            "VELOX_AGENT_LINKS_DIR".to_string(),
            agent_links_dir().to_string_lossy().into_owned(),
        );
        spec
    }

    /// Registers a tap that receives live PTY output and resize notifications.
    pub fn set_tap(&self, tap: Arc<dyn SessionTap>) {
        *self.tap.write().unwrap_or_else(|e| e.into_inner()) = Some(tap);
    }

    /// Returns the last known live PTY dimensions `(cols, rows)` for `session_id`.
    pub fn pty_size(&self, session_id: &str) -> Option<(u16, u16)> {
        self.pty_sizes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(session_id)
            .copied()
    }

    /// Returns raw scrollback bytes together with the stream position they cover.
    pub fn get_scrollback_with_pos(&self, session_id: &str) -> (Vec<u8>, StreamPos) {
        self.pty.get_scrollback_with_pos(session_id)
    }

    /// Returns `true` when the underlying PTY process for `session_id` is alive.
    pub fn is_pty_alive(&self, session_id: &str) -> bool {
        self.pty.is_alive(session_id)
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
        let tap_out = Arc::clone(&self.tap);
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

                if let Some(tap) = tap_out
                    .read()
                    .unwrap_or_else(|e| e.into_inner())
                    .as_ref()
                    .cloned()
                {
                    tap.on_output(session_id, data, pos);
                }

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
        let enters_exit = Arc::clone(&self.enter_timestamps);
        let sizes_exit = Arc::clone(&self.pty_sizes);
        let shutting_down = Arc::clone(&self.shutting_down);
        let persistence_blocked = Arc::clone(&self.persistence_blocked);
        let app_exit = app;

        let on_exit: ExitCallback = Arc::new(move |session_id: &str, exit_code: Option<i32>| {
            // During app shutdown the IDs were already captured; don't let PTY teardown race it.
            if shutting_down.load(Ordering::SeqCst) {
                return;
            }
            sizes_exit
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(session_id);
            let started = started_exit
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(session_id)
                .copied()
                .unwrap_or_else(|| Utc::now() - chrono::Duration::minutes(5));
            let enters = enters_exit
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(session_id)
                .cloned()
                .unwrap_or_default();

            let links_dir = agent_links_dir();
            let (updated, snapshot_to_save) = {
                let mut guard = sessions_exit.write().unwrap_or_else(|e| e.into_inner());
                let claimed = claimed_agent_session_ids(&guard, session_id);
                let updated = if let Some(record) = guard.iter_mut().find(|s| s.id == session_id) {
                    if record.status == SessionStatus::Sleeping {
                        None
                    } else {
                        let resolved = resolve_single_session_id(
                            record,
                            started,
                            &enters,
                            &claimed,
                            &links_dir,
                            None,
                            agy_conversation_exists,
                        );
                        if let Some(found_id) = resolved {
                            if record.agent_session_id.as_deref() != Some(found_id.as_str()) {
                                let old = record.agent_session_id.clone();
                                record.set_agent_session_id_preserving(Some(found_id.clone()));
                                tracing::info!(
                                    session_id = %record.id,
                                    old = ?old,
                                    new = %found_id,
                                    "Captured agent session ID on PTY exit"
                                );
                            }
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
                save_if_allowed(&persistence_blocked, &snapshot);
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
            previous_agent_session_ids: Vec::new(),
            wake_as_shell: preset.kind == crate::domain::agent::AgentKind::Shell,
            pid: None,
            memory_mb: 0.0,
            created_at: now_str.clone(),
            last_active_at: now_str,
            exit_code: None,
            shared: false,
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
        self.enter_timestamps
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&session_id);
        remove_agent_link(&session_id);

        let launch = match record.agent_session_id.as_deref() {
            Some(id) => AgentLaunch::NewWithId(id),
            None => AgentLaunch::Fresh,
        };
        let spec = self.prepare_spawn_spec(
            &preset,
            Path::new(&resolved_cwd),
            launch,
            &session_id,
            &record.title,
        );

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
        let pty_sizes = Arc::clone(&self.pty_sizes);
        let tap = Arc::clone(&self.tap);
        let sessions = Arc::clone(&self.sessions);
        let shutting_down = Arc::clone(&self.shutting_down);
        let persistence_blocked = Arc::clone(&self.persistence_blocked);
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
                let mut final_cols = cols.max(10);
                let mut final_rows = rows.max(2);
                // A resize that raced with the spawn was parked instead of applied.
                if let Some((c, r)) = take_size() {
                    if pty.resize(&session_id, c, r).is_ok() {
                        final_cols = c.max(10);
                        final_rows = r.max(2);
                    }
                }
                pty_sizes
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(session_id.clone(), (final_cols, final_rows));
                if let Some(t) = tap
                    .read()
                    .unwrap_or_else(|e| e.into_inner())
                    .as_ref()
                    .cloned()
                {
                    t.on_resize(&session_id, final_cols, final_rows);
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
                                if let SpawnFailure::BackToSleep(previous_id, previous_history) =
                                    &on_failure
                                {
                                    record.agent_session_id = previous_id.clone();
                                    record.previous_agent_session_ids = previous_history.clone();
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
                        pty_sizes
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .remove(&session_id);
                        let _ = pty.kill(&session_id);
                    }
                }
                Some((session, snapshot)) => {
                    save_if_allowed(&persistence_blocked, &snapshot);
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

        // Only resume conversations that really exist and have content; otherwise preserve the
        // old ID in `previous_agent_session_ids`, log why it wasn't resumable, and start fresh.
        // We never steal another folder's conversation via mtime heuristics.
        let claimed = {
            let guard = self.sessions.read().unwrap_or_else(|e| e.into_inner());
            claimed_agent_session_ids(&guard, session_id)
        };
        let profile = preset.profile.as_deref().or(existing.profile.as_deref());
        let (next_agent_session_id, resume, archived_unusable): (
            Option<String>,
            bool,
            Option<String>,
        ) = if is_shell_wake {
            (existing.agent_session_id.clone(), false, None)
        } else {
            let mut rejected_old: Option<String> = None;
            let usable = match resume_id {
                Some(id) if claimed.contains(id) => {
                    tracing::warn!(
                        session_id,
                        old_id = %id,
                        "Stored conversation is already owned by another tab; preserving in previous_agent_session_ids and starting fresh"
                    );
                    rejected_old = Some(id.to_string());
                    None
                }
                Some(id) => match check_agent_session_resumable(preset.kind, profile, id) {
                    Resumability::Resumable => Some(id.to_string()),
                    Resumability::NotResumable(reason) => {
                        tracing::warn!(
                            session_id,
                            old_id = %id,
                            reason = %reason,
                            "Stored conversation is not resumable; preserving in previous_agent_session_ids and starting fresh"
                        );
                        rejected_old = Some(id.to_string());
                        None
                    }
                },
                None => None,
            };
            match usable {
                Some(id) => (Some(id), true, None),
                None if preset.kind == AgentKind::Claude => {
                    (Some(uuid::Uuid::new_v4().to_string()), false, rejected_old)
                }
                None => (None, false, rejected_old),
            }
        };
        let launch = match next_agent_session_id.as_deref() {
            Some(id) if resume => AgentLaunch::Resume(id),
            Some(id) if !is_shell_wake && preset.kind == AgentKind::Claude => {
                AgentLaunch::NewWithId(id)
            }
            _ => AgentLaunch::Fresh,
        };

        self.pty_sizes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(session_id);
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
        self.enter_timestamps
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(session_id);
        remove_agent_link(session_id);

        let spec = self.prepare_spawn_spec(
            &preset,
            Path::new(&existing.cwd),
            launch,
            session_id,
            &existing.title,
        );

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
            if let Some(old_id) = archived_unusable.as_deref() {
                record.preserve_previous_agent_session_id(old_id);
            }
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
            SpawnFailure::BackToSleep(
                existing.agent_session_id.clone(),
                existing.previous_agent_session_ids.clone(),
            ),
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
        let enters = self
            .enter_timestamps
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(session_id)
            .cloned()
            .unwrap_or_default();

        // Capture resume session ID deterministically before terminating the PTY tree.
        let links_dir = agent_links_dir();
        let (needs_agy_history, claimed, snapshot_record) = {
            let guard = self.sessions.read().unwrap_or_else(|e| e.into_inner());
            let record = guard
                .iter()
                .find(|s| s.id == session_id)
                .cloned()
                .ok_or_else(|| VeloxError::SessionNotFound(session_id.to_string()))?;
            let needs_agy = record.agent_kind == AgentKind::Agy && record.agent_session_id.is_none();
            (
                needs_agy,
                claimed_agent_session_ids(&guard, session_id),
                record,
            )
        };
        let agy_history = if needs_agy_history {
            read_agy_history()
        } else {
            None
        };
        let discovered_id = resolve_single_session_id(
            &snapshot_record,
            started,
            &enters,
            &claimed,
            &links_dir,
            agy_history.as_deref(),
            agy_conversation_exists,
        );

        // Mark sleeping BEFORE kill so the reader thread's on_exit does not flip status to Exited
        let (updated, snapshot) = {
            let mut guard = self.sessions.write().unwrap_or_else(|e| e.into_inner());
            let record = guard
                .iter_mut()
                .find(|s| s.id == session_id)
                .ok_or_else(|| VeloxError::SessionNotFound(session_id.to_string()))?;
            if let Some(id) = discovered_id {
                if record.agent_session_id.as_deref() != Some(id.as_str()) {
                    let old = record.agent_session_id.clone();
                    record.set_agent_session_id_preserving(Some(id.clone()));
                    tracing::info!(
                        session_id = %record.id,
                        old = ?old,
                        new = %id,
                        "Captured agent session ID before sleep"
                    );
                }
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

        self.pty_sizes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(session_id);
        let _ = self.pty.kill(session_id);
        self.persist_snapshot_slice(&snapshot);
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
        self.pty_sizes
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
        self.enter_timestamps
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(session_id);
        remove_agent_link(session_id);
        self.persist_snapshot_slice(&snapshot);
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

        self.persist_snapshot_slice(&snapshot);
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
        self.persist_snapshot_slice(&snapshot);
        snapshot
    }

    pub fn set_session_shared(
        &self,
        app: &AppHandle,
        session_id: &str,
        shared: bool,
    ) -> VeloxResult<SessionRecord> {
        let (updated, snapshot) = {
            let mut guard = self.sessions.write().unwrap_or_else(|e| e.into_inner());
            let record = guard
                .iter_mut()
                .find(|s| s.id == session_id)
                .ok_or_else(|| VeloxError::SessionNotFound(session_id.to_string()))?;
            record.shared = shared;
            let cloned = record.clone();
            (cloned, guard.clone())
        };

        self.persist_snapshot_slice(&snapshot);
        let _ = app.emit(
            "session-updated",
            SessionUpdatedEvent {
                session: updated.clone(),
            },
        );
        Ok(updated)
    }

    pub fn write_pty_bytes(&self, session_id: &str, data: &[u8]) -> VeloxResult<()> {
        self.last_activity
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(session_id.to_string(), Instant::now());
        if data.contains(&b'\r') || data.contains(&b'\n') {
            let now_ms = Utc::now().timestamp_millis();
            let mut enters = self
                .enter_timestamps
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let queue = enters.entry(session_id.to_string()).or_default();
            queue.push_back(now_ms);
            while queue.len() > MAX_ENTER_TIMESTAMPS_PER_SESSION {
                queue.pop_front();
            }
        }
        self.pty.write(session_id, data)
    }

    pub fn write_pty(&self, session_id: &str, data: &str) -> VeloxResult<()> {
        self.write_pty_bytes(session_id, data.as_bytes())
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
        self.pty.resize(session_id, cols, rows)?;
        let clamped = (cols.max(10), rows.max(2));
        self.pty_sizes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(session_id.to_string(), clamped);
        if let Some(tap) = self
            .tap
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .cloned()
        {
            tap.on_resize(session_id, clamped.0, clamped.1);
        }
        Ok(())
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

    /// Reconcile live agent conversation IDs using deterministic hook link files (for Claude &
    /// AGY) and unique Enter-timestamp matching (AGY fallback when unlinked). Never overwrites a
    /// stored ID without strong evidence.
    fn refresh_live_agent_session_ids(&self) -> (Vec<SessionRecord>, Vec<SessionRecord>) {
        let started_snapshot = self
            .started_at
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let enters_snapshot = self
            .enter_timestamps
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();

        let needs_agy_history = {
            let guard = self.sessions.read().unwrap_or_else(|e| e.into_inner());
            guard.iter().any(|s| {
                !matches!(s.status, SessionStatus::Sleeping | SessionStatus::Exited)
                    && s.agent_kind == AgentKind::Agy
                    && s.agent_session_id.is_none()
            })
        };
        let agy_history = if needs_agy_history {
            read_agy_history()
        } else {
            None
        };

        let links_dir = agent_links_dir();
        let mut guard = self.sessions.write().unwrap_or_else(|e| e.into_inner());
        let changed = refresh_live_sessions_with(
            &mut guard,
            &started_snapshot,
            &enters_snapshot,
            &links_dir,
            agy_history.as_deref(),
            agy_conversation_exists,
        );
        (changed, guard.clone())
    }

    /// Periodically persist the current conversation IDs so a crash or forced close still
    /// resumes the right conversations.
    pub fn refresh_agent_session_ids(&self, app: &AppHandle) {
        let (changed, snapshot) = self.refresh_live_agent_session_ids();
        if changed.is_empty() {
            return;
        }
        self.persist_snapshot_slice(&snapshot);
        for session in changed {
            let _ = app.emit("session-updated", SessionUpdatedEvent { session });
        }
    }

    /// Flush live conversation links and persist `sessions.json` without killing PTYs (used on
    /// Windows `WM_QUERYENDSESSION`).
    pub fn persist_snapshot(&self) {
        let (_, snapshot) = self.refresh_live_agent_session_ids();
        self.persist_snapshot_slice(&snapshot);
    }

    pub fn shutdown(&self) {
        if self.shutting_down.swap(true, Ordering::SeqCst) {
            return;
        }
        let (_, snapshot) = self.refresh_live_agent_session_ids();
        self.persist_snapshot_slice(&snapshot);
        self.pty.kill_all();
    }
}

const SPAWN_GRACE_SECS: i64 = 6;

fn save_if_allowed(persistence_blocked: &AtomicBool, snapshot: &[SessionRecord]) {
    if persistence_blocked.load(Ordering::SeqCst) && snapshot.is_empty() {
        tracing::warn!(
            "Skipping empty sessions.json save because startup load failed; protecting existing files"
        );
        return;
    }
    if !snapshot.is_empty() {
        persistence_blocked.store(false, Ordering::SeqCst);
    }
    if let Err(err) = save_sessions(snapshot) {
        tracing::warn!(%err, "Failed to persist sessions.json");
    }
}

fn expected_link_agent(kind: AgentKind) -> Option<&'static str> {
    match kind {
        AgentKind::Claude => Some("claude"),
        AgentKind::Agy => Some("agy"),
        _ => None,
    }
}

fn valid_hook_link_for_session(
    links_dir: &Path,
    record: &SessionRecord,
    min_updated_at_ms: Option<i64>,
) -> Option<AgentLinkRecord> {
    let expected_agent = expected_link_agent(record.agent_kind)?;
    let link = read_agent_link_from(links_dir, &record.id)?;
    if link.agent != expected_agent || !is_uuid(&link.agent_session_id) {
        return None;
    }
    if let Some(min_ms) = min_updated_at_ms {
        if link.updated_at_ms > 0 && link.updated_at_ms < min_ms {
            return None;
        }
    }
    Some(link)
}

fn parse_rfc3339_ms(raw: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|dt| dt.timestamp_millis())
}

/// On startup, adopt any `agent-links/<velox_id>.json` written after `last_active_at` (or when
/// `agent_session_id` is still empty), recovering conversation switches even after an unexpected
/// power loss between periodic saves.
fn adopt_newer_agent_links(sessions: &mut [SessionRecord], links_dir: &Path) {
    for s in sessions.iter_mut() {
        let min_ms = if s.agent_session_id.is_none() {
            None
        } else {
            parse_rfc3339_ms(&s.last_active_at)
        };
        if let Some(link) = valid_hook_link_for_session(links_dir, s, min_ms) {
            if s.agent_session_id.as_deref() != Some(link.agent_session_id.as_str()) {
                let old = s.agent_session_id.clone();
                s.set_agent_session_id_preserving(Some(link.agent_session_id.clone()));
                tracing::info!(
                    session_id = %s.id,
                    old = ?old,
                    new = %link.agent_session_id,
                    source = ?link.source,
                    "Recovered newer agent session ID from hook link on startup"
                );
            }
        }
    }
}

fn recover_from_agent_link(
    links_dir: &Path,
    record: &SessionRecord,
    claimed: &HashSet<String>,
) -> Option<String> {
    let link = valid_hook_link_for_session(links_dir, record, None)?;
    if claimed.contains(&link.agent_session_id) {
        return None;
    }
    Some(link.agent_session_id)
}

fn resolve_single_session_id(
    record: &SessionRecord,
    started_at: DateTime<Utc>,
    enters: &VecDeque<i64>,
    claimed: &HashSet<String>,
    links_dir: &Path,
    agy_history: Option<&[AgyHistoryEntry]>,
    agy_conv_exists: impl Fn(&str) -> bool,
) -> Option<String> {
    let min_ms = Some(started_at.timestamp_millis().saturating_sub(2_000));
    if let Some(link) = valid_hook_link_for_session(links_dir, record, min_ms) {
        return Some(link.agent_session_id);
    }
    if record.agent_kind == AgentKind::Agy && record.agent_session_id.is_none() {
        if let Some(history) = agy_history {
            let candidate = AgyMatchCandidate {
                velox_session_id: record.id.clone(),
                started_at_ms: started_at.timestamp_millis(),
                enter_timestamps_ms: enters.iter().copied().collect(),
            };
            let matched =
                match_agy_history_entries(history, &[candidate], claimed, agy_conv_exists);
            return matched.get(&record.id).cloned();
        }
    }
    None
}

/// Core deterministic refresh logic extracted for unit testing.
fn refresh_live_sessions_with(
    sessions: &mut [SessionRecord],
    started_at: &HashMap<String, DateTime<Utc>>,
    enter_timestamps: &HashMap<String, VecDeque<i64>>,
    links_dir: &Path,
    agy_history: Option<&[AgyHistoryEntry]>,
    agy_conv_exists: impl Fn(&str) -> bool,
) -> Vec<SessionRecord> {
    let mut changed = Vec::new();
    let mut unlinked_agy = Vec::new();

    for record in sessions.iter_mut() {
        if matches!(record.status, SessionStatus::Sleeping | SessionStatus::Exited)
            || !matches!(record.agent_kind, AgentKind::Claude | AgentKind::Agy)
        {
            continue;
        }
        let Some(started) = started_at.get(&record.id).copied() else {
            continue;
        };
        let min_ms = Some(started.timestamp_millis().saturating_sub(2_000));
        if let Some(link) = valid_hook_link_for_session(links_dir, record, min_ms) {
            if record.agent_session_id.as_deref() != Some(link.agent_session_id.as_str()) {
                let old = record.agent_session_id.clone();
                record.set_agent_session_id_preserving(Some(link.agent_session_id.clone()));
                tracing::info!(
                    session_id = %record.id,
                    old = ?old,
                    new = %link.agent_session_id,
                    source = ?link.source,
                    "Updated agent session ID from hook link"
                );
                changed.push(record.clone());
            }
            continue;
        }

        if record.agent_kind == AgentKind::Agy && record.agent_session_id.is_none() {
            let enters = enter_timestamps
                .get(&record.id)
                .map(|q| q.iter().copied().collect())
                .unwrap_or_default();
            unlinked_agy.push(AgyMatchCandidate {
                velox_session_id: record.id.clone(),
                started_at_ms: started.timestamp_millis(),
                enter_timestamps_ms: enters,
            });
        }
    }

    if !unlinked_agy.is_empty() {
        if let Some(history) = agy_history {
            let claimed: HashSet<String> = sessions
                .iter()
                .filter_map(|s| s.agent_session_id.clone())
                .collect();
            let matches =
                match_agy_history_entries(history, &unlinked_agy, &claimed, agy_conv_exists);
            for (tab_id, conv_id) in matches {
                if let Some(record) = sessions.iter_mut().find(|s| s.id == tab_id) {
                    if record.agent_session_id.is_none() {
                        record.agent_session_id = Some(conv_id.clone());
                        tracing::info!(
                            session_id = %record.id,
                            new = %conv_id,
                            "Assigned AGY conversation ID from unique Enter timestamp"
                        );
                        changed.push(record.clone());
                    }
                }
            }
        }
    }

    changed
}

/// What to do with a tab whose background spawn failed.
#[derive(Debug, Clone, PartialEq, Eq)]
enum SpawnFailure {
    /// A brand-new tab: drop it (the frontend is told via `session-spawn-failed`).
    RemoveTab,
    /// A woken tab: put it back to sleep with this conversation ID and history.
    BackToSleep(Option<String>, Vec<String>),
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

/// Older builds could store the same conversation on several tabs, which made them resume the same
/// conversation (duplicated tab) and orphaned the real one. Keep each ID on a single tab, preferring
/// the tab whose folder the conversation belongs to, preserving replaced IDs in
/// `previous_agent_session_ids`.
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
        sessions[i].set_agent_session_id_preserving(None);
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
            previous_agent_session_ids: Vec::new(),
            wake_as_shell: false,
            pid: None,
            memory_mb: 0.0,
            created_at: "2026-11-24T00:00:00Z".to_string(),
            last_active_at: "2026-11-24T00:00:00Z".to_string(),
            exit_code: None,
            shared: false,
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

    #[test]
    fn refresh_never_overwrites_ids_without_hook_or_unique_evidence() {
        let links_dir =
            std::env::temp_dir().join(format!("velox-refresh-noop-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&links_dir).expect("create links dir");

        let now = Utc::now();
        let claude_id = "11111111-1111-4111-8111-111111111111";
        let agy_id = "22222222-2222-4222-8222-222222222222";
        let other_agy_id = "33333333-3333-4333-8333-333333333333";

        let mut c1 = dummy_session("tab-c1");
        c1.agent_kind = AgentKind::Claude;
        c1.status = SessionStatus::Running;
        c1.agent_session_id = Some(claude_id.to_string());

        let mut a1 = dummy_session("tab-a1");
        a1.agent_kind = AgentKind::Agy;
        a1.status = SessionStatus::Running;
        a1.agent_session_id = Some(agy_id.to_string());

        let mut a2 = dummy_session("tab-a2");
        a2.agent_kind = AgentKind::Agy;
        a2.status = SessionStatus::Running;
        a2.agent_session_id = None;

        let mut a3 = dummy_session("tab-a3");
        a3.agent_kind = AgentKind::Agy;
        a3.status = SessionStatus::Running;
        a3.agent_session_id = None;

        let mut sessions = vec![c1, a1, a2, a3];
        let mut started_at = HashMap::new();
        for s in &sessions {
            started_at.insert(s.id.clone(), now - chrono::Duration::seconds(30));
        }
        // Ambiguous Enters on a2 and a3 at the exact same time
        let mut enters = HashMap::new();
        let enter_ms = now.timestamp_millis() - 500;
        enters.insert("tab-a2".to_string(), VecDeque::from([enter_ms]));
        enters.insert("tab-a3".to_string(), VecDeque::from([enter_ms + 20]));

        let history = vec![AgyHistoryEntry {
            timestamp_ms: enter_ms + 200,
            workspace: r"d:\workspace".to_string(),
            conversation_id: Some(other_agy_id.to_string()),
        }];

        let changed = refresh_live_sessions_with(
            &mut sessions,
            &started_at,
            &enters,
            &links_dir,
            Some(&history),
            |_| true,
        );
        let _ = std::fs::remove_dir_all(&links_dir);

        assert!(changed.is_empty(), "refresh must not overwrite or guess ambiguously");
        assert_eq!(sessions[0].agent_session_id.as_deref(), Some(claude_id));
        assert_eq!(sessions[1].agent_session_id.as_deref(), Some(agy_id));
        assert_eq!(sessions[2].agent_session_id, None);
        assert_eq!(sessions[3].agent_session_id, None);
    }

    #[test]
    fn refresh_updates_id_from_hook_link_and_preserves_previous_id() {
        let links_dir =
            std::env::temp_dir().join(format!("velox-refresh-link-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&links_dir).expect("create links dir");

        let now = Utc::now();
        let old_id = "11111111-1111-4111-8111-111111111111";
        let new_id = "44444444-4444-4444-8444-444444444444";

        let mut tab = dummy_session("tab-claude");
        tab.agent_kind = AgentKind::Claude;
        tab.status = SessionStatus::Idle;
        tab.agent_session_id = Some(old_id.to_string());

        let link = AgentLinkRecord {
            velox_session_id: "tab-claude".to_string(),
            agent: "claude".to_string(),
            agent_session_id: new_id.to_string(),
            transcript_path: None,
            cwd: Some(r"D:\Workspace".to_string()),
            source: Some("clear".to_string()),
            updated_at_ms: now.timestamp_millis(),
        };
        std::fs::write(
            links_dir.join("tab-claude.json"),
            serde_json::to_string(&link).unwrap(),
        )
        .unwrap();

        let mut sessions = vec![tab];
        let mut started_at = HashMap::new();
        started_at.insert(
            "tab-claude".to_string(),
            now - chrono::Duration::seconds(10),
        );

        let changed = refresh_live_sessions_with(
            &mut sessions,
            &started_at,
            &HashMap::new(),
            &links_dir,
            None,
            |_| false,
        );
        let _ = std::fs::remove_dir_all(&links_dir);

        assert_eq!(changed.len(), 1);
        assert_eq!(sessions[0].agent_session_id.as_deref(), Some(new_id));
        assert_eq!(
            sessions[0].previous_agent_session_ids,
            vec![old_id.to_string()]
        );
    }

    #[test]
    fn refresh_assigns_agy_from_unique_enter_and_ignores_stale_links() {
        let links_dir =
            std::env::temp_dir().join(format!("velox-refresh-agy-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&links_dir).expect("create links dir");

        let now = Utc::now();
        let agy_conv = "55555555-5555-4555-8555-555555555555";
        let stale_id = "66666666-6666-4666-8666-666666666666";

        // Stale link written long before spawn must be ignored
        let stale_link = AgentLinkRecord {
            velox_session_id: "tab-agy".to_string(),
            agent: "agy".to_string(),
            agent_session_id: stale_id.to_string(),
            transcript_path: None,
            cwd: None,
            source: Some("pre-invocation".to_string()),
            updated_at_ms: now.timestamp_millis() - 60_000,
        };
        std::fs::write(
            links_dir.join("tab-agy.json"),
            serde_json::to_string(&stale_link).unwrap(),
        )
        .unwrap();

        let mut tab = dummy_session("tab-agy");
        tab.agent_kind = AgentKind::Agy;
        tab.status = SessionStatus::Running;

        let mut sessions = vec![tab];
        let mut started_at = HashMap::new();
        started_at.insert("tab-agy".to_string(), now - chrono::Duration::seconds(5));

        let mut enters = HashMap::new();
        let enter_ms = now.timestamp_millis() - 1_000;
        enters.insert("tab-agy".to_string(), VecDeque::from([enter_ms]));

        let history = vec![AgyHistoryEntry {
            timestamp_ms: enter_ms + 300,
            workspace: r"d:\workspace\velox".to_string(),
            conversation_id: Some(agy_conv.to_string()),
        }];

        let changed = refresh_live_sessions_with(
            &mut sessions,
            &started_at,
            &enters,
            &links_dir,
            Some(&history),
            |id| id == agy_conv,
        );
        let _ = std::fs::remove_dir_all(&links_dir);

        assert_eq!(changed.len(), 1);
        assert_eq!(sessions[0].agent_session_id.as_deref(), Some(agy_conv));
    }

    #[test]
    fn startup_adopts_newer_link_and_repair_preserves_lost_duplicate() {
        let links_dir =
            std::env::temp_dir().join(format!("velox-startup-links-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&links_dir).expect("create links dir");

        let old_id = "11111111-1111-4111-8111-111111111111";
        let newer_id = "77777777-7777-4777-8777-777777777777";
        let base_time = Utc::now();

        let mut s = dummy_session("tab-1");
        s.agent_kind = AgentKind::Claude;
        s.agent_session_id = Some(old_id.to_string());
        s.last_active_at = base_time.to_rfc3339();

        let link = AgentLinkRecord {
            velox_session_id: "tab-1".to_string(),
            agent: "claude".to_string(),
            agent_session_id: newer_id.to_string(),
            transcript_path: None,
            cwd: None,
            source: Some("resume".to_string()),
            updated_at_ms: base_time.timestamp_millis() + 1_500,
        };
        std::fs::write(
            links_dir.join("tab-1.json"),
            serde_json::to_string(&link).unwrap(),
        )
        .unwrap();

        let mut sessions = vec![s];
        adopt_newer_agent_links(&mut sessions, &links_dir);
        let _ = std::fs::remove_dir_all(&links_dir);

        assert_eq!(sessions[0].agent_session_id.as_deref(), Some(newer_id));
        assert_eq!(
            sessions[0].previous_agent_session_ids,
            vec![old_id.to_string()]
        );

        // Duplicate repair preserves displaced ID in previous_agent_session_ids
        let mut d1 = dummy_session("d1");
        d1.agent_session_id = Some("dup".to_string());
        let mut d2 = dummy_session("d2");
        d2.agent_session_id = Some("dup".to_string());
        let mut dups = vec![d1, d2];
        repair_duplicate_agent_session_ids(&mut dups, |s, _| s.id == "d1", |_, _| None);
        assert_eq!(dups[0].agent_session_id.as_deref(), Some("dup"));
        assert_eq!(dups[1].agent_session_id, None);
        assert_eq!(dups[1].previous_agent_session_ids, vec!["dup".to_string()]);
    }
}
