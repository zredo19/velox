# Lane B — `velox-team` MCP server, safe task delivery, activity summaries, scripts & docs

**Read first:** `docs/online/PLAN.md` (whole file), `docs/ARCHITECTURE.md` (Rust standards),
`src-tauri/src/team/api.rs` (the `TeamApi` trait you build on), `src-tauri/src/team/protocol.rs`,
`src-tauri/src/mcp/builtin.rs`, `src-tauri/src/mcp/gateway.rs` (how builtins are routed),
`src-tauri/src/session/detector.rs` (transcript discovery you will reuse).
Skills: `.agents/skills/rust-best-practices/SKILL.md`, `.agents/skills/rust-async-patterns/SKILL.md`.

**Work ONLY in:** `D:\Workspace\velox-online\lane-b` (git worktree, branch `online/lane-b`).
Never edit `D:\Workspace\velox` (the main checkout) or the other lanes' worktrees.

## Owned files (edit only these)

- `src-tauri/src/team/mcp.rs`, `team/delivery.rs`, `team/summary.rs`
- `src-tauri/src/session/detector.rs` — ONLY to make `find_claude_transcript` and
  `claude_config_bases` `pub` (no behaviour change)
- `scripts/register-velox-team-mcp.ps1` (new), `scripts/velox-test-instance.ps1` (new)
- `docs/online/MCP-TOOLS.md` (new), `docs/online/TESTING.md` (new)

Do NOT edit: `team/protocol.rs`, `team/api.rs`, `team/hub.rs`, `team/server.rs`, `team/client.rs`,
`domain/*`, `mcp/*`, `lib.rs`, `commands/*`, `session/manager.rs`, `src/` (frontend), `Cargo.toml`.
Lane A implements `TeamHub` (the real `TeamApi`) in parallel and wires your `TeamMcp` in `lib.rs`;
keep the existing public signatures of your three files exactly as they are now.

## Requirements

### 1. `team/mcp.rs` — `TeamMcp` (implements `BuiltinMcp`)
JSON-RPC 2.0 over the gateway (one POST body in, one JSON body out; `None` for notifications):
- `initialize` → `{protocolVersion: <client's requested version, or "2025-06-18">, capabilities: {tools: {listChanged: false}}, serverInfo: {name: "velox-team", version: env!("CARGO_PKG_VERSION")}, instructions: "<short how-to>"}`
- `notifications/*` → `None`; `ping` → `{}`; `tools/list`; `tools/call`; unknown method → error `-32601`;
  malformed request → `-32600`. Batches (JSON arrays) → error `-32600` (not supported).
- Tools (names, JSON Schemas and descriptions written for an LLM "leader" agent; descriptions must say
  that **a human on the receiving machine must accept every task, nothing runs automatically**):
  - `team_list` `{}` → this machine (display name, listen addr), each peer (online/offline, display name)
    with its shared sessions (title, id, agent, status, folder, summary), and this machine's live sessions
    (marked shared or not). Text content optimized for reading + `structuredContent` with the raw `TeamSnapshot`.
  - `team_send_task` `{peer?: string, session: string, text: string, from_session?: string}` —
    `peer` omitted, `""` or `"local"` = this machine. `session` = id or title (case-insensitive; exact id
    wins; exact title next; unique prefix last; ambiguous or unknown → tool error listing the candidates).
    Validate text non-empty and ≤ `MAX_TASK_TEXT_BYTES`. Returns the ticket (`request_id`, state) and a
    sentence like "Waiting for Benjamin to accept in Velox".
  - `team_task_status` `{request_id: string}` → state + message (+ hint to poll again while pending).
  - `team_read_session` `{peer?: string, session: string, lines?: integer (default 60, max MAX_READ_LINES)}`
    → the text. Read-only.
- Tool failures are returned as `{content: [{type: "text", text: "..."}], isError: true}`, never as
  JSON-RPC errors. Unknown tool → JSON-RPC `-32602`.
- Unit tests with a `FakeTeam` implementing `TeamApi`: initialize, tools/list (4 tools, valid schemas),
  every tool's happy path, session resolution (id / title / prefix / ambiguous / unknown), validation errors,
  notification returns `None`.

### 2. `team/delivery.rs` — `build_task_input(kind, text)`
Text typed into a TUI must arrive as ONE message and then be submitted, and must not be able to inject keys:
- Normalize `\r\n` and `\r` to `\n`; trim trailing whitespace.
- Remove every byte/char that is ESC (0x1B), any other C0 control except `\n` and `\t`, DEL (0x7F) and
  C1 controls (U+0080–U+009F). This blocks escaping out of bracketed paste.
