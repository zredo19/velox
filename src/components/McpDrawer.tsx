import { useEffect, useState } from "react";
import { Moon, Server, X, Zap } from "lucide-react";
import type { McpGatewaySnapshot, SystemTelemetry } from "../types/ipc";

export interface McpDrawerProps {
  open: boolean;
  onClose: () => void;
  mcp: McpGatewaySnapshot | null;
  telemetry: SystemTelemetry | null;
  onSleepBackend: (name: string) => Promise<void>;
  onSleepAllMcps: () => Promise<void>;
  onSleepIdleSessions: () => Promise<void>;
}

function formatIdleCountdown(ms: number | null): string {
  if (ms === null || ms <= 0) return "";
  const totalSec = Math.ceil(ms / 1000);
  const mins = Math.floor(totalSec / 60);
  const secs = totalSec % 60;
  if (mins > 0) {
    return `sleeps in ${mins}m ${secs}s`;
  }
  return `sleeps in ${secs}s`;
}

function formatModeLabel(mode: McpGatewaySnapshot["mode"]): {
  label: string;
  className: string;
} {
  switch (mode) {
    case "embedded":
      return {
        label: "Embedded",
        className: "bg-emerald-500/15 text-emerald-300 border-emerald-500/25",
      };
    case "externalAttached":
      return {
        label: "External",
        className: "bg-sky-500/15 text-sky-300 border-sky-500/25",
      };
    case "error":
      return {
        label: "Error",
        className: "bg-rose-500/15 text-rose-300 border-rose-500/25",
      };
    case "disabled":
      return {
        label: "Disabled",
        className: "bg-slate-500/15 text-slate-300 border-slate-500/25",
      };
  }
}

