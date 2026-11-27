import { useEffect, useRef, useState } from "react";
import { ArrowRight, Check, Loader2, Terminal, X } from "lucide-react";
import type { TaskTicket } from "../../types/ipc";

export interface TaskInboxProps {
  pendingIncoming: TaskTicket[];
  onRespond: (requestId: string, accept: boolean) => Promise<TaskTicket>;
}

const MAX_VISIBLE_CARDS = 3;

export function TaskInbox({ pendingIncoming, onRespond }: TaskInboxProps) {
  const [pendingAction, setPendingAction] = useState<
    Record<string, "accept" | "reject">
  >({});
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [pulsingIds, setPulsingIds] = useState<Record<string, boolean>>({});
  const seenIdsRef = useRef<Set<string>>(new Set());

  useEffect(() => {
    const currentIds = new Set(pendingIncoming.map((t) => t.requestId));

    // Prune resolved entries
    setPendingAction((prev) => {
      let changed = false;
      const next: Record<string, "accept" | "reject"> = {};
      for (const [id, action] of Object.entries(prev)) {
        if (currentIds.has(id)) next[id] = action;
        else changed = true;
      }
      return changed ? next : prev;
    });

    setErrors((prev) => {
      let changed = false;
      const next: Record<string, string> = {};
      for (const [id, msg] of Object.entries(prev)) {
        if (currentIds.has(id)) next[id] = msg;
        else changed = true;
      }
      return changed ? next : prev;
    });

    // Attention pulse for newly arrived tasks
    const newlyArrived: string[] = [];
    for (const task of pendingIncoming) {
      if (!seenIdsRef.current.has(task.requestId)) {
        seenIdsRef.current.add(task.requestId);
        newlyArrived.push(task.requestId);
      }
    }

    if (newlyArrived.length === 0) return;

    setPulsingIds((prev) => {
      const next = { ...prev };
      for (const id of newlyArrived) next[id] = true;
      return next;
    });

    const timer = window.setTimeout(() => {
      setPulsingIds((prev) => {
        const next = { ...prev };
        for (const id of newlyArrived) delete next[id];
        return next;
      });
    }, 2400);

    return () => window.clearTimeout(timer);
  }, [pendingIncoming]);

  if (pendingIncoming.length === 0) return null;

  const visible = pendingIncoming.slice(0, MAX_VISIBLE_CARDS);
  const extraCount = pendingIncoming.length - visible.length;

  const handleDecision = async (requestId: string, accept: boolean) => {
    const action = accept ? "accept" : "reject";
    setPendingAction((prev) => ({ ...prev, [requestId]: action }));
    setErrors((prev) => {
      if (!(requestId in prev)) return prev;
      const next = { ...prev };
      delete next[requestId];
      return next;
    });

    try {
      const updated = await onRespond(requestId, accept);
      if (updated.state !== "pending") {
        setPendingAction((prev) => {
          const next = { ...prev };
          delete next[requestId];
          return next;
        });
      }
    } catch (err) {
      const msg =
        err instanceof Error && err.message
          ? err.message
          : "Could not submit response";
      setPendingAction((prev) => {
        const next = { ...prev };
        delete next[requestId];
        return next;
      });
      setErrors((prev) => ({ ...prev, [requestId]: msg }));
    }
  };

  return (
    <div
      aria-live="polite"
      className="fixed bottom-4 right-4 z-[60] flex flex-col gap-2.5 w-[368px] max-w-[calc(100vw-2rem)] pointer-events-none"
    >
      {extraCount > 0 && (
        <div className="self-end pointer-events-auto px-2.5 py-1 rounded-full bg-[var(--velox-elevated)] border border-white/[0.1] text-[11px] font-mono tabular-nums text-[var(--velox-ink-secondary)] shadow-lg">
          +{extraCount} more
        </div>
      )}

      {visible.map((task) => {
        const activeAction = pendingAction[task.requestId];
        const busy = activeAction !== undefined;
        const cardError = errors[task.requestId];
        const isPulsing = Boolean(pulsingIds[task.requestId]);
        const sender = task.peer ?? "This PC";
        const target = task.sessionTitle ?? task.sessionId;

        return (
          <div
            key={task.requestId}
            className={`pointer-events-auto rounded-xl bg-[var(--velox-elevated)] border shadow-2xl p-3.5 space-y-2.5 animate-velox-pop-in transition-colors ${
              isPulsing
                ? "border-amber-400/50 velox-task-pulse"
                : "border-white/[0.1]"
            }`}
          >
            {/* Header */}
            <div className="flex items-start justify-between gap-2">
              <div className="min-w-0 space-y-0.5">
                <div className="flex items-center gap-1.5 text-[12.5px] font-medium text-[var(--velox-ink)] min-w-0">
                  <span
                    className={`w-2 h-2 rounded-full shrink-0 bg-amber-400 ${
                      isPulsing ? "velox-dot-pulse" : ""
                    }`}
                  />
                  <span className="truncate" style={{ color: "#F5B971" }}>
                    {sender}
                  </span>
                  <ArrowRight className="w-3 h-3 shrink-0 text-[var(--velox-ink-muted)]" />
                  <span className="truncate">{target}</span>
                </div>

                {(task.fromSession || task.fromName) && (
                  <div className="flex items-center gap-1.5 text-[11px] text-[var(--velox-ink-muted)] pl-3.5 truncate">
                    <Terminal className="w-3 h-3 shrink-0" />
                    <span className="truncate">
                      {task.fromSession
                        ? `${task.fromName} · ${task.fromSession}`
                        : task.fromName}
                    </span>
                  </div>
                )}
              </div>

              <span className="shrink-0 px-1.5 py-0.5 rounded text-[10px] font-medium bg-amber-500/15 text-amber-300 border border-amber-500/25">
                Task request
              </span>
            </div>

            {/* Task text */}
            <div className="max-h-[160px] overflow-y-auto rounded-lg bg-[var(--velox-canvas)] border border-white/[0.06] px-3 py-2 font-mono text-[12px] leading-relaxed text-[var(--velox-ink)] whitespace-pre-wrap break-words select-text">
              {task.text}
            </div>

            {cardError && (
              <div className="px-2.5 py-1.5 rounded-md bg-rose-500/10 border border-rose-500/25 text-[11.5px] text-rose-300">
                {cardError}
              </div>
            )}

            {/* Actions */}
            <div className="flex items-center justify-end gap-2 pt-0.5">
              <button
                type="button"
                disabled={busy}
                onClick={() => void handleDecision(task.requestId, false)}
                className="h-8 px-3 rounded-lg bg-[#1C2130] hover:bg-[#252C40] disabled:opacity-50 disabled:cursor-not-allowed border border-white/[0.08] text-[12px] font-medium text-[var(--velox-ink-secondary)] hover:text-[var(--velox-ink)] inline-flex items-center gap-1.5 transition-colors cursor-pointer"
              >
                {activeAction === "reject" ? (
                  <Loader2 className="w-3.5 h-3.5 animate-spin" />
                ) : (
                  <X className="w-3.5 h-3.5" />
                )}
                <span>Reject</span>
              </button>

              <button
                type="button"
                disabled={busy}
                onClick={() => void handleDecision(task.requestId, true)}
                className="h-8 px-3.5 rounded-lg bg-[#4E8DF5] hover:bg-[#5E98F7] disabled:opacity-50 disabled:cursor-not-allowed text-[#080B12] text-[12px] font-semibold inline-flex items-center gap-1.5 transition-colors cursor-pointer"
              >
                {activeAction === "accept" ? (
                  <Loader2 className="w-3.5 h-3.5 animate-spin" />
                ) : (
                  <Check className="w-3.5 h-3.5" />
                )}
                <span>Accept</span>
              </button>
            </div>
          </div>
        );
      })}
    </div>
  );
}
