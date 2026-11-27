import { memo } from "react";
import { Command, Folder, Plus } from "lucide-react";
import type { AgentPreset } from "../types/ipc";
import { AgentIcon, VeloxLogo } from "./icons/AgentIcons";

export interface EmptyStateProps {
  presets: AgentPreset[];
  defaultCwd: string;
  onQuickLaunch: (presetId: string) => void;
  onOpenLauncher: () => void;
  onOpenPalette?: () => void;
}

function getFolderName(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

export const EmptyState = memo(function EmptyState({
  presets,
  defaultCwd,
  onQuickLaunch,
  onOpenLauncher,
  onOpenPalette,
}: EmptyStateProps) {
  const folderName = getFolderName(defaultCwd || ".");

  return (
    <div className="relative flex-1 h-full w-full bg-[var(--velox-canvas)] flex flex-col items-center justify-center px-6 py-10 overflow-y-auto velox-chrome-transition">
      {/* Subtle atmospheric glow */}
      <div
        aria-hidden="true"
        className="pointer-events-none absolute inset-0"
        style={{
          background:
            "radial-gradient(circle 480px at 50% 35%, color-mix(in srgb, var(--velox-accent) 10%, transparent), transparent 72%)",
        }}
      />

      <div className="relative w-full max-w-[548px] flex flex-col items-center">
        <div className="mb-5 p-2 rounded-2xl bg-[var(--velox-surface)] border border-white/[0.08] shadow-xl animate-velox-pop-in">
          <VeloxLogo className="w-10 h-10" />
        </div>

        <h1 className="text-[21px] font-medium tracking-[-0.02em] text-[var(--velox-ink)]">
          Start an agent session
        </h1>

        <div className="mt-2 flex items-center gap-1.5 text-[12.5px] text-[var(--velox-ink-soft)]">
          <Folder className="w-3.5 h-3.5 text-[var(--velox-ink-muted)]" />
          <span className="font-mono">{folderName}</span>
          <span className="text-[var(--velox-ink-faint)]">·</span>
          <span>Idle sessions sleep at 0 MB</span>
        </div>

        <div className="grid grid-cols-2 gap-2.5 w-full mt-7">
          {presets.map((preset, idx) => (
            <button
              key={preset.id}
              type="button"
              onClick={() => onQuickLaunch(preset.id)}
              style={{
                animationDelay: `${idx * 35}ms`,
              }}
              className="group text-left px-3.5 py-3 rounded-xl bg-[var(--velox-surface)] hover:bg-[var(--velox-elevated)] active:scale-[0.985] border border-white/[0.07] hover:border-white/[0.16] transition-all duration-150 cursor-pointer flex items-start gap-3 animate-velox-card-rise"
            >
              <div
                className="mt-0.5 w-8 h-8 rounded-lg bg-[var(--velox-canvas)] border border-white/[0.07] flex items-center justify-center shrink-0 transition-transform duration-150 group-hover:scale-105"
              >
                <AgentIcon kind={preset.kind} className="w-4 h-4" active />
              </div>

              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-1.5">
                  <span className="text-[13px] font-medium text-[var(--velox-ink)] truncate">
                    {preset.label}
                  </span>
                  {preset.profile && (
                    <span
                      className="text-[10.5px] px-1.5 py-0.2 rounded bg-white/[0.06] font-medium"
                      style={{
                        color:
                          preset.profile === "trabajo" ? "#7DD3C8" : "#E8B4D9",
                      }}
                    >
                      {preset.profile}
                    </span>
                  )}
                </div>
                <p className="text-[11.5px] text-[var(--velox-ink-soft)] mt-0.5 truncate">
                  {preset.description}
                </p>
              </div>
            </button>
          ))}
        </div>

        <div className="mt-7 flex flex-wrap items-center justify-center gap-3 text-[12px] text-[var(--velox-ink-muted)]">
          <button
            type="button"
            onClick={onOpenLauncher}
            className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-[var(--velox-accent-soft)] bg-[var(--velox-accent)]/12 hover:bg-[var(--velox-accent)]/22 active:scale-[0.98] border border-[var(--velox-accent)]/28 transition-all cursor-pointer"
          >
            <Plus className="w-3.5 h-3.5" />
            <span>Open launcher</span>
            <span className="velox-kbd ml-0.5">Ctrl+N</span>
          </button>

          {onOpenPalette && (
            <button
              type="button"
              onClick={onOpenPalette}
              className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-[var(--velox-ink-secondary)] hover:text-[var(--velox-ink)] bg-[var(--velox-surface)] hover:bg-[var(--velox-elevated)] active:scale-[0.98] border border-white/[0.08] transition-all cursor-pointer"
            >
              <Command className="w-3.5 h-3.5" />
              <span>Command palette</span>
              <span className="velox-kbd ml-0.5">Ctrl+K</span>
            </button>
          )}
        </div>
      </div>
    </div>
  );
});
