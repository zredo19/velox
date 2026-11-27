# Resultado: Persistencia e aislamiento de sesiones + ícono embebido y accesos directos

Trabajo completado en el worktree `D:\Workspace\velox-wt\persist` sobre la rama `fix/session-persistence` (desde `feat/velox-online` en `30da208`).

## 1. Causas raíz confirmadas

### A. Sesiones vacías, cruzadas o sin ID al reabrir / tras apagado
1. **Heurísticas ambiguas de descubrimiento (`detector.rs` / `manager.rs`)**:
   - `discover_agent_session_id` escaneaba archivos `.jsonl` de Claude por fecha de modificación, permitiendo cruces cuando coexistían varias pestañas en la misma carpeta.
   - Para Antigravity CLI, la validación `origins.get(conv) == Some(&want_cwd)` en `history.jsonl` descartaba sesiones válidas si la conversación se había iniciado o movido respecto al `cwd` registrado en Velox. Se comprobó en `%APPDATA%\velox\sessions.json` real: `"velox 1"` (`b295549c-6dc7-47cd-a762-e97d312bd576`, `cwd: D:\Workspace`) tenía `"agentSessionId": null` aunque `history.jsonl` registraba su conversación `ad3fbde2-0603-403e-882a-e7bf3b771cc9` con `"workspace": "D:\\Workspace\\velox"`, y `"Velo nuevo lider"` (`726b15d9-38c2-4f7a-a295-ae5e029e547f`) quedó apuntando a un transcript antiguo (`5ca714ca-a9ec-4842-be11-2370711c79a6`, `7,950` bytes) en lugar de su transcript activo (`0a8c1113-7c23-4691-a361-96fb45e72354.jsonl`).
   - En `wake_session`, un `agent_session_id` no resumible se descartaba sin historial y se intentaba recuperar mediante `recover_agent_session_id`, pudiendo apropiarse de otra conversación en la misma carpeta.
2. **Particularidades reales detectadas en hooks de Claude Code y Antigravity CLI en Windows**:
   - Al probar `agy.exe` (`--log-file`) con `~/.gemini/config/hooks.json`, Go `os/exec` ejecutaba `cmd /c` escapando las comillas dobles del campo `"command"`, haciendo fallar a PowerShell con `-File '"C:\...\session-link.ps1"'` (`Caracteres no válidos en la ruta de acceso`).
   - En Windows, `session-link.ps1` tarda ~2.6 s en arrancar en frío; el `timeout: 5` inicial en `--settings` de Claude podía vencer al abrir varias pestañas seguidas.
3. **Resiliencia de `sessions.json` y apagado de Windows**:
   - `store::save_sessions_to` escribía sin `sync_all()` ni copia de respaldo `.bak`, y `SessionManager::new` utilizaba `load_sessions().unwrap_or_default()`, lo que ante un archivo truncado tras un corte de energía podía sobrescribir todas las pestañas con `[]`.
   - `tao-0.37.3` (`src/platform_impl/windows/event_loop.rs:2344-2360`) no implementa `WM_QUERYENDSESSION` y finaliza con `std::process::exit(0)` en `WM_ENDSESSION`, sin ejecutar `RunEvent::Exit`.

### B. Ícono antiguo en `velox.exe` y accesos directos
1. En `D:\Workspace\velox\src-tauri\target\release\velox.exe` (`7,168,512` bytes, `26-11-2026 2:52:59`) seguía embebido el ícono oscuro anterior (`0 / 1024` píxeles coincidentes frente a `icons/icon.ico` y `521 / 1024` frente a `icons/128x128.png`) porque `libresource.a` (`1,570` bytes, `25-11-2026 16:59:48`) permaneció cacheado tras la actualización de `src-tauri/icons/` (`26-11-2026 01:25:50`), ya que `build.rs` no emitía `cargo:rerun-if-changed` sobre los archivos de íconos.
2. El acceso directo del Escritorio (`C:\Users\Asus\OneDrive\Escritorio\Velox.lnk`) apuntaba `IconLocation` a `icons\icon.ico,0` y no existía acceso directo en el Menú Inicio.

---

## 2. Cambios realizados

