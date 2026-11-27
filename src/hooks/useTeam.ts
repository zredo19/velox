import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { veloxApi } from "../lib/tauri";
import type {
  LocalMessageEvent,
  SessionRecord,
  TaskTicket,
  TeamSnapshot,
} from "../types/ipc";

function getErrorMessage(err: unknown, fallback: string): string {
  if (err instanceof Error && err.message) return err.message;
  if (typeof err === "string" && err.trim().length > 0) return err;
  return fallback;
}

function isUnavailableTeamError(err: unknown): boolean {
  const msg = getErrorMessage(err, "").toLowerCase();
  return (
    msg.includes("unknown command") ||
    (msg.includes("command") && msg.includes("not found")) ||
    msg.includes("tauri_internals") ||
    msg.includes("cannot read properties of undefined")
  );
}

function upsertTask(list: TaskTicket[], ticket: TaskTicket): TaskTicket[] {
  const idx = list.findIndex((t) => t.requestId === ticket.requestId);
  if (idx >= 0) {
    const next = [...list];
    next[idx] = ticket;
    return next;
  }
  return [ticket, ...list].slice(0, 50);
}

export interface UseTeamResult {
  team: TeamSnapshot | null;
  tasks: TaskTicket[];
  pendingIncoming: TaskTicket[];
  localMessages: LocalMessageEvent[];
  error: string | null;
  clearError: () => void;
  dismissLocalMessage: (id: string) => void;
  setShared: (sessionId: string, shared: boolean) => Promise<SessionRecord | null>;
  sendTask: (
    peer: string | null,
    sessionId: string,
    text: string,
  ) => Promise<TaskTicket>;
  respond: (requestId: string, accept: boolean) => Promise<TaskTicket>;
  refresh: () => Promise<void>;
}

const LOCAL_MESSAGE_TTL_MS = 5500;
const LOCAL_MESSAGE_QUEUED_TTL_MS = 180_000;
const MAX_LOCAL_MESSAGES = 3;

function upsertLocalMessage(
  list: LocalMessageEvent[],
  incoming: LocalMessageEvent,
): LocalMessageEvent[] {
  const idx = list.findIndex((m) => m.id === incoming.id);
  if (idx >= 0) {
    const next = [...list];
    next[idx] = incoming;
    return next;
  }
  return [incoming, ...list].slice(0, MAX_LOCAL_MESSAGES);
}

