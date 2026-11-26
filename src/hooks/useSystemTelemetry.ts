import { useCallback, useEffect, useState } from "react";
import type { McpGatewaySnapshot, SystemTelemetry } from "../types/ipc";
import { veloxApi } from "../lib/tauri";

export function useSystemTelemetry() {
  const [telemetry, setTelemetry] = useState<SystemTelemetry | null>(null);
  const [mcp, setMcp] = useState<McpGatewaySnapshot | null>(null);
  const [loadingMcp, setLoadingMcp] = useState(false);

  useEffect(() => {
    let mounted = true;
    let telemetryUnlisten: (() => void) | undefined;
    let mcpUnlisten: (() => void) | undefined;

    void (async () => {
      try {
        const [tel, snap] = await Promise.all([
          veloxApi.getTelemetry(),
          veloxApi.getMcpStatus(),
        ]);
        if (mounted) {
          setTelemetry(tel);
          setMcp(snap);
        }
      } catch {
        // Ignore outside Tauri runtime
      }

      try {
        const u1 = await veloxApi.onTelemetryUpdated((ev) => {
          if (mounted) setTelemetry(ev);
        });
        if (mounted) telemetryUnlisten = u1;
        else u1();

        const u2 = await veloxApi.onMcpUpdated((ev) => {
          if (mounted) setMcp(ev);
        });
        if (mounted) mcpUnlisten = u2;
        else u2();
      } catch {
        // Ignore outside Tauri runtime
      }
    })();

    return () => {
      mounted = false;
      telemetryUnlisten?.();
      mcpUnlisten?.();
    };
  }, []);

  const sleepMcpBackend = useCallback(async (name: string) => {
    setLoadingMcp(true);
    try {
      const updated = await veloxApi.sleepMcpBackend(name);
      setMcp(updated);
    } finally {
      setLoadingMcp(false);
    }
  }, []);

  const sleepAllMcps = useCallback(async () => {
    setLoadingMcp(true);
    try {
      const updated = await veloxApi.sleepAllMcps();
      setMcp(updated);
    } finally {
      setLoadingMcp(false);
    }
  }, []);

  const refresh = useCallback(async () => {
    setLoadingMcp(true);
    try {
      const [tel, snap] = await Promise.all([
        veloxApi.getTelemetry(),
        veloxApi.getMcpStatus(),
      ]);
      setTelemetry(tel);
      setMcp(snap);
    } finally {
      setLoadingMcp(false);
    }
  }, []);

  return {
    telemetry,
    mcp,
    loadingMcp,
    sleepMcpBackend,
    sleepAllMcps,
    refresh,
  };
}
