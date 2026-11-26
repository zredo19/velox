# Lane 5 (`velox-ui-chrome`) — Visual Identity, Custom TitleBar, Lazy MCP Drawer, EmptyState & Bespoke Icons

**Model Tier:** Hy4 Flash (Focused Visual Chrome, SVG Icons & Styling Lane)
**Architecture & Design Spec:** Read `C:\Users\Asus\OneDrive\Escritorio\Proyectos\velox\docs\ARCHITECTURE.md`, `src/types/ipc.ts`, `src/lib/tauri.ts`, and the existing stub files in your lane.

## Owned Files (EDIT ONLY THESE FILES)
- `src/index.css`
- `src/components/icons/AgentIcons.tsx`
- `src/hooks/useSystemTelemetry.ts`
- `src/components/TitleBar.tsx`
- `src/components/McpDrawer.tsx`
- `src/components/EmptyState.tsx`

Do NOT modify `package.json`, `src/types/ipc.ts`, `src/lib/tauri.ts`, `src/App.tsx`, `src/components/Sidebar.tsx`, `src/components/TerminalPane.tsx`, `src/components/AgentLauncherModal.tsx`, or any Rust file.
Preserve ALL exported component/hook names and prop interfaces defined in the existing stub files so Lane 4 integrates seamlessly!

## Design Direction (Read Carefully — Anti-Slop Quality)
- Palette: Canvas Void `#0B0D12`, Chrome Surface `#11141C`, Elevated Popover `#171B26`, Hover `#1C2130`, Hairline Border `rgba(255,255,255,0.07)`, Primary Ink `#E6EAF2`, Secondary Ink `#98A2B8`, Muted Ink `#64708A`.
- Agent semantic colors: Claude `#D97757`, AGY `#4E8DF5`, Codex `#10A37F`, OpenCode `#C4B5FD`, Shell `#94A3B8`, Blocked `#F43F5E`.
- Typography: Inter / Segoe UI Variable for UI chrome, JetBrains Mono / Cascadia Code + `tabular-nums` for RAM and timer readouts.
- Clean sentence case for UI copy (avoid ALL-CAPS eyebrows, generic SaaS gradient boxes, or decorative clutter). Every visual border/badge must communicate real state.
- All UI copy must be in crisp, plain English.

## Exact Requirements

### 1. `src/index.css`
- `@import "tailwindcss";`
- CSS custom properties matching the Velox palette.
- Ultra-slim custom dark scrollbars (`6px`, subtle translucent thumb, transparent track).
- Clean `xterm.js` container sizing tweaks (`.xterm`, `.xterm-viewport` smooth scroll & dark `#0B0D12` background without ugly white flash).
- Subtle status pulse keyframe (`velox-pulse`) and drawer fade/slide animation (`120ms ease-out`, respecting `prefers-reduced-motion`).

### 2. `src/components/icons/AgentIcons.tsx`
Export `AgentIcon({ kind, className, active })` and `VeloxLogo({ className })` with crisp, minimal, geometric inline SVGs (24x24 viewBox, clean strokes/fills):
- `claude`: clean starburst/spark mark in `#D97757` (muted `#8A513E` when `!active`).
- `agy`: crisp geometric arched-A / chevron mark in `#4E8DF5` (muted `#325A9E` when `!active`).
- `codex`: crisp interlocking hex/ring node mark in `#10A37F` (muted `#146652` when `!active`).
- `opencode`: sharp faceted diamond mark in `#C4B5FD` (muted `#746B96` when `!active`).
- `shell`: crisp terminal prompt `>_` mark in `#94A3B8`.
- `VeloxLogo`: sharp angular V chevron matching the app icon in electric ice-blue `#6EA8FC`.

### 3. `src/hooks/useSystemTelemetry.ts`
Export `useSystemTelemetry()`:
- Fetches initial `veloxApi.getTelemetry()` and `veloxApi.getMcpStatus()` on mount.
- Subscribes to `veloxApi.onTelemetryUpdated` and `veloxApi.onMcpUpdated`.
- Provides `sleepMcpBackend(name: string)`, `sleepAllMcps()`, `refresh()`, and `loadingMcp` state.
- Gracefully handles errors without crashing UI (e.g. if running in browser preview outside Tauri).

### 4. `src/components/TitleBar.tsx`
Preserve exact `TitleBarProps` interface.
Build a sleek `h-10` (`40px`) frameless native-feel Windows titlebar (`bg-[#11141C] border-b border-white/[0.07] select-none flex items-center justify-between`):
- **Left**: Sidebar toggle icon button (`PanelLeft`), `<VeloxLogo />` + `"Velox"` wordmark, and a compact `+` quick-launch button (`Plus` icon, opens AgentLauncherModal).
- **Center** (`data-tauri-drag-region` draggable window area!): Shows active session title + muted cwd basename if a session is selected, or subtle `"Ready"` state. Double-clicking drag area toggles window maximize (`getCurrentWindow().toggleMaximize()`).
- **Right**:
  - Compact live **RAM telemetry pill** (`tabular-nums`, e.g. `148 MB`, tooltip breaking down App / Sessions / MCPs).
  - Compact clickable **Lazy MCP Gateway pill** (e.g. `MCP 1/7` with emerald active dot when >0 running or calm slate dot when all sleeping `0 MB` — clicking calls `onToggleMcpDrawer`).
  - Native-style Windows window controls (`Minimize`, `Maximize/Restore`, `Close` with crisp hover `#F43F5E` on Close) calling `getCurrentWindow()` from `@tauri-apps/api/window` (safely wrapped in try/catch).

### 5. `src/components/McpDrawer.tsx`
Preserve exact `McpDrawerProps` interface.
Build a clean slide-over panel (`w-[360px] bg-[#171B26] border-l border-white/[0.08]`) when `open === true` (with subtle backdrop click-to-close):
- Header: `"Lazy MCP Gateway"`, mode badge (`Embedded` / `External` / `Disabled`), listen address (`127.0.0.1:43111`), and close `X` button.
- Summary metrics bar (clean 3-column strip, `tabular-nums`): Total RAM, Active sessions vs Sleeping sessions, Active MCPs vs Total MCPs.
- Quick power actions: **"Sleep all MCPs"** button and **"Sleep idle sessions"** button.
- Backends list: each backend shows name, live status (`Active · 42 MB` + remaining idle countdown e.g. `sleeps in 1m 45s`, OR `Sleeping · 0 MB · 4 cached`), and a one-click **"Sleep"** button when running.
- Calm empty state if `backends.length === 0`.

### 6. `src/components/EmptyState.tsx`
Preserve exact `EmptyStateProps` interface.
Build a calm, minimal, high-craft empty workspace view centered on `#0B0D12`:
- `<VeloxLogo className="w-8 h-8" />`, crisp headline `"Start an agent session"`, subtle folder indicator showing `defaultCwd` basename.
- Clean 2-column grid of direct one-click launch buttons for installed presets (Claude Personal/Trabajo, Antigravity, Codex, OpenCode, PowerShell) with `<AgentIcon />`, label, optional profile badge, and subtle hover border tint matching each agent's accent color.
- Subtle footer shortcut hint: `"Press Ctrl+N to pick folder and agent"` + button to open full launcher.

## Verification
From `C:\Users\Asus\OneDrive\Escritorio\Proyectos\velox`, run:
```bash
pnpm exec tsc --noEmit
pnpm build
```
Fix any TypeScript or build errors in your files until green, then reply concisely with summary.
