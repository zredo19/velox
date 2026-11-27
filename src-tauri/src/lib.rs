pub mod commands;
pub mod domain;
pub mod mcp;
pub mod pty;
pub mod session;
pub mod team;
pub mod telemetry;

use crate::domain::config::VeloxConfig;
use crate::mcp::gateway::McpGatewayManager;
use crate::session::manager::{SessionManager, SessionTap};
use crate::team::hub::TeamHub;
use crate::telemetry::monitor::TelemetryMonitor;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tauri::{Emitter, Manager, RunEvent};

/// Shared application state managed by Tauri across IPC commands and background tasks.
#[derive(Clone)]
pub struct AppState {
    pub sessions: Arc<SessionManager>,
    pub mcp: McpGatewayManager,
    pub telemetry: Arc<TelemetryMonitor>,
    pub team: Arc<TeamHub>,
}

impl AppState {
    pub fn new(config: VeloxConfig) -> Self {
        let team_config = config.team.clone();
        let mcp = McpGatewayManager::new(
            PathBuf::from(&config.mcp_backends_path),
            config.mcp_gateway_port,
        );
        let sessions = Arc::new(SessionManager::new(config));
        let telemetry = Arc::new(TelemetryMonitor::new());
        let team = TeamHub::new(Arc::clone(&sessions), team_config);
        Self {
            sessions,
            mcp,
            telemetry,
            team,
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("velox=info")),
        )
        .try_init();

    let config = VeloxConfig::load_or_default();
    let mcp_enabled = config.mcp_gateway_enabled;
    let mcp_port = config.mcp_gateway_port;
    let state = AppState::new(config);
    let state_for_exit = state.clone();

    let builder = tauri::Builder::default()
        .manage(state.clone())
        .invoke_handler(tauri::generate_handler![
            commands::sessions::list_sessions,
            commands::sessions::create_session,
            commands::sessions::wake_session,
            commands::sessions::sleep_session,
            commands::sessions::sleep_idle_sessions,
            commands::sessions::close_session,
            commands::sessions::rename_session,
            commands::sessions::reorder_sessions,
            commands::sessions::set_session_shared,
            commands::sessions::write_pty,
            commands::sessions::resize_pty,
            commands::sessions::get_scrollback,
            commands::paste::paste_image,
            commands::paste::paste_files,
            commands::agents::list_agent_presets,
            commands::agents::list_recent_dirs,
            commands::agents::pick_directory,
            commands::mcp::get_mcp_status,
            commands::mcp::sleep_mcp_backend,
            commands::mcp::sleep_all_mcps,
            commands::system::get_telemetry,
            commands::system::get_config,
            commands::system::update_config,
            commands::team::get_team_status,
            commands::team::team_subscribe,
            commands::team::team_unsubscribe,
            commands::team::send_team_task,
            commands::team::list_team_tasks,
            commands::team::respond_team_task,
            commands::team::generate_team_token,
        ])
        .setup(move |app| {
            let handle = app.handle().clone();
            let setup_state = state.clone();

            let tap: Arc<dyn SessionTap> = setup_state.team.clone();
            setup_state.sessions.set_tap(tap);
            setup_state.sessions.set_app_handle(handle.clone());
            setup_state.team.start(handle.clone());

            let bootstrap_port = if mcp_enabled { mcp_port } else { 0 };
            tauri::async_runtime::spawn_blocking(move || {
                session::bootstrap::bootstrap_local_messaging(bootstrap_port);
            });

            #[cfg(windows)]
            if let Some(main_win) = app.get_webview_window("main") {
                install_windows_shutdown_hook(&main_win, Arc::clone(&setup_state.sessions));
            }

            // Boot embedded Lazy MCP Gateway if enabled
            if mcp_enabled {
                let mcp_handle = handle.clone();
                let mcp_mgr = setup_state.mcp.clone();
                let team_hub = setup_state.team.clone();
                tauri::async_runtime::spawn(async move {
                    mcp_mgr
                        .register_builtin(
                            team::mcp::TEAM_MCP_NAME,
                            Arc::new(team::mcp::TeamMcp::new(team_hub)),
                        )
                        .await;
                    match mcp_mgr.start().await {
                        Ok(snapshot) => {
                            let _ = mcp_handle.emit("mcp-updated", snapshot);
                        }
                        Err(err) => {
                            tracing::warn!(%err, "Lazy MCP Gateway failed to start");
                        }
                    }
                });
            }

            // Periodic lightweight RAM + MCP telemetry emitter (every 4s) & auto-hibernation (every 60s)
            let tick_handle = handle.clone();
            let tick_state = setup_state;
            tauri::async_runtime::spawn(async move {
                let mut ticker = tokio::time::interval(Duration::from_secs(4));
                let mut ticks: u64 = 0;
                // Last payloads sent: unchanged snapshots are not re-emitted, since every event
                // re-renders the React tree in the WebView.
                // (Compared as serde_json::Value, whose maps are sorted, unlike HashMap.)
                let mut last_telemetry = serde_json::Value::Null;
                let mut last_mcp = serde_json::Value::Null;
                loop {
                    ticker.tick().await;
                    ticks = ticks.wrapping_add(1);

                    let mcp_snap = tick_state.mcp.snapshot().await;
                    let sessions = tick_state.sessions.list_sessions();
                    let sample = tick_state
                        .telemetry
                        .sample_with_agents(&sessions, mcp_snap.total_memory_mb);
                    tick_state.sessions.apply_telemetry(
                        &tick_handle,
                        &sample.telemetry.session_ram,
                        &sample.active_agents,
                    );

                    if let Ok(json) = serde_json::to_value(&sample.telemetry) {
                        if json != last_telemetry {
                            let _ = tick_handle.emit("telemetry-updated", sample.telemetry);
                            last_telemetry = json;
                        }
                    }
                    if let Ok(json) = serde_json::to_value(&mcp_snap) {
                        if json != last_mcp {
                            let _ = tick_handle.emit("mcp-updated", mcp_snap);
                            last_mcp = json;
                        }
                    }

                    // Keep persisted conversation IDs fresh (~every 20s) so a crash or forced
                    // close still resumes the right conversation in every tab.
                    if ticks % 5 == 0 {
                        tick_state.sessions.refresh_agent_session_ids(&tick_handle);
                    }

                    if ticks % 15 == 0 {
                        tick_state.sessions.hibernate_stale_sessions(&tick_handle);
                    }
                }
            });

            Ok(())
        });

