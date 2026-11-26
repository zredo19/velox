import { memo, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import {
  ClipboardCopy,
  Copy,
  Moon,
  Pencil,
  Play,
  Plus,
  Search,
  Trash2,
  X,
} from "lucide-react";
import type { AgentKind, SessionRecord } from "../types/ipc";
import type { AgentActivity } from "../lib/agentActivity";
import { partitionSessions } from "../hooks/useSessions";
import { AgentIcon } from "./icons/AgentIcons";
import { AgentCrew } from "./icons/AgentCrew";

type SessionGroup = "awake" | "sleeping" | "exited";

const DRAG_THRESHOLD_PX = 5;

export interface SidebarProps {
  sessions: SessionRecord[];
  /** What each awake agent is doing, read from its screen. */
  activity: Record<string, AgentActivity | null>;
  activeSessionId: string | null;
  onSelectSession: (id: string) => void;
  onWakeSession: (id: string) => void;
  onSleepSession: (id: string) => void;
  onCloseSession: (id: string) => void;
  onRenameSession: (id: string, title: string) => void;
  onDuplicateSession: (session: SessionRecord) => void;
  onCopyPath: (path: string) => void;
  onReorderSessions: (sessionIds: string[]) => void;
  onSleepIdleSessions: () => void;
  onOpenLauncher: () => void;
  onLaunchAgentKind?: (kind: AgentKind) => void;
}

const ANIMATION_STEP_MS = 100;
const SPIN_FRAME_MS = 300;
const FLOAT_PERIOD_MS = 1400;

/**
 * Moves the icons of working agents, like herdr: Claude's mark turns in steps, the others
 * hover and breathe. Done from a timer because a CSS animation recomposites the whole window
 * at the display's refresh rate.
 */
function useIconAnimation(rootRef: React.RefObject<HTMLElement | null>, active: boolean) {
  useEffect(() => {
    if (!active) return;
    const timer = window.setInterval(() => {
      if (document.hidden) return;
      const now = performance.now();
      const turn = (Math.floor(now / SPIN_FRAME_MS) % 12) * 30;
      const phase = ((now % FLOAT_PERIOD_MS) / FLOAT_PERIOD_MS) * Math.PI * 2;
      rootRef.current?.querySelectorAll<HTMLElement>("[data-anim]").forEach((icon) => {
        if (icon.dataset.anim === "spin") {
          icon.style.transform = `rotate(${turn}deg)`;
        } else {
          icon.style.transform = `translateY(${(Math.sin(phase) * 1.6).toFixed(1)}px)`;
          icon.style.opacity = (0.87 + 0.13 * Math.cos(phase)).toFixed(2);
        }
      });
    }, ANIMATION_STEP_MS);
    return () => window.clearInterval(timer);
  }, [rootRef, active]);
}

function getCwdBasename(cwd: string): string {
  const parts = cwd.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? cwd;
}

function formatMemory(mb: number): string {
  if (mb < 0.5) return "0 MB";
  return `${Math.round(mb)} MB`;
}

export const Sidebar = memo(function Sidebar({
  sessions,
  activity,
  activeSessionId,
  onSelectSession,
  onWakeSession,
  onSleepSession,
  onCloseSession,
  onRenameSession,
  onDuplicateSession,
  onCopyPath,
  onReorderSessions,
  onSleepIdleSessions,
  onOpenLauncher,
  onLaunchAgentKind,
}: SidebarProps) {
  const [renamingId, setRenamingId] = useState<string | null>(null);
  const [renameValue, setRenameValue] = useState("");
  const [filterOpen, setFilterOpen] = useState(false);
  const [filterQuery, setFilterQuery] = useState("");
  const [contextMenu, setContextMenu] = useState<{
    sessionId: string;
    x: number;
    y: number;
  } | null>(null);
  const [menuPos, setMenuPos] = useState<{ left: number; top: number } | null>(null);
  const [dragState, setDragState] = useState<{
    sessionId: string;
    group: SessionGroup;
    dragIndex: number;
    insertSlot: number;
  } | null>(null);

  const renameInputRef = useRef<HTMLInputElement | null>(null);
  const filterInputRef = useRef<HTMLInputElement | null>(null);
  const asideRef = useRef<HTMLElement | null>(null);
  const menuRef = useRef<HTMLDivElement | null>(null);
  const rowRefs = useRef<Map<string, HTMLDivElement>>(new Map());
  const suppressClickRef = useRef(false);
  const cleanupDragRef = useRef<(() => void) | null>(null);

  const anyWorking = useMemo(
    () => sessions.some((s) => activity[s.id] === "working"),
    [sessions, activity],
  );
  useIconAnimation(asideRef, anyWorking);

  const filteredSessions = useMemo(() => {
    const q = filterQuery.trim().toLowerCase();
    if (!q) return sessions;
    return sessions.filter((s) => {
      const kind = s.activeAgent ?? s.agentKind;
      return (
        s.title.toLowerCase().includes(q) ||
        s.cwd.toLowerCase().includes(q) ||
        kind.toLowerCase().includes(q) ||
        (s.profile && s.profile.toLowerCase().includes(q))
      );
    });
  }, [sessions, filterQuery]);

  const groups = useMemo(() => partitionSessions(filteredSessions), [filteredSessions]);
  const { awake: awakeSessions, sleeping: sleepingSessions, exited: exitedSessions } = groups;

  // Map each session to its global 1-9 shortcut index from the unfiltered list
  const shortcutIndexById = useMemo(() => {
    const map = new Map<string, number>();
    const visible = partitionSessions(sessions).visible;
    for (let i = 0; i < Math.min(9, visible.length); i++) {
      map.set(visible[i].id, i + 1);
    }
    return map;
  }, [sessions]);

  const groupsRef = useRef(groups);
  groupsRef.current = groups;
  const onReorderRef = useRef(onReorderSessions);
  onReorderRef.current = onReorderSessions;

  useEffect(() => () => cleanupDragRef.current?.(), []);

  useEffect(() => {
    if (renamingId && renameInputRef.current) {
      renameInputRef.current.focus();
      renameInputRef.current.select();
    }
  }, [renamingId]);

  useEffect(() => {
    if (filterOpen) {
      filterInputRef.current?.focus();
    } else {
      setFilterQuery("");
    }
  }, [filterOpen]);

  useLayoutEffect(() => {
    if (!contextMenu) {
      setMenuPos(null);
      return;
    }
    const menu = menuRef.current;
    if (!menu) return;
    const rect = menu.getBoundingClientRect();
    const margin = 8;
    setMenuPos({
      left: Math.max(margin, Math.min(contextMenu.x, window.innerWidth - rect.width - margin)),
      top: Math.max(margin, Math.min(contextMenu.y, window.innerHeight - rect.height - margin)),
    });
  }, [contextMenu]);

  useEffect(() => {
    if (!contextMenu) return;
    const close = () => setContextMenu(null);
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        close();
      }
    };
    const onPointerDown = (e: PointerEvent) => {
      if (menuRef.current?.contains(e.target as Node)) return;
      close();
    };
    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("pointerdown", onPointerDown, true);
    window.addEventListener("scroll", close, true);
    window.addEventListener("wheel", close, { capture: true, passive: true });
    window.addEventListener("blur", close);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("pointerdown", onPointerDown, true);
      window.removeEventListener("scroll", close, true);
      window.removeEventListener("wheel", close, true);
      window.removeEventListener("blur", close);
    };
  }, [contextMenu]);

  const handleRenameStart = (session: SessionRecord) => {
    setContextMenu(null);
    setRenamingId(session.id);
    setRenameValue(session.title);
  };

  const handleRenameConfirm = (id: string) => {
    const trimmed = renameValue.trim();
    const existing = sessions.find((s) => s.id === id);
    if (trimmed && existing && trimmed !== existing.title) {
      onRenameSession(id, trimmed);
    }
    setRenamingId(null);
    setRenameValue("");
  };

  const handleRenameCancel = () => {
    setRenamingId(null);
    setRenameValue("");
  };

  const handleRowPointerDown = (
    e: React.PointerEvent<HTMLDivElement>,
    session: SessionRecord,
    group: SessionGroup,
    index: number,
  ) => {
    if (filterQuery.trim()) return; // Disable drag-reorder while filtered
    if (e.button !== 0 || renamingId === session.id) return;
    if ((e.target as HTMLElement).closest("button, input")) return;
    if (groupsRef.current[group].length <= 1) return;

    cleanupDragRef.current?.();

    const startX = e.clientX;
    const startY = e.clientY;
    const pointerId = e.pointerId;
    let dragging = false;
    let lastSlot = index;

    const cleanup = () => {
      window.removeEventListener("pointermove", onPointerMove);
      window.removeEventListener("pointerup", onPointerUp);
      window.removeEventListener("pointercancel", onPointerUp);
      window.removeEventListener("blur", onBlur);
      document.body.style.cursor = "";
      cleanupDragRef.current = null;
    };

    const onPointerMove = (ev: PointerEvent) => {
      if (ev.pointerId !== pointerId) return;
      if (!dragging) {
        if (Math.hypot(ev.clientX - startX, ev.clientY - startY) < DRAG_THRESHOLD_PX) return;
        dragging = true;
        suppressClickRef.current = true;
        setContextMenu(null);
        document.body.style.cursor = "grabbing";
      }

      const list = groupsRef.current[group];
      let slot = list.length;
      for (let i = 0; i < list.length; i += 1) {
        const el = rowRefs.current.get(list[i].id);
        if (!el) continue;
        const rect = el.getBoundingClientRect();
        if (ev.clientY < rect.top + rect.height / 2) {
          slot = i;
          break;
        }
      }
      lastSlot = slot;
      const currentDragIdx = list.findIndex((s) => s.id === session.id);
      if (currentDragIdx < 0) return;
      setDragState({
        sessionId: session.id,
        group,
        dragIndex: currentDragIdx,
        insertSlot: slot,
      });
    };

    const finishDrag = (commit: boolean) => {
      cleanup();
      if (dragging) {
        window.setTimeout(() => {
          suppressClickRef.current = false;
        }, 0);
        if (commit) {
          const currentGroups = groupsRef.current;
          const list = currentGroups[group];
          const curIdx = list.findIndex((s) => s.id === session.id);
          if (curIdx >= 0 && lastSlot !== curIdx && lastSlot !== curIdx + 1) {
            const nextGroup = [...list];
            const [moved] = nextGroup.splice(curIdx, 1);
            const targetIdx = lastSlot > curIdx ? lastSlot - 1 : lastSlot;
            nextGroup.splice(targetIdx, 0, moved);
            const allIds = [
              ...(group === "awake" ? nextGroup : currentGroups.awake),
              ...(group === "sleeping" ? nextGroup : currentGroups.sleeping),
              ...(group === "exited" ? nextGroup : currentGroups.exited),
            ].map((s) => s.id);
            onReorderRef.current(allIds);
          }
        }
      }
      setDragState(null);
    };

    const onPointerUp = (ev: PointerEvent) => {
      if (ev.pointerId !== pointerId) return;
      finishDrag(ev.type === "pointerup");
    };

    const onBlur = () => finishDrag(false);

    window.addEventListener("pointermove", onPointerMove);
    window.addEventListener("pointerup", onPointerUp);
    window.addEventListener("pointercancel", onPointerUp);
    window.addEventListener("blur", onBlur);
    cleanupDragRef.current = cleanup;
  };

  const renderGroupRows = (group: SessionGroup, list: SessionRecord[]) =>
    list.map((session, index) => {
      const isActiveDragGroup = dragState?.group === group;
      const showBefore =
        isActiveDragGroup &&
        dragState.insertSlot === index &&
        dragState.insertSlot !== dragState.dragIndex &&
        dragState.insertSlot !== dragState.dragIndex + 1;
      const showAfter =
        isActiveDragGroup &&
        index === list.length - 1 &&
        dragState.insertSlot === list.length &&
        dragState.dragIndex !== list.length - 1;
      const dropIndicator = showBefore ? "before" : showAfter ? "after" : null;

      return (
        <SessionRow
          key={session.id}
          rowRef={(el) => {
            if (el) rowRefs.current.set(session.id, el);
            else rowRefs.current.delete(session.id);
          }}
          session={session}
          shortcutIndex={shortcutIndexById.get(session.id)}
          activity={activity[session.id] ?? null}
          isActive={activeSessionId === session.id}
          isRenaming={renamingId === session.id}
          isDragging={dragState?.sessionId === session.id}
          dropIndicator={dropIndicator}
          renameValue={renameValue}
          renameInputRef={renameInputRef}
          onPointerDown={(e) => handleRowPointerDown(e, session, group, index)}
          onContextMenu={(e) => {
            e.preventDefault();
            e.stopPropagation();
            if (renamingId === session.id) return;
            setContextMenu({ sessionId: session.id, x: e.clientX, y: e.clientY });
          }}
          onSelect={(id) => {
            if (suppressClickRef.current) {
              suppressClickRef.current = false;
              return;
            }
            onSelectSession(id);
          }}
          onWake={onWakeSession}
          onSleep={onSleepSession}
          onClose={onCloseSession}
          onRenameStart={handleRenameStart}
          onRenameChange={setRenameValue}
          onRenameConfirm={handleRenameConfirm}
          onRenameCancel={handleRenameCancel}
        />
      );
    });

  const menuSession = contextMenu
    ? sessions.find((s) => s.id === contextMenu.sessionId) ?? null
    : null;

  return (
    <aside
      ref={asideRef}
      className="velox-sidebar w-[256px] shrink-0 flex flex-col h-full select-none overflow-hidden"
    >
      {/* Header with filter toggle & "+" launcher button */}
      <div className="flex items-center justify-between h-11 px-3 border-b border-white/[0.06]">
        <span className="text-[12.5px] font-medium text-[var(--velox-ink-secondary)]">
          Sessions
          {sessions.length > 0 && (
            <span className="ml-1.5 font-mono text-[11px] text-[var(--velox-ink-muted)] tabular-nums">
              {awakeSessions.length}/{sessions.length}
            </span>
          )}
        </span>
        <div className="flex items-center gap-1">
          {sessions.length > 1 && (
            <button
              type="button"
              onClick={() => setFilterOpen((v) => !v)}
              title="Filter sessions"
              aria-label="Filter sessions"
              className={`h-7 w-7 inline-flex items-center justify-center rounded-md transition-colors cursor-pointer ${
                filterOpen || filterQuery
                  ? "bg-[var(--velox-active)] text-[var(--velox-ink)]"
                  : "text-[var(--velox-ink-muted)] hover:text-[var(--velox-ink)] hover:bg-[var(--velox-hover)]"
              }`}
            >
              <Search className="w-3.5 h-3.5" />
            </button>
          )}
          <button
            type="button"
            onClick={onOpenLauncher}
            title="New agent session (Ctrl+N)"
            aria-label="New agent session"
            className="h-7 px-2.5 inline-flex items-center gap-1 rounded-md bg-[var(--velox-accent)]/15 hover:bg-[var(--velox-accent)]/25 active:scale-[0.97] text-[var(--velox-accent-soft)] border border-[var(--velox-accent)]/30 text-[12px] font-medium transition-all duration-150 cursor-pointer"
          >
            <Plus className="w-3.5 h-3.5" />
            <span>New</span>
          </button>
        </div>
      </div>

      {/* Collapsible inline filter input */}
      {filterOpen && (
        <div className="px-2.5 py-2 border-b border-white/[0.06] animate-velox-toast-in">
          <div className="flex items-center gap-1.5 px-2 h-7 rounded-md bg-[var(--velox-canvas)]/75 border border-white/[0.09] focus-within:border-[var(--velox-accent)]/55 transition-colors">
            <Search className="w-3 h-3 text-[var(--velox-ink-muted)] shrink-0" />
            <input
              ref={filterInputRef}
              type="text"
              value={filterQuery}
              onChange={(e) => setFilterQuery(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Escape") {
                  e.stopPropagation();
                  setFilterOpen(false);
                }
              }}
              placeholder="Filter sessions..."
              className="flex-1 min-w-0 bg-transparent text-[12px] text-[var(--velox-ink)] placeholder:text-[var(--velox-ink-muted)] outline-none"
            />
            {filterQuery && (
              <button
                type="button"
                onClick={() => setFilterQuery("")}
                aria-label="Clear filter"
                className="text-[var(--velox-ink-muted)] hover:text-[var(--velox-ink)] cursor-pointer"
              >
                <X className="w-3 h-3" />
              </button>
            )}
          </div>
        </div>
      )}

      {/* Session list */}
      <div className="flex-1 overflow-y-auto px-2 py-2 space-y-1">
        {renderGroupRows("awake", awakeSessions)}

        {sleepingSessions.length > 0 && (
          <div className={awakeSessions.length > 0 ? "pt-2.5 mt-2 border-t border-white/[0.06]" : ""}>
            <div className="px-2 pb-1.5 flex items-center justify-between text-[11px] text-[var(--velox-ink-muted)]">
              <span>Sleeping</span>
              <span className="font-mono text-[10.5px]">0 MB</span>
            </div>
            <div className="space-y-1">
              {renderGroupRows("sleeping", sleepingSessions)}
            </div>
          </div>
        )}

        {exitedSessions.length > 0 && (
          <div className="pt-2.5 mt-2 border-t border-white/[0.06]">
            <div className="px-2 pb-1.5 text-[11px] text-[var(--velox-ink-muted)]">Exited</div>
            <div className="space-y-1">
              {renderGroupRows("exited", exitedSessions)}
            </div>
          </div>
        )}

        {sessions.length === 0 && (
          <div className="py-12 px-3 text-center">
            <p className="text-[12.5px] text-[var(--velox-ink-muted)]">No active sessions</p>
            <button
              type="button"
              onClick={onOpenLauncher}
              className="mt-2.5 text-[12px] text-[var(--velox-accent-soft)] hover:underline cursor-pointer"
            >
              Launch your first agent
            </button>
          </div>
        )}

        {sessions.length > 0 && filteredSessions.length === 0 && (
          <div className="py-10 px-3 text-center">
            <p className="text-[12px] text-[var(--velox-ink-muted)]">
              No sessions match "{filterQuery}"
            </p>
            <button
              type="button"
              onClick={() => setFilterQuery("")}
              className="mt-2 text-[11.5px] text-[var(--velox-accent-soft)] hover:underline cursor-pointer"
            >
              Clear filter
            </button>
          </div>
        )}
      </div>

      {/* Rich context menu */}
      {contextMenu && menuSession && (
        <div
          ref={menuRef}
          role="menu"
          className="fixed z-50 min-w-[176px] py-1 rounded-xl bg-[var(--velox-elevated)] border border-white/[0.11] shadow-[0_18px_40px_-12px_rgba(0,0,0,0.8)] animate-velox-pop-in"
          style={{
            left: menuPos?.left ?? contextMenu.x,
            top: menuPos?.top ?? contextMenu.y,
            visibility: menuPos ? "visible" : "hidden",
          }}
        >
          <button
            type="button"
            role="menuitem"
            onClick={() => handleRenameStart(menuSession)}
            className="w-full px-3 py-1.5 text-left text-[12px] text-[var(--velox-ink)] hover:bg-[var(--velox-hover)] flex items-center gap-2 transition-colors cursor-pointer"
          >
            <Pencil className="w-3.5 h-3.5 text-[var(--velox-ink-muted)]" />
            <span>Rename</span>
          </button>

          <button
            type="button"
            role="menuitem"
            onClick={() => {
              setContextMenu(null);
              if (menuSession.status === "sleeping" || menuSession.status === "exited") {
                onWakeSession(menuSession.id);
              } else {
                onSleepSession(menuSession.id);
              }
            }}
            className="w-full px-3 py-1.5 text-left text-[12px] text-[var(--velox-ink)] hover:bg-[var(--velox-hover)] flex items-center gap-2 transition-colors cursor-pointer"
          >
            {menuSession.status === "sleeping" || menuSession.status === "exited" ? (
              <>
                <Play className="w-3.5 h-3.5 text-[var(--velox-accent-soft)]" />
                <span>Wake session</span>
              </>
            ) : (
              <>
                <Moon className="w-3.5 h-3.5 text-[var(--velox-ink-muted)]" />
                <span>Sleep (free RAM)</span>
              </>
            )}
          </button>

          <button
            type="button"
            role="menuitem"
            onClick={() => {
              setContextMenu(null);
              onDuplicateSession(menuSession);
            }}
            className="w-full px-3 py-1.5 text-left text-[12px] text-[var(--velox-ink)] hover:bg-[var(--velox-hover)] flex items-center gap-2 transition-colors cursor-pointer"
          >
            <Copy className="w-3.5 h-3.5 text-[var(--velox-ink-muted)]" />
            <span>Duplicate session</span>
          </button>

          <button
            type="button"
            role="menuitem"
            onClick={() => {
              setContextMenu(null);
              onCopyPath(menuSession.cwd);
            }}
            className="w-full px-3 py-1.5 text-left text-[12px] text-[var(--velox-ink)] hover:bg-[var(--velox-hover)] flex items-center gap-2 transition-colors cursor-pointer"
          >
            <ClipboardCopy className="w-3.5 h-3.5 text-[var(--velox-ink-muted)]" />
            <span>Copy folder path</span>
          </button>

          <div className="my-1 border-t border-white/[0.07]" />

          <button
            type="button"
            role="menuitem"
            onClick={() => {
              setContextMenu(null);
              onCloseSession(menuSession.id);
            }}
            className="w-full px-3 py-1.5 text-left text-[12px] text-rose-300 hover:bg-rose-500/15 flex items-center gap-2 transition-colors cursor-pointer"
          >
            <Trash2 className="w-3.5 h-3.5" />
            <span>Close session</span>
          </button>
        </div>
      )}

      {/* Footer quick hibernate action */}
      {awakeSessions.length > 1 && (
        <div className="p-2 border-t border-white/[0.06]">
          <button
            type="button"
            onClick={onSleepIdleSessions}
            className="w-full h-8 px-2.5 rounded-lg inline-flex items-center justify-center gap-1.5 text-[12px] text-[var(--velox-ink-soft)] hover:text-[var(--velox-ink)] hover:bg-[var(--velox-hover)] active:scale-[0.985] transition-all duration-150 cursor-pointer"
          >
            <Moon className="w-3.5 h-3.5" />
            <span>Sleep idle sessions</span>
          </button>
        </div>
      )}

      {/* The crew: Claude, Antigravity, OpenCode and Codex, happy to be here */}
      <AgentCrew onCritterClick={onLaunchAgentKind} />
    </aside>
  );
});