- **Vínculo determinístico por pestaña (`src-tauri/src/session/links.rs`, `src-tauri/src/session/session_link_hook.ps1`, `src-tauri/src/session/mod.rs`)**:
  - Cada pestaña recibe `VELOX_SESSION_ID` y `VELOX_AGENT_LINKS_DIR` y escribe su vínculo en `agent-links/<VELOX_SESSION_ID>.json`.
  - **Claude Code**: `build_claude_settings_json` genera `hooks/claude-settings.json` con hook `SessionStart` (`timeout: 15`), inyectado por `--settings` mediante `resolve_launch_spec_with_settings` (`src-tauri/src/domain/agent.rs`), conservando los hooks globales del usuario (como `herdr`) y capturando `/clear` o `/resume`.
  - **Antigravity CLI**: `upsert_agy_hooks_json` instala `hooks/velox-session-link.ps1` y `hooks/velox-session-link.cmd` junto a `~/.gemini/config/hooks.json` y registra la entrada `"velox"` (`PreInvocation` -> `hooks\velox-session-link.cmd`, sin comillas ni paréntesis y preservando `"herdr"` y otros hooks). Fuera de Velox (`VELOX_SESSION_ID` vacío) el `.cmd` responde `{}` sin iniciar PowerShell.
  - Nota: la presencia de `VELOX_SESSION_ID` en `CommandBuilder` (`src-tauri/src/pty/manager.rs`) mantiene el contrato compartido con mensajería entre sesiones.

- **Gestión de sesiones sin cruces (`src-tauri/src/session/detector.rs`, `src-tauri/src/session/manager.rs`)**:
  - Se eliminó el escaneo ambiguo por mtime en Claude y la recuperación cruzada (`discover_agent_session_id` / `recover_agent_session_id`).
  - Se añadió `check_agent_session_resumable` (`Resumability`) tratando errores distintos de `NotFound` sobre transcripts existentes como resumibles.
  - Se incorporó `match_agy_history_entries` como respaldo determinístico por timestamps de `Enter` (`enter_timestamps` en `write_pty_bytes`), asignando únicamente cuando existe un solo candidato en la ventana `[-500 ms, +6000 ms]`.
  - Se agregó `persistence_blocked` en `SessionManager::new` para impedir sobrescribir `sessions.json` si ocurre un error irrecuperable al cargar sesiones.

- **Historial de IDs previos (`src-tauri/src/domain/session.rs`, `src/types/ipc.ts`, `src-tauri/src/telemetry/monitor.rs`, `src-tauri/tests/team_tests.rs`)**:
  - Nuevo campo `previous_agent_session_ids: Vec<String>` (`#[serde(default, skip_serializing_if = "Vec::is_empty")]`) y helpers `preserve_previous_agent_session_id` y `set_agent_session_id_preserving` (`MAX_PREVIOUS_AGENT_SESSION_IDS = 20`).

- **Escritura atómica y cierre en Windows (`src-tauri/src/session/store.rs`, `src-tauri/src/lib.rs`)**:
  - `save_sessions_to` realiza `sync_all()` en el temporal antes del reemplazo y rota `sessions.json.bak`. Ante JSON inválido archiva `.corrupt-<timestamp>` e intenta recuperar desde `.bak`, devolviendo `Err` si ambos fallan.
  - `install_windows_shutdown_hook` subclasifica la ventana `"main"` (`SetWindowLongPtrW(GWLP_WNDPROC)`) para llamar a `sessions.persist_snapshot()` en `WM_QUERYENDSESSION` y `sessions.shutdown()` en `WM_ENDSESSION`.

- **Recompilación automática de íconos y accesos directos (`src-tauri/build.rs`, `scripts/install-shortcut.ps1`)**:
  - `build.rs` emite `cargo:rerun-if-changed` para `tauri.conf.json`, `icons/icon.ico`, `icons/32x32.png` y `icons/128x128.png` manteniendo el enlace de `libresource.a`.
  - Se creó `scripts/install-shortcut.ps1` (parámetros `-Exe`, `-Name`, `-DesktopDir`, `-StartMenuDir`, `-NoIconCacheRefresh`, `-DryRun`) para crear y reparar de forma idempotente `Velox.lnk` en Escritorio y Menú Inicio con `IconLocation = "<exe>,0"` y refresco de caché (`SHChangeNotify` + `ie4uinit.exe -show`).

