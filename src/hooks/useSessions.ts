import { useCallback, useEffect, useRef, useState } from "react";
import { veloxApi } from "../lib/tauri";
import type {
  AgentPreset,
  SessionRecord,
  SessionUpdatedEvent,
  SystemTelemetry,
  VeloxConfig,
} from "../types/ipc";

function upsertSession(
  list: SessionRecord[],
  incoming: SessionRecord,
): SessionRecord[] {
  const idx = list.findIndex((s) => s.id === incoming.id);
  if (idx >= 0) {
    const next = [...list];
    next[idx] = incoming;
    return next;
  }
  return [...list, incoming];
}

export function partitionSessions(sessions: SessionRecord[]) {
  const awake: SessionRecord[] = [];
  const sleeping: SessionRecord[] = [];
  const exited: SessionRecord[] = [];
  for (const s of sessions) {
    if (s.status === "sleeping") sleeping.push(s);
    else if (s.status === "exited") exited.push(s);
    else awake.push(s);
  }
  return { awake, sleeping, exited, visible: [...awake, ...sleeping, ...exited] };
}

function reorderListByIds(
  list: SessionRecord[],
  sessionIds: string[],
): SessionRecord[] {
  const byId = new Map(list.map((s) => [s.id, s]));
  const seen = new Set<string>();
  const next: SessionRecord[] = [];
  for (const id of sessionIds) {
    const item = byId.get(id);
    if (item && !seen.has(id)) {
      seen.add(id);
      next.push(item);
    }
  }
  for (const s of list) {
    if (!seen.has(s.id)) next.push(s);
  }
  return next;
}

