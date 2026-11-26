import { Folder, Plus } from "lucide-react";
import type { AgentPreset } from "../types/ipc";
import { AgentIcon, VeloxLogo } from "./icons/AgentIcons";

export interface EmptyStateProps {
  presets: AgentPreset[];
  defaultCwd: string;
  onQuickLaunch: (presetId: string) => void;
  onOpenLauncher: () => void;
}

function getFolderName(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

export function EmptyState({
  presets,
  defaultCwd,
  onQuickLaunch,
  onOpenLauncher,
}: EmptyStateProps) {
  const folderName = getFolderName(defaultCwd || ".");

  return (
    <div className="flex-1 h-full w-full bg-[#0B0D12] flex flex-col items-center justify-center px-6 py-10 overflow-y-auto">
      <div className="w-full max-w-[540px] flex flex-col items-center">
        <VeloxLogo className="w-10 h-10 mb-5" />

        <h1 className="text-[20px] font-medium tracking-[-0.02em] text-[#E6EAF2]">
          Start an agent session
        </h1>

        <div className="mt-2 flex items-center gap-1.5 text-[12.5px] text-[#7E889E]">
          <Folder className="w-3.5 h-3.5 text-[#64708A]" />
          <span className="font-mono">{folderName}</span>
          <span className="text-[#4B556B]">·</span>
          <span>Idle sessions sleep at 0 MB</span>
        </div>

        <div className="grid grid-cols-2 gap-2.5 w-full mt-7">
          {presets.map((preset) => (
            <button
              key={preset.id}
              type="button"
              onClick={() => onQuickLaunch(preset.id)}
              className="group text-left px-3.5 py-3 rounded-xl bg-[#11141C] hover:bg-[#171C28] border border-white/[0.07] hover:border-white/[0.14] transition-all duration-120 cursor-pointer flex items-start gap-3"
            >
              <div className="mt-0.5 w-8 h-8 rounded-lg bg-[#0B0D12] border border-white/[0.06] flex items-center justify-center shrink-0">
                <AgentIcon kind={preset.kind} className="w-4 h-4" active />
              </div>

              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-1.5">
                  <span className="text-[13px] font-medium text-[#E6EAF2] truncate">
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
                <p className="text-[11.5px] text-[#7E889E] mt-0.5 truncate">
                  {preset.description}
                </p>
              </div>
            </button>
          ))}
        </div>

        <div className="mt-6 flex items-center gap-3 text-[12px] text-[#64708A]">
          <span>Press Ctrl+N to pick folder & agent</span>
          <button
            type="button"
            onClick={onOpenLauncher}
            className="inline-flex items-center gap-1 px-2.5 py-1 rounded-md text-[#9CC2FF] bg-[#4E8DF5]/10 hover:bg-[#4E8DF5]/20 border border-[#4E8DF5]/25 transition-colors cursor-pointer"
          >
            <Plus className="w-3.5 h-3.5" />
            <span>Open launcher</span>
          </button>
        </div>
      </div>
    </div>
  );
}