- Claude / Agy / Codex / OpenCode: chunks `["\x1b[200~" + text + "\x1b[201~", "\r"]`.
- Shell: newlines → `" "`, chunks `[text, "\r"]`.
- Empty after cleaning → `vec![]` (caller treats as failure).
- Keep `CHUNK_DELAY_MS` (150). Unit tests for every rule above, including an injection attempt
  (`"hi\x1b[201~\r rm -rf"` must not contain ESC in the output).

### 3. `team/summary.rs` — `last_action_summary(kind, profile, agent_session_id)`
- Claude: find the transcript (`find_claude_transcript(profile, id)` from detector, made `pub`), read only
  the last 64 KB, scan JSONL lines from the end for the newest `assistant` message and summarize:
  `tool_use` → by tool: `Edit`/`Write`/`MultiEdit`/`NotebookEdit` → `Edit <file name or last 2 path parts>`;
  `Read` → `Read <file>`; `Bash`/`PowerShell` → `Bash: <command, first 50 chars>`;
  `Grep`/`Glob` → `Search <pattern>`; `Agent`/`Task` → `Agent: <description>`;
  `mcp__<server>__<tool>` → `<server>: <tool>`; other → tool name.
  Text block → its first non-empty line. Max 80 chars, cut on a char boundary with `…`.
  Skip `isSidechain` entries and thinking blocks.
- Agy: look for a cheap source in `~/.gemini/antigravity-cli/` (e.g. the latest `history.jsonl` entry
  whose conversation matches `agent_session_id` → `Prompt: <first 60 chars>`). If nothing reliable, `None`.
- Other kinds → `None`. Never panic, never read whole large files. Tests with temp JSONL fixtures
  (write a helper that takes a path, so tests don't touch the real home dir).

### 4. `scripts/register-velox-team-mcp.ps1`
Idempotently registers `velox-team` → `http://127.0.0.1:43111/mcp/velox-team` for:
- every Claude profile dir that exists (`~/.claude`, `~/.claude-personal`, `~/.claude-trabajo`, any
  `~/.claude-*` with a `.claude.json`): run `claude mcp add --transport http --scope user velox-team <url>`
  with `$env:CLAUDE_CONFIG_DIR` set to that dir (unset for `~/.claude`); skip if `claude mcp get velox-team` succeeds;
- Antigravity: `~/.gemini/antigravity-cli/mcp_config.json` and any `mcp_config.json` under
  `~/.gemini/antigravity-cli/profiles/*/` — add `"velox-team": {"serverUrl": "<url>"}` under `mcpServers`,
  keeping every other key, after writing a `.bak-velox-team` backup. Inspect the real files first.
- `-DryRun` switch prints what would change and touches nothing. `-Port` parameter (default 43111).
- You may run it ONLY with `-DryRun`. The coordinator runs it for real.

### 5. `scripts/velox-test-instance.ps1`
Starts a second, isolated Velox for end-to-end tests on one PC:
params `-Exe <path to velox.exe>`, `-Token <string>`, `-Name TestB`, `-ConfigDir "$env:TEMP\velox-test-b"`.
Writes `config.toml` there (only if missing, unless `-Reset`): `mcpGatewayPort = 43211`,
`lazyRestoreOnStartup = false`, `autoHibernateMinutes = 0`, and `[team]` with `enabled = true`,
`displayName = <Name>`, `listenPort = 43220`, `bindHost = "127.0.0.1"`, `token = <Token>`,
and one peer `{name = "Main", host = "127.0.0.1", port = 43120}`. Then launches the exe with
`$env:VELOX_CONFIG_DIR` set. Print how to point the main instance at it
(peer `{name="TestB", host="127.0.0.1", port=43220}` and `bindHost = "127.0.0.1"`).
Do not run it (there is no built exe yet); validate it with the PowerShell parser
(`[System.Management.Automation.Language.Parser]::ParseFile(...)`).

### 6. Docs
- `docs/online/MCP-TOOLS.md`: each tool, arguments, example calls and outputs, the approval rule.
- `docs/online/TESTING.md`: step-by-step end-to-end checklist (single PC with the test instance, then
  two PCs over Tailscale): share a session, see it in the other sidebar, open the mirror, resize the source,
  send a task from the UI and from an agent via MCP, accept/reject, unshare → mirror closes, wrong token → refused.

## Validation (run from `src-tauri`)
`cargo check --all-targets`, `cargo test`, `cargo clippy --all-targets -- -D warnings` — all green.
No `.unwrap()`/`.expect()` outside tests.

## Working rules
- Commit early and often on `online/lane-b` (small, clear messages). Do not push. Do not merge.
- After each milestone, update `D:\Workspace\velox-online\status\lane-b.md` (template inside).
- Blocked on a contract? Write it under "Questions" in the status file, pick the safest workaround, continue.
- Do not launch `velox.exe` or `pnpm tauri dev`.
- When everything is green: set status to `DONE`, list what was built and anything left undone.