export function useTeam(): UseTeamResult {
  const [team, setTeam] = useState<TeamSnapshot | null>(null);
  const [tasks, setTasks] = useState<TaskTicket[]>([]);
  const [localMessages, setLocalMessages] = useState<LocalMessageEvent[]>([]);
  const [error, setError] = useState<string | null>(null);
  const messageTimersRef = useRef<Map<string, number>>(new Map());

  const clearError = useCallback(() => setError(null), []);

  const dismissLocalMessage = useCallback((id: string) => {
    const existing = messageTimersRef.current.get(id);
    if (existing !== undefined) {
      window.clearTimeout(existing);
      messageTimersRef.current.delete(id);
    }
    setLocalMessages((prev) => prev.filter((m) => m.id !== id));
  }, []);

  const refresh = useCallback(async () => {
    try {
      const [statusResult, tasksResult] = await Promise.allSettled([
        veloxApi.getTeamStatus(),
        veloxApi.listTeamTasks(),
      ]);

      if (statusResult.status === "fulfilled") {
        setTeam(statusResult.value);
        setError(null);
      } else if (isUnavailableTeamError(statusResult.reason)) {
        setTeam(null);
        setTasks([]);
        setError(null);
        return;
      } else {
        setError(getErrorMessage(statusResult.reason, "Failed to load team status"));
      }

      if (tasksResult.status === "fulfilled") {
        setTasks(tasksResult.value);
      } else if (isUnavailableTeamError(tasksResult.reason)) {
        setTasks([]);
      }
    } catch (err) {
      if (isUnavailableTeamError(err)) {
        setTeam(null);
        setTasks([]);
        setError(null);
        return;
      }
      setError(getErrorMessage(err, "Failed to load team status"));
    }
  }, []);

  useEffect(() => {
    let mounted = true;
    let unlistenTeam: (() => void) | undefined;
    let unlistenTasks: (() => void) | undefined;
    let unlistenLocalMsg: (() => void) | undefined;
    const timers = messageTimersRef.current;

    void (async () => {
      await refresh();
      if (!mounted) return;

      try {
        const uTeam = await veloxApi.onTeamUpdated((ev) => {
          if (mounted) {
            setTeam(ev);
            setError(null);
          }
        });
        if (mounted) unlistenTeam = uTeam;
        else uTeam();

        const uTasks = await veloxApi.onTeamTasksUpdated((ev) => {
          if (mounted) setTasks(ev);
        });
        if (mounted) unlistenTasks = uTasks;
        else uTasks();

        const uLocal = await veloxApi.onTeamLocalMessage((ev) => {
          if (!mounted) return;
          setLocalMessages((prev) => upsertLocalMessage(prev, ev));
          const prevTimer = timers.get(ev.id);
          if (prevTimer !== undefined) {
            window.clearTimeout(prevTimer);
          }
          const ttl =
            ev.status === "queued"
              ? LOCAL_MESSAGE_QUEUED_TTL_MS
              : LOCAL_MESSAGE_TTL_MS;
          const nextTimer = window.setTimeout(() => {
            timers.delete(ev.id);
            if (mounted) {
              setLocalMessages((prev) => prev.filter((m) => m.id !== ev.id));
            }
          }, ttl);
          timers.set(ev.id, nextTimer);
        });
        if (mounted) unlistenLocalMsg = uLocal;
        else uLocal();
      } catch {
        // Ignore event listener failures outside Tauri runtime or when disabled
      }
    })();

    return () => {
      mounted = false;
      unlistenTeam?.();
      unlistenTasks?.();
      unlistenLocalMsg?.();
      for (const timer of timers.values()) {
        window.clearTimeout(timer);
      }
      timers.clear();
    };
  }, [refresh]);

  const pendingIncoming = useMemo(
    () =>
      tasks.filter(
        (t) => t.direction === "incoming" && t.state === "pending",
      ),
    [tasks],
  );

  const setShared = useCallback(
    async (sessionId: string, shared: boolean): Promise<SessionRecord | null> => {
      try {
        setError(null);
        const updated = await veloxApi.setSessionShared(sessionId, shared);
        setTeam((prev) => {
          if (!prev) return prev;
          return {
            ...prev,
            localSessions: prev.localSessions.map((s) =>
              s.id === sessionId ? { ...s, shared } : s,
            ),
          };
        });
        return updated;
      } catch (err) {
        if (!isUnavailableTeamError(err)) {
          setError(getErrorMessage(err, "Failed to update session sharing"));
        }
        return null;
      }
    },
    [],
  );

  const sendTask = useCallback(
    async (
      peer: string | null,
      sessionId: string,
      text: string,
    ): Promise<TaskTicket> => {
      const ticket = await veloxApi.sendTeamTask(peer, sessionId, text);
      setTasks((prev) => upsertTask(prev, ticket));
      return ticket;
    },
    [],
  );

  const respond = useCallback(
    async (requestId: string, accept: boolean): Promise<TaskTicket> => {
      const ticket = await veloxApi.respondTeamTask(requestId, accept);
      setTasks((prev) => upsertTask(prev, ticket));
      return ticket;
    },
    [],
  );

  return {
    team,
    tasks,
    pendingIncoming,
    localMessages,
    error,
    clearError,
    dismissLocalMessage,
    setShared,
    sendTask,
    respond,
    refresh,
  };
}
