import { useEffect, useRef } from "react";
import { ArrowDownLeft, ArrowUpRight, Users } from "lucide-react";
import type { TaskTicket, TeamSnapshot } from "../../types/ipc";
import { formatTaskTime, getTaskStateBadge } from "./taskStyles";

export interface TeamTasksPopoverProps {
  open: boolean;
  onClose: () => void;
  team: TeamSnapshot;
  tasks: TaskTicket[];
  anchorRef: React.RefObject<HTMLButtonElement | null>;
}

export function TeamTasksPopover({
  open,
  onClose,
  team,
  tasks,
  anchorRef,
}: TeamTasksPopoverProps) {
  const popoverRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!open) return;

    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        onClose();
      }
    };

    const onPointerDown = (e: PointerEvent) => {
      const target = e.target as Node;
      if (
        popoverRef.current?.contains(target) ||
        anchorRef.current?.contains(target)
      ) {
        return;
      }
      onClose();
    };

    window.addEventListener("keydown", onKeyDown, true);
    window.addEventListener("pointerdown", onPointerDown, true);
    return () => {
      window.removeEventListener("keydown", onKeyDown, true);
      window.removeEventListener("pointerdown", onPointerDown, true);
    };
  }, [open, onClose, anchorRef]);

  if (!open) return null;

  const recentTasks = tasks.slice(0, 20);
  const onlinePeers = team.peers.filter((p) => p.online).length;

  return (
    <div
      ref={popoverRef}
      role="dialog"
      aria-label="Team activity and tasks"
      className="fixed right-3 top-11 z-50 w-[396px] max-w-[calc(100vw-1.5rem)] max-h-[70vh] rounded-xl bg-[var(--velox-elevated)] border border-white/[0.1] shadow-2xl flex flex-col overflow-hidden animate-velox-pop-in"
    >
      <div className="px-3.5 py-2.5 border-b border-white/[0.07] flex items-center justify-between gap-2 bg-[var(--velox-surface)]/80">
        <div className="flex items-center gap-2 min-w-0">
          <Users className="w-3.5 h-3.5 text-[#9CC2FF] shrink-0" />
          <span className="text-[12.5px] font-medium text-[var(--velox-ink)] truncate">
            Team tasks
          </span>
        </div>
        <div className="flex items-center gap-2 text-[11px] font-mono tabular-nums text-[var(--velox-ink-muted)] shrink-0">
          <span>
            {onlinePeers}/{team.peers.length} online
          </span>
          {team.watchers.length > 0 && (
            <span title={team.watchers.join(", ")}>
              · {team.watchers.length} watching
            </span>
          )}
        </div>
      </div>

      {team.watchers.length > 0 && (
        <div className="px-3.5 py-1.5 border-b border-white/[0.06] bg-white/[0.02] text-[11px] text-[var(--velox-ink-secondary)] truncate">
          Watching now: {team.watchers.join(", ")}
        </div>
      )}

      <div className="flex-1 overflow-y-auto divide-y divide-white/[0.06]">
        {recentTasks.length === 0 ? (
          <div className="py-10 px-4 text-center text-[12px] text-[var(--velox-ink-muted)]">
            No team tasks yet
          </div>
        ) : (
          recentTasks.map((task) => {
            const badge = getTaskStateBadge(task.state);
            const peerLabel = task.peer ?? "This PC";
            const sessionLabel = task.sessionTitle ?? task.sessionId;
            const time = formatTaskTime(task.updatedAt || task.createdAt);

            return (
              <div
                key={task.requestId}
                className="px-3.5 py-2.5 space-y-1 hover:bg-white/[0.02] transition-colors"
              >
                <div className="flex items-center justify-between gap-2">
                  <div className="flex items-center gap-1.5 min-w-0 text-[12px]">
                    {task.direction === "incoming" ? (
                      <ArrowDownLeft
                        aria-label="Incoming"
                        className="w-3.5 h-3.5 text-amber-300 shrink-0"
                      />
                    ) : (
                      <ArrowUpRight
                        aria-label="Outgoing"
                        className="w-3.5 h-3.5 text-[#9CC2FF] shrink-0"
                      />
                    )}
                    <span
                      className="font-medium shrink-0"
                      style={{ color: "#F5B971" }}
                    >
                      {peerLabel}
                    </span>
                    <span className="text-[var(--velox-ink-muted)]">·</span>
                    <span className="text-[var(--velox-ink)] truncate">
                      {sessionLabel}
                    </span>
                  </div>

                  <div className="flex items-center gap-1.5 shrink-0">
                    <span
                      className={`px-1.5 py-0.5 text-[10px] font-medium rounded border ${badge.className}`}
                    >
                      {badge.label}
                    </span>
                    {time && (
                      <span className="text-[10.5px] font-mono tabular-nums text-[var(--velox-ink-muted)]">
                        {time}
                      </span>
                    )}
                  </div>
                </div>

                <p className="pl-5 text-[11.5px] font-mono text-[var(--velox-ink-secondary)] line-clamp-2 break-words">
                  {task.text}
                </p>

                {task.message && (
                  <p className="pl-5 text-[11px] text-[var(--velox-ink-muted)] truncate">
                    {task.message}
                  </p>
                )}
              </div>
            );
          })
        )}
      </div>
    </div>
  );
}
