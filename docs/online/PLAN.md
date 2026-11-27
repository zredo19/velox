# Velox Online — plan

Goal for 2026-11-26: two teammates (Rodolfo and Benjamin), each on their own PC with Velox,
see each other's **shared** agent sessions live in the Velox sidebar (labeled with the
teammate's name), and agents can send each other **task requests** that the receiving human
approves with one click. Sessions of the same machine can be addressed the same way.

## Decisions (already taken, do not revisit)

| Topic | Decision |
|---|---|
| Network | **Tailscale**, direct Velox-to-Velox. No relay server, no cloud database. |
| Transport | One WebSocket per direction: each Velox runs a team server and dials every configured peer. |
| Auth | Tailscale (device-level) + a shared **team token** (`Authorization: Bearer`) checked on every connection. |
| Bind address | Only the Tailscale IPv4 (100.64.0.0/10) or an explicit `bindHost` (127.0.0.1 for tests). **Never 0.0.0.0.** |
| What is shared | Only sessions with `shared = true` (opt-in per session, off by default). |
| Remote view | Read-only mirror of the PTY byte stream, rendered at the **source's cols/rows**, scaled to fit. Streamed only while someone has it open. |
| Sidebar summary | One-line "latest action" from the Claude transcript (cheap, always on). |
| Remote control | **None.** Viewers cannot type into a remote terminal. |
| Tasks | Sender (human UI or agent via MCP) sends a `TaskRequest`; the receiving Velox shows an Accept/Reject card; on Accept, Velox types the text into the target terminal (bracketed paste + Enter). |
| Agent access | Built-in MCP server `velox-team` served by the embedded gateway at `http://127.0.0.1:43111/mcp/velox-team`. |
| Secret filtering | Not done on the raw stream (unreliable). Protection = opt-in sharing + Tailscale + token. |

## Architecture

```
            Rodolfo's PC                                   Benjamin's PC
┌──────────────────────────────────┐          ┌──────────────────────────────────┐
│ Velox                            │          │ Velox                            │
│  SessionManager ──tap──► TeamHub │          │ TeamHub ◄──tap── SessionManager  │
│                    │      │  │   │  ws (TS) │   │  │      │                    │
│                    │  server◄─┼───┼──────────┼───┼─client   │                    │
│                    │      │ client──────────┼──►server     │                    │
│  gateway /mcp/velox-team ─┘      │          │      └─ gateway /mcp/velox-team   │
│  React: sidebar team section,    │          │                                  │
│  remote mirror, task inbox       │          │                                  │
└──────────────────────────────────┘          └──────────────────────────────────┘
```

* `src-tauri/src/team/protocol.rs` — wire messages (`ServerMsg`, `ClientMsg`). **Contract.**
* `src-tauri/src/team/api.rs` — `TeamApi` trait, `TeamSnapshot`, `TaskTicket`, Tauri event payloads. **Contract.**
* `src-tauri/src/team/hub.rs` — `TeamHub`: config, local shared sessions, peer clients, tasks; implements `TeamApi`.
* `src-tauri/src/team/server.rs` / `client.rs` — axum WebSocket server, tokio-tungstenite clients.
* `src-tauri/src/team/mcp.rs` — `velox-team` MCP tools over `TeamApi`.
* `src-tauri/src/team/delivery.rs` — safe bytes for typing a task into a TUI.
* `src-tauri/src/team/summary.rs` — latest-action line from transcripts.
* `src-tauri/src/mcp/builtin.rs` — `BuiltinMcp` trait; gateway routes `/mcp/{name}` to builtins first (done).
* `src-tauri/src/domain/config.rs` — `TeamConfig` (`[team]` in config.toml); `VELOX_CONFIG_DIR` override (done).
* `src-tauri/src/domain/session.rs` — `SessionRecord.shared` (done).
* `src/types/ipc.ts`, `src/lib/tauri.ts` — frontend contract (done).

### Config (`%APPDATA%/velox/config.toml`)

