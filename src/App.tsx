import { useCallback, useEffect, useState } from "react";
import { partitionSessions, useSessions } from "./hooks/useSessions";
import { useSystemTelemetry } from "./hooks/useSystemTelemetry";
import { TitleBar } from "./components/TitleBar";
import { Sidebar } from "./components/Sidebar";
import { TerminalPane } from "./components/TerminalPane";
import { AgentLauncherModal } from "./components/AgentLauncherModal";
import { McpDrawer } from "./components/McpDrawer";
import { EmptyState } from "./components/EmptyState";
import { SettingsModal } from "./components/SettingsModal";
import { ThemeBackground } from "./themes/ThemeBackground";
import { getAgentTheme } from "./themes/registry";
import type { AgentActivity } from "./lib/agentActivity";

export function App() {
  const {
    sessions,
    presets,
    recentDirs,
    config,
    activeSessionId,
    activeSession,
    loading,
    error,
    createSession,
    selectSession,
    wakeSession,
    sleepSession,
    sleepIdleSessions,
    closeSession,
    renameSession,
    reorderSessions,
    updateConfig,
  } = useSessions();

  const { telemetry, mcp, sleepMcpBackend, sleepAllMcps } =
    useSystemTelemetry();

  const [sidebarOpen, setSidebarOpen] = useState(true);
  const [launcherOpen, setLauncherOpen] = useState(false);
  const [mcpDrawerOpen, setMcpDrawerOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [activity, setActivity] = useState<Record<string, AgentActivity | null>>({});

  const reportActivity = useCallback((id: string, next: AgentActivity | null) => {
    setActivity((prev) => (prev[id] === next ? prev : { ...prev, [id]: next }));
  }, []);

  const defaultCwd = config?.defaultCwd ?? "";
  const fontFamily =
    config?.fontFamily ??
    "'JetBrains Mono', 'Cascadia Code', Consolas, monospace";
  const fontSize = config?.fontSize ?? 13;
  const animated = config?.animatedBackgrounds ?? true;
  const activeTheme = getAgentTheme(
    activeSession ? (activeSession.activeAgent ?? activeSession.agentKind) : undefined,
    animated,
  );
  const visibleSessions = partitionSessions(sessions).visible;
  // One shared canvas, only while an awake terminal is on screen.
  const showBackground =
    !loading && !!activeSession && activeSession.status !== "sleeping";

  const handleLaunch = useCallback(
    (presetId: string, cwd: string, title?: string) => {
      setLauncherOpen(false);
      void createSession(presetId, cwd, title);
    },
    [createSession],
  );

  // Stable so the memoized TerminalPanes skip re-rendering on unrelated App updates.
  const handlePaneWake = useCallback(
    (id: string, cols: number, rows: number) => {
      void wakeSession(id, cols, rows);
    },
    [wakeSession],
  );

  const handleQuickLaunch = useCallback(
    (presetId: string) => {
      void createSession(presetId, defaultCwd || undefined);
    },
    [createSession, defaultCwd],
  );

  // Global keyboard shortcuts
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      // xterm reads keys through a hidden textarea; it is not a form field.
      const inTerminal =
        e.target instanceof HTMLElement && e.target.classList.contains("xterm-helper-textarea");
      const inInput =
        !inTerminal &&
        (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement);

      if (inInput || !e.ctrlKey || e.altKey) return;
      const key = e.key.toLowerCase();
      const run = (action: () => void) => {
        e.preventDefault();
        action();
      };

      if (e.shiftKey) {
        if (key === "n") run(() => setLauncherOpen((v) => !v));
        else if (key === "b") run(() => setSidebarOpen((v) => !v));
        else if (key === "m") run(() => setMcpDrawerOpen((v) => !v));
        else if (key === "h" && activeSessionId) run(() => void sleepSession(activeSessionId));
        return;
      }

      // Plain Ctrl+N also opens the launcher, like any app's "new" (Ctrl+Shift+N still works).
      if (key === "n") run(() => setLauncherOpen((v) => !v));
      else if (key === ",") run(() => setSettingsOpen((v) => !v));
      // Ctrl+W deletes a word inside the terminal, so it only closes from outside it.
      else if (key === "w" && activeSessionId && !inTerminal) {
        run(() => void closeSession(activeSessionId));
      } else if (/^[1-9]$/.test(key)) {
        const target = visibleSessions[Number(key) - 1];
        if (target) run(() => void selectSession(target.id));
      }
    };

    // Capture phase: runs before any element below can swallow the shortcut.
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [activeSessionId, visibleSessions, selectSession, sleepSession, closeSession]);

  return (
    <div
      data-agent-theme={activeTheme.id}
      className="h-screen w-screen bg-[var(--velox-canvas)] text-[var(--velox-ink)] flex flex-col overflow-hidden">
      <TitleBar
        sidebarOpen={sidebarOpen}
        onToggleSidebar={() => setSidebarOpen((v) => !v)}
        onOpenLauncher={() => setLauncherOpen(true)}
        activeSession={activeSession}
        telemetry={telemetry}
        mcp={mcp}
        mcpDrawerOpen={mcpDrawerOpen}
        onToggleMcpDrawer={() => setMcpDrawerOpen((v) => !v)}
        onOpenSettings={() => setSettingsOpen(true)}
      />

      {error && (
        <div className="px-4 py-1.5 bg-rose-500/12 border-b border-rose-500/25 text-[12px] text-rose-300 flex items-center justify-between">
          <span className="truncate">{error}</span>
        </div>
      )}

      <div className="flex-1 flex min-h-0 overflow-hidden">
        {sidebarOpen && (
          <Sidebar
            sessions={sessions}
            activity={activity}
            activeSessionId={activeSessionId}
            onSelectSession={(id) => void selectSession(id)}
            onWakeSession={(id) => void wakeSession(id)}
            onSleepSession={(id) => void sleepSession(id)}
            onCloseSession={(id) => void closeSession(id)}
            onRenameSession={(id, title) => void renameSession(id, title)}
            onReorderSessions={(ids) => void reorderSessions(ids)}
            onSleepIdleSessions={() => void sleepIdleSessions()}
            onOpenLauncher={() => setLauncherOpen(true)}
          />
        )}

        <main className="flex-1 min-w-0 h-full bg-[var(--velox-canvas)] relative overflow-hidden">
          {showBackground && <ThemeBackground theme={activeTheme} />}
          {loading ? (
            <div className="h-full w-full flex items-center justify-center text-[13px] text-[var(--velox-ink-muted)]">
              Loading workspace...
            </div>
          ) : sessions.length === 0 ? (
            <EmptyState
              presets={presets}
              defaultCwd={defaultCwd}
              onQuickLaunch={handleQuickLaunch}
              onOpenLauncher={() => setLauncherOpen(true)}
            />
          ) : (
            sessions.map((session) => (
              <TerminalPane
                key={session.id}
                session={session}
                isActive={activeSessionId === session.id}
                fontFamily={fontFamily}
                fontSize={fontSize}
                theme={getAgentTheme(session.activeAgent ?? session.agentKind, animated)}
                onActivity={reportActivity}
                onWake={handlePaneWake}
              />
            ))
          )}
        </main>
      </div>

      <AgentLauncherModal
        open={launcherOpen}
        onClose={() => setLauncherOpen(false)}
        presets={presets}
        recentDirs={recentDirs}
        defaultCwd={defaultCwd}
        onLaunch={handleLaunch}
      />

      <McpDrawer
        open={mcpDrawerOpen}
        onClose={() => setMcpDrawerOpen(false)}
        mcp={mcp}
        telemetry={telemetry}
        onSleepBackend={sleepMcpBackend}
        onSleepAllMcps={sleepAllMcps}
        onSleepIdleSessions={sleepIdleSessions}
      />

      <SettingsModal
        open={settingsOpen}
        onClose={() => setSettingsOpen(false)}
        config={config}
        onSave={updateConfig}
      />
    </div>
  );
}
