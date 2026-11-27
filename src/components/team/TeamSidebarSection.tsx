import type { RemoteSessionInfo, TeamSnapshot } from "../../types/ipc";
import { AgentIcon } from "../icons/AgentIcons";
import { getSessionStatusDotClass } from "./taskStyles";

export interface SelectedRemoteTarget {
  peer: string;
  sessionId: string;
}

export interface TeamSidebarSectionProps {
  team: TeamSnapshot | null;
  selectedRemote: SelectedRemoteTarget | null;
  onSelectRemote: (target: SelectedRemoteTarget) => void;
  onOpenSettings?: () => void;
}

export function TeamSidebarSection({
  team,
  selectedRemote,
  onSelectRemote,
  onOpenSettings,
}: TeamSidebarSectionProps) {
  if (!team?.enabled) return null;

  if (team.peers.length === 0) {
    return (
      <div className="pt-2.5 mt-2 border-t border-white/[0.06]">
        <div className="px-2 pb-1 text-[11px] text-[var(--velox-ink-muted)]">
          Team
        </div>
        {onOpenSettings ? (
          <button
            type="button"
            onClick={onOpenSettings}
            className="w-full px-2 py-1 text-left text-[11.5px] text-[var(--velox-ink-muted)] hover:text-[var(--velox-ink-secondary)] transition-colors cursor-pointer"
          >
            Add a teammate in Settings
          </button>
        ) : (
          <div className="px-2 py-1 text-[11.5px] text-[var(--velox-ink-muted)]">
            Add a teammate in Settings
          </div>
        )}
      </div>
    );
  }

  return (
    <div className="pt-2.5 mt-2 border-t border-white/[0.06] space-y-3">
      {team.peers.map((peer) => {
        const peerTitle = peer.displayName
          ? `${peer.displayName} (${peer.host}:${peer.port})`
          : `${peer.host}:${peer.port}`;

        return (
          <div key={peer.name} className="space-y-1">
            <div
              className="px-2 pb-0.5 flex items-center justify-between text-[11px] text-[var(--velox-ink-muted)]"
              title={peerTitle}
            >
              <div className="flex items-center gap-1.5 min-w-0">
                <span
                  className={`w-1.5 h-1.5 rounded-full shrink-0 ${
                    peer.online ? "bg-emerald-400" : "bg-[var(--velox-ink-faint)]"
                  }`}
                />
                <span className="truncate font-medium text-[var(--velox-ink-secondary)]">
                  {peer.name}
                </span>
              </div>
              <span className="font-mono text-[10.5px] tabular-nums shrink-0">
                {peer.sessions.length}
              </span>
            </div>

            {!peer.online ? (
              <div
                title={peer.lastError ?? undefined}
                className="px-2.5 py-1.5 text-[11.5px] text-[var(--velox-ink-muted)]"
              >
                Offline
              </div>
            ) : peer.sessions.length === 0 ? (
              <div className="px-2.5 py-1 text-[11.5px] text-[var(--velox-ink-faint)]">
                No shared sessions
              </div>
            ) : (
              <div className="space-y-1">
                {peer.sessions.map((session) => (
                  <RemoteSessionRow
                    key={`${peer.name}:${session.id}`}
                    peerName={peer.name}
                    session={session}
                    isActive={
                      selectedRemote?.peer === peer.name &&
                      selectedRemote?.sessionId === session.id
                    }
                    onSelect={() =>
                      onSelectRemote({ peer: peer.name, sessionId: session.id })
                    }
                  />
                ))}
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
}

interface RemoteSessionRowProps {
  peerName: string;
  session: RemoteSessionInfo;
  isActive: boolean;
  onSelect: () => void;
}

function RemoteSessionRow({
  peerName,
  session,
  isActive,
  onSelect,
}: RemoteSessionRowProps) {
  const effectiveKind = session.activeAgent ?? session.agentKind;
  const isAsleep = session.status === "sleeping" || session.status === "exited";
  const secondaryLine = session.summary ?? session.folder;

  return (
    <div
      role="button"
      tabIndex={0}
      onClick={onSelect}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onSelect();
        }
      }}
      className={`group relative flex items-center gap-2.5 px-2.5 py-2 rounded-lg transition-colors cursor-pointer ${
        isActive
          ? "velox-row-active bg-[var(--velox-active)] border border-white/[0.09]"
          : "velox-row border border-transparent hover:bg-[var(--velox-elevated)]"
      }`}
    >
      <div className="shrink-0" title={session.status}>
        <span className="flex w-[18px] h-[18px]">
          <AgentIcon
            kind={effectiveKind}
            className="w-[18px] h-[18px]"
            active={!isAsleep}
            blocked={session.status === "blocked"}
          />
        </span>
      </div>

      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-1.5 min-w-0">
          <span
            className={`truncate text-[12.5px] font-medium ${
              isAsleep
                ? "text-[var(--velox-ink-soft)]"
                : "text-[var(--velox-ink)]"
            }`}
          >
            {session.title}
          </span>
          <span
            className="shrink-0 px-1 py-0.2 rounded text-[10px] font-medium bg-white/[0.06]"
            style={{ color: "#F5B971" }}
          >
            {peerName}
          </span>
          <span
            title={session.status}
            className={`w-1.5 h-1.5 rounded-full shrink-0 ml-auto ${getSessionStatusDotClass(
              session.status,
            )}`}
          />
        </div>
        <div
          title={secondaryLine}
          className="truncate text-[11px] font-mono text-[var(--velox-ink-muted)]"
        >
          {secondaryLine}
        </div>
      </div>
    </div>
  );
}
