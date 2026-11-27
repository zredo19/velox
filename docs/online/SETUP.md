# Velox Online: puesta en marcha (Rodolfo + Benjamín)

Unos 10 minutos entre los dos. Se hace una sola vez.

## 0. La versión nueva de Velox (cómo actualizar)

Al arrancar Velox **el MCP `velox-team` y el comando CLI `velox` (`velox list`, `velox msg`, `velox read`) se registran e instalan solos** automáticamente para Claude Code (todos tus perfiles `~/.claude*`) y Antigravity CLI (`~/.gemini/config/mcp_config.json` y `~/.gemini/antigravity-cli/mcp_config.json`), junto con las reglas globales y los hooks de persistencia de sesión. No hace falta correr scripts manuales salvo que uses un setup personalizado.

**Rodolfo:** cierra el Velox actual, reemplaza `D:\Workspace\velox\src-tauri\target\release\velox.exe` por el binario nuevo (o recompila con `pnpm build` + `cargo build --release --features custom-protocol`), opcionalmente ejecuta `.\scripts\install-shortcut.ps1` para actualizar los accesos directos e íconos, y ábrelo de nuevo. Tus sesiones vuelven dormidas como siempre y retoman su conversación al hacer clic.

**Benjamín (`SrDip12`):** desde tu copia local del repositorio `zredo19/velox`:

```powershell
git pull
pnpm install
pnpm tauri dev
```

Si prefieres compilar el ejecutable de release:

```powershell
git pull
pnpm install
pnpm tauri build
```

Al abrir Velox ya quedan registrados solos el MCP `velox-team` y el comando `velox` dentro de cada pestaña.

## 1. Tailscale

**Rodolfo (dueño de la red, ya hecho):** Tailscale instalado, sesión con GitHub (`zredo19`),
PC llamado `rodolfo`.

**Antes de invitar a Benjamín, pegar las reglas de acceso.** Una red nueva de Tailscale deja que
cada PC entre a todo lo del otro. Con estas reglas, entre ustedes solo queda abierto el puerto de
Velox (43120), y cada uno mantiene acceso completo a sus propios equipos.

1. Entra a https://login.tailscale.com/admin/acls (con GitHub).
2. Reemplaza todo el contenido por esto y guarda (**Save**):

```jsonc
{
  "acls": [
    // Cada persona entra a sus propios equipos sin restricciones
    { "action": "accept", "src": ["autogroup:member"], "dst": ["autogroup:self:*"] },
    // Entre ustedes: solo Velox Online (TCP 43120)
    { "action": "accept", "proto": "tcp", "src": ["autogroup:member"], "dst": ["autogroup:member:43120"] }
  ]
}
```

3. Comprueba las reglas en la pestaña **Preview rules** de esa misma página: elige a Benjamín y
   revisa que hacia `rodolfo` solo aparezca `tcp:43120`.

**Invitar a Benjamín:** https://login.tailscale.com/admin/users → **Invite external users** →
escribe su correo → **Invite** (o **Copy invite link** y se lo mandas). El plan gratis permite hasta
6 personas.

**Benjamín:**
1. Instala Tailscale: `winget install -e --id Tailscale.Tailscale`
2. Abre la invitación y entra con su cuenta.
3. Cambia el nombre de su PC a `benjamin` desde PowerShell:
   `& "C:\Program Files\Tailscale\tailscale.exe" set --hostname=benjamin`
4. Comprueba la conexión: `& "C:\Program Files\Tailscale\tailscale.exe" ping rodolfo`

## 2. Velox (en los dos PCs)

1. Abre la versión nueva de Velox.
2. **Ajustes (Ctrl+,) → Team (Velox Online)**:
   - **Enabled:** activado.
   - **Display name:** `Rodolfo` / `Benjamin`.
   - **Listen port:** `43120`.
   - **Bind host:** vacío. Velox usa solo la IP de Tailscale y nunca escucha en el wifi ni en la red local.
   - **Token:** uno de los dos aprieta **Generate**, lo copia y se lo pasa al otro por un canal
     privado (por ejemplo, WhatsApp). Tiene que ser **el mismo** en los dos PCs.
   - **Peers:** agrega al otro:
     - En el PC de Rodolfo: nombre `Benjamin`, host `benjamin`, puerto `43120`.
     - En el PC de Benjamín: nombre `Rodolfo`, host `rodolfo`, puerto `43120`.
   - Guarda los cambios.
