import { useCallback, useEffect, useMemo, useState } from "react";
import { veloxApi } from "../lib/tauri";
import type { SessionRecord, TaskTicket, TeamSnapshot } from "../types/ipc";

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
  error: string | null;
  clearError: () => void;
  setShared: (sessionId: string, shared: boolean) => Promise<SessionRecord | null>;
  sendTask: (
    peer: string | null,
    sessionId: string,
    text: string,
  ) => Promise<TaskTicket>;
  respond: (requestId: string, accept: boolean) => Promise<TaskTicket>;
  refresh: () => Promise<void>;
}

export function useTeam(): UseTeamResult {
  const [team, setTeam] = useState<TeamSnapshot | null>(null);
  const [tasks, setTasks] = useState<TaskTicket[]>([]);
  const [error, setError] = useState<string | null>(null);

  const clearError = useCallback(() => setError(null), []);

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
      } catch {
        // Ignore event listener failures outside Tauri runtime or when disabled
      }
    })();

    return () => {
      mounted = false;
      unlistenTeam?.();
      unlistenTasks?.();
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
    error,
    clearError,
    setShared,
    sendTask,
    respond,
    refresh,
  };
}