---

## 3. Verificación realizada

Todas las compilaciones Rust se ejecutaron con `CARGO_TARGET_DIR=D:\Workspace\velox\src-tauri\target-lanes` sin tocar `src-tauri\target` ni el proceso principal en ejecución (`velox.exe`).

1. **Suite Rust (`cargo test --tests` + `cargo clippy --all-targets -- -D warnings`)**:
   - `velox_lib` (unitarios): `87 passed; 0 failed; 1 ignored` (`folder_picker_smoke_test_closes_cleanly_with_ok_none`).
   - `src\main.rs`: `0 passed; 0 failed`.
   - `integration_tests.rs`: `7 passed; 0 failed; 2 ignored` (`real_conpty_*`).
   - `team_tests.rs`: `8 passed; 0 failed`.
   - `clippy --all-targets -- -D warnings`: 0 warnings, 0 errors.
2. **Frontend y script de accesos directos**:
   - `pnpm build` (`tsc --noEmit && vite build`): `1685 modules transformed`, sin errores.
   - `.\scripts\install-shortcut.ps1 -Exe "D:\Workspace\velox\src-tauri\target-lanes\release\velox.exe" -DryRun -NoIconCacheRefresh`: confirmó actualización del `.lnk` de Escritorio y creación del `.lnk` de Menú Inicio con `<exe>,0`. (La ejecución real sobre directorios temporales se probó previamente validando idempotencia).
3. **Binario release e ícono**:
   - `cargo build --release --features custom-protocol` generó `D:\Workspace\velox\src-tauri\target-lanes\release\velox.exe` (`7,393,792` bytes).
   - `[System.Drawing.Icon]::ExtractAssociatedIcon` sobre el ejecutable coincidió **1024 / 1024** píxeles frente a `src-tauri/icons/32x32.png` (y la verificación `WM_GETICON` 32x32 en ventana viva previa al último ajuste de `links.rs` también coincidió **1024 / 1024** píxeles).
4. **Prueba E2E aislada (3 pestañas Claude + 2 pestañas Antigravity en la misma carpeta `velox-e2e-proj`, `lazyRestoreOnStartup = true`)**:
   - **Creación y aislamiento**: las 5 pestañas (`C1`, `C2`, `C3`, `A1`, `A2`) recibieron 5 `agentSessionId` distintos mediante los hooks (`SessionStart` / `PreInvocation`) y respondieron con su token propio (`E2E-TOKEN-C1`..`E2E-TOKEN-A2`).
   - **Cierre limpio (`WM_CLOSE`) y reapertura (`CLEAN_CLOSE`)**: `sessions.json` y `sessions.json.bak` conservaron las 5 sesiones intactas; al reabrir en modo `sleeping` y despertar cada pestaña, las 5 retomaron su conversación exacta y recordaron su token respectivo.
   - **Kill forzado (`Stop-Process -Force`) y reapertura (`FORCE_KILL`)**: simulando corte de energía sin cierre limpio, `sessions.json` conservó los 5 registros en disco; al reabrir y despertar las 5 pestañas, todas retomaron exactamente su conversación y recordaron su token sin cruces ni pérdidas.

---

## 4. Cómo aplicar y probar en el entorno principal

1. **Reemplazar el binario cuando corresponda**:
   - Cerrar Velox manualmente y copiar `D:\Workspace\velox\src-tauri\target-lanes\release\velox.exe` a `D:\Workspace\velox\src-tauri\target\release\velox.exe` (o recompilar tras el merge a `feat/velox-online`; si hubiese `libresource.a` previo en `target\release\build\velox-*\out\`, el `rerun-if-changed` de `build.rs` ya fuerza su regeneración).
2. **Reparar accesos directos**:
   ```powershell
   .\scripts\install-shortcut.ps1
   ```
3. **Nota para pruebas paralelas (`scripts/velox-test-instance.ps1`)**:
   - Cuando el `velox.exe` principal está abierto, WebView2 bloquea el directorio por defecto (`%LOCALAPPDATA%\dev.velox.ade`). Para levantar una segunda instancia aislada en paralelo es necesario definir también `WEBVIEW2_USER_DATA_FOLDER` además de `VELOX_CONFIG_DIR`.
