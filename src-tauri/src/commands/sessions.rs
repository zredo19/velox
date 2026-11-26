use crate::domain::error::VeloxResult;
use crate::domain::session::SessionRecord;
use crate::AppState;
use tauri::{AppHandle, State};

// Lifecycle commands are `async` so Tauri runs them on its worker pool instead of the main
// thread: a slow disk write or process spawn there froze the whole window.
// `write_pty`/`resize_pty` stay on the main thread, which keeps keystrokes strictly ordered.

#[tauri::command(async)]
pub fn list_sessions(state: State<'_, AppState>) -> VeloxResult<Vec<SessionRecord>> {
    Ok(state.sessions.list_sessions())
}

#[tauri::command(async)]
pub fn create_session(
    app: AppHandle,
    state: State<'_, AppState>,
    preset_id: String,
    cwd: Option<String>,
    title: Option<String>,
    cols: u16,
    rows: u16,
) -> VeloxResult<SessionRecord> {
    state
        .sessions
        .create_session(&app, &preset_id, cwd, title, cols, rows)
}

#[tauri::command(async)]
pub fn wake_session(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    cols: u16,
    rows: u16,
) -> VeloxResult<SessionRecord> {
    state.sessions.wake_session(&app, &session_id, cols, rows)
}

#[tauri::command(async)]
pub fn sleep_session(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> VeloxResult<SessionRecord> {
    state.sessions.sleep_session(&app, &session_id)
}

#[tauri::command(async)]
pub fn sleep_idle_sessions(
    app: AppHandle,
    state: State<'_, AppState>,
    exclude_session_id: Option<String>,
) -> VeloxResult<Vec<SessionRecord>> {
    state
        .sessions
        .sleep_idle_sessions(&app, exclude_session_id.as_deref())
}

#[tauri::command(async)]
pub fn close_session(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> VeloxResult<()> {
    state.sessions.close_session(&app, &session_id)
}

#[tauri::command(async)]
pub fn rename_session(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    title: String,
) -> VeloxResult<SessionRecord> {
    state.sessions.rename_session(&app, &session_id, title)
}

#[tauri::command(async)]
pub fn reorder_sessions(
    state: State<'_, AppState>,
    session_ids: Vec<String>,
) -> VeloxResult<Vec<SessionRecord>> {
    Ok(state.sessions.reorder_sessions(&session_ids))
}

#[tauri::command]
pub fn write_pty(state: State<'_, AppState>, session_id: String, data: String) -> VeloxResult<()> {
    state.sessions.write_pty(&session_id, &data)
}

#[tauri::command]
pub fn resize_pty(
    state: State<'_, AppState>,
    session_id: String,
    cols: u16,
    rows: u16,
) -> VeloxResult<()> {
    state.sessions.resize_pty(&session_id, cols, rows)
}

#[tauri::command(async)]
pub fn get_scrollback(
    state: State<'_, AppState>,
    session_id: String,
) -> VeloxResult<crate::domain::session::ScrollbackSnapshot> {
    Ok(state.sessions.get_scrollback_snapshot(&session_id))
}
