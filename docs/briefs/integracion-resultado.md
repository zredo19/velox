# Informe de integración en `main` y modo "Control total"

## 1. Qué entró en `main`

Sobre la base de `github/main` con el PR #1 ya mergeado (`1b6604d`), se integró de forma lineal (strict fast-forward, sin reescribir historia ni usar `--force`) todo el trabajo local pendiente más la nueva funcionalidad de control total mutuo:

1. **PR #1 de Benjamín (`SrDip12`, commits `6b4b25b`, `d8eee11`, `0b0d9c5`, mergeado en `1b6604d`)**:
   - Optimizaciones de rendimiento del frontend: code-splitting en `vite.config.ts` y carga lazy de modales/drawers, listener `pty-output` multiplexado por `sessionId`, coalescing de ráfagas PTY y deduplicación de `resizePty` en `TerminalPane.tsx`, memoización de componentes y workers sin asignaciones innecesarias.
   - 7 temas animados personalizados con galería (`Ctrl+Shift+T`), íconos y escenas oficiales de Codex y OpenCode, Command Palette (`Ctrl+K`), atajos (`Ctrl+/`) y zoom de fuente en vivo.
   - `rust-toolchain.toml` actualizado a `channel = "stable"` para compatibilidad con `rustup 1.29+`.
2. **Velox Online + mensajería directa entre sesiones (`73e91b9..feat/velox-online`, 17 commits trasladados)**:
   - Hub WebSocket de equipo (`src-tauri/src/team/`), detección de Tailscale, servidor MCP embebido `velox-team`, vista remota de terminales compartidas, bandeja de tareas con tarjeta Accept/Reject, saneamiento de tareas y resúmenes de actividad.
   - Mensajería local entre sesiones (`velox list`, `velox msg`, `velox read`), inyección de variables `VELOX_SESSION_ID` / `VELOX_SESSION_TITLE` / `VELOX_GATEWAY_URL`, wrappers CLI, auto-registro del MCP `velox-team` al arrancar y despertar de sesiones dormidas al recibir mensajes.
3. **Persistencia determinista de sesiones e ícono en Windows (`fix/session-persistence`, commits `3dad630` y `b9a3a7f`, terminados por `velox 1`)**:
   - Vínculo determinista de sesiones de agentes (`links.rs`, hook de Claude Code vía `--settings`, reanudación segura y persistencia a prueba de cierres inesperados en Windows con `install_windows_shutdown_hook`).
   - Reconstrucción automática de recursos de ícono en `src-tauri/build.rs` al cambiar `icons/icon.ico` y script `scripts/install-shortcut.ps1` para crear/reparar accesos directos en Escritorio y Menú Inicio.
4. **Modo "Control total" mutuo (`2630d5c`)**:
   - Interruptor **Control total** por compañero en **Ajustes → Team** (apagado por defecto, persistido en `config.toml` como `fullControl = false`).
   - Activación estrictamente mutua (`local && remote`), sincronizada en tiempo real por WebSocket en el handshake (`Hello.full_control`) y ante cambios en caliente (`FullControl { enabled }`), sin cortar la conexión WebSocket.
   - Estados claros en la UI: `"Activo"`, `"Esperando a que <nombre> lo active"`, `"<nombre> ya lo activó — enciéndelo para habilitar"`, o `"Apagado"`.
   - Indicador permanente en la barra de título (`"<nombre> tiene control total"`) con acceso directo a Ajustes → Team.
   - Entrega directa de tareas sin tarjeta Accept/Reject cuando está activo (manteniendo el saneamiento de tareas), registro visible en `TeamTasksPopover`, notificación toast (`"<compañero> → <sesión>: …"`) y despertar automático si la sesión destino compartida está dormida.
   - Terminal remota interactiva en `RemoteTerminalPane.tsx` (teclas sin filtrar incluyendo `Ctrl+C`, flechas, Enter y pegado) solo mientras el modo mutuo esté activo; al apagarse en cualquiera de los dos lados, vuelve instantáneamente a `Read-only` sin cerrar ni remontar la vista.
   - Seguridad aplicada en backend (`team/hub.rs`): sesiones no compartidas siguen siendo invisibles e inaccesibles y cualquier `TerminalInput` sin acuerdo mutuo es rechazado por el servidor; reglas de token y Tailscale sin cambios.
