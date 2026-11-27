use crate::domain::config::VeloxConfig;
use crate::domain::error::VeloxResult;
use crate::domain::session::SystemTelemetry;
use crate::AppState;
use tauri::State;

#[tauri::command]
pub async fn get_telemetry(state: State<'_, AppState>) -> VeloxResult<SystemTelemetry> {
    let mcp_snap = state.mcp.snapshot().await;
    let sessions = state.sessions.list_sessions();
    let sample = state.telemetry.sample(&sessions, mcp_snap.total_memory_mb);
    state.sessions.apply_ram_telemetry(&sample.session_ram);
    Ok(sample)
}

#[tauri::command]
pub fn get_config(state: State<'_, AppState>) -> VeloxResult<VeloxConfig> {
    Ok(state.sessions.get_config())
}

#[tauri::command]
pub async fn update_config(
    state: State<'_, AppState>,
    config: VeloxConfig,
) -> VeloxResult<VeloxConfig> {
    let team_cfg = config.team.clone();
    let updated = state.sessions.update_config(config)?;
    state.team.apply_config(team_cfg).await;
    Ok(updated)
}
