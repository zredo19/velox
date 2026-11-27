# Lane C — Velox Online frontend (team sidebar, live mirror, task inbox, settings)

**Read first:** `docs/online/PLAN.md` (whole file), `docs/ARCHITECTURE.md` (design tokens & UI
discipline), `src/types/ipc.ts` (the "Velox Online" section at the end), `src/lib/tauri.ts`
(`getTeamStatus` … `onTeamSessionGone`), then `src/App.tsx`, `src/components/Sidebar.tsx`,
`src/components/TerminalPane.tsx`, `src/components/SettingsModal.tsx`, `src/components/TitleBar.tsx`,
`src/hooks/useSessions.ts`, `src/components/icons/AgentIcons.tsx`, `src/index.css`.

**Work ONLY in:** `D:\Workspace\velox-online\lane-c` (git worktree, branch `online/lane-c`).
Never edit `D:\Workspace\velox` (the main checkout) or the other lanes' worktrees.
First run `pnpm install --frozen-lockfile` in your worktree.

## Owned files (edit only these)

- New: `src/hooks/useTeam.ts`, `src/components/team/*` (any files you need there)
- Edit: `src/App.tsx`, `src/components/Sidebar.tsx`, `src/components/SettingsModal.tsx`,
  `src/components/TitleBar.tsx`, `src/index.css` (append only, at the end)

Do NOT edit: `src/types/ipc.ts`, `src/lib/tauri.ts` (the contract — the backend lanes implement it
in parallel; if you need a change write it under "Questions" in your status file), `src/components/TerminalPane.tsx`
(copy what you need into your own component instead), `src-tauri/**`, `package.json`, the lockfile.
No new npm dependencies. Icons: `lucide-react` (already installed) or inline SVG.

The backend is not ready yet: build strictly against the `veloxApi` contract. Every call can reject
(team disabled, peer offline): catch, show a calm inline message, never crash.

## Requirements

### 1. `src/hooks/useTeam.ts`
Loads `getTeamStatus()` and `listTeamTasks()` on mount, listens to `team-updated` and
`team-tasks-updated`, and exposes `{ team: TeamSnapshot | null, tasks: TaskTicket[], pendingIncoming,
setShared(sessionId, shared), sendTask(peer, sessionId, text), respond(requestId, accept), refresh }`.
Unlisten on unmount. Tolerate the backend throwing "unknown command" (treat as disabled).

### 2. Sidebar (`Sidebar.tsx` + your components)
- Local rows: a share toggle (lucide `Radio` or similar) next to the existing row actions, tooltip
  "Share with team" / "Stop sharing". When `session.shared` it stays visible in the accent color so you
  can tell at a glance what is being broadcast. Hidden entirely when the team is disabled.
- Team section under the local sessions, one block per `team.peers[]`:
  header = peer name + online dot (green when `online`, muted otherwise) + session count;
  rows = `AgentIcon` for `activeAgent ?? agentKind`, title, a pill with the **peer name** (new accent
  `#F5B971`, same pill style as the `personal`/`trabajo` pill), status dot, and a second line with
  `summary ?? folder`. Offline peer → one muted line "Offline" (+ `lastError` as tooltip).
- Team enabled with zero peers → a single subtle hint "Add a teammate in Settings". Team disabled → render nothing.
- Clicking a remote row selects it (see App) and highlights it like the active local row.

### 3. App state (`App.tsx`)
`selectedRemote: { peer: string; sessionId: string } | null`. Selecting a remote session clears the
active local selection visually and shows `RemoteTerminalPane` in `<main>`; selecting a local session
clears `selectedRemote`. Keep the existing local panes mounted (do not break their state). If the
selected remote session disappears from `team.peers`, show a "Session no longer shared" state with a
button to go back.

### 4. `RemoteTerminalPane`
- Read-only xterm (`disableStdin: true`, same font/theme options as `TerminalPane`, WebGL addon with the
  same fallback). **Never** call `resizePty`/`writePty`; never use FitAddon to drive the size.
- Mount → `teamSubscribe(peer, sessionId)`; unmount or selection change → `teamUnsubscribe`.
- `team-pty-snapshot` for this (peer, sessionId) → `term.reset()`, `term.resize(cols, rows)`, write bytes,
  remember `generation` + `endOffset`. `team-pty-output` → ignore if generation differs or
  `endOffset` ≤ last; else write. `team-pty-resize` → `term.resize`. `team-session-gone` → overlay.
  Filter every event by both `peer` and `sessionId`.
- Fit by scaling, not resizing: compute the terminal's pixel size and apply
  `transform: scale(min(1, availW / w, availH / h))` with top-left origin, centered; recompute on
  container resize (ResizeObserver) and on cols/rows change.
- Header strip: `<peer> · <title>` + agent icon + status + a "Read-only" badge and a live dot.
- Loading state until the first snapshot ("Connecting to <peer>…"); error state if subscribe rejects.

### 5. Send a task (bottom of `RemoteTerminalPane`)
Textarea (2–6 rows, auto-grow) + "Send task" button; Ctrl+Enter sends. Calls
`sendTeamTask(peer, sessionId, text)`, clears on success, then shows the newest ticket for this
session from `tasks`: pending → "Waiting for <peer> to accept…", delivered → "Delivered",
rejected/failed/expired → message. Disabled while the peer is offline. Helper text: "They review it
before it runs."

### 6. Incoming task cards (`TaskInbox`)
Every `tasks` item with `direction === "incoming" && state === "pending"` → a card stacked bottom-right
(above everything, max 3 visible + "+N more"): "<peer ?? 'This PC'> → <sessionTitle>", `fromSession`,
the text in a scrollable monospace block (max-height ~160px), buttons **Accept** (primary) and
**Reject**. Buttons call `respond(requestId, accept)` and show a spinner until the state changes.
Never auto-accept. Optional subtle sound-free attention cue (pulse) when a new card appears.

### 7. TitleBar
A small team indicator (lucide `Users`) visible only when the team is enabled: online peers count,
an amber badge with the pending incoming count, and "N watching" when `team.watchers` is non-empty.
Click → popover listing the last 20 tasks (both directions) with state chips and time.

### 8. Settings (`SettingsModal.tsx` → a "Team (Velox Online)" section)
Enabled toggle; display name; listen port; bind host (placeholder "Auto (Tailscale IP)"); token
(password input + "Generate" via `generateTeamToken()` + "Copy"); "Auto-approve tasks from this PC"
toggle; peers editor (rows of name / host / port with add & remove; validate non-empty unique names,
port 1–65535). Status line from `team`: "Listening on <listenAddr>" (green) or `serverError` (rose),
and "Tailscale IP: <tailscaleIp | not detected>". Saves through the existing `onSave(updateConfig)`
with `config.team` updated; keep every other config field untouched.

### 9. Style
Follow the ARCHITECTURE design tokens: dark industrial minimal, 1px hairlines, 120ms ease-out,
sentence case, English copy, `tabular-nums` for numbers. Reuse existing CSS variables
(`--velox-*`). Respect `prefers-reduced-motion`.

## Validation
`pnpm build` (tsc + vite) passes with zero TypeScript errors; no `any`; no new warnings.

## Working rules
- Commit early and often on `online/lane-c` (small, clear messages). Do not push. Do not merge.
- After each milestone, update `D:\Workspace\velox-online\status\lane-c.md` (template inside).
- Blocked on a contract? Write it under "Questions" in the status file, pick the safest workaround, continue.
- Do not launch `pnpm tauri dev` or `velox.exe`.
- When `pnpm build` is green and everything above is done: set status to `DONE`, list what was built
  and anything left undone.
