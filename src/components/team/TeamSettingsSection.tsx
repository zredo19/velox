import { useEffect, useRef, useState } from "react";
import { Check, Copy, Eye, EyeOff, KeyRound, Plus, Trash2 } from "lucide-react";
import { veloxApi } from "../../lib/tauri";
import type { PeerConfig, TeamConfig, TeamSnapshot } from "../../types/ipc";

export const DEFAULT_TEAM_CONFIG: TeamConfig = {
  enabled: false,
  displayName: "",
  listenPort: 43120,
  bindHost: "",
  token: "",
  autoApproveLocal: false,
  peers: [],
};

export interface TeamValidationResult {
  valid: boolean;
  listenPortError: string | null;
  peerErrors: Record<number, string>;
  summaryError: string | null;
}

export function validateTeamConfig(team: TeamConfig): TeamValidationResult {
  let listenPortError: string | null = null;
  if (
    !Number.isInteger(team.listenPort) ||
    team.listenPort < 1 ||
    team.listenPort > 65535
  ) {
    listenPortError = "Listen port must be between 1 and 65535.";
  }

  const peerErrors: Record<number, string> = {};
  const seenNames = new Map<string, number>();

  team.peers.forEach((peer, index) => {
    const trimmedName = peer.name.trim();
    if (!trimmedName) {
      peerErrors[index] = "Name is required.";
      return;
    }

    const key = trimmedName.toLowerCase();
    const firstIndex = seenNames.get(key);
    if (firstIndex !== undefined) {
      peerErrors[index] = `Duplicate peer name "${trimmedName}".`;
      if (!peerErrors[firstIndex]) {
        peerErrors[firstIndex] = `Duplicate peer name "${trimmedName}".`;
      }
      return;
    }
    seenNames.set(key, index);

    if (
      !Number.isInteger(peer.port) ||
      peer.port < 1 ||
      peer.port > 65535
    ) {
      peerErrors[index] = "Port must be between 1 and 65535.";
    }
  });

  const firstPeerErr = Object.values(peerErrors)[0] ?? null;
  const summaryError = listenPortError ?? firstPeerErr;
  return {
    valid: !summaryError,
    listenPortError,
    peerErrors,
    summaryError,
  };
}

export interface TeamSettingsSectionProps {
  value: TeamConfig;
  teamStatus: TeamSnapshot | null;
  validation: TeamValidationResult;
  onChange: (next: TeamConfig) => void;
}

