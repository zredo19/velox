import { memo, useCallback, useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  Copy,
  Cpu,
  Keyboard,
  Minus,
  Palette,
  PanelLeft,
  Plus,
  Search,
  Settings,
  Square,
  X,
} from "lucide-react";
import type {
  McpGatewaySnapshot,
  SessionRecord,
  SystemTelemetry,
} from "../types/ipc";
import { AgentIcon, VeloxLogo } from "./icons/AgentIcons";

export interface TitleBarProps {
  sidebarOpen: boolean;
  onToggleSidebar: () => void;
  onOpenLauncher: () => void;
  onOpenPalette: () => void;
  onOpenThemes: () => void;
  onOpenShortcuts: () => void;
  onCopyPath?: (path: string) => void;
  activeSession: SessionRecord | null;
  blockedCount?: number;
  workingCount?: number;
  telemetry: SystemTelemetry | null;
  mcp: McpGatewaySnapshot | null;
  mcpDrawerOpen: boolean;
  onToggleMcpDrawer: () => void;
  onOpenSettings?: () => void;
}

function getPathBasename(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

function formatMb(mb: number): string {
  if (mb >= 1024) {
    return `${(mb / 1024).toFixed(1)} GB`;
  }
  return `${Math.round(mb)} MB`;
}

export const TitleBar = memo(function TitleBar({
  sidebarOpen,
  onToggleSidebar,
  onOpenLauncher,
  onOpenPalette,
  onOpenThemes,
  onOpenShortcuts,
  onCopyPath,
  activeSession,
  blockedCount = 0,
  workingCount = 0,
  telemetry,
  mcp,
  mcpDrawerOpen,
  onToggleMcpDrawer,
  onOpenSettings,
}: TitleBarProps) {
  const [maximized, setMaximized] = useState(false);

  const refreshMaximized = useCallback(async () => {
    try {
      const isMax = await getCurrentWindow().isMaximized();
      setMaximized(isMax);
    } catch {
      // Running outside Tauri window context
    }
  }, []);

  useEffect(() => {
    void refreshMaximized();
    let unlisten: (() => void) | undefined;
    try {
      void getCurrentWindow()
        .onResized(() => {
          void refreshMaximized();
        })
        .then((fn) => {
          unlisten = fn;
        });
    } catch {
      // Running outside Tauri window context
    }
    return () => {
      unlisten?.();
    };
  }, [refreshMaximized]);

  const handleMinimize = useCallback(() => {
    try {
      void getCurrentWindow().minimize();
    } catch {
      // Ignore outside Tauri
    }
  }, []);

  const handleToggleMaximize = useCallback(() => {
    try {
      void getCurrentWindow().toggleMaximize();
    } catch {
      // Ignore outside Tauri
    }
  }, []);

  const handleClose = useCallback(() => {
    try {
      void getCurrentWindow().close();
    } catch {
      window.close();
    }
  }, []);

  const totalMb = telemetry?.totalMemoryMb ?? 0;
  const ramTooltip = telemetry
    ? `Total ${formatMb(telemetry.totalMemoryMb)} — App ${formatMb(telemetry.appMemoryMb)} · Agents ${formatMb(telemetry.sessionsMemoryMb)} · MCP ${formatMb(telemetry.mcpMemoryMb)} (click to inspect)`
    : "Memory telemetry";

  const activeMcps = mcp?.activeBackends ?? 0;
  const totalMcps = mcp?.totalBackends ?? 0;
  const cwdBasename = activeSession ? getPathBasename(activeSession.cwd) : "";

  return (
    <header className="h-10 shrink-0 bg-[var(--velox-surface)] border-b border-white/[0.07] select-none flex items-center justify-between pl-2.5 pr-0 velox-chrome-transition">
      {/* Left navigation & quick-launch controls */}
      <div className="flex items-center gap-1.5">
        <button
          type="button"
          onClick={onToggleSidebar}
          title={sidebarOpen ? "Hide sidebar (Ctrl+Shift+B)" : "Show sidebar (Ctrl+Shift+B)"}
          aria-label="Toggle sidebar"
          className={`relative h-7 w-7 flex items-center justify-center rounded-md transition-all duration-150 active:scale-95 cursor-pointer ${
            sidebarOpen
              ? "text-[var(--velox-ink-secondary)] hover:text-[var(--velox-ink)] hover:bg-[var(--velox-hover)]"
              : "text-[var(--velox-ink-muted)] hover:text-[var(--velox-ink)] hover:bg-[var(--velox-hover)]"
          }`}
        >
          <PanelLeft className="w-3.5 h-3.5" />
          {/* Surface background tab alerts when the sidebar is collapsed */}
          {!sidebarOpen && (blockedCount > 0 || workingCount > 0) && (
            <span
              className={`absolute top-1 right-1 w-1.5 h-1.5 rounded-full ${
                blockedCount > 0 ? "bg-rose-500 animate-pulse" : "bg-[var(--velox-accent)]"
              }`}
            />
          )}
        </button>

        <div className="flex items-center gap-1.5 pl-0.5 pr-1">
          <VeloxLogo className="w-[18px] h-[18px]" />
          <span className="text-[13px] font-medium tracking-[-0.01em] text-[var(--velox-ink)]">
            Velox
          </span>
        </div>

        <button
          type="button"
          onClick={onOpenLauncher}
          title="New agent session (Ctrl+N)"
          aria-label="New agent session"
          className="h-6 px-2 flex items-center gap-1 rounded-md text-[12px] font-medium text-[var(--velox-accent-soft)] bg-[var(--velox-accent)]/14 hover:bg-[var(--velox-accent)]/24 active:scale-[0.97] border border-[var(--velox-accent)]/28 transition-all duration-150 cursor-pointer"
        >
          <Plus className="w-3.5 h-3.5" />
          <span className="hidden sm:inline">New</span>
        </button>

        <button
          type="button"
          onClick={onOpenPalette}
          title="Command palette (Ctrl+K)"
          aria-label="Open command palette"
          className="hidden lg:inline-flex h-6 px-2 items-center gap-1.5 rounded-md bg-white/[0.03] hover:bg-[var(--velox-hover)] border border-white/[0.06] text-[11.5px] text-[var(--velox-ink-muted)] hover:text-[var(--velox-ink-secondary)] transition-colors cursor-pointer"
        >
          <Search className="w-3 h-3" />
          <span>Search…</span>
          <span className="text-[10px] font-mono text-[var(--velox-ink-faint)] pl-1">Ctrl K</span>
        </button>
      </div>

      {/* Center draggable region with active session breadcrumb */}
      <div
        data-tauri-drag-region
        onDoubleClick={handleToggleMaximize}
        className="flex-1 h-full flex items-center justify-center gap-2 px-4 min-w-0 cursor-default"
      >
        {activeSession ? (
          <div
            data-tauri-drag-region
            className="flex items-center gap-2 min-w-0 max-w-md"
          >
            <AgentIcon
              kind={activeSession.activeAgent ?? activeSession.agentKind}
              className="w-3.5 h-3.5 shrink-0 pointer-events-none"
              active={activeSession.status !== "sleeping"}
            />
            <span
              data-tauri-drag-region
              className="text-[12.5px] font-medium text-[var(--velox-ink)] truncate"
            >
              {activeSession.title}
            </span>
            {activeSession.profile && (
              <span
                className="text-[10.5px] px-1.5 py-0.2 rounded bg-white/[0.06] shrink-0 pointer-events-none"
                style={{
                  color:
                    activeSession.profile === "trabajo" ? "#7DD3C8" : "#E8B4D9",
                }}
              >
                {activeSession.profile}
              </span>
            )}
            <button
              type="button"
              onClick={() => onCopyPath?.(activeSession.cwd)}
              title={`Copy path: ${activeSession.cwd}`}
              className="text-[11.5px] font-mono text-[var(--velox-ink-muted)] hover:text-[var(--velox-ink-secondary)] truncate hidden md:inline px-1.5 py-0.5 rounded hover:bg-white/[0.05] transition-colors cursor-pointer"
            >
              {cwdBasename}
            </button>
          </div>
        ) : (
          <span
            data-tauri-drag-region
            className="text-[12px] text-[var(--velox-ink-muted)] pointer-events-none"
          >
            Ready
          </span>
        )}
      </div>

      {/* Right telemetry pills & native Windows window controls */}
      <div className="flex items-center h-full">
        <div className="flex items-center gap-1.5 pr-2">
          <button
            type="button"
            onClick={onToggleMcpDrawer}
            title={ramTooltip}
            className="h-6 px-2 flex items-center gap-1.5 rounded-md bg-white/[0.03] hover:bg-[var(--velox-hover)] border border-white/[0.06] text-[11.5px] font-mono tabular-nums text-[var(--velox-ink-secondary)] hover:text-[var(--velox-ink)] transition-colors cursor-pointer"
          >
            <Cpu className="w-3 h-3 text-[var(--velox-ink-muted)]" />
            <span>{formatMb(totalMb)}</span>
          </button>

          <button
            type="button"
            onClick={onToggleMcpDrawer}
            title="Lazy MCP Gateway controls (Ctrl+Shift+M)"
            aria-label="Toggle Lazy MCP Gateway drawer"
            className={`h-6 px-2 flex items-center gap-1.5 rounded-md border text-[11.5px] font-mono tabular-nums transition-all duration-150 cursor-pointer ${
              mcpDrawerOpen
                ? "bg-[var(--velox-active)] border-[var(--velox-accent)]/40 text-[var(--velox-ink)]"
                : "bg-white/[0.03] border-white/[0.06] text-[var(--velox-ink-secondary)] hover:text-[var(--velox-ink)] hover:bg-[var(--velox-hover)]"
            }`}
          >
            <span
              className={`w-1.5 h-1.5 rounded-full transition-colors ${
                activeMcps > 0
                  ? "bg-emerald-400 shadow-[0_0_6px_rgba(52,211,153,0.75)]"
                  : "bg-[var(--velox-ink-faint)]"
              }`}
            />
            <span>
              MCP {activeMcps}/{totalMcps}
            </span>
          </button>

          <button
            type="button"
            onClick={onOpenThemes}
            title="Themes & wallpapers (Ctrl+Shift+T)"
            aria-label="Open theme picker"
            className="h-6 w-6 flex items-center justify-center rounded-md text-[var(--velox-accent-soft)] hover:text-[var(--velox-ink)] hover:bg-[var(--velox-hover)] transition-colors cursor-pointer"
          >
            <Palette className="w-3.5 h-3.5" />
          </button>

          <button
            type="button"
            onClick={onOpenShortcuts}
            title="Keyboard shortcuts (Ctrl+/)"
            aria-label="Keyboard shortcuts"
            className="h-6 w-6 flex items-center justify-center rounded-md text-[var(--velox-ink-soft)] hover:text-[var(--velox-ink)] hover:bg-[var(--velox-hover)] transition-colors cursor-pointer"
          >
            <Keyboard className="w-3.5 h-3.5" />
          </button>

          {onOpenSettings && (
            <button
              type="button"
              onClick={onOpenSettings}
              title="Settings (Ctrl+,)"
              aria-label="Open settings"
              className="h-6 w-6 flex items-center justify-center rounded-md text-[var(--velox-ink-soft)] hover:text-[var(--velox-ink)] hover:bg-[var(--velox-hover)] transition-colors cursor-pointer"
            >
              <Settings className="w-3.5 h-3.5" />
            </button>
          )}
        </div>

        {/* Native Windows caption buttons */}
        <div className="flex items-center h-full border-l border-white/[0.06]">
          <button
            type="button"
            onClick={handleMinimize}
            aria-label="Minimize window"
            className="h-full w-11 flex items-center justify-center text-[var(--velox-ink-secondary)] hover:bg-white/[0.06] hover:text-[var(--velox-ink)] transition-colors"
          >
            <Minus className="w-3.5 h-3.5" />
          </button>
          <button
            type="button"
            onClick={handleToggleMaximize}
            aria-label={maximized ? "Restore window" : "Maximize window"}
            className="h-full w-11 flex items-center justify-center text-[var(--velox-ink-secondary)] hover:bg-white/[0.06] hover:text-[var(--velox-ink)] transition-colors"
          >
            {maximized ? (
              <Copy className="w-3 h-3" />
            ) : (
              <Square className="w-3 h-3" />
            )}
          </button>
          <button
            type="button"
            onClick={handleClose}
            aria-label="Close window"
            className="h-full w-11 flex items-center justify-center text-[var(--velox-ink-secondary)] hover:bg-[#E11D48] hover:text-white transition-colors"
          >
            <X className="w-3.5 h-3.5" />
          </button>
        </div>
      </div>
    </header>
  );
});
