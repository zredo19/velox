import { useEffect, useMemo, useRef, useState } from "react";
import { FolderOpen, X } from "lucide-react";
import { veloxApi } from "../lib/tauri";
import type { AgentKind, AgentPreset } from "../types/ipc";
import { AgentIcon } from "./icons/AgentIcons";

export interface AgentLauncherModalProps {
  open: boolean;
  onClose: () => void;
  presets: AgentPreset[];
  recentDirs: string[];
  defaultCwd: string;
  onLaunch: (presetId: string, cwd: string, title?: string) => void;
}

const AGENT_ORDER: AgentKind[] = [
  "claude",
  "agy",
  "codex",
  "opencode",
  "shell",
];

function getShortPath(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  if (parts.length <= 2) return path;
  return `…/${parts.slice(-2).join("/")}`;
}

export function AgentLauncherModal({
  open,
  onClose,
  presets,
  recentDirs,
  defaultCwd,
  onLaunch,
}: AgentLauncherModalProps) {
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [cwd, setCwd] = useState(defaultCwd);
  const [title, setTitle] = useState("");
  const [browsing, setBrowsing] = useState(false);
  const [browseError, setBrowseError] = useState<string | null>(null);
  const cwdInputRef = useRef<HTMLInputElement | null>(null);

  const sortedPresets = useMemo(() => {
    return [...presets].sort((a, b) => {
      const aIdx = AGENT_ORDER.indexOf(a.kind);
      const bIdx = AGENT_ORDER.indexOf(b.kind);
      return (aIdx === -1 ? 99 : aIdx) - (bIdx === -1 ? 99 : bIdx);
    });
  }, [presets]);

  useEffect(() => {
    if (open) {
      setSelectedIndex(0);
      setCwd(defaultCwd);
      setTitle("");
      setBrowseError(null);
    }
  }, [open, defaultCwd]);

  useEffect(() => {
    if (!open) return;

    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        onClose();
        return;
      }

      const inTextInput =
        e.target instanceof HTMLInputElement ||
        e.target instanceof HTMLTextAreaElement;

      if (e.key === "Enter" && !e.shiftKey) {
        const chosen = sortedPresets[selectedIndex];
        if (chosen) {
          e.preventDefault();
          onLaunch(chosen.id, cwd.trim() || defaultCwd, title.trim() || undefined);
        }
        return;
      }

      if (inTextInput) return;

      if (e.key === "ArrowDown" || e.key === "ArrowRight") {
        e.preventDefault();
        setSelectedIndex((i) => Math.min(i + 1, sortedPresets.length - 1));
      } else if (e.key === "ArrowUp" || e.key === "ArrowLeft") {
        e.preventDefault();
        setSelectedIndex((i) => Math.max(i - 1, 0));
      } else if (e.key >= "1" && e.key <= "9") {
        const idx = parseInt(e.key, 10) - 1;
        const chosen = sortedPresets[idx];
        if (chosen) {
          e.preventDefault();
          onLaunch(chosen.id, cwd.trim() || defaultCwd, title.trim() || undefined);
        }
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [open, sortedPresets, selectedIndex, cwd, defaultCwd, title, onClose, onLaunch]);

  const handleBrowse = async () => {
    setBrowsing(true);
    setBrowseError(null);
    try {
      const selected = await veloxApi.pickDirectory(cwd || defaultCwd);
      if (selected) setCwd(selected);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setBrowseError(msg || "Failed to open folder picker");
    } finally {
      setBrowsing(false);
      cwdInputRef.current?.focus();
    }
  };

  if (!open) return null;

  const activePreset = sortedPresets[selectedIndex];

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-[2px] p-4"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
      role="dialog"
      aria-modal="true"
      aria-labelledby="velox-launcher-title"
    >
      <div className="w-full max-w-[548px] bg-[#151923] border border-white/[0.09] rounded-xl shadow-2xl overflow-hidden animate-velox-pop-in">
        {/* Header */}
        <div className="flex items-center justify-between px-5 py-3.5 border-b border-white/[0.07]">
          <div>
            <h2
              id="velox-launcher-title"
              className="text-[14.5px] font-medium text-[#E6EAF2]"
            >
              Launch agent
            </h2>
            <p className="text-[12px] text-[#7E889E] mt-0.5">
              Pick an agent and project folder — press 1–{sortedPresets.length} or Enter
            </p>
          </div>
          <button
            type="button"
            onClick={onClose}
            aria-label="Close launcher"
            className="h-7 w-7 flex items-center justify-center rounded-md text-[#7E889E] hover:text-[#E6EAF2] hover:bg-[#1C2130] transition-colors cursor-pointer"
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        <div className="p-5 space-y-4">
          {/* Agent grid */}
          <div className="grid grid-cols-2 gap-2">
            {sortedPresets.map((preset, index) => {
              const selected = index === selectedIndex;
              return (
                <button
                  key={preset.id}
                  type="button"
                  onMouseEnter={() => setSelectedIndex(index)}
                  onClick={() =>
                    onLaunch(
                      preset.id,
                      cwd.trim() || defaultCwd,
                      title.trim() || undefined,
                    )
                  }
                  style={{
                    borderColor: selected
                      ? preset.accentColor
                      : "rgba(255, 255, 255, 0.07)",
                  }}
                  className={`text-left px-3 py-2.5 rounded-lg border transition-colors flex items-start gap-2.5 cursor-pointer ${
                    selected ? "bg-[#1C2233]" : "bg-[#11141C] hover:bg-[#181D2B]"
                  }`}
                >
                  <div className="mt-0.5 w-7 h-7 rounded-md bg-[#0B0D12] border border-white/[0.06] flex items-center justify-center shrink-0">
                    <AgentIcon kind={preset.kind} className="w-4 h-4" active />
                  </div>

                  <div className="min-w-0 flex-1">
                    <div className="flex items-center justify-between gap-1.5">
                      <div className="flex items-center gap-1.5 min-w-0">
                        <span className="text-[13px] font-medium text-[#E6EAF2] truncate">
                          {preset.label}
                        </span>
                        {preset.profile && (
                          <span
                            className="text-[10.5px] px-1.5 py-0.2 rounded bg-white/[0.07] font-medium shrink-0"
                            style={{
                              color:
                                preset.profile === "trabajo"
                                  ? "#7DD3C8"
                                  : "#E8B4D9",
                            }}
                          >
                            {preset.profile}
                          </span>
                        )}
                      </div>
                      <span className="text-[10.5px] font-mono text-[#64708A] shrink-0">
                        {index + 1}
                      </span>
                    </div>

                    <div className="flex items-center gap-1.5 mt-0.5">
                      <p className="text-[11.5px] text-[#7E889E] truncate">
                        {preset.description}
                      </p>
                      {!preset.installed && (
                        <span className="shrink-0 text-[10px] px-1 py-0.2 rounded bg-rose-500/15 text-rose-300">
                          missing
                        </span>
                      )}
                    </div>
                  </div>
                </button>
              );
            })}
          </div>

          {/* Folder picker + recent dirs */}
          <div>
            <label className="block text-[12px] font-medium text-[#98A2B8] mb-1.5">
              Workspace folder
            </label>
            <div className="flex gap-2">
              <input
                ref={cwdInputRef}
                type="text"
                value={cwd}
                onChange={(e) => {
                  setCwd(e.target.value);
                  if (browseError) setBrowseError(null);
                }}
                placeholder="C:\Users\..."
                className="flex-1 h-9 px-3 rounded-lg bg-[#0B0D12] border border-white/[0.08] focus:border-[#4E8DF5]/60 text-[12.5px] font-mono text-[#E6EAF2] placeholder:text-[#52607A] outline-none"
              />
              <button
                type="button"
                disabled={browsing}
                onClick={() => void handleBrowse()}
                className="h-9 px-3 rounded-lg bg-[#1C2130] hover:bg-[#232A3D] border border-white/[0.08] text-[12.5px] text-[#E6EAF2] inline-flex items-center gap-1.5 transition-colors cursor-pointer disabled:opacity-50"
              >
                <FolderOpen className="w-3.5 h-3.5 text-[#98A2B8]" />
                <span>{browsing ? "..." : "Browse"}</span>
              </button>
            </div>

            {browseError && (
              <p role="alert" className="mt-1.5 text-[11.5px] text-rose-300">
                {browseError}
              </p>
            )}

            {recentDirs.length > 0 && (
              <div className="mt-2 flex flex-wrap gap-1.5">
                {recentDirs.slice(0, 5).map((dir) => (
                  <button
                    key={dir}
                    type="button"
                    onClick={() => setCwd(dir)}
                    title={dir}
                    className={`px-2 py-0.5 rounded text-[11px] font-mono border transition-colors cursor-pointer ${
                      cwd.toLowerCase() === dir.toLowerCase()
                        ? "bg-[#1F2638] border-[#4E8DF5]/40 text-[#9CC2FF]"
                        : "bg-[#11141C] border-white/[0.06] text-[#7E889E] hover:text-[#E6EAF2] hover:border-white/[0.12]"
                    }`}
                  >
                    {getShortPath(dir)}
                  </button>
                ))}
              </div>
            )}
          </div>

          {/* Optional custom title */}
          <div>
            <label className="block text-[12px] font-medium text-[#98A2B8] mb-1.5">
              Session label (optional)
            </label>
            <input
              type="text"
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              placeholder="Defaults to folder name"
              className="w-full h-9 px-3 rounded-lg bg-[#0B0D12] border border-white/[0.08] focus:border-[#4E8DF5]/60 text-[12.5px] text-[#E6EAF2] placeholder:text-[#52607A] outline-none"
            />
          </div>
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
            disabled={!activePreset}
            onClick={() => {
              if (activePreset) {
                onLaunch(
                  activePreset.id,
                  cwd.trim() || defaultCwd,
                  title.trim() || undefined,
                );
              }
            }}
            className="h-8 px-4 rounded-lg bg-[#4E8DF5] hover:bg-[#5E98F7] text-[#080B12] text-[12.5px] font-semibold transition-colors cursor-pointer disabled:opacity-40"
          >
            Launch {activePreset?.label ?? "agent"}
          </button>
        </div>
      </div>
    </div>
  );
}