3. La primera vez, Windows pregunta si Velox puede usar la red: **Permitir** (marca privadas y
   públicas). Es seguro porque Velox solo escucha en la IP de Tailscale.
4. En la barra lateral, comparte las sesiones que quieras que el otro vea con el botón de
   transmisión de cada fila. Por defecto **no se comparte ninguna**.

## 3. Uso

- **Ver al otro:** sus sesiones compartidas aparecen bajo su nombre en la barra lateral. Al hacer
  clic ves su terminal en vivo (solo lectura por defecto).
- **Mandar una tarea a mano:** abre su sesión y escribe en la caja de abajo → **Send task**. Por
  defecto, el otro ve una tarjeta con **Accept** y **Reject** y si acepta, Velox la escribe en su
  terminal.
- **Modo "Control total" (opt-in mutuo entre compañeros):** pensado para trabajar en equipo cuando
  uno no está frente al teclado (por ejemplo, responder permisos de Claude/agy o mandar tareas sin
  esperar Accept):
  - **Cómo activarlo:** en **Ajustes (`Ctrl+,`) → Team**, cada compañero tiene un interruptor
    **"Control total"** (apagado por defecto). Solo entra en vigor cuando **ambos** lo encienden
    mutuamente (si solo uno lo activa, muestra *"Esperando a que \<nombre\> lo active"* y no cambia
    nada).
  - **Qué permite cuando está activo:**
    - Las tareas (`team_send_task` y **Send task** en la UI) se entregan **directo** sin tarjeta de
      aprobación, quedan registradas en el historial visible del popover de Team y muestran un toast
      (`"Benjamin → velox 1: …"`). Siguen saneadas igual que siempre.
    - La vista remota pasa a ser **interactiva**: tu teclado controla directo las sesiones
      compartidas del otro (incluyendo Enter, flechas, Esc y `Ctrl+C`).
    - Si la sesión compartida destino está dormida, Velox la despierta automáticamente antes de
      entregar la tarea (igual que en mensajería local).
  - **Límites estrictos:** solo aplica a sesiones que el dueño compartió con el botón de transmisión
    (lo no compartido sigue invisible e inaccesible). Mientras está activo aparece siempre un
    indicador en la barra de título (**"\<nombre\> tiene control total"**). Apagar el interruptor en
    **cualquiera** de los dos lados corta el acceso al instante (también en el backend), incluso con
    una vista remota abierta.
- **Mensajería directa entre sesiones del mismo PC (`velox list` / `velox msg` / `velox read`):**
  Velox inyecta `velox` en el `PATH` de cada pestaña y registra el MCP `velox-team` (`session_list`,
  `session_send`, `team_list`, `team_send_task`, `team_task_status`, `team_read_session`) al
  arrancar. Los agentes locales pueden hablarse directo sin aprobación manual, despertando sesiones
  dormidas si hace falta.
- **Que los Claude o agy se coordinen entre PCs:** el agente líder usa `team_list`, `team_send_task`,
  `team_task_status` y `team_read_session`. Sin "Control total", cada tarea remota requiere Accept;
  con "Control total" mutuo activo, se entrega directo a las sesiones compartidas.

## Si algo no conecta

| Síntoma | Revisa |
|---|---|
| "Tailscale not detected" en Ajustes | Tailscale abierto y conectado (ícono en la bandeja). |
| El otro aparece **Offline** | `tailscale ping <nombre>`; mismo token; mismo puerto; Velox abierto en su PC. |
| Conecta pero no ves sesiones | El otro no compartió ninguna (botón de transmisión). |
| Rechazado (401) | Los tokens no son iguales. |
