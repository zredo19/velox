pub mod commands;
pub mod domain;
pub mod mcp;
pub mod pty;
pub mod session;
pub mod team;
pub mod telemetry;

use crate::domain::config::VeloxConfig;
use crate::mcp::gateway::McpGatewayManager;
use crate::session::manager::SessionManager;
use crate::telemetry::monitor::TelemetryMonitor;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tauri::{Emitter, RunEvent};

/// Shared application state managed by Tauri across IPC commands and background tasks.
#[derive(Clone)]
pub struct AppState {
    pub sessions: Arc<SessionManager>,
    pub mcp: McpGatewayManager,
    pub telemetry: Arc<TelemetryMonitor>,
}

impl AppState {
    pub fn new(config: VeloxConfig) -> Self {
        let mcp = McpGatewayManager::new(
            PathBuf::from(&config.mcp_backends_path),
            config.mcp_gateway_port,
        );
        let sessions = Arc::new(SessionManager::new(config));
        let telemetry = Arc::new(TelemetryMonitor::new());
        Self {
            sessions,
            mcp,
            telemetry,
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
        ])
        .setup(move |app| {
            let handle = app.handle().clone();
            let setup_state = state.clone();

            // Boot embedded Lazy MCP Gateway if enabled
            if mcp_enabled {
                let mcp_handle = handle.clone();
                let mcp_mgr = setup_state.mcp.clone();
                tauri::async_runtime::spawn(async move {
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
            tauri::async_runtime::block_on(state_for_exit.mcp.shutdown());
        }
    });
}