5. **Documentación actualizada (`f037c6b`)**:
   - `docs/online/SETUP.md` y `README.md` actualizados con explicación de **Control total**, activación mutua, auto-registro del MCP `velox-team` y CLI `velox`, flujo de actualización para Benjamín y nota de compilación con MinGW/MSVC en Windows.

---

## 2. Conflictos encontrados y cómo se resolvieron

### 2.1 Frontend: PR #1 (`SrDip12`) vs Velox Online y mensajería

Se presentaron 7 archivos con cambios superpuestos; en todos se conservaron íntegramente ambas funcionalidades:

- `src/lib/tauri.ts`: se preservó el listener `pty-output` multiplexado del PR #1 junto con todos los comandos/eventos `team*` y `onTeamLocalMessage`, agregando también los stubs correspondientes en `createDevMockApi()`.
- `src/components/Sidebar.tsx`: se mantuvieron `React.memo`, filtro inline, menú contextual extendido y `AgentCrew` interactivo junto con `TeamSidebarSection`, botón de transmisión (`Radio`) y notificaciones `localMessages`.
- `src/components/TitleBar.tsx`: se conservaron `React.memo`, selector de temas, atajos y búsqueda rápida junto con el botón de estado de equipo, `TeamTasksPopover` y el indicador de **Control total**.
- `src/App.tsx`: se mantuvieron los modales/drawers lazy (code-splitting), temas personalizados, Command Palette y toasts junto con `useTeam`, `RemoteTerminalPane`, `TaskInbox` y el paso de props `team` y `onSave` al `SettingsModal` lazy. Se eliminaron dos callbacks huérfanos tras la fusión.
- `src/components/SettingsModal.tsx`: se preservaron los controles de temas y estilos adaptables junto con `TeamSettingsSection` (con guarda `draft?.team ?? DEFAULT_TEAM_CONFIG` antes del retorno condicional cuando el modal está cerrado).
- `src/index.css` y `README.md`: se fusionaron los estilos de animación/temas con los del espejo remoto, y las secciones de documentación.
- Verificación de aislamiento de `TerminalPane.tsx` vs `RemoteTerminalPane.tsx`: la vista remota escucha eventos independientes (`team-pty-snapshot` / `team-pty-output`) y escala por CSS las dimensiones `cols`/`rows` fijas del dueño, por lo que no queda afectada por el multiplexador `pty-output`, el coalescing ni la deduplicación de `resizePty` del PR #1. Adicionalmente se ajustó `applyOutputChunk` en `RemoteTerminalPane.tsx` para aceptar saltos de generación (`ev.generation > streamGen`) cuando una sesión compartida dormida se despierta bajo Control total.

### 2.2 Backend: mensajería entre sesiones vs `fix/session-persistence`

Al incorporar `3dad630` sobre el trabajo de mensajería se resolvieron 4 conflictos preservando ambas capacidades:

- `src-tauri/src/domain/agent.rs`: se unificó `resolve_launch_spec_with_options` para inyectar simultáneamente `--settings` (hook de Claude Code) y `SessionLaunchContext` (`VELOX_SESSION_ID`, `VELOX_SESSION_TITLE`, `VELOX_GATEWAY_URL`, `PATH` con wrappers CLI y `--append-system-prompt`), manteniendo wrappers compatibles para todas las firmas previas.
- `src-tauri/src/session/manager.rs`: se unificó `prepare_spawn_spec` combinando el contexto de mensajería local con `ensure_claude_settings_path`, `VELOX_AGENT_LINKS_DIR` y la preservación determinista del vínculo del agente.
- `src-tauri/src/lib.rs`: se conservaron ambos pasos de arranque (`bootstrap_local_messaging` e `install_windows_shutdown_hook`).
- `src-tauri/src/session/detector.rs`: se conservaron los tests unitarios de ambas ramas.

### 2.3 Toolchain en Windows (`rust-toolchain.toml`)

El PR #1 (`d8eee11`) cambió `rust-toolchain.toml` a `channel = "stable"` para soportar `rustup 1.29+` y MSVC (usado por Benjamín). En la PC de Rodolfo no hay `link.exe` de MSVC y se compila con WinLibs MinGW (`gcc.exe`), por lo que en esta integración se mantuvo `channel = "stable"` y se compiló/probó usando `RUSTUP_TOOLCHAIN=stable-x86_64-pc-windows-gnu` (documentado en `README.md` y en los pasos para Rodolfo).

---

## 3. Resultados de validación