    let app = match builder.build(tauri::generate_context!()) {
        Ok(app) => app,
        Err(err) => {
            tracing::error!(%err, "Failed to build Velox Tauri application");
            return;
        }
    };

    app.run(move |_app_handle, event| {
        if matches!(event, RunEvent::Exit | RunEvent::ExitRequested { .. }) {
            state_for_exit.sessions.shutdown();
            tauri::async_runtime::block_on(async {
                state_for_exit.team.shutdown().await;
                state_for_exit.mcp.shutdown().await;
            });
        }
    });
}

#[cfg(windows)]
fn install_windows_shutdown_hook(win: &tauri::WebviewWindow, sessions: Arc<SessionManager>) {
    use std::sync::atomic::{AtomicIsize, Ordering};
    use std::sync::OnceLock;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        CallWindowProcW, DefWindowProcW, SetWindowLongPtrW, GWLP_WNDPROC, WM_ENDSESSION,
        WM_QUERYENDSESSION, WNDPROC,
    };

    static SESSIONS: OnceLock<Arc<SessionManager>> = OnceLock::new();
    static PREV_WNDPROC: AtomicIsize = AtomicIsize::new(0);

    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if msg == WM_QUERYENDSESSION {
            if let Some(mgr) = SESSIONS.get() {
                mgr.persist_snapshot();
            }
        } else if msg == WM_ENDSESSION && wparam.0 != 0 {
            if let Some(mgr) = SESSIONS.get() {
                mgr.shutdown();
            }
        }
        let prev = PREV_WNDPROC.load(Ordering::SeqCst);
        if prev != 0 {
            let prev_fn: WNDPROC = Some(std::mem::transmute::<
                isize,
                unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT,
            >(prev));
            CallWindowProcW(prev_fn, hwnd, msg, wparam, lparam)
        } else {
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
    }

    let Ok(raw_hwnd) = win.hwnd() else {
        return;
    };
    let hwnd = HWND(raw_hwnd.0);
    let _ = SESSIONS.set(sessions);
    // Only install once; compare_exchange prevents overwriting PREV_WNDPROC on re-entry.
    if PREV_WNDPROC.load(Ordering::SeqCst) == 0 {
        unsafe {
            let prev = SetWindowLongPtrW(hwnd, GWLP_WNDPROC, wnd_proc as *const () as isize);
            if prev != 0 {
                PREV_WNDPROC.store(prev, Ordering::SeqCst);
            }
        }
    }
}
