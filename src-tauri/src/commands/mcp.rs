use crate::domain::error::VeloxResult;
use crate::domain::session::McpGatewaySnapshot;
use crate::AppState;
use tauri::{AppHandle, Emitter, State};

#[tauri::command]
pub async fn get_mcp_status(state: State<'_, AppState>) -> VeloxResult<McpGatewaySnapshot> {
    Ok(state.mcp.snapshot().await)
}

#[tauri::command]
pub async fn sleep_mcp_backend(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
) -> VeloxResult<McpGatewaySnapshot> {
    let snapshot = state.mcp.sleep_backend(&name).await?;
    let _ = app.emit("mcp-updated", snapshot.clone());
    Ok(snapshot)
}

#[tauri::command]
pub async fn sleep_all_mcps(
    app: AppHandle,
    state: State<'_, AppState>,
) -> VeloxResult<McpGatewaySnapshot> {
    let snapshot = state.mcp.sleep_all().await?;
    let _ = app.emit("mcp-updated", snapshot.clone());
    Ok(snapshot)
}
