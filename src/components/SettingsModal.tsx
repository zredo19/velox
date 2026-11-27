import { useEffect, useState } from "react";
import { Check, FolderOpen, Palette, Settings, X } from "lucide-react";
import { veloxApi } from "../lib/tauri";
import { THEME_OPTIONS } from "../themes/registry";
import type { ThemePreference } from "../themes/types";
import type { TeamSnapshot, VeloxConfig } from "../types/ipc";
import {
  DEFAULT_TEAM_CONFIG,
  TeamSettingsSection,
  validateTeamConfig,
} from "./team/TeamSettingsSection";

export interface SettingsModalProps {
  open: boolean;
  onClose: () => void;
  config: VeloxConfig | null;
  team?: TeamSnapshot | null;
  themePreference: ThemePreference;
  onSelectTheme: (pref: ThemePreference) => void;
  onSave: (next: VeloxConfig) => Promise<void>;
}

const SCROLLBACK_OPTIONS = [
  { label: "1 MB", bytes: 1 * 1024 * 1024 },
  { label: "2 MB", bytes: 2 * 1024 * 1024 },
  { label: "4 MB", bytes: 4 * 1024 * 1024 },
  { label: "8 MB", bytes: 8 * 1024 * 1024 },
];

const HIBERNATE_PRESETS = [0, 10, 20, 30, 60];

const FONT_PRESETS = [
  {
    label: "JetBrains Mono",
    value: "'JetBrains Mono', 'Cascadia Code', Consolas, monospace",
  },
  {
    label: "Cascadia Code",
    value: "'Cascadia Code', 'JetBrains Mono', Consolas, monospace",
  },
  {
    label: "Fira Code",
    value: "'Fira Code', 'JetBrains Mono', Consolas, monospace",
  },
  {
    label: "Consolas",
    value: "Consolas, 'Courier New', monospace",
  },
];

function ToggleSwitch({
  checked,
  onChange,
}: {
  checked: boolean;
  onChange: (next: boolean) => void;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      onClick={() => onChange(!checked)}
      className={`relative inline-flex h-5 w-9 shrink-0 items-center rounded-full border transition-colors duration-180 cursor-pointer ${
        checked
          ? "bg-[var(--velox-accent)] border-[var(--velox-accent)]"
          : "bg-[var(--velox-elevated)] border-white/[0.12]"
      }`}
    >
      <span
        className={`inline-block h-3.5 w-3.5 rounded-full bg-white shadow-sm transition-transform duration-180 ${
          checked ? "translate-x-[18px]" : "translate-x-[2px]"
        }`}
      />
    </button>
  );
}

