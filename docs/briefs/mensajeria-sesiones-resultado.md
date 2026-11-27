# Resultado: mensajería directa entre sesiones de Velox

Rama: `feat/session-messaging` en el worktree `D:\Workspace\velox-wt\msg` (basada en `feat/velox-online`).  
No cerré `velox.exe`, no compilé en `src-tauri\target` (compilación exclusiva en `D:\Workspace\velox\src-tauri\target-lanes`) y no hice `git push`.

## Qué hice por punto del encargo

### 1. Identidad de cada sesión y entorno PTY
- **`src-tauri/src/domain/agent.rs`**: añadí `SessionLaunchContext`, `prepend_path_env`, `claude_session_system_prompt` y `resolve_launch_spec_with_context` (manteniendo `resolve_launch_spec` como wrapper compatible). Inyecta en el env del PTY:
  - `VELOX_SESSION_ID` = `SessionRecord.id`
  - `VELOX_SESSION_TITLE` = título inicial / al despertar (`record.title` / `existing.title`)
  - `VELOX_GATEWAY_URL` = `http://127.0.0.1:<mcpGatewayPort>` (cuando el gateway está habilitado y el puerto > 0)
  - `%LOCALAPPDATA%\velox\bin` antepuesto en `PATH` sin duplicados (eliminando clave `Path` previa para evitar colisiones case-sensitive).
- **`src-tauri/src/session/manager.rs`**:
  - `create_session` y `wake_session` construyen el `AgentSpawnSpec` vía `build_launch_spec`, pasando el contexto de sesión y agregando `--append-system-prompt` para `AgentKind::Claude`.
  - Añadí `app_handle: RwLock<Option<AppHandle>>`, `set_app_handle` y `wake_session_headless` para que `TeamHub` pueda despertar sesiones `Sleeping` sin pasar un `AppHandle` desde IPC.
- **`src-tauri/src/lib.rs`**: registra `setup_state.sessions.set_app_handle(handle.clone())` y dispara en `setup` vía `spawn_blocking` la provisión inicial `session::bootstrap::bootstrap_local_messaging(bootstrap_port)`.

### 2. Envío directo local en backend + detección de estado + UI
- **`src-tauri/src/team/api.rs`**:
  - Añadí `EVT_TEAM_LOCAL_MESSAGE` (`"team-local-message"`), `LocalMessageStatus` (`delivered` / `queued` / `failed`), `LocalMessageEvent`, `SendLocalMessageRequest`, `LocalMessageReceipt`, y los métodos `TeamApi::all_local_sessions` y `TeamApi::send_local_message`.
  - Extraje el helper compartido `resolve_session_by_query` (ID exacto → título exacto → prefijo único, case-insensitive) para que `TeamHub` y `TeamMcp` usen exactamente las mismas reglas sin duplicar lógica.
- **`src-tauri/src/session/detector.rs`**:
  - Porté la detección de actividad por pantalla de `src/lib/agentActivity.ts` sobre `vt100::Parser` + lectura del último título OSC (`ESC ] 0;` / `ESC ] 2;`): `AgentActivity`, `last_osc_title`, `render_screen_lines`, `detect_activity_from_lines`, `detect_screen_activity` e `is_agent_ready_for_input`.
- **`src-tauri/src/team/hub.rs`**:
  - Extendí `LocalSessions` con `wake_session` (implementado en `SessionManager::wake_session_headless`).
  - `TeamHub::send_local_message` valida texto (`1..=16384` bytes), prohíbe auto-envío (`sender_id == target.id`), ignora sesiones `Exited`, resuelve destino entre todas las sesiones locales no finalizadas (vivas + `Sleeping`), formatea el prompt exacto:
    ```
    [Velox · mensaje de "<título origen>" (<id corto>)]
    <texto>

    (Para responder: velox msg "<título origen>" "<tu respuesta>")
    ```
    y entrega vía `team::delivery::build_task_input` + `CHUNK_DELAY_MS` sin crear entradas en la bandeja (`list_tasks()` queda intacto y los tasks remotos siguen pidiendo aprobación).
  - Si el destino está en `Sleeping`, lo despierta con `wake_session`, encola en una cola FIFO por sesión (`local_queues`) y espera hasta que `is_agent_ready_for_input` indique que el TUI está montado y no `Blocked` (poll cada 200 ms, timeout de cola 120 s). Si ya estaba bloqueado o arrancando, también encola hasta desbloquearse.
  - Emite eventos `team-local-message` (`queued` → `delivered` / `failed`) para el indicador de tráfico.