export function McpDrawer({
  open,
  onClose,
  mcp,
  telemetry,
  onSleepBackend,
  onSleepAllMcps,
  onSleepIdleSessions,
}: McpDrawerProps) {
  const [busyKey, setBusyKey] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onClose();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [open, onClose]);

  if (!open) return null;

  const modeBadge = mcp
    ? formatModeLabel(mcp.mode)
    : {
      label: "Offline",
      className: "bg-slate-500/15 text-slate-400 border-slate-500/25",
    };

  const backends = mcp?.backends ?? [];
  const totalRam = Math.round(telemetry?.totalMemoryMb ?? 0);
  const appRam = Math.round(telemetry?.appMemoryMb ?? 0);
  const agentsRam = Math.round(telemetry?.sessionsMemoryMb ?? 0);
  const mcpRam = Math.round(telemetry?.mcpMemoryMb ?? 0);
  const ramDenom = Math.max(1, appRam + agentsRam + mcpRam);
  const appPct = (appRam / ramDenom) * 100;
  const agentsPct = (agentsRam / ramDenom) * 100;
  const mcpPct = (mcpRam / ramDenom) * 100;

  const activeSessions = telemetry?.activeSessions ?? 0;
  const sleepingSessions = telemetry?.sleepingSessions ?? 0;
  const activeMcps = mcp?.activeBackends ?? 0;
  const totalMcps = mcp?.totalBackends ?? 0;

  const runAction = async (key: string, fn: () => Promise<void>) => {
    setBusyKey(key);
    try {
      await fn();
    } finally {
      setBusyKey(null);
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex justify-end">
      {/* Backdrop */}
      <div
        className="fixed inset-0 bg-black/45 backdrop-blur-[1.5px] animate-velox-fade-in"
        onClick={onClose}
      />

      {/* Slide-over drawer */}
      <aside className="relative z-10 w-[378px] max-w-[92vw] h-full bg-[var(--velox-surface)] border-l border-white/[0.1] shadow-2xl flex flex-col animate-velox-slide-in">
        {/* Header */}
        <div className="px-4 py-3.5 border-b border-white/[0.07] flex items-start justify-between gap-3">
          <div className="min-w-0">
            <div className="flex items-center gap-2">
              <Server className="w-4 h-4 text-[var(--velox-accent-soft)] shrink-0" />
              <h2 className="text-[14px] font-medium text-[var(--velox-ink)]">
                Lazy MCP Gateway
              </h2>
            </div>
            <div className="flex items-center gap-2 mt-1.5">
              <span
                className={`px-1.5 py-0.5 text-[11px] font-medium rounded border ${modeBadge.className}`}
              >
                {modeBadge.label}
              </span>
              <span className="text-[11.5px] font-mono text-[var(--velox-ink-soft)] truncate">
                {mcp?.listenAddr ?? "127.0.0.1:43111"}
              </span>
            </div>
          </div>

          <button
            type="button"
            onClick={onClose}
            aria-label="Close MCP drawer"
            className="h-7 w-7 flex items-center justify-center rounded-md text-[var(--velox-ink-soft)] hover:text-[var(--velox-ink)] hover:bg-[var(--velox-hover)] transition-colors cursor-pointer"
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        {/* Summary metrics */}
        <div className="grid grid-cols-3 border-b border-white/[0.07] divide-x divide-white/[0.06] bg-[var(--velox-canvas)]/55">
          <div className="px-3.5 py-2.5">
            <div className="text-[11px] text-[var(--velox-ink-soft)]">Total RAM</div>
            <div className="text-[14px] font-mono font-semibold tabular-nums text-[var(--velox-ink)] mt-0.5">
              {totalRam} MB
            </div>
          </div>
          <div className="px-3.5 py-2.5">
            <div className="text-[11px] text-[var(--velox-ink-soft)]">Sessions</div>
            <div className="text-[13px] font-mono tabular-nums text-[var(--velox-ink)] mt-0.5">
              {activeSessions}
              <span className="text-[var(--velox-ink-muted)]"> / {sleepingSessions}z</span>
            </div>
          </div>
          <div className="px-3.5 py-2.5">
            <div className="text-[11px] text-[var(--velox-ink-soft)]">MCP active</div>
            <div className="text-[13px] font-mono tabular-nums text-[var(--velox-ink)] mt-0.5">
              {activeMcps}
              <span className="text-[var(--velox-ink-muted)]"> / {totalMcps}</span>
            </div>
          </div>
        </div>

        {/* Visual RAM distribution bar */}
        <div className="px-4 py-2.5 border-b border-white/[0.07] bg-[var(--velox-canvas)]/35">
          <div className="h-1.5 w-full rounded-full bg-white/[0.06] overflow-hidden flex gap-px">
            <div
              className="h-full bg-slate-400/75 transition-all duration-300"
              style={{ width: `${appPct}%` }}
              title={`Velox App: ${appRam} MB`}
            />
            <div
              className="h-full bg-[var(--velox-accent)] transition-all duration-300"
              style={{ width: `${agentsPct}%` }}
              title={`Agents: ${agentsRam} MB`}
            />
            <div
              className="h-full bg-emerald-400 transition-all duration-300"
              style={{ width: `${mcpPct}%` }}
              title={`MCP Backends: ${mcpRam} MB`}
            />
          </div>
          <div className="mt-1.5 flex items-center justify-between text-[10.5px] font-mono tabular-nums text-[var(--velox-ink-muted)]">
            <span className="inline-flex items-center gap-1">
              <span className="w-1.5 h-1.5 rounded-full bg-slate-400/75" />
              App {appRam}M
            </span>
            <span className="inline-flex items-center gap-1">
              <span className="w-1.5 h-1.5 rounded-full bg-[var(--velox-accent)]" />
              Agents {agentsRam}M
            </span>
            <span className="inline-flex items-center gap-1">
              <span className="w-1.5 h-1.5 rounded-full bg-emerald-400" />
              MCP {mcpRam}M
            </span>
          </div>
        </div>

        {/* Quick power actions */}
        <div className="px-4 py-3 border-b border-white/[0.07] flex gap-2">
          <button
            type="button"
            disabled={busyKey !== null || activeMcps === 0}
            onClick={() => void runAction("all-mcps", onSleepAllMcps)}
            className="flex-1 h-8 px-2.5 rounded-lg text-[12px] font-medium flex items-center justify-center gap-1.5 bg-[var(--velox-elevated)] hover:bg-[var(--velox-hover)] active:scale-[0.98] disabled:opacity-40 disabled:cursor-not-allowed text-[var(--velox-ink)] border border-white/[0.08] transition-all cursor-pointer"
          >
            <Moon className="w-3.5 h-3.5 text-[var(--velox-accent-soft)]" />
            <span>Sleep all MCPs</span>
          </button>
          <button
            type="button"
            disabled={busyKey !== null || activeSessions <= 1}
            onClick={() => void runAction("idle-sessions", onSleepIdleSessions)}
            className="flex-1 h-8 px-2.5 rounded-lg text-[12px] font-medium flex items-center justify-center gap-1.5 bg-[var(--velox-elevated)] hover:bg-[var(--velox-hover)] active:scale-[0.98] disabled:opacity-40 disabled:cursor-not-allowed text-[var(--velox-ink)] border border-white/[0.08] transition-all cursor-pointer"
          >
            <Zap className="w-3.5 h-3.5 text-emerald-300" />
            <span>Sleep idle tabs</span>
          </button>
        </div>

        {/* Error banner if any */}
        {mcp?.error && (
          <div className="mx-4 mt-3 px-3 py-2 rounded-lg bg-rose-500/10 border border-rose-500/25 text-[12px] text-rose-300">
            {mcp.error}
          </div>
        )}

        {/* Backends list */}
        <div className="flex-1 overflow-y-auto px-3 py-3 space-y-1.5">
          {backends.length === 0 ? (
            <div className="py-12 px-4 text-center">
              <p className="text-[13px] text-[var(--velox-ink-secondary)]">
                No MCP backends configured
              </p>
              <p className="text-[12px] text-[var(--velox-ink-muted)] mt-1">
                Point backends.json in Settings to enable scale-to-zero stdio MCP
                management.
              </p>
            </div>
          ) : (
            backends.map((backend) => {
              const countdown = formatIdleCountdown(backend.remainingIdleMs);
              const isBusy = busyKey === backend.name;

              return (
                <div
                  key={backend.name}
                  className="px-3 py-2.5 rounded-xl bg-[var(--velox-canvas)]/70 border border-white/[0.06] hover:border-white/[0.11] transition-colors flex items-center justify-between gap-2"
                >
                  <div className="min-w-0">
                    <div className="flex items-center gap-2">
                      <span
                        className={`w-1.5 h-1.5 rounded-full shrink-0 ${backend.running
                            ? "bg-emerald-400 shadow-[0_0_6px_rgba(52,211,153,0.75)]"
                            : "bg-[var(--velox-ink-faint)]"
                          }`}
                      />
                      <span className="text-[12.5px] font-medium text-[var(--velox-ink)] truncate">
                        {backend.name}
                      </span>
                    </div>
                    <div className="text-[11.5px] font-mono tabular-nums text-[var(--velox-ink-soft)] mt-1 pl-3.5">
                      {backend.running ? (
                        <>
                          <span className="text-emerald-300/90">
                            Active · {Math.round(backend.memoryMb)} MB
                          </span>
                          {countdown && (
                            <span className="text-[var(--velox-ink-muted)]"> · {countdown}</span>
                          )}
                        </>
                      ) : (
                        <span>
                          Sleeping · 0 MB
                          {backend.cachedMethods > 0
                            ? ` · ${backend.cachedMethods} cached`
                            : ""}
                        </span>
                      )}
                    </div>
                  </div>

                  {backend.running && (
                    <button
                      type="button"
                      disabled={isBusy}
                      onClick={() =>
                        void runAction(backend.name, () =>
                          onSleepBackend(backend.name),
                        )
                      }
                      className="shrink-0 h-7 px-2.5 rounded-md text-[11.5px] font-medium text-[var(--velox-ink-secondary)] hover:text-[var(--velox-ink)] bg-[var(--velox-elevated)] hover:bg-[var(--velox-hover)] active:scale-95 border border-white/[0.07] transition-all cursor-pointer disabled:opacity-50"
                    >
                      {isBusy ? "..." : "Sleep"}
                    </button>
                  )}
                </div>
              );
            })
          )}
        </div>
      </aside>
    </div>
  );
}