export function useSessions() {
  const [sessions, setSessions] = useState<SessionRecord[]>([]);
  const [presets, setPresets] = useState<AgentPreset[]>([]);
  const [recentDirs, setRecentDirs] = useState<string[]>([]);
  const [config, setConfig] = useState<VeloxConfig | null>(null);
  const [activeSessionId, setActiveSessionId] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const sessionsRef = useRef<SessionRecord[]>([]);
  sessionsRef.current = sessions;

  const activeIdRef = useRef<string | null>(null);
  activeIdRef.current = activeSessionId;

  useEffect(() => {
    let mounted = true;
    let unlistenSession: (() => void) | undefined;
    let unlistenTelemetry: (() => void) | undefined;
    let unlistenSpawnFailed: (() => void) | undefined;

    async function init() {
      try {
        setLoading(true);
        setError(null);
        const [sessionsData, presetsData, recentDirsData, configData] =
          await Promise.all([
            veloxApi.listSessions(),
            veloxApi.listAgentPresets(),
            veloxApi.listRecentDirs(),
            veloxApi.getConfig(),
          ]);

        if (!mounted) return;
        setSessions(sessionsData);
        setPresets(presetsData);
        setRecentDirs(recentDirsData);
        setConfig(configData);

        if (sessionsData.length > 0 && !activeIdRef.current) {
          const firstAwake = sessionsData.find(
            (s) => s.status !== "sleeping" && s.status !== "exited",
          );
          setActiveSessionId(firstAwake?.id ?? sessionsData[0].id);
        }
      } catch (err) {
        if (!mounted) return;
        setError(
          err instanceof Error ? err.message : "Failed to load Velox state",
        );
      } finally {
        if (mounted) setLoading(false);
      }

      try {
        unlistenSession = await veloxApi.onSessionUpdated(
          (ev: SessionUpdatedEvent) => {
            if (!mounted) return;
            setSessions((prev) => upsertSession(prev, ev.session));
          },
        );
        // create/wake return as soon as the tab exists; a spawn that then fails is reported here.
        unlistenSpawnFailed = await veloxApi.onSessionSpawnFailed((ev) => {
          if (!mounted) return;
          setError(ev.message || "Failed to start session");
          if (!ev.removed) return;
          setSessions((prev) => {
            const next = prev.filter((s) => s.id !== ev.sessionId);
            if (activeIdRef.current === ev.sessionId) {
              const awake = next.find(
                (s) => s.status !== "sleeping" && s.status !== "exited",
              );
              setActiveSessionId(awake?.id ?? next[next.length - 1]?.id ?? null);
            }
            return next;
          });
        });
        unlistenTelemetry = await veloxApi.onTelemetryUpdated(
          (ev: SystemTelemetry) => {
            if (!mounted) return;
            setSessions((prev) => {
              let changed = false;
              const next = prev.map((s) => {
                const updatedMb = ev.sessionRam[s.id];
                const nextMb =
                  s.status === "sleeping" || s.status === "exited"
                    ? 0
                    : (updatedMb ?? s.memoryMb);
                if (Math.abs(nextMb - s.memoryMb) >= 0.5) {
                  changed = true;
                  return { ...s, memoryMb: nextMb };
                }
                return s;
              });
              return changed ? next : prev;
            });
          },
        );
      } catch {
        // Ignore event subscription failures outside Tauri runtime
      }
    }

    void init();

    return () => {
      mounted = false;
      unlistenSession?.();
      unlistenTelemetry?.();
      unlistenSpawnFailed?.();
    };
  }, []);

  const refreshRecentDirs = useCallback(async () => {
    try {
      const dirs = await veloxApi.listRecentDirs();
      setRecentDirs(dirs);
    } catch {
      // Ignore
    }
  }, []);

  const createSession = useCallback(
    async (presetId: string, cwd?: string, title?: string) => {
      try {
        setError(null);
        const session = await veloxApi.createSession({
          presetId,
          cwd,
          title,
          cols: 120,
          rows: 34,
        });
        setSessions((prev) => upsertSession(prev, session));
        setActiveSessionId(session.id);
        void refreshRecentDirs();
        return session;
      } catch (err) {
        const msg =
          err instanceof Error ? err.message : "Failed to launch agent session";
        setError(msg);
        throw err;
      }
    },
    [refreshRecentDirs],
  );

  const wakeSession = useCallback(async (id: string, cols = 120, rows = 34) => {
    try {
      setError(null);
      const session = await veloxApi.wakeSession(id, cols, rows);
      setSessions((prev) => upsertSession(prev, session));
      setActiveSessionId(session.id);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to wake session");
      throw err;
    }
  }, []);

  const selectSession = useCallback(
    async (id: string) => {
      const target = sessionsRef.current.find((s) => s.id === id);
      if (!target) return;
      setActiveSessionId(id);
      if (target.status === "sleeping") {
        await wakeSession(id);
      }
    },
    [wakeSession],
  );

  const sleepSession = useCallback(async (id: string) => {
    try {
      setError(null);
      const session = await veloxApi.sleepSession(id);
      setSessions((prev) => upsertSession(prev, session));
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to sleep session");
      throw err;
    }
  }, []);

  const sleepIdleSessions = useCallback(async () => {
    try {
      setError(null);
      const updated = await veloxApi.sleepIdleSessions(activeIdRef.current);
      setSessions(updated);
    } catch (err) {
      setError(
        err instanceof Error ? err.message : "Failed to hibernate idle tabs",
      );
      throw err;
    }
  }, []);

  const closeSession = useCallback(async (id: string) => {
    try {
      setError(null);
      await veloxApi.closeSession(id);
      setSessions((prev) => {
        const next = prev.filter((s) => s.id !== id);
        if (activeIdRef.current === id) {
          const awake = next.find(
            (s) => s.status !== "sleeping" && s.status !== "exited",
          );
          setActiveSessionId(awake?.id ?? next[next.length - 1]?.id ?? null);
        }
        return next;
      });
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to close session");
      throw err;
    }
  }, []);

  const renameSession = useCallback(async (id: string, title: string) => {
    try {
      setError(null);
      const session = await veloxApi.renameSession(id, title);
      setSessions((prev) => upsertSession(prev, session));
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to rename session");
      throw err;
    }
  }, []);

  const reorderSessions = useCallback(async (sessionIds: string[]) => {
    const previous = sessionsRef.current;
    setSessions((prev) => reorderListByIds(prev, sessionIds));
    try {
      setError(null);
      const ordered = await veloxApi.reorderSessions(sessionIds);
      setSessions(ordered);
    } catch (err) {
      setSessions(previous);
      setError(
        err instanceof Error ? err.message : "Failed to reorder sessions",
      );
    }
  }, []);

  const updateConfig = useCallback(async (nextConfig: VeloxConfig) => {
    try {
      setError(null);
      const saved = await veloxApi.updateConfig(nextConfig);
      setConfig(saved);
      setRecentDirs(saved.recentDirs);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to save settings");
      throw err;
    }
  }, []);

  const applyLoadedConfig = useCallback((saved: VeloxConfig) => {
    setConfig(saved);
    setRecentDirs(saved.recentDirs);
  }, []);

  const clearError = useCallback(() => {
    setError(null);
  }, []);

  const activeSession =
    sessions.find((s) => s.id === activeSessionId) ?? null;

  return {
    sessions,
    presets,
    recentDirs,
    config,
    activeSessionId,
    activeSession,
    loading,
    error,
    clearError,
    createSession,
    selectSession,
    wakeSession,
    sleepSession,
    sleepIdleSessions,
    closeSession,
    renameSession,
    reorderSessions,
    updateConfig,
    applyLoadedConfig,
  };
}