- **UI del sidebar**:
  - `src/types/ipc.ts`, `src/lib/tauri.ts`, `src/hooks/useTeam.ts`, `src/components/Sidebar.tsx`, `src/App.tsx`: escucha `team-local-message` y muestra un indicador/toast encima del pie del sidebar (`fromTitle → toTitle`, badge `waking`/`queued`/`failed`, click para enfocar destino, botón cerrar y auto-dismiss tras 5,5 s).

### 3. Superficies para agentes y auto-registro
- **`src-tauri/src/mcp/builtin.rs` y `src-tauri/src/mcp/gateway.rs`**:
  - Agregado `BuiltinMcp::handle_jsonrpc_with_session`; `/mcp/{name}` extrae `X-Velox-Session` (descartando vacíos o placeholders `${...}` sin expandir).
- **`src-tauri/src/team/mcp.rs`**:
  - Añadidas las tools `session_list` (marca `[current session]` e `isSelf`) y `session_send` (`to`, `text`, `from_session` opcional; el header `X-Velox-Session` tiene prioridad sobre `from_session`).
  - `team_send_task` usa el título de la sesión del header `X-Velox-Session` como `from_session` por defecto si se omite.
  - Reutilizada `team_read_session` (local/remoto).
- **`src-tauri/src/session/bootstrap.rs` y `scripts/register-velox-team-mcp.ps1`**:
  - Instala idempotente en `%LOCALAPPDATA%\velox\bin`: `velox.ps1` (envía JSON-RPC UTF-8 explícito con `WebClient.UploadData` y header `X-Velox-Session`, expone `velox list`, `velox msg`, `velox read`), `velox.cmd` (cmd/PowerShell) y el shim POSIX `velox` (para Git Bash).
  - Registra/repara `velox-team` en `~/.claude.json`, perfiles `~/.claude-*`, `~/.gemini/config/mcp_config.json`, `~/.gemini/antigravity-cli/mcp_config.json` y perfiles de AGY.
  - Mantiene el bloque `<!-- velox:begin --> ... <!-- velox:end -->` al principio de `~/.gemini/GEMINI.md` (y `~/.codex/AGENTS.md`, `~/.config/opencode/AGENTS.md` si existen sus directorios).

## Decisiones donde el diseño original hubo que ajustarlo a la realidad
1. **Comportamiento real de `agy` 1.5.3 con input mientras trabaja**: comprobado en ConPTY real contra `agy.exe` 1.5.3: al inyectar texto + Enter mientras está `Generating...`, `agy` lo encola nativamente (`▸ <texto> (alt+m to send immediately)` / `Press up to edit queued messages`) sin mezclarlo ni perderlo, igual que Claude Code. Donde sí falla o activa opciones de diálogo es durante el arranque (`Starting` / PowerShell previo a Ink), al despertar de `Sleeping` o cuando muestra modales de confianza/permisos (`Blocked`). Por eso el backend entrega directo a `agy` live (`Idle`/`Working`) y encola en `TeamHub` únicamente cuando está `Sleeping`, `Starting` o `Blocked`.
2. **Tope de 24 KB en reglas de Antigravity CLI (`~/.gemini/GEMINI.md`)**: `agy --help` no expone ningún flag equivalente a `--append-system-prompt`, y el archivo `~/.gemini/GEMINI.md` de este equipo supera los 76 KB, mientras que AGY trunca archivos de reglas a 24.000 bytes. Por eso `bootstrap.rs` **antepone** el bloque `<!-- velox:begin --> ... <!-- velox:end -->` al tope del archivo (y lo reubica arriba si detecta uno existente más allá de los 16 KB).
3. **Config MCP real de Antigravity CLI v2 y expansión en Claude Code**:
   - Empíricamente contra un servidor HTTP mock, Claude Code sí expande `"X-Velox-Session": "${VELOX_SESSION_ID:-}"` al ID real (y a `""` si la variable no está definida; la forma sin `:-` mandaba el literal `"${VELOX_SESSION_ID}"`).
   - La configuración MCP activa de AGY 1.5.3 vive en `~/.gemini/config/mcp_config.json` (además del legacy `~/.gemini/antigravity-cli/mcp_config.json`) y su esquema HTTP usa `url`/`serverUrl` sin soporte de `headers` por env var, por lo que los agentes AGY firman vía `VELOX_SESSION_ID` usando el CLI `velox` o `from_session` si invocan MCP directo.
