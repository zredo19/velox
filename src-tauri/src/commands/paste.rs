use crate::domain::agent::AgentKind;
use crate::domain::error::{VeloxError, VeloxResult};
use crate::session::paste::{self, SshTarget};
use crate::AppState;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use std::path::Path;
use tauri::State;

fn effective_agent_kind(agent_kind: AgentKind, active_agent: Option<AgentKind>) -> AgentKind {
    active_agent.unwrap_or(agent_kind)
}

/// Only shell sessions are inspected: an agent that happens to be running `ssh`
/// as a tool still lives on this machine and must receive local paths.
async fn ssh_target(
    state: &State<'_, AppState>,
    session_id: &str,
) -> VeloxResult<Option<SshTarget>> {
    let session = state
        .sessions
        .list_sessions()
        .into_iter()
        .find(|s| s.id == session_id)
        .ok_or_else(|| VeloxError::SessionNotFound(session_id.to_string()))?;
    let effective_kind = effective_agent_kind(session.agent_kind, session.active_agent);
    let (AgentKind::Shell, Some(pid)) = (effective_kind, session.pid) else {
        return Ok(None);
    };
    tauri::async_runtime::spawn_blocking(move || paste::find_ssh_target(pid))
        .await
        .map_err(|e| VeloxError::Pty(e.to_string()))
}

fn too_large() -> VeloxError {
    VeloxError::InvalidInput("image is larger than 20 MB".into())
}

/// Returns the text to paste for a clipboard image.
#[tauri::command]
pub async fn paste_image(
    state: State<'_, AppState>,
    session_id: String,
    extension: String,
    data_b64: String,
) -> VeloxResult<String> {
    let data = BASE64
        .decode(data_b64)
        .map_err(|e| VeloxError::InvalidInput(e.to_string()))?;
    if data.len() as u64 > paste::MAX_IMAGE_BYTES {
        return Err(too_large());
    }
    match ssh_target(&state, &session_id).await? {
        Some(target) => paste::upload(&target, &extension, &data).await,
        None => {
            let path = paste::stage(&extension, &data)?;
            Ok(paste::quote_local(&path.to_string_lossy()))
        }
    }
}

/// Returns the text to paste for files dropped on the terminal.
#[tauri::command]
pub async fn paste_files(
    state: State<'_, AppState>,
    session_id: String,
    paths: Vec<String>,
) -> VeloxResult<String> {
    let remote = ssh_target(&state, &session_id).await?;
    let mut pasted = Vec::with_capacity(paths.len());
    for path in paths {
        let file = Path::new(&path);
        match &remote {
            // Images are copied over; any other file keeps its local path.
            Some(target) if paste::is_image(file) => {
                if std::fs::metadata(file)?.len() > paste::MAX_IMAGE_BYTES {
                    return Err(too_large());
                }
                let extension = file.extension().and_then(|e| e.to_str()).unwrap_or("png");
                pasted.push(paste::upload(target, extension, &std::fs::read(file)?).await?);
            }
            _ => pasted.push(paste::quote_local(&path)),
        }
    }
    Ok(pasted.join(" "))
}

#[cfg(test)]
mod tests {
    use super::effective_agent_kind;
    use crate::domain::agent::AgentKind;

    #[test]
    fn effective_agent_kind_uses_active_agent_when_present() {
        // Agent dropped back to PowerShell -> treated as Shell so ssh paste uploads
        assert_eq!(
            effective_agent_kind(AgentKind::Agy, Some(AgentKind::Shell)),
            AgentKind::Shell
        );
        // Shell session currently running Claude -> treated as Claude so local path is kept
        assert_eq!(
            effective_agent_kind(AgentKind::Shell, Some(AgentKind::Claude)),
            AgentKind::Claude
        );
        // Unknown active_agent -> falls back to preset agent_kind
        assert_eq!(
            effective_agent_kind(AgentKind::Claude, None),
            AgentKind::Claude
        );
    }
}