export function TeamSettingsSection({
  value,
  teamStatus,
  validation,
  onChange,
}: TeamSettingsSectionProps) {
  const [showToken, setShowToken] = useState(false);
  const [generating, setGenerating] = useState(false);
  const [copied, setCopied] = useState(false);
  const [tokenError, setTokenError] = useState<string | null>(null);
  const copyTimerRef = useRef<number | undefined>(undefined);

  useEffect(() => () => window.clearTimeout(copyTimerRef.current), []);

  const handleGenerateToken = async () => {
    setGenerating(true);
    setTokenError(null);
    try {
      const token = await veloxApi.generateTeamToken();
      onChange({ ...value, token });
    } catch (err) {
      setTokenError(
        err instanceof Error && err.message
          ? err.message
          : "Could not generate token",
      );
    } finally {
      setGenerating(false);
    }
  };

  const handleCopyToken = async () => {
    if (!value.token) return;
    try {
      await navigator.clipboard.writeText(value.token);
      setCopied(true);
      window.clearTimeout(copyTimerRef.current);
      copyTimerRef.current = window.setTimeout(() => setCopied(false), 1500);
    } catch {
      setTokenError("Clipboard copy failed");
    }
  };

  const updatePeer = (index: number, patch: Partial<PeerConfig>) => {
    const nextPeers = value.peers.map((p, i) =>
      i === index ? { ...p, ...patch } : p,
    );
    onChange({ ...value, peers: nextPeers });
  };

  const addPeer = () => {
    onChange({
      ...value,
      peers: [...value.peers, { name: "", host: "", port: 43120 }],
    });
  };

  const removePeer = (index: number) => {
    onChange({
      ...value,
      peers: value.peers.filter((_, i) => i !== index),
    });
  };

  return (
    <section className="space-y-3.5 pt-4 border-t border-white/[0.07]">
      <div className="flex items-center justify-between gap-2">
        <h3 className="text-[12.5px] font-medium text-[#E6EAF2]">
          Team (Velox Online)
        </h3>
        <span className="text-[11px] font-mono tabular-nums text-[#7E889E]">
          Tailscale IP: {teamStatus?.tailscaleIp ?? "not detected"}
        </span>
      </div>

      {/* Live server status line */}
      {teamStatus && (teamStatus.listenAddr || teamStatus.serverError) && (
        <div
          className={`px-3 py-2 rounded-lg border text-[11.5px] font-mono ${
            teamStatus.listenAddr
              ? "bg-emerald-500/10 border-emerald-500/25 text-emerald-300"
              : "bg-rose-500/10 border-rose-500/25 text-rose-300"
          }`}
        >
          {teamStatus.listenAddr
            ? `Listening on ${teamStatus.listenAddr}`
            : teamStatus.serverError}
        </div>
      )}

      <label className="flex items-center justify-between gap-4 px-3 py-2.5 rounded-lg bg-[#11141C] border border-white/[0.06] cursor-pointer">
        <div>
          <div className="text-[12.5px] text-[#E6EAF2]">
            Enable team sharing
          </div>
          <div className="text-[11.5px] text-[#7E889E]">
            See teammates&apos; shared sessions and exchange task requests over Tailscale
          </div>
        </div>
        <input
          type="checkbox"
          checked={value.enabled}
          onChange={(e) => onChange({ ...value, enabled: e.target.checked })}
          className="w-4 h-4 accent-[#4E8DF5] cursor-pointer"
        />
      </label>

      <div className="grid grid-cols-3 gap-2.5">
        <div>
          <label className="block text-[11.5px] text-[#98A2B8] mb-1">
            Display name
          </label>
          <input
            type="text"
            value={value.displayName}
            placeholder="Rodolfo"
            onChange={(e) =>
              onChange({ ...value, displayName: e.target.value })
            }
            className="w-full h-9 px-2.5 rounded-lg bg-[#0B0D12] border border-white/[0.08] text-[12.5px] text-[#E6EAF2] placeholder:text-[#52607A] outline-none focus:border-[#4E8DF5]/60"
          />
        </div>

        <div>
          <label className="block text-[11.5px] text-[#98A2B8] mb-1">
            Listen port
          </label>
          <input
            type="number"
            min={1}
            max={65535}
            value={Number.isNaN(value.listenPort) ? "" : value.listenPort}
            onChange={(e) => {
              const parsed = e.target.value === "" ? 0 : Number(e.target.value);
              onChange({ ...value, listenPort: Math.trunc(parsed) });
            }}
            className={`w-full h-9 px-2.5 rounded-lg bg-[#0B0D12] border text-[12.5px] font-mono tabular-nums text-[#E6EAF2] outline-none focus:border-[#4E8DF5]/60 ${
              validation.listenPortError
                ? "border-rose-500/50"
                : "border-white/[0.08]"
            }`}
          />
        </div>

        <div>
          <label className="block text-[11.5px] text-[#98A2B8] mb-1">
            Bind host
          </label>
          <input
            type="text"
            value={value.bindHost}
            placeholder="Auto (Tailscale IP)"
            onChange={(e) => onChange({ ...value, bindHost: e.target.value })}
            className="w-full h-9 px-2.5 rounded-lg bg-[#0B0D12] border border-white/[0.08] text-[12px] font-mono text-[#E6EAF2] placeholder:text-[#52607A] outline-none focus:border-[#4E8DF5]/60"
          />
        </div>
      </div>

      {validation.listenPortError && (
        <div className="text-[11.5px] text-rose-300">
          {validation.listenPortError}
        </div>
      )}

      <div>
        <label className="block text-[11.5px] text-[#98A2B8] mb-1">
          Shared team token
        </label>
        <div className="flex items-center gap-1.5">
          <div className="relative flex-1">
            <input
              type={showToken ? "text" : "password"}
              value={value.token}
              placeholder="Paste or generate a shared secret"
              onChange={(e) => {
                setTokenError(null);
                onChange({ ...value, token: e.target.value });
              }}
              className="w-full h-9 pl-2.5 pr-8 rounded-lg bg-[#0B0D12] border border-white/[0.08] text-[12px] font-mono text-[#E6EAF2] placeholder:text-[#52607A] outline-none focus:border-[#4E8DF5]/60"
            />
            <button
              type="button"
              onClick={() => setShowToken((v) => !v)}
              title={showToken ? "Hide token" : "Show token"}
              aria-label={showToken ? "Hide token" : "Show token"}
              className="absolute right-1.5 top-1/2 -translate-y-1/2 h-6 w-6 flex items-center justify-center rounded text-[#7E889E] hover:text-[#E6EAF2] cursor-pointer"
            >
              {showToken ? (
                <EyeOff className="w-3.5 h-3.5" />
              ) : (
                <Eye className="w-3.5 h-3.5" />
              )}
            </button>
          </div>

          <button
            type="button"
            disabled={generating}
            onClick={() => void handleGenerateToken()}
            className="h-9 px-2.5 rounded-lg bg-[#1C2130] hover:bg-[#232A3D] disabled:opacity-50 border border-white/[0.08] text-[12px] text-[#E6EAF2] inline-flex items-center gap-1.5 cursor-pointer shrink-0"
          >
            <KeyRound className="w-3.5 h-3.5 text-[#9CC2FF]" />
            <span>{generating ? "..." : "Generate"}</span>
          </button>

          <button
            type="button"
            disabled={!value.token}
            onClick={() => void handleCopyToken()}
            className="h-9 px-2.5 rounded-lg bg-[#1C2130] hover:bg-[#232A3D] disabled:opacity-40 border border-white/[0.08] text-[12px] text-[#E6EAF2] inline-flex items-center gap-1.5 cursor-pointer shrink-0"
          >
            {copied ? (
              <Check className="w-3.5 h-3.5 text-emerald-400" />
            ) : (
              <Copy className="w-3.5 h-3.5 text-[#98A2B8]" />
            )}
            <span>{copied ? "Copied" : "Copy"}</span>
          </button>
        </div>
        {tokenError && (
          <div className="mt-1 text-[11.5px] text-rose-300">{tokenError}</div>
        )}
      </div>

      <label className="flex items-center justify-between gap-4 px-3 py-2.5 rounded-lg bg-[#11141C] border border-white/[0.06] cursor-pointer">
        <div>
          <div className="text-[12.5px] text-[#E6EAF2]">
            Auto-approve tasks from this PC
          </div>
          <div className="text-[11.5px] text-[#7E889E]">
            Skip confirmation for tasks sent between your own local sessions
          </div>
        </div>
        <input
          type="checkbox"
          checked={value.autoApproveLocal}
          onChange={(e) =>
            onChange({ ...value, autoApproveLocal: e.target.checked })
          }
          className="w-4 h-4 accent-[#4E8DF5] cursor-pointer"
        />
      </label>

      {/* Peers editor */}
      <div className="space-y-2">
        <div className="flex items-center justify-between">
          <span className="text-[12px] text-[#98A2B8]">Teammate peers</span>
          <button
            type="button"
            onClick={addPeer}
            className="h-7 px-2.5 rounded-md bg-[#4E8DF5]/15 hover:bg-[#4E8DF5]/25 border border-[#4E8DF5]/30 text-[11.5px] font-medium text-[#9CC2FF] inline-flex items-center gap-1 cursor-pointer"
          >
            <Plus className="w-3 h-3" />
            <span>Add peer</span>
          </button>
        </div>

        {value.peers.length === 0 ? (
          <div className="px-3 py-2.5 rounded-lg bg-[#11141C] border border-white/[0.06] text-[11.5px] text-[#7E889E]">
            No teammates added yet.
          </div>
        ) : (
          <div className="space-y-2">
            {value.peers.map((peer, index) => {
              const rowError = validation.peerErrors[index];
              return (
                <div key={index} className="space-y-1">
                  <div className="flex items-center gap-1.5">
                    <input
                      type="text"
                      value={peer.name}
                      placeholder="Name (e.g. Benjamin)"
                      onChange={(e) =>
                        updatePeer(index, { name: e.target.value })
                      }
                      className={`w-[32%] h-8 px-2.5 rounded-lg bg-[#0B0D12] border text-[12px] text-[#E6EAF2] placeholder:text-[#52607A] outline-none focus:border-[#4E8DF5]/60 ${
                        rowError ? "border-rose-500/50" : "border-white/[0.08]"
                      }`}
                    />
                    <input
                      type="text"
                      value={peer.host}
                      placeholder="Tailscale host or IP"
                      onChange={(e) =>
                        updatePeer(index, { host: e.target.value })
                      }
                      className="flex-1 h-8 px-2.5 rounded-lg bg-[#0B0D12] border border-white/[0.08] text-[12px] font-mono text-[#E6EAF2] placeholder:text-[#52607A] outline-none focus:border-[#4E8DF5]/60"
                    />
                    <input
                      type="number"
                      min={1}
                      max={65535}
                      value={Number.isNaN(peer.port) ? "" : peer.port}
                      onChange={(e) => {
                        const parsed =
                          e.target.value === "" ? 0 : Number(e.target.value);
                        updatePeer(index, { port: Math.trunc(parsed) });
                      }}
                      className={`w-[82px] h-8 px-2 rounded-lg bg-[#0B0D12] border text-[12px] font-mono tabular-nums text-[#E6EAF2] outline-none focus:border-[#4E8DF5]/60 ${
                        rowError ? "border-rose-500/50" : "border-white/[0.08]"
                      }`}
                    />
                    <button
                      type="button"
                      onClick={() => removePeer(index)}
                      title="Remove peer"
                      aria-label="Remove peer"
                      className="h-8 w-8 flex items-center justify-center rounded-lg text-[#7E889E] hover:text-rose-300 hover:bg-rose-500/15 transition-colors cursor-pointer shrink-0"
                    >
                      <Trash2 className="w-3.5 h-3.5" />
                    </button>
                  </div>
                  {rowError && (
                    <div className="text-[11px] text-rose-300 pl-1">
                      {rowError}
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        )}
      </div>
    </section>
  );
}
