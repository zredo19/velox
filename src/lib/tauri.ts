import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AgentPreset,
  CreateSessionRequest,
  McpGatewaySnapshot,
  PtyOutputEvent,
  ScrollbackSnapshot,
  SessionRecord,
  SessionSpawnFailedEvent,
  SessionUpdatedEvent,
  SystemTelemetry,
  VeloxConfig,
} from "../types/ipc";

export const veloxApi = {
  // Sessions
  listSessions: () => invoke<SessionRecord[]>("list_sessions"),
  createSession: (req: CreateSessionRequest) =>
    invoke<SessionRecord>("create_session", {
      presetId: req.presetId,
      cwd: req.cwd ?? null,
      title: req.title ?? null,
      cols: req.cols,
      rows: req.rows,
    }),
  wakeSession: (sessionId: string, cols: number, rows: number) =>
    invoke<SessionRecord>("wake_session", { sessionId, cols, rows }),
  sleepSession: (sessionId: string) =>
    invoke<SessionRecord>("sleep_session", { sessionId }),
  sleepIdleSessions: (excludeSessionId?: string | null) =>
    invoke<SessionRecord[]>("sleep_idle_sessions", {
      excludeSessionId: excludeSessionId ?? null,
    }),
  closeSession: (sessionId: string) =>
    invoke<void>("close_session", { sessionId }),
  renameSession: (sessionId: string, title: string) =>
    invoke<SessionRecord>("rename_session", { sessionId, title }),
  reorderSessions: (sessionIds: string[]) =>
    invoke<SessionRecord[]>("reorder_sessions", { sessionIds }),
  writePty: (sessionId: string, data: string) =>
    invoke<void>("write_pty", { sessionId, data }),
  resizePty: (sessionId: string, cols: number, rows: number) =>
    invoke<void>("resize_pty", { sessionId, cols, rows }),
  getScrollback: (sessionId: string) =>
    invoke<ScrollbackSnapshot>("get_scrollback", { sessionId }),

  // Paste & drop: both resolve to the text that should be pasted into the terminal
  pasteImage: (sessionId: string, extension: string, dataB64: string) =>
    invoke<string>("paste_image", { sessionId, extension, dataB64 }),
  pasteFiles: (sessionId: string, paths: string[]) =>
    invoke<string>("paste_files", { sessionId, paths }),

  // Agents & directories
  listAgentPresets: () => invoke<AgentPreset[]>("list_agent_presets"),
  listRecentDirs: () => invoke<string[]>("list_recent_dirs"),
  pickDirectory: (defaultPath?: string): Promise<string | null> =>
    invoke<string | null>("pick_directory", {
      defaultPath: defaultPath ?? null,
    }),

  // Lazy MCP Gateway
  getMcpStatus: () => invoke<McpGatewaySnapshot>("get_mcp_status"),
  sleepMcpBackend: (name: string) =>
    invoke<McpGatewaySnapshot>("sleep_mcp_backend", { name }),
  sleepAllMcps: () => invoke<McpGatewaySnapshot>("sleep_all_mcps"),

  // Telemetry & config
  getTelemetry: () => invoke<SystemTelemetry>("get_telemetry"),
  getConfig: () => invoke<VeloxConfig>("get_config"),
  updateConfig: (config: VeloxConfig) =>
    invoke<VeloxConfig>("update_config", { config }),

  // Events
  onPtyOutput: (cb: (ev: PtyOutputEvent) => void): Promise<UnlistenFn> =>
    listen<PtyOutputEvent>("pty-output", (e) => cb(e.payload)),
  onSessionUpdated: (
    cb: (ev: SessionUpdatedEvent) => void,
  ): Promise<UnlistenFn> =>
    listen<SessionUpdatedEvent>("session-updated", (e) => cb(e.payload)),
  onSessionSpawnFailed: (
    cb: (ev: SessionSpawnFailedEvent) => void,
  ): Promise<UnlistenFn> =>
    listen<SessionSpawnFailedEvent>("session-spawn-failed", (e) => cb(e.payload)),
  onTelemetryUpdated: (
    cb: (ev: SystemTelemetry) => void,
  ): Promise<UnlistenFn> =>
    listen<SystemTelemetry>("telemetry-updated", (e) => cb(e.payload)),
  onMcpUpdated: (
    cb: (ev: McpGatewaySnapshot) => void,
  ): Promise<UnlistenFn> =>
    listen<McpGatewaySnapshot>("mcp-updated", (e) => cb(e.payload)),
};

export function decodeBase64ToBytes(b64: string): Uint8Array {
  const binary = atob(b64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes;
}

export function encodeBytesToBase64(bytes: Uint8Array): string {
  let binary = "";
  // Chunked so a multi-megabyte image does not overflow the argument list.
  for (let i = 0; i < bytes.length; i += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  }
  return btoa(binary);
}