export function SettingsModal({
  open,
  onClose,
  config,
  team = null,
  themePreference,
  onSelectTheme,
  onSave,
}: SettingsModalProps) {
  const [draft, setDraft] = useState<VeloxConfig | null>(config);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);

  useEffect(() => {
    if (open && config) {
      setDraft({
        ...config,
        team: config.team
          ? {
              ...config.team,
              peers: config.team.peers.map((p) => ({ ...p })),
            }
          : { ...DEFAULT_TEAM_CONFIG, peers: [] },
      });
      setSaveError(null);
    }
  }, [open, config]);

  const isDirty = Boolean(
    draft && config && JSON.stringify(draft) !== JSON.stringify(config),
  );

  const teamDraft = draft?.team ?? DEFAULT_TEAM_CONFIG;
  const teamValidation = validateTeamConfig(teamDraft);

  const handleSave = async () => {
    if (!draft) return;
    if (!teamValidation.valid) {
      setSaveError(teamValidation.summaryError);
      return;
    }
    setSaving(true);
    setSaveError(null);
    try {
      const normalizedTeam = {
        ...teamDraft,
        displayName: teamDraft.displayName.trim(),
        bindHost: teamDraft.bindHost.trim(),
        peers: teamDraft.peers.map((p) => ({
          ...p,
          name: p.name.trim(),
          host: p.host.trim(),
        })),
      };
      await onSave({ ...draft, team: normalizedTeam });
      onClose();
    } catch (err) {
      setSaveError(
        err instanceof Error && err.message
          ? err.message
          : "Failed to save settings",
      );
    } finally {
      setSaving(false);
    }
  };

  useEffect(() => {
    if (!open) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        onClose();
      } else if (e.ctrlKey && e.key.toLowerCase() === "s") {
        e.preventDefault();
        void handleSave();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  });

  if (!open || !draft) return null;

  const handlePickDefaultCwd = async () => {
    const selected = await veloxApi.pickDirectory(draft.defaultCwd);
    if (selected) {
      setDraft((prev) => (prev ? { ...prev, defaultCwd: selected } : prev));
    }
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-[2px] p-4 animate-velox-fade-in"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
      role="dialog"
      aria-modal="true"
      aria-labelledby="velox-settings-title"
    >
      <div className="w-full max-w-[548px] max-h-[86vh] bg-[var(--velox-surface)] border border-white/[0.1] rounded-xl shadow-[0_28px_70px_-20px_rgba(0,0,0,0.85)] flex flex-col overflow-hidden animate-velox-pop-in">
        {/* Header */}
        <div className="px-5 py-3.5 border-b border-white/[0.07] flex items-center justify-between">
          <div className="flex items-center gap-2">
            <Settings className="w-4 h-4 text-[var(--velox-accent-soft)]" />
            <h2
              id="velox-settings-title"
              className="text-[14.5px] font-medium text-[var(--velox-ink)]"
            >
              Settings
            </h2>
            {isDirty && (
              <span className="text-[10.5px] px-1.5 py-0.5 rounded-full bg-[var(--velox-accent)]/15 text-[var(--velox-accent-soft)] border border-[var(--velox-accent)]/30">
                Unsaved
              </span>
            )}
          </div>
          <button
            type="button"
            onClick={onClose}
            aria-label="Close settings"
            className="h-7 w-7 flex items-center justify-center rounded-md text-[var(--velox-ink-soft)] hover:text-[var(--velox-ink)] hover:bg-[var(--velox-hover)] transition-colors cursor-pointer"
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        {/* Body */}
        <div className="flex-1 overflow-y-auto p-5 space-y-6">
          {/* Section 1: Memory & Hibernation */}
          <section className="space-y-3.5">
            <h3 className="text-[12.5px] font-medium text-[var(--velox-ink)]">
              Memory & hibernation
            </h3>

            <div className="flex items-center justify-between gap-4 px-3.5 py-2.5 rounded-xl bg-[var(--velox-canvas)]/70 border border-white/[0.06]">
              <div>
                <div className="text-[12.5px] text-[var(--velox-ink)]">
                  Lazy restore on startup
                </div>
                <div className="text-[11.5px] text-[var(--velox-ink-soft)]">
                  Load saved sessions as sleeping (0 MB RAM) until clicked
                </div>
              </div>
              <ToggleSwitch
                checked={draft.lazyRestoreOnStartup}
                onChange={(checked) =>
                  setDraft({ ...draft, lazyRestoreOnStartup: checked })
                }
              />
            </div>

            <div>
              <div className="flex items-center justify-between mb-1.5">
                <span className="text-[12px] text-[var(--velox-ink-secondary)]">
                  Auto-hibernate idle sessions
                </span>
                <span className="text-[11.5px] font-mono tabular-nums text-[var(--velox-ink-soft)]">
                  {draft.autoHibernateMinutes === 0
                    ? "Off"
                    : `${draft.autoHibernateMinutes} min`}
                </span>
              </div>
              <div className="flex gap-1.5">
                {HIBERNATE_PRESETS.map((mins) => (
                  <button
                    key={mins}
                    type="button"
                    onClick={() =>
                      setDraft({ ...draft, autoHibernateMinutes: mins })
                    }
                    className={`flex-1 h-8 rounded-lg text-[12px] font-mono border transition-all cursor-pointer ${
                      draft.autoHibernateMinutes === mins
                        ? "bg-[var(--velox-active)] border-[var(--velox-accent)]/50 text-[var(--velox-accent-soft)]"
                        : "bg-[var(--velox-canvas)]/70 border-white/[0.06] text-[var(--velox-ink-soft)] hover:text-[var(--velox-ink)] hover:border-white/[0.12]"
                    }`}
                  >
                    {mins === 0 ? "Off" : `${mins}m`}
                  </button>
                ))}
              </div>
            </div>

            <div>
              <div className="flex items-center justify-between mb-1.5">
                <span className="text-[12px] text-[var(--velox-ink-secondary)]">
                  Scrollback cap per pane
                </span>
                <span className="text-[11.5px] font-mono tabular-nums text-[var(--velox-ink-soft)]">
                  {Math.round(draft.scrollbackBytes / (1024 * 1024))} MB
                </span>
              </div>
              <div className="grid grid-cols-4 gap-1.5">
                {SCROLLBACK_OPTIONS.map((opt) => (
                  <button
                    key={opt.bytes}
                    type="button"
                    onClick={() =>
                      setDraft({ ...draft, scrollbackBytes: opt.bytes })
                    }
                    className={`h-8 rounded-lg text-[12px] font-mono border transition-all cursor-pointer ${
                      draft.scrollbackBytes === opt.bytes
                        ? "bg-[var(--velox-active)] border-[var(--velox-accent)]/50 text-[var(--velox-accent-soft)]"
                        : "bg-[var(--velox-canvas)]/70 border-white/[0.06] text-[var(--velox-ink-soft)] hover:text-[var(--velox-ink)] hover:border-white/[0.12]"
                    }`}
                  >
                    {opt.label}
                  </button>
                ))}
              </div>
            </div>
          </section>

          {/* Section 2: Lazy MCP Gateway */}
          <section className="space-y-3 pt-4 border-t border-white/[0.07]">
            <h3 className="text-[12.5px] font-medium text-[var(--velox-ink)]">
              Lazy MCP Gateway
            </h3>

            <div className="flex items-center justify-between gap-4 px-3.5 py-2.5 rounded-xl bg-[var(--velox-canvas)]/70 border border-white/[0.06]">
              <div>
                <div className="text-[12.5px] text-[var(--velox-ink)]">
                  Enable embedded gateway
                </div>
                <div className="text-[11.5px] text-[var(--velox-ink-soft)]">
                  Scale stdio MCP backends to zero when idle
                </div>
              </div>
              <ToggleSwitch
                checked={draft.mcpGatewayEnabled}
                onChange={(checked) =>
                  setDraft({ ...draft, mcpGatewayEnabled: checked })
                }
              />
            </div>

            <div className="grid grid-cols-3 gap-2.5">
              <div>
                <label className="block text-[11.5px] text-[var(--velox-ink-secondary)] mb-1">
                  Port
                </label>
                <input
                  type="number"
                  min={1024}
                  max={65535}
                  value={draft.mcpGatewayPort}
                  onChange={(e) =>
                    setDraft({
                      ...draft,
                      mcpGatewayPort: Math.max(
                        1024,
                        Math.min(65535, Number(e.target.value) || 43111),
                      ),
                    })
                  }
                  className="w-full h-9 px-2.5 rounded-lg bg-[var(--velox-canvas)] border border-white/[0.08] text-[12.5px] font-mono tabular-nums text-[var(--velox-ink)] outline-none focus:border-[var(--velox-accent)]/60 transition-colors"
                />
              </div>
              <div className="col-span-2">
                <label className="block text-[11.5px] text-[var(--velox-ink-secondary)] mb-1">
                  backends.json path
                </label>
                <input
                  type="text"
                  value={draft.mcpBackendsPath}
                  onChange={(e) =>
                    setDraft({ ...draft, mcpBackendsPath: e.target.value })
                  }
                  className="w-full h-9 px-2.5 rounded-lg bg-[var(--velox-canvas)] border border-white/[0.08] text-[12px] font-mono text-[var(--velox-ink)] outline-none focus:border-[var(--velox-accent)]/60 transition-colors"
                />
              </div>
            </div>
          </section>

          {/* Section 3: Terminal & Workspace */}
          <section className="space-y-3 pt-4 border-t border-white/[0.07]">
            <h3 className="text-[12.5px] font-medium text-[var(--velox-ink)]">
              Terminal & workspace
            </h3>

            <div>
              <label className="block text-[11.5px] text-[var(--velox-ink-secondary)] mb-1">
                Default working directory
              </label>
              <div className="flex gap-2">
                <input
                  type="text"
                  value={draft.defaultCwd}
                  onChange={(e) =>
                    setDraft({ ...draft, defaultCwd: e.target.value })
                  }
                  className="flex-1 h-9 px-2.5 rounded-lg bg-[var(--velox-canvas)] border border-white/[0.08] text-[12px] font-mono text-[var(--velox-ink)] outline-none focus:border-[var(--velox-accent)]/60 transition-colors"
                />
                <button
                  type="button"
                  onClick={() => void handlePickDefaultCwd()}
                  className="h-9 px-3 rounded-lg bg-[var(--velox-elevated)] hover:bg-[var(--velox-hover)] active:scale-[0.97] border border-white/[0.08] text-[12px] text-[var(--velox-ink)] inline-flex items-center gap-1.5 transition-all cursor-pointer"
                >
                  <FolderOpen className="w-3.5 h-3.5 text-[var(--velox-ink-secondary)]" />
                  <span>Browse</span>
                </button>
              </div>
            </div>

            <div className="grid grid-cols-3 gap-2.5">
              <div className="col-span-2">
                <label className="block text-[11.5px] text-[var(--velox-ink-secondary)] mb-1">
                  Font family
                </label>
                <input
                  type="text"
                  value={draft.fontFamily}
                  onChange={(e) =>
                    setDraft({ ...draft, fontFamily: e.target.value })
                  }
                  className="w-full h-9 px-2.5 rounded-lg bg-[var(--velox-canvas)] border border-white/[0.08] text-[12px] font-mono text-[var(--velox-ink)] outline-none focus:border-[var(--velox-accent)]/60 transition-colors"
                />
              </div>
              <div>
                <label className="block text-[11.5px] text-[var(--velox-ink-secondary)] mb-1">
                  Font size ({draft.fontSize}px)
                </label>
                <div className="flex items-center gap-1.5 h-9">
                  <button
                    type="button"
                    onClick={() =>
                      setDraft({
                        ...draft,
                        fontSize: Math.max(10, draft.fontSize - 1),
                      })
                    }
                    className="h-9 w-9 rounded-lg bg-[var(--velox-elevated)] hover:bg-[var(--velox-hover)] active:scale-95 border border-white/[0.08] text-[var(--velox-ink)] font-mono transition-all cursor-pointer"
                  >
                    -
                  </button>
                  <span className="flex-1 text-center font-mono text-[12.5px] tabular-nums text-[var(--velox-ink)]">
                    {draft.fontSize}
                  </span>
                  <button
                    type="button"
                    onClick={() =>
                      setDraft({
                        ...draft,
                        fontSize: Math.min(20, draft.fontSize + 1),
                      })
                    }
                    className="h-9 w-9 rounded-lg bg-[var(--velox-elevated)] hover:bg-[var(--velox-hover)] active:scale-95 border border-white/[0.08] text-[var(--velox-ink)] font-mono transition-all cursor-pointer"
                  >
                    +
                  </button>
                </div>
              </div>
            </div>

            <div className="flex flex-wrap gap-1.5">
              {FONT_PRESETS.map((fp) => (
                <button
                  key={fp.label}
                  type="button"
                  onClick={() => setDraft({ ...draft, fontFamily: fp.value })}
                  className={`px-2 py-0.5 rounded-md text-[11px] border transition-colors cursor-pointer ${
                    draft.fontFamily === fp.value
                      ? "bg-[var(--velox-active)] border-[var(--velox-accent)]/45 text-[var(--velox-accent-soft)]"
                      : "bg-[var(--velox-canvas)]/70 border-white/[0.06] text-[var(--velox-ink-soft)] hover:text-[var(--velox-ink)]"
                  }`}
                >
                  {fp.label}
                </button>
              ))}
            </div>

            <div
              className="px-3 py-2.5 rounded-lg bg-[var(--velox-canvas)] border border-white/[0.07] text-[var(--velox-ink-secondary)] truncate"
              style={{
                fontFamily: draft.fontFamily,
                fontSize: `${draft.fontSize}px`,
              }}
            >
              <span className="text-[var(--velox-accent-soft)]">~/Projects/velox</span>{" "}
              <span className="text-[var(--velox-accent)]">❯</span> cargo test --release
            </div>

            <div className="flex items-center justify-between gap-4 px-3.5 py-2.5 rounded-xl bg-[var(--velox-canvas)]/70 border border-white/[0.06]">
              <div>
                <div className="text-[12.5px] text-[var(--velox-ink)]">
                  Animated backgrounds
                </div>
                <div className="text-[11.5px] text-[var(--velox-ink-soft)]">
                  Live OffscreenCanvas wallpaper for agents & custom themes
                </div>
              </div>
              <ToggleSwitch
                checked={draft.animatedBackgrounds}
                onChange={(checked) =>
                  setDraft({ ...draft, animatedBackgrounds: checked })
                }
              />
            </div>

            {/* Theme selector */}
            <div className="pt-1">
              <div className="flex items-center justify-between mb-2">
                <span className="inline-flex items-center gap-1.5 text-[12px] font-medium text-[var(--velox-ink-secondary)]">
                  <Palette className="w-3.5 h-3.5 text-[var(--velox-accent-soft)]" />
                  Appearance & custom themes
                </span>
                <span className="text-[11px] text-[var(--velox-ink-muted)]">
                  Applies live
                </span>
              </div>
              <div className="grid grid-cols-2 sm:grid-cols-3 gap-1.5">
                {THEME_OPTIONS.map((meta) => {
                  const active = themePreference === meta.id;
                  const [bg, , accent, accentSoft] = meta.swatches;
                  return (
                    <button
                      key={meta.id}
                      type="button"
                      onClick={() => onSelectTheme(meta.id)}
                      style={{
                        borderColor: active ? accent : "rgba(255, 255, 255, 0.07)",
                      }}
                      className={`px-2.5 py-2 rounded-lg border text-left flex items-center gap-2 transition-all cursor-pointer ${
                        active
                          ? "bg-[var(--velox-active)] text-[var(--velox-ink)]"
                          : "bg-[var(--velox-canvas)]/70 hover:bg-[var(--velox-elevated)] text-[var(--velox-ink-secondary)]"
                      }`}
                    >
                      <span
                        className="w-5 h-5 rounded-md border border-white/15 shrink-0 flex items-center justify-center"
                        style={{ backgroundColor: bg }}
                      >
                        {active ? (
                          <Check className="w-3 h-3" style={{ color: accent }} />
                        ) : (
                          <span className="flex gap-0.5">
                            <span
                              className="w-1.5 h-1.5 rounded-full"
                              style={{ backgroundColor: accent }}
                            />
                            <span
                              className="w-1.5 h-1.5 rounded-full"
                              style={{ backgroundColor: accentSoft }}
                            />
                          </span>
                        )}
                      </span>
                      <span className="text-[11.5px] font-medium truncate">
                        {meta.label}
                      </span>
                    </button>
                  );
                })}
              </div>
            </div>
          </section>

          {/* Section 4: Team (Velox Online) */}
          <TeamSettingsSection
            value={teamDraft}
            teamStatus={team}
            validation={teamValidation}
            onChange={(nextTeam) => {
              setSaveError(null);
              setDraft({ ...draft, team: nextTeam });
            }}
          />
        </div>

        {/* Footer */}
        <div className="px-5 py-3 bg-[var(--velox-canvas)]/60 border-t border-white/[0.07] flex items-center justify-between gap-3">
          {saveError || !teamValidation.valid ? (
            <div className="min-w-0 text-[11.5px] text-rose-300 truncate">
              {saveError ?? teamValidation.summaryError}
            </div>
          ) : (
            <span className="text-[11px] text-[var(--velox-ink-muted)] hidden sm:inline">
              Tip: Press <span className="velox-kbd">Ctrl+S</span> to save
            </span>
          )}
          <div className="flex items-center gap-2 shrink-0 ml-auto">
            <button
              type="button"
              onClick={onClose}
              className="h-8 px-3.5 rounded-lg text-[12.5px] text-[var(--velox-ink-secondary)] hover:text-[var(--velox-ink)] hover:bg-[var(--velox-hover)] transition-colors cursor-pointer"
            >
              Cancel
            </button>
            <button
              type="button"
              disabled={saving || !teamValidation.valid}
              onClick={() => void handleSave()}
              className="h-8 px-4 rounded-lg bg-[var(--velox-accent)] hover:brightness-110 active:scale-[0.98] text-[#080B12] text-[12.5px] font-semibold transition-all cursor-pointer disabled:opacity-50"
            >
              {saving ? "Saving..." : "Save changes"}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
