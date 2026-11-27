use crate::domain::config::velox_config_dir;
use crate::domain::error::{VeloxError, VeloxResult};
use crate::domain::session::{SessionRecord, SessionStatus};
use chrono::Utc;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

/// Path to persisted Velox sessions (`%APPDATA%/velox/sessions.json`).
pub fn sessions_file_path() -> PathBuf {
    velox_config_dir().join("sessions.json")
}

/// Path to the last known good backup (`%APPDATA%/velox/sessions.json.bak`).
pub fn sessions_backup_path(path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.bak", path.display()))
}

fn normalize_restored_sessions(mut sessions: Vec<SessionRecord>) -> Vec<SessionRecord> {
    for session in &mut sessions {
        if session.active_agent == Some(crate::domain::agent::AgentKind::Shell) {
            session.wake_as_shell = true;
        } else if session.active_agent.is_some() {
            session.wake_as_shell = false;
        }
        session.active_agent = None;
        session.status = SessionStatus::Sleeping;
        session.pid = None;
        session.memory_mb = 0.0;
        session.exit_code = None;
    }
    sessions
}

fn write_synced_atomic(path: &Path, bytes: &[u8]) -> VeloxResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = PathBuf::from(format!("{}.tmp", path.display()));
    {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn unique_corrupt_path(path: &Path) -> PathBuf {
    let stamp = Utc::now().format("%Y%m%d-%H%M%S").to_string();
    let candidate = PathBuf::from(format!("{}.corrupt-{stamp}", path.display()));
    if !candidate.exists() {
        return candidate;
    }
    for n in 1..=999 {
        let numbered = PathBuf::from(format!("{}.corrupt-{stamp}-{n}", path.display()));
        if !numbered.exists() {
            return numbered;
        }
    }
    PathBuf::from(format!(
        "{}.corrupt-{stamp}-{}",
        path.display(),
        uuid::Uuid::new_v4()
    ))
}

fn archive_corrupt_file(path: &Path) -> Option<PathBuf> {
    let target = unique_corrupt_path(path);
    match std::fs::rename(path, &target) {
        Ok(()) => Some(target),
        Err(err) => {
            tracing::error!(
                path = %path.display(),
                target = %target.display(),
                %err,
                "Failed to move corrupt sessions file aside"
            );
            None
        }
    }
}

enum ReadParseOutcome {
    Parsed(Vec<SessionRecord>),
    Corrupt(String),
    IoFailure(std::io::Error),
}

fn try_read_sessions_file(path: &Path) -> ReadParseOutcome {
    match std::fs::read_to_string(path) {
        Ok(raw) => match serde_json::from_str::<Vec<SessionRecord>>(&raw) {
            Ok(sessions) => ReadParseOutcome::Parsed(sessions),
            Err(err) => ReadParseOutcome::Corrupt(err.to_string()),
        },
        Err(err) if err.kind() == ErrorKind::InvalidData => {
            ReadParseOutcome::Corrupt(err.to_string())
        }
        Err(err) => ReadParseOutcome::IoFailure(err),
    }
}

/// Load persisted sessions from the default path with Lazy-Restore enforced (0 MB RAM on startup).
pub fn load_sessions() -> VeloxResult<Vec<SessionRecord>> {
    load_sessions_from(&sessions_file_path())
}

/// Load persisted sessions from an explicit path, forcing every restored entry into Sleeping (0 MB).
///
/// If the main file is corrupt, moves it to `<path>.corrupt-<timestamp>` and recovers from
/// `<path>.bak` when available. Returns `Err` if existing files cannot be recovered, so callers
/// never overwrite persisted state with an empty list on read failure.
pub fn load_sessions_from(path: &Path) -> VeloxResult<Vec<SessionRecord>> {
    let bak = sessions_backup_path(path);
    let primary_present = path.exists();
    let mut primary_transient_io_err: Option<std::io::Error> = None;
    let mut primary_corrupt_reason: Option<String> = None;

    if primary_present {
        match try_read_sessions_file(path) {
            ReadParseOutcome::Parsed(sessions) => {
                return Ok(normalize_restored_sessions(sessions));
            }
            ReadParseOutcome::Corrupt(reason) => {
                let moved = archive_corrupt_file(path);
                tracing::error!(
                    path = %path.display(),
                    archived_to = ?moved,
                    reason = %reason,
                    "sessions.json failed to parse; attempting .bak recovery"
                );
                primary_corrupt_reason = Some(reason);
            }
            ReadParseOutcome::IoFailure(err) => {
                tracing::warn!(
                    path = %path.display(),
                    %err,
                    "Failed to read sessions.json; attempting .bak fallback without modifying file"
                );
                primary_transient_io_err = Some(err);
            }
        }
    }

    if bak.is_file() {
        match try_read_sessions_file(&bak) {
            ReadParseOutcome::Parsed(sessions) => {
                tracing::warn!(
                    bak = %bak.display(),
                    count = sessions.len(),
                    "Recovered sessions from backup file"
                );
                // Restore main file only when it was absent or corrupt (moved aside), never when
                // a transient IO lock prevented reading it.
                if primary_transient_io_err.is_none() {
                    if let Ok(bytes) = serde_json::to_vec_pretty(&sessions) {
                        if let Err(err) = write_synced_atomic(path, &bytes) {
                            tracing::warn!(
                                path = %path.display(),
                                %err,
                                "Recovered from .bak in memory, but could not rewrite primary file"
                            );
                        }
                    }
                }
                return Ok(normalize_restored_sessions(sessions));
            }
            ReadParseOutcome::Corrupt(reason) => {
                let moved = archive_corrupt_file(&bak);
                tracing::error!(
                    bak = %bak.display(),
                    archived_to = ?moved,
                    reason = %reason,
                    "sessions.json.bak is also corrupt"
                );
                return Err(VeloxError::Config(format!(
                    "Both sessions file and backup are corrupt ({reason})"
                )));
            }
            ReadParseOutcome::IoFailure(err) => {
                return Err(VeloxError::Io(err));
            }
        }
    }

    if let Some(err) = primary_transient_io_err {
        return Err(VeloxError::Io(err));
    }
    if let Some(reason) = primary_corrupt_reason {
        return Err(VeloxError::Config(format!(
            "sessions.json is corrupt and no backup exists ({reason})"
        )));
    }

    Ok(Vec::new())
}

/// Atomically persist sessions to the default path.
pub fn save_sessions(sessions: &[SessionRecord]) -> VeloxResult<()> {
    save_sessions_to(&sessions_file_path(), sessions)
}

/// Atomically persist sessions to an explicit path via `sync_all` + rename, keeping
/// `<path>.bak` updated with the last known good snapshot.
pub fn save_sessions_to(path: &Path, sessions: &[SessionRecord]) -> VeloxResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let content = serde_json::to_vec_pretty(sessions)?;
    let bak = sessions_backup_path(path);

    // Keep `.bak` pointing at the previous valid on-disk snapshot before replacing `path`.
    if path.is_file() {
        if let Ok(existing_raw) = std::fs::read(path) {
            if serde_json::from_slice::<Vec<SessionRecord>>(&existing_raw).is_ok() {
                if let Err(err) = write_synced_atomic(&bak, &existing_raw) {
                    tracing::warn!(bak = %bak.display(), %err, "Failed to update sessions.json.bak");
                }
            }
        }
    } else if !bak.is_file() && !sessions.is_empty() {
        if let Err(err) = write_synced_atomic(&bak, &content) {
            tracing::warn!(bak = %bak.display(), %err, "Failed to seed initial sessions.json.bak");
        }
    }

    write_synced_atomic(path, &content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::agent::AgentKind;

    fn sample_session(id: &str, agent_kind: AgentKind, agent_session_id: Option<&str>) -> SessionRecord {
        SessionRecord {
            id: id.to_string(),
            title: id.to_string(),
            agent_kind,
            active_agent: Some(agent_kind),
            preset_id: match agent_kind {
                AgentKind::Claude => "claude-personal".to_string(),
                AgentKind::Agy => "agy".to_string(),
                _ => "shell".to_string(),
            },
            profile: (agent_kind == AgentKind::Claude).then(|| "personal".to_string()),
            cwd: ".".to_string(),
            status: SessionStatus::Running,
            agent_session_id: agent_session_id.map(ToOwned::to_owned),
            previous_agent_session_ids: Vec::new(),
            wake_as_shell: false,
            pid: Some(4242),
            memory_mb: 312.5,
            created_at: "2026-11-24T00:00:00Z".to_string(),
            last_active_at: "2026-11-24T00:00:00Z".to_string(),
            exit_code: Some(0),
            shared: false,
        }
    }

    #[test]
    fn lazy_restore_forces_all_loaded_sessions_to_sleeping_zero_mb() {
        let dir = std::env::temp_dir().join(format!("velox-store-test-{}", uuid::Uuid::new_v4()));
        let file = dir.join("sessions.json");

        let sessions = vec![
            sample_session("s1", AgentKind::Claude, Some("11111111-2222-3333-4444-555555555555")),
            SessionRecord {
                status: SessionStatus::Exited,
                active_agent: None,
                ..sample_session("s2", AgentKind::Agy, None)
            },
        ];

        save_sessions_to(&file, &sessions).expect("save should succeed");
        let loaded = load_sessions_from(&file).expect("load should succeed");
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(loaded.len(), 2);
        for s in &loaded {
            assert_eq!(s.status, SessionStatus::Sleeping);
            assert_eq!(s.pid, None);
            assert_eq!(s.memory_mb, 0.0);
            assert_eq!(s.exit_code, None);
        }
        assert_eq!(
            loaded[0].agent_session_id.as_deref(),
            Some("11111111-2222-3333-4444-555555555555")
        );
    }

    #[test]
    fn corrupt_main_json_recovers_from_backup_and_archives_corrupt_file() {
        let dir = std::env::temp_dir().join(format!("velox-store-corrupt-{}", uuid::Uuid::new_v4()));
        let file = dir.join("sessions.json");
        let bak = sessions_backup_path(&file);

        let initial = vec![sample_session(
            "tab-1",
            AgentKind::Claude,
            Some("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"),
        )];
        save_sessions_to(&file, &initial).expect("initial save");
        assert!(bak.is_file(), "backup should be created on save");

        // Second save ensures .bak holds the previous valid snapshot and main holds updated snapshot.
        let updated = vec![
            sample_session(
                "tab-1",
                AgentKind::Claude,
                Some("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"),
            ),
            sample_session(
                "tab-2",
                AgentKind::Agy,
                Some("11111111-2222-3333-4444-555555555555"),
            ),
        ];
        save_sessions_to(&file, &updated).expect("second save");
        // Write once more so .bak also has both sessions (simulating steady-state before outage).
        save_sessions_to(&file, &updated).expect("third save");

        // Simulate power cut mid-write corrupting sessions.json (truncated + NUL bytes).
        std::fs::write(&file, b"[{\"id\":\"tab-1\",\"broken\0\0").expect("corrupt main file");

        let recovered = load_sessions_from(&file).expect("should recover from .bak");
        assert_eq!(recovered.len(), 2);
        assert_eq!(recovered[0].id, "tab-1");
        assert_eq!(recovered[1].id, "tab-2");
        assert_eq!(
            recovered[1].agent_session_id.as_deref(),
            Some("11111111-2222-3333-4444-555555555555")
        );

        let corrupt_files: Vec<_> = std::fs::read_dir(&dir)
            .expect("read dir")
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("sessions.json.corrupt-")
            })
            .collect();
        assert_eq!(corrupt_files.len(), 1, "corrupt file should be renamed aside");
        assert!(file.is_file(), "primary file should be restored from .bak");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_main_without_backup_returns_error_instead_of_empty_list() {
        let dir = std::env::temp_dir().join(format!("velox-store-nobak-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let file = dir.join("sessions.json");
        std::fs::write(&file, "{not valid json").expect("write corrupt");

        let result = load_sessions_from(&file);
        assert!(
            result.is_err(),
            "corrupt sessions.json with no .bak must return Err rather than silent empty list"
        );
        let has_corrupt_archive = std::fs::read_dir(&dir)
            .expect("read dir")
            .flatten()
            .any(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("sessions.json.corrupt-")
            });
        assert!(has_corrupt_archive);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
