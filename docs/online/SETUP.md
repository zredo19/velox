# Velox Online: puesta en marcha (Rodolfo + Benjamín)

Unos 10 minutos entre los dos. Se hace una sola vez.

## 0. La versión nueva de Velox

**Rodolfo:** cierra Velox y ábrelo con el acceso directo del escritorio (`Velox.lnk`, que apunta a
`D:\Workspace\velox\src-tauri\target\release\velox.exe`, ya compilado con Velox Online). Tus
sesiones vuelven dormidas, como siempre; con un clic retoman su conversación. El MCP
`velox-team` ya quedó registrado en tus perfiles de Claude (`~/.claude`, `-personal`,
`-trabajo`) y en agy.

**Benjamín:** copia `velox.exe` (está en `D:\Workspace\velox-online\release\`) a su PC y ábrelo.
Para que sus agentes puedan usar las herramientas del equipo, en PowerShell:

```powershell
claude mcp add --transport http --scope user velox-team http://127.0.0.1:43111/mcp/velox-team
```

En agy, agregar en `%USERPROFILE%\.gemini\antigravity-cli\mcp_config.json`, dentro de `mcpServers`:
`"velox-team": { "serverUrl": "http://127.0.0.1:43111/mcp/velox-team" }`.

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
  clic ves su terminal en vivo, solo lectura.
- **Mandar una tarea a mano:** abre su sesión y escribe en la caja de abajo → **Send task**. El otro
  ve una tarjeta con **Accept** y **Reject**. Si acepta, Velox la escribe en su terminal.
- **Que los Claude o agy se coordinen solos:** cada agente tiene el MCP `velox-team`, con
  `team_list`, `team_send_task`, `team_task_status` y `team_read_session`. El agente líder reparte
  tareas con `team_send_task` y revisa el avance con `team_read_session`. Cada tarea que llega al
  otro PC la tiene que aceptar una persona; nunca se ejecuta sola.

- **Un líder coordinando agentes en tu mismo PC (como herdr):** `team_send_task` sin `peer`
  manda la tarea a una sesión local, y `team_read_session` sin `peer` lee su pantalla. Esto
  funciona aunque el modo equipo esté apagado. Para que las tareas locales se entreguen sin
  pedirte confirmación, activa en Ajustes → Team la opción **Auto-approve tasks from this PC**.
  Las tareas que llegan de otro PC siempre necesitan tu Accept.

## Si algo no conecta

| Síntoma | Revisa |
|---|---|
| "Tailscale not detected" en Ajustes | Tailscale abierto y conectado (ícono en la bandeja). |
| El otro aparece **Offline** | `tailscale ping <nombre>`; mismo token; mismo puerto; Velox abierto en su PC. |
| Conecta pero no ves sesiones | El otro no compartió ninguna (botón de transmisión). |
| Rechazado (401) | Los tokens no son iguales. |