```toml
[team]
enabled = true
displayName = "Rodolfo"
listenPort = 43120
bindHost = ""            # auto = Tailscale IP
token = "<same secret on both PCs>"
autoApproveLocal = false

[[team.peers]]
name = "Benjamin"
host = "benjamin"        # Tailscale MagicDNS name or 100.x IP
port = 43120
```

### Tauri IPC (all in `commands/team.rs` unless noted)

| Command | Args | Returns |
|---|---|---|
| `get_team_status` | – | `TeamSnapshot` |
| `set_session_shared` | `session_id, shared` | `SessionRecord` |
| `team_subscribe` | `peer, session_id` | `()` (then `team-pty-snapshot` event) |
| `team_unsubscribe` | `peer, session_id` | `()` |
| `send_team_task` | `peer: Option<String>, session_id, text` | `TaskTicket` |
| `list_team_tasks` | – | `Vec<TaskTicket>` (newest first, max 50) |
| `respond_team_task` | `request_id, accept` | `TaskTicket` |
| `generate_team_token` | – | `String` (64 hex chars) |
| `update_config` (existing) | `config` | also applies `config.team` live (restart server/clients if changed) |

Events: `team-updated` (TeamSnapshot), `team-tasks-updated` (TaskTicket[]), `team-pty-output`,
`team-pty-snapshot`, `team-pty-resize`, `team-session-gone`. Payloads in `team/api.rs` and `ipc.ts`.

### MCP tools (`velox-team`)

| Tool | Purpose |
|---|---|
| `team_list` | This machine, peers (online?), their shared sessions (id, title, agent, status, summary), local sessions. |
| `team_send_task` | `{peer?, session, text, from_session?}` → ticket. `peer` omitted/"local" = this machine. Human must accept. |
| `team_task_status` | `{request_id}` → state/message. |
| `team_read_session` | `{peer?, session, lines?}` → current screen text (read-only). |

### Security rules (non-negotiable)

1. Never bind 0.0.0.0. No Tailscale IP and no `bindHost` → server does not start (`serverError` explains).
2. Empty token → server does not start and clients do not dial.
3. Constant-time token comparison. 401 before upgrading.
4. Only `shared` sessions are listed, mirrored, readable or targetable by remote peers.
5. Task text: max 16 KB; all ESC/C0 control bytes stripped (except `\n`, `\t`) before typing.
6. Never auto-accept remote tasks. `autoApproveLocal` affects only tasks from this machine.
7. Remote frames > 64 KB are rejected; max 20 task requests per minute per connection.
8. RemoteSessionInfo never exposes full paths, env, pids or tokens.

## Lanes (parallel, isolated git worktrees)

| Lane | Session in Velox | Worktree / branch | Brief |
|---|---|---|---|
| A — backend core | `Velox trabajo` | `D:\Workspace\velox-online\lane-a` / `online/lane-a` | `docs/tasks/online-a-backend.md` |
| B — MCP, delivery, summaries, scripts | `Velox trabajo 2` | `D:\Workspace\velox-online\lane-b` / `online/lane-b` | `docs/tasks/online-b-mcp.md` |
| C — frontend | `Velox trabajo 3` | `D:\Workspace\velox-online\lane-c` / `online/lane-c` | `docs/tasks/online-c-ui.md` |

Status files (outside git, update after every milestone): `D:\Workspace\velox-online\status\lane-{a,b,c}.md`.
The coordinator (Claude session `Velox Lider`) merges the three branches into `feat/velox-online`,
runs the end-to-end test with a second isolated instance (`VELOX_CONFIG_DIR`), builds the
release, registers the MCP server and writes the Tailscale ACL.

## Definition of done (tomorrow morning)

- `cargo test` + `cargo clippy --all-targets -- -D warnings` + `pnpm build` green on `feat/velox-online`.
- Two Velox instances on one PC (main + test instance on 127.0.0.1) see each other's shared sessions,
  mirror one live, send a task, accept it, and the text lands in the target agent.
- `velox.exe` release build ready to hand to Benjamin, plus `docs/online/SETUP.md` (Spanish) with
  the 10-minute setup for both PCs and the Tailscale ACL.
