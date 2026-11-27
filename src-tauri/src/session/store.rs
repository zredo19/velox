use crate::domain::config::velox_config_dir;
use crate::domain::error::VeloxResult;
use crate::domain::session::{SessionRecord, SessionStatus};
use std::path::{Path, PathBuf};

/// Path to persisted Velox sessions (`%APPDATA%/velox/sessions.json`).
pub fn sessions_file_path() -> PathBuf {
    velox_config_dir().join("sessions.json")
}

/// Load persisted sessions from the default path with Lazy-Restore enforced (0 MB RAM on startup).
pub fn load_sessions() -> VeloxResult<Vec<SessionRecord>> {
    load_sessions_from(&sessions_file_path())
}

/// Load persisted sessions from an explicit path, forcing every restored entry into Sleeping (0 MB).
pub fn load_sessions_from(path: &Path) -> VeloxResult<Vec<SessionRecord>> {
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let raw = std::fs::read_to_string(path)?;
    let mut sessions: Vec<SessionRecord> = serde_json::from_str(&raw)?;
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
    Ok(sessions)
}

/// Atomically persist sessions to the default path.
pub fn save_sessions(sessions: &[SessionRecord]) -> VeloxResult<()> {
    save_sessions_to(&sessions_file_path(), sessions)
}

/// Atomically persist sessions to an explicit path via temp-file + rename.
pub fn save_sessions_to(path: &Path, sessions: &[SessionRecord]) -> VeloxResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    let content = serde_json::to_string_pretty(sessions)?;
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::agent::AgentKind;

    #[test]
    fn lazy_restore_forces_all_loaded_sessions_to_sleeping_zero_mb() {
        let dir = std::env::temp_dir().join(format!("velox-store-test-{}", uuid::Uuid::new_v4()));
        let file = dir.join("sessions.json");

        let sessions = vec![
            SessionRecord {
                id: "s1".to_string(),
                title: "Velox".to_string(),
                agent_kind: AgentKind::Claude,
                active_agent: Some(AgentKind::Claude),
                preset_id: "claude-personal".to_string(),
                profile: Some("personal".to_string()),
                cwd: ".".to_string(),
                status: SessionStatus::Running,
                agent_session_id: Some("11111111-2222-3333-4444-555555555555".to_string()),
                wake_as_shell: false,
                pid: Some(4242),
                memory_mb: 312.5,
                created_at: "2026-11-24T00:00:00Z".to_string(),
                last_active_at: "2026-11-24T00:00:00Z".to_string(),
                exit_code: Some(0),
                shared: false,
            },
            SessionRecord {
                id: "s2".to_string(),
                title: "API".to_string(),
                agent_kind: AgentKind::Agy,
                active_agent: None,
                preset_id: "agy".to_string(),
                profile: None,
                cwd: ".".to_string(),
                status: SessionStatus::Exited,
                agent_session_id: None,
                wake_as_shell: false,
                pid: Some(9001),
                memory_mb: 180.0,
                created_at: "2026-11-24T00:00:00Z".to_string(),
                last_active_at: "2026-11-24T00:00:00Z".to_string(),
                exit_code: Some(1),
                shared: false,
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
}
