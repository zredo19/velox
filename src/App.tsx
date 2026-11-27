import { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { X } from "lucide-react";
import { partitionSessions, useSessions } from "./hooks/useSessions";
import { useSystemTelemetry } from "./hooks/useSystemTelemetry";
import { useTeam } from "./hooks/useTeam";
import { TitleBar } from "./components/TitleBar";
import { Sidebar } from "./components/Sidebar";
import { TerminalPane } from "./components/TerminalPane";
import { EmptyState } from "./components/EmptyState";
import { RemoteTerminalPane } from "./components/team/RemoteTerminalPane";
import { TaskInbox } from "./components/team/TaskInbox";
import type { SelectedRemoteTarget } from "./components/team/TeamSidebarSection";
import { ThemeBackground } from "./themes/ThemeBackground";
import {
  getAgentTheme,
  loadThemePreference,
  saveThemePreference,
  THEME_OPTIONS,
} from "./themes/registry";
import type { ThemePreference } from "./themes/types";
import type { AgentActivity } from "./lib/agentActivity";
import type { AgentKind, SessionRecord, VeloxConfig } from "./types/ipc";

// Lazily code-split overlay modals & drawers to keep initial startup bundle lean
const AgentLauncherModal = lazy(() =>
  import("./components/AgentLauncherModal").then((m) => ({ default: m.AgentLauncherModal })),
);
const McpDrawer = lazy(() =>
  import("./components/McpDrawer").then((m) => ({ default: m.McpDrawer })),
);
const SettingsModal = lazy(() =>
  import("./components/SettingsModal").then((m) => ({ default: m.SettingsModal })),
);
const CommandPalette = lazy(() =>
  import("./components/CommandPalette").then((m) => ({ default: m.CommandPalette })),
);
const ShortcutsModal = lazy(() =>
  import("./components/ShortcutsModal").then((m) => ({ default: m.ShortcutsModal })),
);
const ThemePickerModal = lazy(() =>
  import("./components/ThemePickerModal").then((m) => ({ default: m.ThemePickerModal })),
);

const DEFAULT_FONT_SIZE = 13;

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
    clearError,
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

  const {
    team,
    tasks,
    pendingIncoming,
    localMessages,
    error: teamError,
    dismissLocalMessage,
    setShared,
    sendTask,
    respond,
    refresh: refreshTeam,
  } = useTeam();

  const [sidebarOpen, setSidebarOpen] = useState(true);
  const [launcherOpen, setLauncherOpen] = useState(false);
  const [launcherKind, setLauncherKind] = useState<AgentKind | null>(null);
  const [mcpDrawerOpen, setMcpDrawerOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [shortcutsOpen, setShortcutsOpen] = useState(false);
  const [themesOpen, setThemesOpen] = useState(false);
  const [themePreference, setThemePreference] = useState<ThemePreference>(() =>
    loadThemePreference(),
  );
  const [clearTrigger, setClearTrigger] = useState(0);
  const [toast, setToast] = useState<string | null>(null);
  const [selectedRemote, setSelectedRemote] =
    useState<SelectedRemoteTarget | null>(null);
  const [activity, setActivity] = useState<Record<string, AgentActivity | null>>({});
  const toastTimerRef = useRef<number | undefined>(undefined);

  const showToast = useCallback((message: string) => {
    window.clearTimeout(toastTimerRef.current);
    setToast(message);
    toastTimerRef.current = window.setTimeout(() => {
      setToast(null);
    }, 2000);
  }, []);

  useEffect(() => () => window.clearTimeout(toastTimerRef.current), []);

  const reportActivity = useCallback((id: string, next: AgentActivity | null) => {
    setActivity((prev) => (prev[id] === next ? prev : { ...prev, [id]: next }));
  }, []);

  const selectedPeerObj = selectedRemote
    ? (team?.peers.find((p) => p.name === selectedRemote.peer) ?? null)
    : null;
  const selectedRemoteSession =
    selectedRemote && selectedPeerObj
      ? (selectedPeerObj.sessions.find((s) => s.id === selectedRemote.sessionId) ??
        null)
      : null;
  const remoteSessionMissing =
    selectedRemote !== null &&
    team !== null &&
    (!team.enabled ||
      !selectedPeerObj ||
      (selectedPeerObj.online && !selectedRemoteSession));

  const defaultCwd = config?.defaultCwd ?? "";
  const fontFamily =
    config?.fontFamily ??
    "'JetBrains Mono', 'Cascadia Code', Consolas, monospace";
  const fontSize = config?.fontSize ?? DEFAULT_FONT_SIZE;
  const animated = config?.animatedBackgrounds ?? true;
  const activeTheme = getAgentTheme(
    selectedRemote
      ? (selectedRemoteSession?.activeAgent ?? selectedRemoteSession?.agentKind)
      : activeSession
        ? (activeSession.activeAgent ?? activeSession.agentKind)
        : undefined,
    animated && !selectedRemote,
    themePreference,
  );

  const { visible: visibleSessions } = useMemo(
    () => partitionSessions(sessions),
    [sessions],
  );

  const { blockedCount, workingCount } = useMemo(() => {
    let blocked = 0;
    let working = 0;
    for (const s of sessions) {
      if (s.status === "sleeping" || s.status === "exited") continue;
      const a = activity[s.id];
      if (a === "blocked") blocked += 1;
      else if (a === "working") working += 1;
    }
    return { blockedCount: blocked, workingCount: working };
  }, [sessions, activity]);

  // One shared canvas, only while an awake local terminal is on screen.
  const showBackground =
    !loading &&
    !selectedRemote &&
    !!activeSession &&
    activeSession.status !== "sleeping";

  const openLauncher = useCallback((kind: AgentKind | null = null) => {
    setLauncherKind(kind);
    setLauncherOpen(true);
  }, []);

  const handleOpenDefaultLauncher = useCallback(() => openLauncher(null), [openLauncher]);

  const closeLauncher = useCallback(() => {
    setLauncherOpen(false);
    setLauncherKind(null);
  }, []);

  const handleToggleSidebar = useCallback(() => setSidebarOpen((v) => !v), []);
  const handleToggleMcpDrawer = useCallback(() => setMcpDrawerOpen((v) => !v), []);
  const handleOpenSettings = useCallback(() => setSettingsOpen(true), []);
  const handleOpenPalette = useCallback(() => setPaletteOpen(true), []);
  const handleOpenShortcuts = useCallback(() => setShortcutsOpen(true), []);
  const handleOpenThemes = useCallback(() => setThemesOpen(true), []);
  const handleCloseMcpDrawer = useCallback(() => setMcpDrawerOpen(false), []);
  const handleCloseSettings = useCallback(() => setSettingsOpen(false), []);
  const handleClosePalette = useCallback(() => setPaletteOpen(false), []);
  const handleCloseShortcuts = useCallback(() => setShortcutsOpen(false), []);
  const handleCloseThemes = useCallback(() => setThemesOpen(false), []);

  const handleSelectTheme = useCallback(
    (pref: ThemePreference) => {
      setThemePreference(pref);
      saveThemePreference(pref);
      const label = THEME_OPTIONS.find((t) => t.id === pref)?.label ?? pref;
      showToast(`Theme · ${label}`);
    },
    [showToast],
  );

  const handleLaunch = useCallback(
    (presetId: string, cwd: string, title?: string) => {
      closeLauncher();
      setSelectedRemote(null);
      void createSession(presetId, cwd, title);
    },
    [closeLauncher, createSession],
  );

  // Stable so the memoized TerminalPanes & Sidebar skip re-rendering on unrelated App updates.
  const handlePaneWake = useCallback(
    (id: string, cols: number, rows: number) => {
      void wakeSession(id, cols, rows);
    },
    [wakeSession],
  );

  const handleSleepSession = useCallback(
    (id: string) => {
      void sleepSession(id);
    },
    [sleepSession],
  );

  const handleCloseSession = useCallback(
    (id: string) => {
      void closeSession(id);
    },
    [closeSession],
  );

  const handleRenameSession = useCallback(
    (id: string, title: string) => {
      void renameSession(id, title);
    },
    [renameSession],
  );

  const handleReorderSessions = useCallback(
    (ids: string[]) => {
      void reorderSessions(ids);
    },
    [reorderSessions],
  );

  const handleDuplicateSession = useCallback(
    (session: SessionRecord) => {
      setSelectedRemote(null);
      void createSession(session.presetId, session.cwd, `${session.title} (2)`);
      showToast(`Duplicated ${session.title}`);
    },
    [createSession, showToast],
  );

  const handleCopyPath = useCallback(
    (path: string) => {
      void navigator.clipboard
        .writeText(path)
        .then(() => showToast("Folder path copied"))
        .catch(() => {});
    },
    [showToast],
  );

  const handleSleepIdleSessions = useCallback(() => {
    void sleepIdleSessions().then(() => showToast("Idle sessions hibernated"));
  }, [sleepIdleSessions, showToast]);

  const handleQuickLaunch = useCallback(
    (presetId: string) => {
      setSelectedRemote(null);
      void createSession(presetId, defaultCwd || undefined);
    },
    [createSession, defaultCwd],
  );

  const handleLaunchAgentKind = useCallback(
    (kind: AgentKind) => {
      openLauncher(kind);
    },
    [openLauncher],
  );

  const adjustFontSize = useCallback(
    (nextSize: number) => {
      if (!config) return;
      const clamped = Math.max(10, Math.min(20, nextSize));
      if (clamped === config.fontSize) {
        showToast(`Font size · ${clamped}px`);
        return;
      }
      void updateConfig({ ...config, fontSize: clamped });
      showToast(`Font size · ${clamped}px`);
    },
    [config, updateConfig, showToast],
  );

  const handleZoomIn = useCallback(
    () => adjustFontSize(fontSize + 1),
    [adjustFontSize, fontSize],
  );
  const handleZoomOut = useCallback(
    () => adjustFontSize(fontSize - 1),
    [adjustFontSize, fontSize],
  );
  const handleZoomReset = useCallback(
    () => adjustFontSize(DEFAULT_FONT_SIZE),
    [adjustFontSize],
  );

  const handleToggleAnimated = useCallback(() => {
    if (!config) return;
    const next = !config.animatedBackgrounds;
    void updateConfig({ ...config, animatedBackgrounds: next });
    showToast(next ? "Animated backgrounds enabled" : "Animated backgrounds disabled");
  }, [config, updateConfig, showToast]);

  const handleClearTerminal = useCallback(() => {
    setClearTrigger((n) => n + 1);
  }, []);

  const handleSleepActive = useCallback(() => {
    if (activeSessionId && !selectedRemote) {
      void sleepSession(activeSessionId);
      showToast("Session hibernated · 0 MB RAM");
    }
  }, [activeSessionId, selectedRemote, sleepSession, showToast]);

  const handleSelectLocalSession = useCallback(
    (id: string) => {
      setSelectedRemote(null);
      void selectSession(id);
    },
    [selectSession],
  );

  const handleWakeLocalSession = useCallback(
    (id: string) => {
      setSelectedRemote(null);
      void wakeSession(id);
    },
    [wakeSession],
  );

  const handleToggleShare = useCallback(
    (id: string, shared: boolean) => {
      void setShared(id, shared);
    },
    [setShared],
  );

  const handleSaveConfig = useCallback(
    async (next: VeloxConfig) => {
      await updateConfig(next);
      void refreshTeam();
    },
    [updateConfig, refreshTeam],
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
        else if (key === "p") run(() => setPaletteOpen((v) => !v));
        else if (key === "t") run(() => setThemesOpen((v) => !v));
        else if (key === "h" && activeSessionId && !selectedRemote) run(handleSleepActive);
        return;
      }

      // Plain Ctrl+N opens launcher, Ctrl+K opens Command Palette, Ctrl+/ opens Shortcuts
      if (key === "n") run(() => setLauncherOpen((v) => !v));
      else if (key === "k") run(() => setPaletteOpen((v) => !v));
      else if (key === "/") run(() => setShortcutsOpen((v) => !v));
      else if (key === ",") run(() => setSettingsOpen((v) => !v));
      else if (key === "=" || key === "+") run(handleZoomIn);
      else if (key === "-") run(handleZoomOut);
      else if (key === "0") run(handleZoomReset);
      // Ctrl+W deletes a word inside the terminal, so it only closes from outside it.
      else if (key === "w" && activeSessionId && !selectedRemote && !inTerminal) {
        run(() => void closeSession(activeSessionId));
      } else if (/^[1-9]$/.test(key)) {
        const target = visibleSessions[Number(key) - 1];
        if (target) {
          run(() => {
            setSelectedRemote(null);
            void selectSession(target.id);
          });
        }
      }
    };

    // Capture phase: runs before any element below can swallow the shortcut.
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [
    activeSessionId,
    selectedRemote,
    visibleSessions,
    selectSession,
    closeSession,
    handleSleepActive,
    handleZoomIn,
    handleZoomOut,
    handleZoomReset,
  ]);

  const bannerError = error ?? teamError;

  return (
    <div
      data-agent-theme={activeTheme.id}
      className="h-screen w-screen bg-[var(--velox-canvas)] text-[var(--velox-ink)] flex flex-col overflow-hidden velox-chrome-transition"
    >
      <TitleBar
        sidebarOpen={sidebarOpen}
        onToggleSidebar={handleToggleSidebar}
        onOpenLauncher={handleOpenDefaultLauncher}
        onOpenPalette={handleOpenPalette}
        onOpenThemes={handleOpenThemes}
        onOpenShortcuts={handleOpenShortcuts}
        onCopyPath={handleCopyPath}
        activeSession={selectedRemote ? null : activeSession}
        blockedCount={blockedCount}
        workingCount={workingCount}
        telemetry={telemetry}
        mcp={mcp}
        mcpDrawerOpen={mcpDrawerOpen}
        onToggleMcpDrawer={handleToggleMcpDrawer}
        onOpenSettings={handleOpenSettings}
        team={team}
        tasks={tasks}
        pendingIncomingCount={pendingIncoming.length}
      />

      {bannerError && (
        <div className="px-4 py-1.5 bg-rose-500/12 border-b border-rose-500/25 text-[12px] text-rose-300 flex items-center justify-between gap-3 animate-velox-toast-in">
          <span className="truncate">{bannerError}</span>
          {error && (
            <button
              type="button"
              onClick={clearError}
              aria-label="Dismiss error"
              className="shrink-0 p-0.5 rounded hover:bg-rose-500/20 text-rose-300 transition-colors cursor-pointer"
            >
              <X className="w-3.5 h-3.5" />
            </button>
          )}
        </div>
      )}

      <div className="flex-1 flex min-h-0 overflow-hidden">
        <div
          className="velox-sidebar-shell"
          style={{
            width: sidebarOpen ? 256 : 0,
            opacity: sidebarOpen ? 1 : 0,
            pointerEvents: sidebarOpen ? "auto" : "none",
          }}
          aria-hidden={!sidebarOpen}
        >
          <Sidebar
            sessions={sessions}
            activity={activity}
            activeSessionId={selectedRemote ? null : activeSessionId}
            team={team}
            localMessages={localMessages}
            selectedRemote={selectedRemote}
            onSelectSession={handleSelectLocalSession}
            onDismissLocalMessage={dismissLocalMessage}
            onSelectRemote={setSelectedRemote}
            onToggleShare={handleToggleShare}
            onWakeSession={handleWakeLocalSession}
            onSleepSession={handleSleepSession}
            onCloseSession={handleCloseSession}
            onRenameSession={handleRenameSession}
            onDuplicateSession={handleDuplicateSession}
            onCopyPath={handleCopyPath}
            onReorderSessions={handleReorderSessions}
            onSleepIdleSessions={handleSleepIdleSessions}
            onOpenLauncher={handleOpenDefaultLauncher}
            onLaunchAgentKind={handleLaunchAgentKind}
            onOpenSettings={handleOpenSettings}
          />
        </div>

        <main className="flex-1 min-w-0 h-full bg-[var(--velox-canvas)] relative overflow-hidden velox-chrome-transition">
          {showBackground && <ThemeBackground theme={activeTheme} />}
          {loading ? (
            <div className="h-full w-full flex flex-col items-center justify-center gap-2.5 text-[13px] text-[var(--velox-ink-muted)]">
              <div className="w-5 h-5 rounded-full border-2 border-[var(--velox-accent)]/30 border-t-[var(--velox-accent)] animate-spin" />
              <span>Loading workspace…</span>
            </div>
          ) : (
            <>
              {sessions.length === 0 && !selectedRemote && (
                <EmptyState
                  presets={presets}
                  defaultCwd={defaultCwd}
                  onQuickLaunch={handleQuickLaunch}
                  onOpenLauncher={handleOpenDefaultLauncher}
                  onOpenPalette={handleOpenPalette}
                />
              )}

              {sessions.map((session) => (
                <TerminalPane
                  key={session.id}
                  session={session}
                  isActive={!selectedRemote && activeSessionId === session.id}
                  fontFamily={fontFamily}
                  fontSize={fontSize}
                  theme={getAgentTheme(
                    session.activeAgent ?? session.agentKind,
                    animated,
                    themePreference,
                  )}
                  clearTrigger={clearTrigger}
                  onActivity={reportActivity}
                  onWake={handlePaneWake}
                />
              ))}

              {selectedRemote &&
                (remoteSessionMissing ? (
                  <div className="relative z-10 h-full w-full bg-[var(--velox-canvas)] flex items-center justify-center p-6">
                    <div className="w-full max-w-[360px] rounded-xl bg-[var(--velox-elevated)] border border-white/[0.08] p-6 text-center shadow-xl space-y-3">
                      <div className="text-[14px] font-medium text-[var(--velox-ink)]">
                        Session no longer shared
                      </div>
                      <p className="text-[12px] text-[var(--velox-ink-muted)]">
                        This remote session is no longer available from{" "}
                        {selectedRemote.peer}.
                      </p>
                      <button
                        type="button"
                        onClick={() => setSelectedRemote(null)}
                        className="h-8 px-4 rounded-lg bg-[var(--velox-accent)] hover:brightness-110 text-[#080B12] text-[12px] font-semibold cursor-pointer"
                      >
                        Go back
                      </button>
                    </div>
                  </div>
                ) : (
                  <RemoteTerminalPane
                    peer={selectedRemote.peer}
                    sessionId={selectedRemote.sessionId}
                    remoteSession={selectedRemoteSession}
                    peerOnline={selectedPeerObj?.online ?? true}
                    tasks={tasks}
                    fontFamily={fontFamily}
                    fontSize={fontSize}
                    onSendTask={sendTask}
                    onBack={() => setSelectedRemote(null)}
                  />
                ))}
            </>
          )}
        </main>
      </div>

      {/* Subtle floating action toast */}
      {toast && (
        <div
          role="status"
          aria-live="polite"
          className="pointer-events-none fixed bottom-4 right-4 z-50 px-3.5 py-2 rounded-xl bg-[var(--velox-elevated)]/95 border border-white/[0.12] text-[12px] font-medium text-[var(--velox-ink)] shadow-[0_14px_34px_-10px_rgba(0,0,0,0.85)] animate-velox-toast-in"
        >
          {toast}
        </div>
      )}

      <Suspense fallback={null}>
        {launcherOpen && (
          <AgentLauncherModal
            open={launcherOpen}
            onClose={closeLauncher}
            presets={presets}
            recentDirs={recentDirs}
            defaultCwd={defaultCwd}
            initialKind={launcherKind}
            onLaunch={handleLaunch}
          />
        )}

        {mcpDrawerOpen && (
          <McpDrawer
            open={mcpDrawerOpen}
            onClose={handleCloseMcpDrawer}
            mcp={mcp}
            telemetry={telemetry}
            onSleepBackend={sleepMcpBackend}
            onSleepAllMcps={sleepAllMcps}
            onSleepIdleSessions={sleepIdleSessions}
          />
        )}

        {settingsOpen && (
          <SettingsModal
            open={settingsOpen}
            onClose={handleCloseSettings}
            config={config}
            team={team}
            themePreference={themePreference}
            onSelectTheme={handleSelectTheme}
            onSave={handleSaveConfig}
          />
        )}

        {paletteOpen && (
          <CommandPalette
            open={paletteOpen}
            onClose={handleClosePalette}
            sessions={sessions}
            presets={presets}
            activeSession={selectedRemote ? null : activeSession}
            animatedBackgrounds={animated}
            themePreference={themePreference}
            onSelectSession={handleSelectLocalSession}
            onQuickLaunch={handleQuickLaunch}
            onOpenLauncher={handleOpenDefaultLauncher}
            onOpenThemes={handleOpenThemes}
            onSelectTheme={handleSelectTheme}
            onToggleSidebar={handleToggleSidebar}
            onToggleMcpDrawer={handleToggleMcpDrawer}
            onOpenSettings={handleOpenSettings}
            onOpenShortcuts={handleOpenShortcuts}
            onSleepActive={handleSleepActive}
            onSleepIdle={handleSleepIdleSessions}
            onToggleAnimated={handleToggleAnimated}
            onClearTerminal={handleClearTerminal}
            onZoomIn={handleZoomIn}
            onZoomOut={handleZoomOut}
            onZoomReset={handleZoomReset}
          />
        )}

        {themesOpen && (
          <ThemePickerModal
            open={themesOpen}
            onClose={handleCloseThemes}
            selected={themePreference}
            animatedBackgrounds={animated}
            onSelectTheme={handleSelectTheme}
            onToggleAnimated={handleToggleAnimated}
          />
        )}

        {shortcutsOpen && (
          <ShortcutsModal open={shortcutsOpen} onClose={handleCloseShortcuts} />
        )}
      </Suspense>

      <TaskInbox pendingIncoming={pendingIncoming} onRespond={respond} />
    </div>
  );
}
