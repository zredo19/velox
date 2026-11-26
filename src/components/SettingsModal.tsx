import { useEffect, useState } from "react";
import { FolderOpen, Settings, X } from "lucide-react";
import { veloxApi } from "../lib/tauri";
import type { VeloxConfig } from "../types/ipc";

export interface SettingsModalProps {
  open: boolean;
  onClose: () => void;
  config: VeloxConfig | null;
  onSave: (next: VeloxConfig) => Promise<void>;
}

const SCROLLBACK_OPTIONS = [
  { label: "1 MB", bytes: 1 * 1024 * 1024 },
  { label: "2 MB", bytes: 2 * 1024 * 1024 },
  { label: "4 MB", bytes: 4 * 1024 * 1024 },
  { label: "8 MB", bytes: 8 * 1024 * 1024 },
];

const HIBERNATE_PRESETS = [0, 10, 20, 30, 60];

export function SettingsModal({
  open,
  onClose,
  config,
  onSave,
}: SettingsModalProps) {
  const [draft, setDraft] = useState<VeloxConfig | null>(config);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (open && config) {
      setDraft({ ...config });
    }
  }, [open, config]);

  useEffect(() => {
    if (!open) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        onClose();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [open, onClose]);

  if (!open || !draft) return null;

  const handleSave = async () => {
    setSaving(true);
    try {
      await onSave(draft);
      onClose();
    } finally {
      setSaving(false);
    }
  };

  const handlePickDefaultCwd = async () => {
    const selected = await veloxApi.pickDirectory(draft.defaultCwd);
    if (selected) {
      setDraft((prev) => (prev ? { ...prev, defaultCwd: selected } : prev));
    }
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-[2px] p-4"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
      role="dialog"
      aria-modal="true"
      aria-labelledby="velox-settings-title"
    >
      <div className="w-full max-w-[540px] max-h-[86vh] bg-[#151923] border border-white/[0.09] rounded-xl shadow-2xl flex flex-col overflow-hidden animate-velox-pop-in">
        {/* Header */}
        <div className="px-5 py-3.5 border-b border-white/[0.07] flex items-center justify-between">
          <div className="flex items-center gap-2">
            <Settings className="w-4 h-4 text-[#6EA8FC]" />
            <h2
              id="velox-settings-title"
              className="text-[14.5px] font-medium text-[#E6EAF2]"
            >
              Settings
            </h2>
          </div>
          <button
            type="button"
            onClick={onClose}
            aria-label="Close settings"
            className="h-7 w-7 flex items-center justify-center rounded-md text-[#7E889E] hover:text-[#E6EAF2] hover:bg-[#1C2130] transition-colors cursor-pointer"
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        {/* Body */}
        <div className="flex-1 overflow-y-auto p-5 space-y-6">
          {/* Section 1: Memory & Hibernation */}
          <section className="space-y-3.5">
            <h3 className="text-[12.5px] font-medium text-[#E6EAF2]">
              Memory & hibernation
            </h3>

            <label className="flex items-center justify-between gap-4 px-3 py-2.5 rounded-lg bg-[#11141C] border border-white/[0.06] cursor-pointer">
              <div>
                <div className="text-[12.5px] text-[#E6EAF2]">
                  Lazy restore on startup
                </div>
                <div className="text-[11.5px] text-[#7E889E]">
                  Load saved sessions as sleeping (0 MB RAM) until clicked
                </div>
              </div>
              <input
                type="checkbox"
                checked={draft.lazyRestoreOnStartup}
                onChange={(e) =>
                  setDraft({ ...draft, lazyRestoreOnStartup: e.target.checked })
                }
                className="w-4 h-4 accent-[#4E8DF5] cursor-pointer"
              />
            </label>

            <div>
              <div className="flex items-center justify-between mb-1.5">
                <span className="text-[12px] text-[#98A2B8]">
                  Auto-hibernate idle sessions
                </span>
                <span className="text-[11.5px] font-mono tabular-nums text-[#7E889E]">
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
                    className={`flex-1 h-8 rounded-md text-[12px] font-mono border transition-colors cursor-pointer ${
                      draft.autoHibernateMinutes === mins
                        ? "bg-[#1F2638] border-[#4E8DF5]/50 text-[#9CC2FF]"
                        : "bg-[#11141C] border-white/[0.06] text-[#7E889E] hover:text-[#E6EAF2]"
                    }`}
                  >
                    {mins === 0 ? "Off" : `${mins}m`}
                  </button>
                ))}
              </div>
            </div>

            <div>
              <div className="flex items-center justify-between mb-1.5">
                <span className="text-[12px] text-[#98A2B8]">
                  Scrollback cap per pane
                </span>
                <span className="text-[11.5px] font-mono tabular-nums text-[#7E889E]">
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
                    className={`h-8 rounded-md text-[12px] font-mono border transition-colors cursor-pointer ${
                      draft.scrollbackBytes === opt.bytes
                        ? "bg-[#1F2638] border-[#4E8DF5]/50 text-[#9CC2FF]"
                        : "bg-[#11141C] border-white/[0.06] text-[#7E889E] hover:text-[#E6EAF2]"
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
            <h3 className="text-[12.5px] font-medium text-[#E6EAF2]">
              Lazy MCP Gateway
            </h3>

            <label className="flex items-center justify-between gap-4 px-3 py-2.5 rounded-lg bg-[#11141C] border border-white/[0.06] cursor-pointer">
              <div>
                <div className="text-[12.5px] text-[#E6EAF2]">
                  Enable embedded gateway
                </div>
                <div className="text-[11.5px] text-[#7E889E]">
                  Scale stdio MCP backends to zero when idle
                </div>
              </div>
              <input
                type="checkbox"
                checked={draft.mcpGatewayEnabled}
                onChange={(e) =>
                  setDraft({ ...draft, mcpGatewayEnabled: e.target.checked })
                }
                className="w-4 h-4 accent-[#4E8DF5] cursor-pointer"
              />
            </label>

            <div className="grid grid-cols-3 gap-2.5">
              <div>
                <label className="block text-[11.5px] text-[#98A2B8] mb-1">
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
                  className="w-full h-9 px-2.5 rounded-lg bg-[#0B0D12] border border-white/[0.08] text-[12.5px] font-mono tabular-nums text-[#E6EAF2] outline-none focus:border-[#4E8DF5]/60"
                />
              </div>
              <div className="col-span-2">
                <label className="block text-[11.5px] text-[#98A2B8] mb-1">
                  backends.json path
                </label>
                <input
                  type="text"
                  value={draft.mcpBackendsPath}
                  onChange={(e) =>
                    setDraft({ ...draft, mcpBackendsPath: e.target.value })
                  }
                  className="w-full h-9 px-2.5 rounded-lg bg-[#0B0D12] border border-white/[0.08] text-[12px] font-mono text-[#E6EAF2] outline-none focus:border-[#4E8DF5]/60"
                />
              </div>
            </div>
          </section>

          {/* Section 3: Terminal & Workspace */}
          <section className="space-y-3 pt-4 border-t border-white/[0.07]">
            <h3 className="text-[12.5px] font-medium text-[#E6EAF2]">
              Terminal & workspace
            </h3>

            <div>
              <label className="block text-[11.5px] text-[#98A2B8] mb-1">
                Default working directory
              </label>
              <div className="flex gap-2">
                <input
                  type="text"
                  value={draft.defaultCwd}
                  onChange={(e) =>
                    setDraft({ ...draft, defaultCwd: e.target.value })
                  }
                  className="flex-1 h-9 px-2.5 rounded-lg bg-[#0B0D12] border border-white/[0.08] text-[12px] font-mono text-[#E6EAF2] outline-none focus:border-[#4E8DF5]/60"
                />
                <button
                  type="button"
                  onClick={() => void handlePickDefaultCwd()}
                  className="h-9 px-3 rounded-lg bg-[#1C2130] hover:bg-[#232A3D] border border-white/[0.08] text-[12px] text-[#E6EAF2] inline-flex items-center gap-1.5 cursor-pointer"
                >
                  <FolderOpen className="w-3.5 h-3.5 text-[#98A2B8]" />
                  <span>Browse</span>
                </button>
              </div>
            </div>

            <div className="grid grid-cols-3 gap-2.5">
              <div className="col-span-2">
                <label className="block text-[11.5px] text-[#98A2B8] mb-1">
                  Font family
                </label>
                <input
                  type="text"
                  value={draft.fontFamily}
                  onChange={(e) =>
                    setDraft({ ...draft, fontFamily: e.target.value })
                  }
                  className="w-full h-9 px-2.5 rounded-lg bg-[#0B0D12] border border-white/[0.08] text-[12px] font-mono text-[#E6EAF2] outline-none focus:border-[#4E8DF5]/60"
                />
              </div>
              <div>
                <label className="block text-[11.5px] text-[#98A2B8] mb-1">
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
                    className="h-9 w-9 rounded-lg bg-[#1C2130] hover:bg-[#232A3D] border border-white/[0.08] text-[#E6EAF2] font-mono cursor-pointer"
                  >
                    -
                  </button>
                  <span className="flex-1 text-center font-mono text-[12.5px] tabular-nums text-[#E6EAF2]">
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
                    className="h-9 w-9 rounded-lg bg-[#1C2130] hover:bg-[#232A3D] border border-white/[0.08] text-[#E6EAF2] font-mono cursor-pointer"
                  >
                    +
                  </button>
                </div>
              </div>
            </div>

            <div
              className="px-3 py-2 rounded-lg bg-[#0B0D12] border border-white/[0.06] text-[#98A2B8] truncate"
              style={{
                fontFamily: draft.fontFamily,
                fontSize: `${draft.fontSize}px`,
              }}
            >
              ~/Proyectos/velox ❯ cargo test --release
            </div>

            <label className="flex items-center justify-between gap-4 px-3 py-2.5 rounded-lg bg-[#11141C] border border-white/[0.06] cursor-pointer">
              <div>
                <div className="text-[12.5px] text-[#E6EAF2]">
                  Animated backgrounds
                </div>
                <div className="text-[11.5px] text-[#7E889E]">
                  Live ASCII wallpaper behind Claude and Antigravity sessions
                </div>
              </div>
              <input
                type="checkbox"
                checked={draft.animatedBackgrounds}
                onChange={(e) =>
                  setDraft({ ...draft, animatedBackgrounds: e.target.checked })
                }
                className="w-4 h-4 accent-[#4E8DF5] cursor-pointer"
              />
            </label>
          </section>
        </div>

        {/* Footer */}
        <div className="px-5 py-3 bg-[#11141C]/80 border-t border-white/[0.07] flex items-center justify-end gap-2">
          <button
            type="button"
            onClick={onClose}
            className="h-8 px-3.5 rounded-lg text-[12.5px] text-[#98A2B8] hover:text-[#E6EAF2] hover:bg-[#1C2130] transition-colors cursor-pointer"
          >
            Cancel
          </button>
          <button
            type="button"
            disabled={saving}
            onClick={() => void handleSave()}
            className="h-8 px-4 rounded-lg bg-[#4E8DF5] hover:bg-[#5E98F7] text-[#080B12] text-[12.5px] font-semibold transition-colors cursor-pointer disabled:opacity-50"
          >
            {saving ? "Saving..." : "Save changes"}
          </button>
        </div>
      </div>
    </div>
  );
}
