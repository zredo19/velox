# Velox Online — End-to-End Testing Checklist

This checklist verifies Velox Online first on a **single PC** (using `127.0.0.1` and an isolated `VELOX_CONFIG_DIR` test instance) and then across **two PCs over Tailscale**.

---

## Part 1: Single-PC Verification (Main + Isolated `TestB`)

### Prerequisites

- Release or debug `velox.exe` built.
- A shared 64-hex test token (generate in Velox Settings → Team, or use a test string such as `0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef`).
- Register the MCP server once if testing agent-to-agent calls:
  ```powershell
  .\scripts\register-velox-team-mcp.ps1
  ```

### Step 1 — Configure Main Instance & Launch `TestB`

1. In your main `%APPDATA%\velox\config.toml`, configure `[team]`:
   ```toml
   [team]
   enabled = true
   displayName = "Main"
   listenPort = 43120
   bindHost = "127.0.0.1"
   token = "<SHARED_TOKEN>"

   [[team.peers]]
   name = "TestB"
   host = "127.0.0.1"
   port = 43220
   ```
2. Launch the main `velox.exe`.
3. Launch the second isolated instance:
   ```powershell
   .\scripts\velox-test-instance.ps1 -Exe <path-to-velox.exe> -Token "<SHARED_TOKEN>" -Reset
   ```
   `TestB` starts with `VELOX_CONFIG_DIR=$env:TEMP\velox-test-b`, MCP gateway on `43211`, and team server on `127.0.0.1:43220`.

### Step 2 — Share a Session & Verify Sidebar Discovery

1. In `TestB`, start a Claude or PowerShell session.
2. Verify it does **not** appear in `Main`'s sidebar yet (sharing is opt-in, off by default).
3. In `TestB`, toggle **Share** (`shared = true`) on that session.
4. **Expected**: Within ~1 second, `TestB` shows as `online` in `Main`'s sidebar and lists the shared session with its title, agent badge, status, folder basename, and latest-action summary.

### Step 3 — Live Read-Only Mirror & Resize

1. In `Main`, click `TestB`'s shared session to open the remote mirror.
2. **Expected**: Existing scrollback loads via `team-pty-snapshot`, and new terminal output in `TestB` streams live to `Main`.
3. Try typing or pasting inside the mirror terminal on `Main`.
4. **Expected**: Input is ignored (remote control is strictly disabled).
5. Resize the `TestB` window / terminal pane so its PTY `cols`/`rows` change.
6. **Expected**: `Main`'s mirror resizes (`team-pty-resize`) to match the source's exact `cols` and `rows`, scaled to fit.

### Step 4 — Send Task from UI (Accept & Reject)

1. From `Main`, use **Send Task** on `TestB`'s shared session with a safe multi-line prompt or command (e.g., `echo hello from Main`).
2. **Expected**: Nothing runs automatically on `TestB`. `Main` shows the outgoing ticket as `Pending`; `TestB` shows an incoming **Accept / Reject** card.
3. Click **Reject** on `TestB`.
4. **Expected**: Both instances update the ticket to `Rejected`, and nothing is typed into `TestB`'s terminal.
5. Send another task from `Main` and click **Accept** on `TestB`.
6. **Expected**: Ticket moves to `Accepted` → `Delivered`, and the text is typed as a single bracketed-paste message (or single line for Shell) followed by Enter (`\r`) in `TestB`'s target session.

### Step 5 — Send Task & Read Output via MCP (`velox-team`)

1. Open an agent session (Claude Code or Antigravity) on `Main` with `velox-team` registered.
2. Ask the agent to call `team_list`.
   - **Expected**: Output includes `Main` local sessions (`[shared]` / `[not shared]`) and `TestB`'s shared session.
3. Ask the agent to call `team_read_session` for `peer: "TestB"`.
   - **Expected**: Returns ANSI-stripped tail lines of `TestB`'s shared session.
4. Ask the agent to call `team_send_task` targeting `peer: "TestB"`.
   - **Expected**: Returns `request_id` with state `pending` and a message stating it is waiting for `TestB` to accept in Velox.
5. Poll `team_task_status` while pending, then click **Accept** in `TestB`, and poll `team_task_status` again.
   - **Expected**: State transitions from `pending` to `delivered`.

### Step 6 — Unshare Closes Mirror

1. While `Main` has the remote mirror open for `TestB`'s session, toggle **Share** off (`shared = false`) in `TestB`.
2. **Expected**: `Main` receives `team-session-gone`, closes the mirror cleanly, and removes the session from `TestB`'s peer list. Subsequent `team_read_session` or `team_send_task` calls targeting that session fail with an unshared/not-found error.

### Step 7 — Wrong Token Is Refused

1. In `TestB`, change `[team].token` to a different value (or restart `TestB` with `-Token "wrong-token" -Reset`).
2. **Expected**:
   - WebSocket upgrade returns HTTP `401 Unauthorized`.
   - `Main` marks `TestB` as `offline` with a clear `lastError`, and exposes zero sessions from `TestB`.
   - Empty token prevents the team server from starting and prevents clients from dialing.

---

## Part 2: Two-PC Verification Over Tailscale

1. Ensure both PCs (e.g., Rodolfo and Benjamin) are on the same Tailscale tailnet and the Tailscale ACL permits TCP port `43120` between them.
2. On both PCs, configure `%APPDATA%\velox\config.toml`:
   - Leave `bindHost = ""` (auto-detects the `100.64.0.0/10` Tailscale IPv4; never binds `0.0.0.0`).
   - Set the identical 64-hex `token` on both PCs.
   - Set each PC's `[[team.peers]]` to the teammate's Tailscale MagicDNS name (e.g., `host = "benjamin"`) or `100.x.y.z` IP on port `43120`.
3. Repeat Steps 2–7 across both physical machines:
   - Verify `listenAddr` displays `<tailscale-ip>:43120`.
   - Share a Claude Code session on PC A; verify PC B sees its live one-line `summary` in the sidebar.
   - Open the mirror on PC B, resize PC A, send a task via MCP from PC B, accept on PC A, unshare, and verify wrong-token rejection.