4. **PowerShell 5.1 UTF-8 y Git Bash**: en Windows PowerShell 5.1, `Invoke-RestMethod` serializa cuerpos `string` UTF-8 como Latin-1, rompiendo tildes y `·`; `velox.ps1` usa `System.Net.WebClient::UploadData` sobre bytes UTF-8. Además Git Bash no resuelve `.cmd` sin extensión, por lo que `bootstrap.rs` instala también el shim `#!/bin/sh` llamado `velox`.
5. **Espera acotada en `session_send` (15 s síncrono / 120 s background)**: para wakes normales (~3–6 s), `session_send` espera a que el worker de cola termine y devuelve `delivered` con `wokeUp: true`. Si el destino queda `Blocked` más de 15 s, devuelve `queued` para no colgar la llamada MCP/CLI mientras la cola en background sigue hasta 120 s actualizando el toast del sidebar.

## Cómo probarlo / verificaciones ejecutadas
- **Compilación y tests Rust** (`CARGO_TARGET_DIR=D:\Workspace\velox\src-tauri\target-lanes` en `D:\Workspace\velox-wt\msg\src-tauri`):
  - `cargo check --tests`: pasó con código 0, sin warnings en código fuente.
  - `cargo test`:
    - `velox_lib` unit tests: **85 passed, 0 failed, 1 ignored** (incluye tests nuevos para `SessionLaunchContext`, `X-Velox-Session` en `gateway.rs`, `detect_screen_activity`/`is_agent_ready_for_input` en `detector.rs`, `bootstrap.rs`, `session_list`/`session_send` en `team/mcp.rs`).
    - `tests/integration_tests.rs`: **7 passed, 0 failed, 2 ignored** (`real_conpty_*` preexistentes).
    - `tests/team_tests.rs`: **12 passed, 0 failed, 0 ignored** (incluye 4 tests nuevos end-to-end: entrega directa sin bandeja + formato exacto, rechazos auto-envío/`Exited`/`>16KB`, wake desde `Sleeping` esperando prompt listo, cola ante `Blocked` + entrega directa a `agy` ocupado).
- **Frontend**: `pnpm build` (`tsc --noEmit && vite build`) en `D:\Workspace\velox-wt\msg`: pasó con código 0.
- **Smoke tests empíricos adicionales**:
  - `claude.exe` interactivo con `--append-system-prompt` + `--session-id` y `--resume` (`probe_agy_busy_input_and_claude_interactive` en ConPTY): respondió incluyendo la etiqueta pedida en ambos turnos.
  - `agy.exe` 1.5.3 ConPTY ocupado: confirmó encolado nativo (`▸ ... (alt+m to send immediately)`).
  - Wrappers CLI (`velox.cmd` en `cmd.exe` y shim `velox` en Git Bash contra mock HTTP): verificó `velox list`, `velox msg` con UTF-8 (`¡Hola sesión líder!` y `Desde Git Bash · OK`), `velox read`, header `X-Velox-Session` y código de salida `1` ante error.

## Qué quedó pendiente
- Compilar el nuevo ejecutable e instalar/reiniciar `velox.exe` desde la rama principal tras revisar y mergear `feat/session-messaging` (para no cerrar la instancia actual donde corren las sesiones).
- Una vez lanzado el `velox.exe` nuevo, confirmar automáticamente en un arranque real que `%LOCALAPPDATA%\velox\bin`, las configs MCP (`velox-team` con `X-Velox-Session`) y `~/.gemini/GEMINI.md` queden provisionados, y hacer una prueba punta a punta entre dos pestañas reales (una Claude y una AGY, incluyendo una hibernada).
