# Velox

Lightweight Rust Agentic Development Environment (ADE) desktop terminal built with **Tauri v2**, **ConPTY (`portable-pty`)**, **Windows Job Objects**, **Lazy Session Hibernation**, and an **embedded Scale-to-Zero Lazy MCP Gateway**.

## Key Features

- **One-click `+` Agent Launcher (`Ctrl+N`)**: Instantly launch Claude Code, Antigravity (`agy`), Codex, OpenCode, or PowerShell in any project directory.
- **Detects your agents on first launch**: installed CLIs are found on `PATH` / npm, every Claude profile (`~/.claude`, or each `~/.claude-<name>` used with `CLAUDE_CONFIG_DIR`) and every Antigravity account profile becomes its own launcher entry, and past conversations are resumed per folder.
- **Lazy-Restore & Hibernation (Sleep / Wake)**: Persisted sessions appear in the sidebar on startup at `0 MB` RAM and wake on click via each agent's native resume flag (`--resume`, `--conversation`, `--session`). Idle sessions auto-hibernate cleanly.
- **Zero Orphan Processes**: Every PTY and MCP backend runs inside a Windows Job Object (`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`) so closing or hibernating a session immediately cleans up its entire process tree.
- **Embedded Scale-to-Zero Lazy MCP Gateway**: Native Rust Streamable HTTP MCP gateway (`127.0.0.1:43111`) with schema caching and per-backend idle teardown, plus live RAM/MCP drawer controls.
- **Bounded Scrollback**: Fixed-capacity ring buffer (default `2 MB` per pane) with GPU-accelerated `xterm.js` WebGL rendering.

## Download (Windows)

Grab `velox.exe` from the [latest release](../../releases/latest) and run it (Windows 10/11 with the WebView2 runtime, which ships with Windows 11). Install the agent CLIs you use (`claude`, `agy`, `codex`, `opencode`) beforehand; Velox picks them up automatically.

## Shortcuts

| Keys | Action |
|---|---|
| `Ctrl+K` (or `Ctrl+Shift+P`) | Command palette (jump to session, launch agent, run actions) |
| `Ctrl+N` (or `Ctrl+Shift+N`) | New agent session |
| `Ctrl+1` … `Ctrl+9` | Switch to session 1–9 |
| `Ctrl+Shift+B` | Toggle sidebar |
| `Ctrl+Shift+M` | MCP gateway drawer |
| `Ctrl+Shift+H` | Sleep the current session |
| `Ctrl+=` / `Ctrl+-` / `Ctrl+0` | Zoom in / out / reset terminal font size |
| `Ctrl+,` | Settings |
| `Ctrl+/` | Keyboard shortcuts |
| `Ctrl+W` | Close session (outside the terminal) |
| `Ctrl+Shift+C` / `Ctrl+V` | Copy / paste (images too) |

## Development

```bash
pnpm install
pnpm tauri dev
```

## Build

```bash
pnpm build
cd src-tauri && cargo test && cargo build --release --features custom-protocol
```