Todas las compilaciones y pruebas en Rust se ejecutaron con `CARGO_TARGET_DIR=D:\Workspace\velox\src-tauri\target-integ` y `RUSTUP_TOOLCHAIN=stable-x86_64-pc-windows-gnu`, sin tocar el ejecutable en ejecución (`D:\Workspace\velox\src-tauri\target\release\velox.exe`).

| Validación | Resultado | Detalle |
| --- | --- | --- |
| `pnpm build` (`tsc --noEmit && vite build`) | Verde | 1694 módulos compilados, 0 errores, code-splitting preservado |
| `cargo test` (`velox_lib` unitarios) | Verde | 99 passed, 0 failed, 1 ignored (incluye las 4 combinaciones de `is_mutual_full_control`, cambio en caliente sin reconexión y round-trips de protocolo) |
| `cargo test` (`integration_tests`) | Verde | 7 passed, 0 failed, 2 ignored |
| `cargo test` (`team_tests`) | Verde | 13 passed, 0 failed (incluye `full_control_two_hubs_mutual_task_and_remote_input_and_hot_cutoff`: ambos apagados, un solo lado, acuerdo mutuo con entrega directa + toast + input crudo + rechazo a sesión privada + despertar sesión dormida, y corte en caliente) |
| `cargo clippy --all-targets -- -D warnings` | Verde | 0 warnings, 0 errors |
| `cargo build --release --features custom-protocol` | Verde | Generó `D:\Workspace\velox\src-tauri\target-integ\release\velox.exe` (7,590,400 bytes, ícono embebido verificado 32x32) |

---

## 4. Prueba real con dos instancias aisladas

Se ejecutó una prueba end-to-end automatizada sobre el binario release real `D:\Workspace\velox\src-tauri\target-integ\release\velox.exe`, levantando dos instancias completamente aisladas (`TestA` = `RodolfoTest` en puerto `43320`, `TestB` = `BenjaminTest` en puerto `43420`) con `VELOX_CONFIG_DIR` y `WEBVIEW2_USER_DATA_FOLDER` temporales en `%TEMP%`, `mcpGatewayEnabled = false` (para no alterar el registro MCP real de Rodolfo en puerto `43111`) y controladas por WebView2 CDP.

Los 9 pasos verificados en verde:

1. **Arranque simultáneo aislado** de `TestA` y `TestB` sin interferir con el Velox activo de Rodolfo.
2. **UI integrada (PR #1 + Velox Online)**: verificados botones de temas, atajos, Team popover y apertura del `SettingsModal` lazy con la sección `Team (Velox Online)` y el interruptor `Control total`.
3. **Sesiones PowerShell reales (ConPTY)**: creadas `Shell Compartida` (compartida) y `Shell Privada` (no compartida) en `TestA`.
4. **Visibilidad remota y modo inicial**: `TestB` descubrió únicamente `Shell Compartida` (sin filtrar `Shell Privada`) y abrió `RemoteTerminalPane` en modo `Read-only`.
5. **Control total en un solo lado (`TestB` ON, `TestA` OFF)**: `TestB` mostró `"Esperando a que RodolfoTest lo active"`, no apareció banner en ninguna barra de título, la vista siguió `Read-only`, `team_send_input` fue rechazado por el backend y una tarea enviada quedó `pending` mostrando tarjeta `Accept / Reject` en `TestA`.
6. **Activación mutua (`TestA` también ON)**: `Settings` pasó a `"Activo"`, aparecieron los banners `"BenjaminTest tiene control total"` y `"RodolfoTest tiene control total"` en ambas barras de título, y la vista remota abierta en `TestB` cambió en vivo a interactiva (`Control total`) sin recargar.
7. **Entrega directa de tarea + toast + historial**: una tarea desde `TestB` se entregó sin tarjeta Accept/Reject, apareció el toast `"BenjaminTest → Shell Compartida: …"` en `TestA`, quedó registrada en `TeamTasksPopover` y ejecutó `AUTO_TASK_OK_777` en el ConPTY real de `TestA`.
8. **Teclado remoto real + teclas sin filtrar + aislamiento privado**: escritura real por teclado en el `xterm` de `RemoteTerminalPane` en `TestB` ejecutó `REMOTE_KEY_OK_888` en `TestA`; `Ctrl+C` canceló una línea pendiente sin filtrarse y el siguiente comando ejecutó `AFTER_CTRLC_OK_999`; un intento directo de `team_send_input` contra `Shell Privada` fue bloqueado sin tocar su scrollback.
9. **Corte en caliente**: al apagar **Control total** en `TestA`, desaparecieron los banners en ambas barras de título, la vista remota abierta en `TestB` volvió instantáneamente a `Read-only`, `team_send_input` volvió a rechazarse en backend y la siguiente tarea volvió a requerir aprobación (`pending`).

Al finalizar, ambas instancias temporales y sus directorios `%TEMP%\velox-integ-test-*` se limpiaron completamente, quedando únicamente el Velox original de Rodolfo en ejecución y sus configuraciones MCP intactas.

---

## 5. Historia final de `main`

- `1b6604d` — `Merge pull request #1 from SrDip12/main` (ya mergeado en GitHub preservando los commits `6b4b25b`, `d8eee11`, `0b0d9c5` de `SrDip12`).
- `7d9f71f..f406de5` — 17 commits de Velox Online y mensajería directa entre sesiones (trasladados encima de `1b6604d`).
- `3dad630` — `fix(sessions): deterministic agent linking and crash-safe persistence`.
- `b9a3a7f` — `fix(windows): rebuild icon resources on change and add shortcut repair script`.
- `2630d5c` — `feat(team): add mutual full-control mode with direct delivery and live remote input`.
- `f037c6b` — `docs(online): document mutual Control total, auto-registration, and update flow`.
- Commit final de `main`: el commit que añade este informe (`docs/briefs/integracion-resultado.md`), pusheado en fast-forward exclusivo a `github/main` (sin subir ramas temporales ni locales).

---

## 6. Pasos exactos para Rodolfo (cambiar al ejecutable nuevo)

El binario release nuevo ya está compilado y probado en `D:\Workspace\velox\src-tauri\target-integ\release\velox.exe`. Cuando quieras cambiarte:

1. **Cierra el Velox que está corriendo** (para liberar el archivo `velox.exe`).
2. En PowerShell, reemplaza el ejecutable y actualiza los accesos directos e ícono:
   ```powershell
   Copy-Item -Force `
     "D:\Workspace\velox\src-tauri\target-integ\release\velox.exe" `
     "D:\Workspace\velox\src-tauri\target\release\velox.exe"

   & "D:\Workspace\velox-wt\integ\scripts\install-shortcut.ps1" `
     -Exe "D:\Workspace\velox\src-tauri\target\release\velox.exe"
   ```
3. *(Opcional, para dejar tu checkout principal `D:\Workspace\velox` sincronizado con `github/main`)*:
   ```powershell
   Set-Location D:\Workspace\velox
   git fetch github main
   git checkout -B main github/main
   ```
4. *(Recomendado una sola vez si quieres compilar sin pasar `RUSTUP_TOOLCHAIN` manualmente)*:
   ```powershell
   rustup default stable-x86_64-pc-windows-gnu
   ```
5. Abre Velox normalmente desde el acceso directo. Si quieres darle **Control total** a Benjamín, abre **Ajustes → Team**, activa el interruptor **Control total** junto a su compañero y guarda (queda activo recién cuando él también lo encienda de su lado).

---

## 7. Pasos exactos para Benjamín (`SrDip12`)

1. Actualiza el repositorio e instala dependencias:
   ```powershell
   git checkout main
   git pull origin main
   pnpm install
   ```
2. Arranca en modo desarrollo o compila el release:
   ```powershell
   pnpm tauri dev
   # o para generar el ejecutable release:
   pnpm tauri build
   ```
3. **Auto-registro incluido**: al iniciar Velox, el servidor MCP `velox-team` y el comando `velox` (`velox list`, `velox msg`, `velox read`) se registran automáticamente (no hace falta configurar manualmente `.claude.json` ni `mcp_config.json`).
4. **Activar "Control total" (opcional y mutuo)**:
   - Abre **Ajustes → Team**, ubica a Rodolfo en la lista de compañeros y enciende el interruptor **Control total**.
   - Mientras Rodolfo no lo active de su lado, verás `"Esperando a que <nombre> lo active"` y todo seguirá en modo normal (solo lectura + aprobación manual).
   - Cuando ambos lo tengan encendido, aparecerá `"Activo"` y el indicador permanente `"<nombre> tiene control total"` en la barra de título: podrás enviar tareas directas y escribir en tiempo real en cualquier sesión que él haya compartido (lo no compartido sigue bloqueado).
   - Apagar el interruptor desde cualquiera de los dos PC corta el control al instante.
