# Lane 4 (`velox-ui-core`) — TerminalPane (xterm.js WebGL), Sidebar, `+` Agent Launcher & App Shell

**Model Tier:** MiMo-V2.6-Flash (Core Interactive Frontend & Terminal Engine Lane)
**Architecture & Design Spec:** Read `C:\Users\Asus\OneDrive\Escritorio\Proyectos\velox\docs\ARCHITECTURE.md`, `src/types/ipc.ts`, `src/lib/tauri.ts`, and existing component prop contracts in `src/components/*.tsx`.

## Owned Files (EDIT ONLY THESE FILES)
- `src/hooks/useSessions.ts`
- `src/components/TerminalPane.tsx`
- `src/components/Sidebar.tsx`
- `src/components/AgentLauncherModal.tsx`
- `src/App.tsx`

Do NOT modify `package.json`, `src/types/ipc.ts`, `src/lib/tauri.ts`, `src/index.css`, `src/components/TitleBar.tsx`, `src/components/McpDrawer.tsx`, `src/components/EmptyState.tsx`, `src/components/icons/AgentIcons.tsx`, or `src/hooks/useSystemTelemetry.ts`.
Import those Lane 5 components/hooks using their exact exported TypeScript interfaces already present in those stub files!

## Exact Requirements

### 1. `src/hooks/useSessions.ts`
Manage full session lifecycle cleanly:
- Loads `listSessions()`, `listAgentPresets()`, `listRecentDirs()`, and `getConfig()` on mount.
- Subscribes to `veloxApi.onSessionUpdated` and `veloxApi.onTelemetryUpdated` (updating per-session `memoryMb` live without re-rendering terminal panes!).
- Tracks `activeSessionId: string | null`, `presets`, `recentDirs`, `config`, `loading`, `error`.
- Actions:
  - `createSession(presetId: string, cwd?: string, title?: string)` -> calls `veloxApi.createSession({ presetId, cwd, title, cols: 120, rows: 34 })`, sets `activeSessionId`, refreshes `recentDirs`.
  - `selectSession(id: string)` -> selects session; if `status === "sleeping"`, automatically wakes it (`wakeSession(id)`).
  - `wakeSession(id: string, cols?: number, rows?: number)` -> calls `veloxApi.wakeSession(id, cols ?? 120, rows ?? 34)`.
  - `sleepSession(id: string)` -> calls `veloxApi.sleepSession(id)`.
  - `sleepIdleSessions()` -> calls `veloxApi.sleepIdleSessions(activeSessionId)`.
  - `closeSession(id: string)` -> calls `veloxApi.closeSession(id)`, selects neighbor session if active was closed.
  - `renameSession(id: string, title: string)` -> calls `veloxApi.renameSession(id, title)`.

### 2. `src/components/TerminalPane.tsx`
Production-grade `xterm.js` terminal component for Windows ConPTY agents (`Claude`, `AGY`, `Codex`, `OpenCode`, `PowerShell`):
- Props: `{ session: SessionRecord; isActive: boolean; fontFamily: string; fontSize: number; onWake: (id: string, cols: number, rows: number) => void; }`
- **Sleeping State Overlay**: When `session.status === "sleeping"`, DO NOT mount an active WebGL context (saves GPU + RAM!). Render a calm, crisp sleeping card centered on `#0B0D12` showing the agent icon, session title, folder path, `0 MB RAM` badge, and a prominent **"Wake session"** button (plus `Enter` shortcut hint; notes that Claude/AGY resume automatically if a session ID was captured).
- **Awake State (`xterm.js`)**:
  - Mount `Terminal` with custom Velox dark theme matching `#0B0D12` canvas, crisp high-contrast ANSI palette, `cursorBlink: true`, `cursorStyle: "bar"`, `scrollback: 5000`, `allowProposedApi: true`, `fontFamily`, `fontSize`, `lineHeight: 1.25`.
  - Load `FitAddon`, `WebLinksAddon`, `Unicode11Addon` (activate `unicode.activeVersion = "11"`), and `WebglAddon` wrapped in `try/catch` with `onContextLoss(() => webgl.dispose())` fallback to canvas/DOM renderer so it NEVER crashes!
  - On mount: hydrate existing scrollback once via `veloxApi.getScrollback(session.id)` -> `decodeBase64ToBytes` -> `term.write(bytes)`.
  - Subscribe to `veloxApi.onPtyOutput`: when `ev.sessionId === session.id`, decode base64 via `decodeBase64ToBytes(ev.dataB64)` and call `term.write(bytes)`.
  - Forward user input `term.onData((data) => veloxApi.writePty(session.id, data))`.
  - **Clipboard ergonomics**: Support `Ctrl+Shift+C` (copy selection) and `Ctrl+Shift+V` / right-click paste, and let `Ctrl+C` copy when text is selected or send SIGINT (`\x03`) when no text is selected!
  - **ResizeObserver** (debounced ~40ms): calls `fitAddon.fit()` and `veloxApi.resizePty(session.id, term.cols, term.rows)` whenever active pane resizes or becomes visible (`isActive` toggles true -> refit + `term.focus()`).
  - Keep mounted panes alive via CSS (`hidden` / `opacity-0 pointer-events-none absolute inset-0`) ONLY for awake sessions so switching tabs is instant with zero flicker, while sleeping sessions unmount their `xterm` instance cleanly to free WebGL/DOM memory!

