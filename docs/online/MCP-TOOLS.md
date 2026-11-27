# `velox-team` MCP Tools

Velox embeds an in-process Streamable HTTP MCP server named `velox-team` inside the Lazy MCP Gateway at:

```
http://127.0.0.1:43111/mcp/velox-team
```

(Replace `43111` with your configured `mcpGatewayPort` if customized.)

## Core Rule: Human-in-the-Loop Approval

**A human on the receiving machine must accept every task; nothing runs automatically.**

- Sending a task via `team_send_task` creates a `TaskRequest` in `pending` state.
- On the receiving Velox instance, the human sees an **Accept / Reject** card in the Team Task Inbox.
- Only after the human clicks **Accept** does Velox sanitize the text (stripping ESC/C0/DEL/C1 control sequences) and type it into the target terminal as bracketed paste followed by Enter (`\r`).
- If `team.autoApproveLocal = true` is configured, only tasks originating on the **same machine** are delivered immediately without prompting. Remote tasks from peers are **never** auto-accepted.
- Remote peers can only list, read, and send tasks to sessions marked `shared = true`.

## Registration

Register `velox-team` for all local Claude Code profiles and Antigravity CLI configs with:

```powershell
.\scripts\register-velox-team-mcp.ps1 -DryRun
.\scripts\register-velox-team-mcp.ps1
```

## Protocol Notes

- Transport: HTTP `POST` to `/mcp/velox-team` with JSON-RPC 2.0 body.
- Notifications (`notifications/*`, or any message without `id`) return `202 Accepted` (`None` from handler).
- Batch requests (JSON arrays) are not supported and return `-32600` (`Invalid Request`).
- Unknown tools return JSON-RPC `-32602` (`Invalid params`).
- Tool validation or execution failures return `{ "content": [{ "type": "text", "text": "..." }], "isError": true }`.

---

## Tools

### 1. `team_list`

Lists this machine's team status, live local sessions (marked `[shared]` or `[not shared]`), and every configured teammate with their online/offline state and shared sessions.

#### Arguments

None (`{}`).

#### Example `tools/call`

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "tools/call",
  "params": {
    "name": "team_list",
    "arguments": {}
  }
}
```

#### Example Output

```json
{
  "content": [
    {
      "type": "text",
      "text": "This machine: Rodolfo | team=enabled | listen=100.64.0.10:43120\nTailscale IP: 100.64.0.10\n\nLocal live sessions (1):\n    - [shared] \"Velox Lider\" (id=local-1111, agent=Claude Code, status=Idle, folder=velox) — Read team/api.rs\n\nPeers (1):\n  * Benjamin [online] (benjamin:43120)\n      - \"API Server\" (id=rem-aaaa, agent=Claude Code, status=Idle, folder=api) — Edit routes.rs"
    }
  ],
  "structuredContent": {
    "enabled": true,
    "displayName": "Rodolfo",
    "listenAddr": "100.64.0.10:43120",
    "peers": [ ... ],
    "localSessions": [ ... ],
    "watchers": []
  },
  "isError": false
}
```

---

### 2. `team_send_task`

Sends a task request to a teammate's shared session or a live session on this machine. **A human on the receiving machine must accept every task; nothing runs automatically.**

#### Arguments

| Name | Type | Required | Description |
|---|---|---|---|
| `peer` | `string` | No | Teammate name from `team_list`. Omit, pass `""`, or pass `"local"` to target this machine. |
| `session` | `string` | Yes | Target session ID or title (case-insensitive: exact ID wins, then exact title, then unique prefix). |
| `text` | `string` | Yes | Task instructions to type into the target session after human approval (non-empty, max `16384` bytes). |
| `from_session` | `string` | No | Optional title of the requesting session, shown on the receiver's approval card. |

#### Example `tools/call`

```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "method": "tools/call",
  "params": {
    "name": "team_send_task",
    "arguments": {
      "peer": "Benjamin",
      "session": "API Server",
      "text": "Run cargo test in src-tauri and report any failures.",
      "from_session": "Velox Lider"
    }
  }
}
```

#### Example Output

```json
{
  "content": [
    {
      "type": "text",
      "text": "Task queued (request_id: req-1, state: pending). Waiting for Benjamin to accept in Velox. Poll team_task_status with this request_id to check progress."
    }
  ],
  "structuredContent": {
    "requestId": "req-1",
    "direction": "outgoing",
    "peer": "Benjamin",
    "sessionId": "rem-aaaa",
    "sessionTitle": "API Server",
    "fromName": "Rodolfo",
    "fromSession": "Velox Lider",
    "text": "Run cargo test in src-tauri and report any failures.",
    "state": "pending",
    "message": null,
    "createdAt": "2026-11-26T04:00:00Z",
    "updatedAt": "2026-11-26T04:00:00Z"
  },
  "isError": false
}
```

If `session` matches multiple sessions or none, the tool returns `isError: true` with the candidate titles and IDs.

---

### 3. `team_task_status`

Polls the current state of a task request created by `team_send_task`.

#### Arguments

| Name | Type | Required | Description |
|---|---|---|---|
| `request_id` | `string` | Yes | Ticket `request_id` returned by `team_send_task`. |

#### Task States

- `pending`: Waiting for the human on the receiving machine to accept or reject.
- `accepted`: Accepted by the human; being typed into the PTY.
- `delivered`: Typed into the target terminal and submitted.
- `rejected`: Rejected by the human on the receiving machine.
- `failed`: Delivery failed (session asleep/closed, unshared, or PTY write error).
- `expired`: Timed out after 30 minutes without a response.

#### Example `tools/call`

```json
{
  "jsonrpc": "2.0",
  "id": 3,
  "method": "tools/call",
  "params": {
    "name": "team_task_status",
    "arguments": {
      "request_id": "req-1"
    }
  }
}
```

#### Example Output

```json
{
  "content": [
    {
      "type": "text",
      "text": "Task req-1 for session \"API Server\": state=delivered — Typed into terminal"
    }
  ],
  "structuredContent": {
    "requestId": "req-1",
    "state": "delivered",
    "message": "Typed into terminal"
  },
  "isError": false
}
```

---

### 4. `team_read_session`

Read-only inspection of the tail of a session's terminal output (ANSI escape sequences stripped). Remote peers only allow reading sessions marked `shared = true`.

#### Arguments

| Name | Type | Required | Description |
|---|---|---|---|
| `peer` | `string` | No | Teammate name from `team_list`. Omit, pass `""`, or pass `"local"` for this machine. |
| `session` | `string` | Yes | Session ID or title (case-insensitive: exact ID, exact title, or unique prefix). |
| `lines` | `integer` | No | Number of lines to read from the end (default `60`, max `400`). |

#### Example `tools/call`

```json
{
  "jsonrpc": "2.0",
  "id": 4,
  "method": "tools/call",
  "params": {
    "name": "team_read_session",
    "arguments": {
      "peer": "Benjamin",
      "session": "API Server",
      "lines": 40
    }
  }
}
```

#### Example Output

```json
{
  "content": [
    {
      "type": "text",
      "text": "test result: ok. 42 passed; 0 failed; 0 ignored"
    }
  ],
  "isError": false
}
```
