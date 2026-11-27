# Velox

Lightweight Rust Agentic Development Environment (ADE) desktop terminal built with **Tauri v2**, **ConPTY (`portable-pty`)**, **Windows Job Objects**, **Lazy Session Hibernation**, and an **embedded Scale-to-Zero Lazy MCP Gateway**.

## Key Features

- **One-click `+` Agent Launcher (`Ctrl+N`)**: Instantly launch Claude Code, Antigravity (`agy`), Codex, OpenCode, or PowerShell in any project directory.
- **Detects your agents on first launch**: installed CLIs are found on `PATH` / npm, every Claude profile (`~/.claude`, or each `~/.claude-<name>` used with `CLAUDE_CONFIG_DIR`) and every Antigravity account profile becomes its own launcher entry, and past conversations are resumed per folder.
- **Lazy-Restore & Hibernation (Sleep / Wake)**: Persisted sessions appear in the sidebar on startup at `0 MB` RAM and wake on click via each agent's native resume flag (`--resume`, `--conversation`, `--session`). Idle sessions auto-hibernate cleanly.
- **Zero Orphan Processes**: Every PTY and MCP backend runs inside a Windows Job Object (`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`) so closing or hibernating a session immediately cleans up its entire process tree.
- **Embedded Scale-to-Zero Lazy MCP Gateway**: Native Rust Streamable HTTP MCP gateway (`127.0.0.1:43111`) with schema caching and per-backend idle teardown, plus live RAM/MCP drawer controls.
- **Bounded Scrollback**: Fixed-capacity ring buffer (default `2 MB` per pane) with GPU-accelerated `xterm.js` WebGL rendering.
- **Velox Online (team) & Mutual "Control total"**: Share sessions with teammates over Tailscale. Shared tabs appear in the other Velox's sidebar with a live terminal mirror (read-only by default) and Accept/Reject task cards. For trusted pairing when one teammate is away, both sides can opt into **Control total** per peer in **Settings → Team** (off by default, requires mutual activation): tasks deliver directly with toast + visible history (waking sleeping shared sessions), the remote terminal becomes interactive (Enter, arrows, `Ctrl+C`), an always-visible titlebar badge shows who has full control, and turning it off on either side cuts access immediately (enforced in the backend; unshared tabs stay invisible and inaccessible). Setup: [`docs/online/SETUP.md`](docs/online/SETUP.md) · design: [`docs/online/PLAN.md`](docs/online/PLAN.md).
- **Direct Local Session Messaging & Auto-Setup**: On startup, Velox automatically registers the built-in `velox-team` MCP server (`session_list`, `session_send`, `team_list`, `team_send_task`, `team_task_status`, `team_read_session`) across Claude Code and Antigravity CLI configs and installs the `velox` CLI (`velox list`, `velox msg`, `velox read`) onto each tab's `PATH`.
- **Deterministic Session Persistence**: Claude `SessionStart` and Antigravity `PreInvocation` hooks link each tab deterministically without cross-tab mixups, backed by atomic `.bak` persistence and Windows shutdown capture (`WM_QUERYENDSESSION` / `WM_ENDSESSION`).
- **Custom Animated Themes, Command Palette & Polish**: 7 animated custom themes (`Ctrl+Shift+T`), authentic agent scenes/icons, Command Palette (`Ctrl+K`), keyboard shortcuts modal (`Ctrl+/`), live font zoom, and code-split lazy overlays.

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
| `Ctrl+Shift+T` | Themes & animated wallpapers gallery |
| `Ctrl+Shift+H` | Sleep the current session |
| `Ctrl+=` / `Ctrl+-` / `Ctrl+0` | Zoom in / out / reset terminal font size |
| `Ctrl+,` | Settings |
| `Ctrl+/` | Keyboard shortcuts |
| `Ctrl+W` | Close session (outside the terminal) |
| `Ctrl+Shift+C` / `Ctrl+V` | Copy / paste (images too) |

## Development / Updating

To pull the latest changes and run Velox locally (the `velox-team` MCP server and the `velox` CLI command register themselves automatically on startup):

```bash
git pull
pnpm install
pnpm tauri dev
```

## Build

```bash
git pull
pnpm install
pnpm tauri build
```

Or build the frontend and embedded Rust release executable manually:

```bash
pnpm build
cd src-tauri && cargo test && cargo build --release --features custom-protocol
```

> **Windows MinGW/GNU note:** `rust-toolchain.toml` uses `channel = "stable"` so MSVC and GNU setups both work. If your machine uses MinGW/WinLibs instead of Visual Studio Build Tools, run `rustup default stable-x86_64-pc-windows-gnu` (or set `RUSTUP_TOOLCHAIN=stable-x86_64-pc-windows-gnu`).