interface SessionRowProps {
  rowRef: (el: HTMLDivElement | null) => void;
  session: SessionRecord;
  shortcutIndex?: number;
  activity: AgentActivity | null;
  isActive: boolean;
  isRenaming: boolean;
  isDragging: boolean;
  dropIndicator: "before" | "after" | null;
  renameValue: string;
  renameInputRef: React.RefObject<HTMLInputElement | null>;
  onPointerDown: (e: React.PointerEvent<HTMLDivElement>) => void;
  onContextMenu: (e: React.MouseEvent<HTMLDivElement>) => void;
  onSelect: (id: string) => void;
  onWake: (id: string) => void;
  onSleep: (id: string) => void;
  onClose: (id: string) => void;
  onRenameStart: (session: SessionRecord) => void;
  onRenameChange: (val: string) => void;
  onRenameConfirm: (id: string) => void;
  onRenameCancel: () => void;
}

const SessionRow = memo(function SessionRow({
  rowRef,
  session,
  shortcutIndex,
  activity,
  isActive,
  isRenaming,
  isDragging,
  dropIndicator,
  renameValue,
  renameInputRef,
  onPointerDown,
  onContextMenu,
  onSelect,
  onWake,
  onSleep,
  onClose,
  onRenameStart,
  onRenameChange,
  onRenameConfirm,
  onRenameCancel,
}: SessionRowProps) {
  const isSleeping = session.status === "sleeping";
  const isAsleep = isSleeping || session.status === "exited";
  const effectiveKind = session.activeAgent ?? session.agentKind;
  const state = isAsleep ? session.status : (activity ?? "idle");
  const animation =
    state === "working" ? (effectiveKind === "claude" ? "spin" : "float") : undefined;

  return (
    <div
      ref={rowRef}
      role="button"
      tabIndex={0}
      onPointerDown={onPointerDown}
      onContextMenu={onContextMenu}
      onClick={() => {
        if (!isRenaming) onSelect(session.id);
      }}
      onDoubleClick={() => onRenameStart(session)}
      onKeyDown={(e) => {
        if (!isRenaming && (e.key === "Enter" || e.key === " ")) {
          e.preventDefault();
          onSelect(session.id);
        }
      }}
      className={`group relative flex items-center gap-2.5 px-2.5 py-2 rounded-lg transition-all duration-150 cursor-pointer ${
        isDragging ? "opacity-45 scale-[0.98]" : ""
      } ${
        isActive
          ? "velox-row-active bg-[var(--velox-active)] border border-white/[0.1]"
          : "velox-row border border-transparent hover:bg-[var(--velox-elevated)]"
      }`}
    >
      {/* Active glowing accent rail */}
      <span
        aria-hidden="true"
        className={`pointer-events-none absolute left-0 top-2 bottom-2 w-[2.5px] rounded-r-full bg-[var(--velox-accent)] transition-all duration-200 ${
          isActive
            ? "opacity-100 scale-y-100 shadow-[0_0_8px_var(--velox-accent)]"
            : "opacity-0 scale-y-50"
        }`}
      />

      {dropIndicator === "before" && (
        <div
          aria-hidden="true"
          className="pointer-events-none absolute left-1 right-1 -top-[3px] h-[2px] rounded-full bg-[var(--velox-accent)] shadow-[0_0_6px_var(--velox-accent)] z-20"
        />
      )}
      {dropIndicator === "after" && (
        <div
          aria-hidden="true"
          className="pointer-events-none absolute left-1 right-1 -bottom-[3px] h-[2px] rounded-full bg-[var(--velox-accent)] shadow-[0_0_6px_var(--velox-accent)] z-20"
        />
      )}

      {/* Agent icon: moves while working, red while it waits for an answer */}
      <div
        className="relative shrink-0"
        title={state === "blocked" ? "Waiting for your input" : state}
      >
        <span
          // Remounted on every state change, which also drops the animation styles.
          key={state}
          data-anim={animation}
          className="flex w-[18px] h-[18px]"
        >
          <AgentIcon
            kind={effectiveKind}
            className="w-[18px] h-[18px]"
            active={!isAsleep}
            blocked={state === "blocked"}
          />
        </span>
        {state === "blocked" && (
          <span
            aria-hidden="true"
            className="absolute -top-0.5 -right-0.5 w-2 h-2 rounded-full bg-rose-500 ring-2 ring-[var(--velox-surface)] animate-pulse"
          />
        )}
      </div>

      {/* Title, profile pill & folder */}
      <div className="flex-1 min-w-0">
        {isRenaming ? (
          <input
            ref={renameInputRef}
            type="text"
            value={renameValue}
            onClick={(e) => e.stopPropagation()}
            onChange={(e) => onRenameChange(e.target.value)}
            onBlur={() => onRenameConfirm(session.id)}
            onKeyDown={(e) => {
              e.stopPropagation();
              if (e.key === "Enter") {
                e.preventDefault();
                onRenameConfirm(session.id);
              } else if (e.key === "Escape") {
                e.preventDefault();
                onRenameCancel();
              }
            }}
            className="w-full bg-[var(--velox-canvas)] border border-[var(--velox-accent)]/55 rounded px-1.5 py-0.5 text-[12.5px] text-[var(--velox-ink)] outline-none"
          />
        ) : (
          <div className="flex items-center gap-1.5 min-w-0">
            <span
              className={`truncate text-[12.5px] font-medium ${
                isSleeping ? "text-[var(--velox-ink-soft)]" : "text-[var(--velox-ink)]"
              }`}
            >
              {session.title}
            </span>
            {session.profile && (
              <span
                className="shrink-0 px-1 py-0.2 rounded text-[10px] font-medium bg-white/[0.06]"
                style={{
                  color:
                    session.profile === "trabajo" ? "#7DD3C8" : "#E8B4D9",
                }}
              >
                {session.profile}
              </span>
            )}
          </div>
        )}
        <div className="flex items-center gap-1.5 min-w-0">
          <span className="truncate text-[11px] font-mono text-[var(--velox-ink-muted)]">
            {getCwdBasename(session.cwd)}
          </span>
          {state === "blocked" && (
            <span className="shrink-0 text-[9.5px] font-medium px-1 rounded bg-rose-500/18 text-rose-300">
              input
            </span>
          )}
          {state === "working" && (
            <span className="shrink-0 text-[9.5px] font-medium text-[var(--velox-accent-soft)] opacity-85">
              working
            </span>
          )}
        </div>
      </div>

      {/* Right slot: RAM pill smoothly cross-fades to hover action buttons */}
      <div className="relative shrink-0 flex items-center justify-end min-w-[46px] h-6">
        <div className="flex items-center gap-1 transition-opacity duration-150 group-hover:opacity-0 group-hover:pointer-events-none">
          {shortcutIndex !== undefined && (
            <span className="text-[9.5px] font-mono text-[var(--velox-ink-faint)] opacity-0 group-focus-visible:opacity-100">
              ^{shortcutIndex}
            </span>
          )}
          <span
            className={`text-[11px] font-mono tabular-nums ${
              isSleeping ? "text-[var(--velox-ink-faint)]" : "text-[var(--velox-ink-soft)]"
            }`}
          >
            {formatMemory(session.memoryMb)}
          </span>
        </div>

        <div className="absolute right-0 flex items-center gap-0.5 opacity-0 translate-x-1 pointer-events-none group-hover:opacity-100 group-hover:translate-x-0 group-hover:pointer-events-auto transition-all duration-150">
          {isSleeping || session.status === "exited" ? (
            <button
              type="button"
              onClick={(e) => {
                e.stopPropagation();
                onWake(session.id);
              }}
              title="Wake session"
              aria-label="Wake session"
              className="h-6 w-6 flex items-center justify-center rounded text-[var(--velox-accent-soft)] hover:bg-white/[0.1] active:scale-90 transition-all cursor-pointer"
            >
              <Play className="w-3 h-3 fill-current" />
            </button>
          ) : (
            <button
              type="button"
              onClick={(e) => {
                e.stopPropagation();
                onSleep(session.id);
              }}
              title="Sleep session (free RAM)"
              aria-label="Sleep session"
              className="h-6 w-6 flex items-center justify-center rounded text-[var(--velox-ink-secondary)] hover:text-[var(--velox-ink)] hover:bg-white/[0.1] active:scale-90 transition-all cursor-pointer"
            >
              <Moon className="w-3 h-3" />
            </button>
          )}
          <button
            type="button"
            onClick={(e) => {
              e.stopPropagation();
              onClose(session.id);
            }}
            title="Close session"
            aria-label="Close session"
            className="h-6 w-6 flex items-center justify-center rounded text-[var(--velox-ink-soft)] hover:text-[#F43F5E] hover:bg-[#F43F5E]/15 active:scale-90 transition-all cursor-pointer"
          >
            <X className="w-3.5 h-3.5" />
          </button>
        </div>
      </div>
    </div>
  );
});
