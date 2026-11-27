//! Tauri IPC commands for Velox Online (team sharing, mirrors, and task requests).

use crate::domain::error::VeloxResult;
use crate::team::api::{SendTaskRequest, TaskTicket, TeamApi, TeamSnapshot};
use crate::AppState;
use tauri::State;
use uuid::Uuid;

#[tauri::command]
pub fn get_team_status(state: State<'_, AppState>) -> VeloxResult<TeamSnapshot> {
    Ok(state.team.snapshot())
}

#[tauri::command]
pub async fn team_subscribe(
    state: State<'_, AppState>,
    peer: String,
    session_id: String,
) -> VeloxResult<()> {
    state.team.subscribe(&peer, &session_id).await
}

#[tauri::command]
pub async fn team_unsubscribe(
    state: State<'_, AppState>,
    peer: String,
    session_id: String,
) -> VeloxResult<()> {
    state.team.unsubscribe(&peer, &session_id).await
}

#[tauri::command]
pub async fn send_team_task(
    state: State<'_, AppState>,
    peer: Option<String>,
    session_id: String,
    text: String,
) -> VeloxResult<TaskTicket> {
    state
        .team
        .send_task(SendTaskRequest {
            peer,
            session_id,
            text,
            from_session: None,
        })
        .await
}

#[tauri::command]
pub fn list_team_tasks(state: State<'_, AppState>) -> VeloxResult<Vec<TaskTicket>> {
    Ok(state.team.list_tasks())
}

#[tauri::command]
pub async fn respond_team_task(
    state: State<'_, AppState>,
    request_id: String,
    accept: bool,
) -> VeloxResult<TaskTicket> {
    state.team.respond_task(&request_id, accept).await
}

#[tauri::command]
pub fn generate_team_token() -> VeloxResult<String> {
    Ok(format!(
        "{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_team_token_returns_64_lowercase_hex_chars() {
        let tok = generate_team_token().expect("token");
        assert_eq!(tok.len(), 64);
        assert!(tok
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)));
    }
}