### 3. `src/components/Sidebar.tsx`
Intentional, razor-clean agent workspace sidebar (`w-[252px] bg-[#11141C] border-r border-white/[0.07]`):
- Top header row: concise `"Sessions"` label + a crisp, unmistakable **`+` New agent button** (`bg-[#4E8DF5]/15 hover:bg-[#4E8DF5]/25 text-[#9CC2FF] border border-[#4E8DF5]/30` with Plus icon and `Ctrl+N` tooltip).
- Split cleanly into active vs sleeping (or unified ordered list with visual distinction):
  - Each session row displays:
    - `<AgentIcon kind={s.agentKind} active={s.status === "running" || s.status === "idle"} />`
    - Status indicator: subtle pulsing dot when `running`, steady semantic accent when `idle`, vivid `#F43F5E` pulse when `blocked`, muted moon icon when `sleeping`, dim dot when `exited`.
    - Session `title` (double-click to inline rename!) + optional compact profile pill (`personal` in `#E8B4D9` / `trabajo` in `#7DD3C8`) + subtle basename/cwd subtitle.
    - Right-aligned `tabular-nums` RAM chip (`142 MB` for awake, `0 MB` muted for sleeping).
    - Hover action buttons (clean icon buttons, no clutter): **Sleep (`Moon`)** when awake / **Wake (`Play`)** when sleeping, and **Close (`X`)**.
- Compact footer action if >1 awake session: quiet `"Sleep idle"` button to hibernate inactive tabs in one click.

### 4. `src/components/AgentLauncherModal.tsx`
The signature **`+` Quick Agent Launcher** (`Ctrl+N` or clicking `+`):
- Centered elevated modal (`bg-[#171B26] border border-white/[0.09] rounded-xl shadow-2xl max-w-[540px] w-full`) with subtle dark backdrop blur.
- **Agent Picker Grid / List**: Shows all presets (`Claude · personal`, `Claude · trabajo`, `Antigravity`, `Codex`, `OpenCode`, `PowerShell`) with bespoke `<AgentIcon />`, accent indicator, description, installed status badge, and keyboard navigation (`ArrowUp`/`ArrowDown` + `Enter`, or single click, or number keys `1`-`6`!).
- **Workspace Folder Selector**:
  - Editable path input prefilled with `defaultCwd` + native **"Browse..."** button calling `veloxApi.pickDirectory(cwd)`.
  - Quick chips for `recentDirs` (showing last 2 path segments so they stay clean and readable).
- Optional custom session title input (defaults cleanly to folder name if left blank).
- Instant launch on Enter or clicking any preset card (or clicking "Launch agent"), closes modal smoothly (`Escape` dismisses).

### 5. `src/App.tsx`
Composes the full workspace layout cleanly:
- Uses `useSessions()` and `useSystemTelemetry()`.
- Layout:
  - Top `<TitleBar />` (custom frameless window header with RAM & MCP status pills)
  - Main horizontal split: collapsible `<Sidebar />` + main terminal viewport (`bg-[#0B0D12] relative flex-1 overflow-hidden`) rendering `<EmptyState />` when 0 sessions or the `<TerminalPane />` list.
  - Slide-over `<McpDrawer />` and modal `<AgentLauncherModal />`.
- Global keyboard shortcuts (ignoring when typing inside `<input>`/`<textarea>`, except `Escape`):
  - `Ctrl+N`: Open `+` Agent Launcher modal
  - `Ctrl+B`: Toggle sidebar
  - `Ctrl+W`: Close active session (only when a session is active; prevent browser tab close)
  - `Ctrl+Shift+H`: Sleep (hibernate) active session
  - `Ctrl+1`..`Ctrl+9`: Switch to session 1..9
  - `Ctrl+M`: Toggle Lazy MCP Gateway drawer

## Verification
From `C:\Users\Asus\OneDrive\Escritorio\Proyectos\velox`, run:
```bash
pnpm exec tsc --noEmit
pnpm build
```
Fix any TypeScript or build errors in your files until green, then reply concisely with summary.
