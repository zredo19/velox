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
  TaskTicket,
  TeamPtyOutputEvent,
  TeamPtyResizeEvent,
  TeamPtySnapshotEvent,
  TeamSessionGoneEvent,
  TeamSnapshot,
  VeloxConfig,
} from "../types/ipc";

function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/* ---- Multiplexed PTY output dispatcher --------------------------------------------------
 * Instead of attaching one global Tauri `pty-output` listener per open TerminalPane (O(N)
 * fan-out on every chunk), share a single IPC listener and route in O(1) by sessionId.
 */
type PtyListener = (ev: PtyOutputEvent) => void;
const ptySubscribers = new Map<string, Set<PtyListener>>();
let sharedPtyUnlisten: UnlistenFn | null = null;
let sharedPtyInitPromise: Promise<void> | null = null;

async function ensureSharedPtyListener(): Promise<void> {
  if (sharedPtyUnlisten) return;
  if (!sharedPtyInitPromise) {
    sharedPtyInitPromise = listen<PtyOutputEvent>("pty-output", (e) => {
      const listeners = ptySubscribers.get(e.payload.sessionId);
      if (!listeners) return;
      for (const cb of listeners) cb(e.payload);
    })
      .then((unlisten) => {
        if (ptySubscribers.size === 0) {
          unlisten();
        } else {
          sharedPtyUnlisten = unlisten;
        }
      })
      .finally(() => {
        sharedPtyInitPromise = null;
      });
  }
  await sharedPtyInitPromise;
}

function teardownSharedPtyListenerIfEmpty(): void {
  if (ptySubscribers.size === 0 && sharedPtyUnlisten) {
    const fn = sharedPtyUnlisten;
    sharedPtyUnlisten = null;
    fn();
  }
}

/* ---- Dev-only browser preview mock (tree-shaken in production builds) ------------------- */
function createDevMockApi() {
  const now = new Date().toISOString();
  const encoder = new TextEncoder();

  const presets: AgentPreset[] = [
    {
      id: "claude-default",
      kind: "claude",
      label: "Claude Code",
      profile: "personal",
      description: "Anthropic Claude coding agent",
      accentColor: "#D97757",
      command: "claude",
      args: [],
      env: {},
      installed: true,
    },
    {
      id: "agy-default",
      kind: "agy",
      label: "Antigravity",
      profile: "trabajo",
      description: "Google DeepMind agentic CLI",
      accentColor: "#4285F4",
      command: "agy",
      args: [],
      env: {},
      installed: true,
    },
    {
      id: "codex-default",
      kind: "codex",
      label: "Codex",
      profile: null,
      description: "OpenAI Codex terminal agent",
      accentColor: "#19C37D",
      command: "codex",
      args: [],
      env: {},
      installed: true,
    },
    {
      id: "opencode-default",
      kind: "opencode",
      label: "OpenCode",
      profile: null,
      description: "Open-source terminal coding agent",
      accentColor: "#F4F2F0",
      command: "opencode",
      args: [],
      env: {},
      installed: true,
    },
    {
      id: "shell-pwsh",
      kind: "shell",
      label: "PowerShell",
      profile: null,
      description: "Native Windows PowerShell",
      accentColor: "#94A3B8",
      command: "pwsh",
      args: [],
      env: {},
      installed: true,
    },
  ];

  let config: VeloxConfig = {
    scrollbackBytes: 2 * 1024 * 1024,
    autoHibernateMinutes: 20,
    lazyRestoreOnStartup: true,
    mcpGatewayEnabled: true,
    mcpGatewayPort: 43111,
    mcpBackendsPath: "C:\\Users\\dev\\.velox\\backends.json",
    defaultCwd: "C:\\Users\\dev\\Projects\\velox",
    fontFamily: "'JetBrains Mono', 'Cascadia Code', Consolas, monospace",
    fontSize: 13,
    animatedBackgrounds: true,
    recentDirs: [
      "C:\\Users\\dev\\Projects\\velox",
      "C:\\Users\\dev\\Projects\\api-gateway",
      "C:\\Users\\dev\\Projects\\design-system",
    ],
    team: {
      enabled: false,
      displayName: "Dev",
      listenPort: 43120,
      bindHost: "",
      token: "",
      autoApproveLocal: true,
      peers: [],
    },
  };

  let sessions: SessionRecord[] = [
    {
      id: "demo-claude",
      title: "velox-ui",
      agentKind: "claude",
      activeAgent: "claude",
      presetId: "claude-default",
      profile: "personal",
      cwd: "C:\\Users\\dev\\Projects\\velox",
      status: "running",
      agentSessionId: "sess_claude_01",
      pid: 14210,
      memoryMb: 142,
      createdAt: now,
      lastActiveAt: now,
      exitCode: null,
      shared: false,
    },
    {
      id: "demo-agy",
      title: "api-gateway",
      agentKind: "agy",
      activeAgent: "agy",
      presetId: "agy-default",
      profile: "trabajo",
      cwd: "C:\\Users\\dev\\Projects\\api-gateway",
      status: "running",
      agentSessionId: "sess_agy_02",
      pid: 18442,
      memoryMb: 118,
      createdAt: now,
      lastActiveAt: now,
      exitCode: null,
      shared: false,
    },
    {
      id: "demo-codex",
      title: "rust-pty-core",
      agentKind: "codex",
      activeAgent: "codex",
      presetId: "codex-default",
      profile: null,
      cwd: "C:\\Users\\dev\\Projects\\velox",
      status: "running",
      agentSessionId: "sess_codex_03",
      pid: 22190,
      memoryMb: 96,
      createdAt: now,
      lastActiveAt: now,
      exitCode: null,
      shared: false,
    },
    {
      id: "demo-opencode",
      title: "design-system",
      agentKind: "opencode",
      activeAgent: "opencode",
      presetId: "opencode-default",
      profile: null,
      cwd: "C:\\Users\\dev\\Projects\\design-system",
      status: "running",
      agentSessionId: null,
      pid: 24901,
      memoryMb: 84,
      createdAt: now,
      lastActiveAt: now,
      exitCode: null,
      shared: false,
    },
    {
      id: "demo-sleep-1",
      title: "billing-refactor",
      agentKind: "claude",
      activeAgent: "claude",
      presetId: "claude-default",
      profile: "trabajo",
      cwd: "C:\\Users\\dev\\Projects\\billing",
      status: "sleeping",
      agentSessionId: "sess_claude_old",
      pid: null,
      memoryMb: 0,
      createdAt: now,
      lastActiveAt: now,
      exitCode: null,
      shared: false,
    },
  ];

  let mcp: McpGatewaySnapshot = {
    mode: "embedded",
    listenAddr: "127.0.0.1:43111",
    totalBackends: 4,
    activeBackends: 2,
    totalMemoryMb: 64,
    error: null,
    backends: [
      {
        name: "github",
        running: true,
        pid: 9120,
        memoryMb: 38,
        idleMs: 12000,
        remainingIdleMs: 168000,
        inFlight: 0,
        cachedMethods: 18,
        lastUsedAt: now,
      },
      {
        name: "postgres-dev",
        running: true,
        pid: 9412,
        memoryMb: 26,
        idleMs: 42000,
        remainingIdleMs: 138000,
        inFlight: 0,
        cachedMethods: 9,
        lastUsedAt: now,
      },
      {
        name: "playwright",
        running: false,
        pid: null,
        memoryMb: 0,
        idleMs: 0,
        remainingIdleMs: null,
        inFlight: 0,
        cachedMethods: 22,
        lastUsedAt: null,
      },
      {
        name: "linear",
        running: false,
        pid: null,
        memoryMb: 0,
        idleMs: 0,
        remainingIdleMs: null,
        inFlight: 0,
        cachedMethods: 11,
        lastUsedAt: null,
      },
    ],
  };

  const ptyStreams = new Map<string, { gen: number; offset: number }>();

  const makeBanner = (s: SessionRecord) => {
    const color =
      s.agentKind === "claude"
        ? "\x1b[38;2;217;119;87m"
        : s.agentKind === "agy"
          ? "\x1b[38;2;110;168;252m"
          : s.agentKind === "codex"
            ? "\x1b[38;2;31;195;152m"
            : s.agentKind === "opencode"
              ? "\x1b[38;2;196;181;253m"
              : "\x1b[38;2;148;163;184m";
    return (
      `${color}╭───────────────────────────────────────────────────────────╮\r\n` +
      `│  Velox Preview · ${s.title.padEnd(20)} (${s.agentKind})`.padEnd(60) +
      `│\r\n` +
      `╰───────────────────────────────────────────────────────────╯\x1b[0m\r\n\r\n` +
      `\x1b[90m  cwd:\x1b[0m ${s.cwd}\r\n` +
      `\x1b[90m  tip:\x1b[0m Press \x1b[1mCtrl+K\x1b[0m for Command Palette · \x1b[1mCtrl+/\x1b[0m for Shortcuts\r\n\r\n` +
      `${color}❯\x1b[0m `
    );
  };

  const getTelemetry = (): SystemTelemetry => {
    const awake = sessions.filter((s) => s.status !== "sleeping" && s.status !== "exited");
    const sleeping = sessions.filter((s) => s.status === "sleeping");
    const sessionsMemoryMb = awake.reduce((sum, s) => sum + s.memoryMb, 0);
    const appMemoryMb = 48;
    const mcpMemoryMb = mcp.backends.reduce((sum, b) => sum + (b.running ? b.memoryMb : 0), 0);
    const sessionRam: Record<string, number> = {};
    for (const s of sessions) sessionRam[s.id] = s.memoryMb;
    return {
      appMemoryMb,
      sessionsMemoryMb,
      mcpMemoryMb,
      totalMemoryMb: appMemoryMb + sessionsMemoryMb + mcpMemoryMb,
      activeSessions: awake.length,
      sleepingSessions: sleeping.length,
      sessionRam,
    };
  };

  return {
    listSessions: async () => [...sessions],
    createSession: async (req: CreateSessionRequest) => {
      const preset = presets.find((p) => p.id === req.presetId) ?? presets[0];
      const cwd = req.cwd || config.defaultCwd;
      const parts = cwd.split(/[\\/]/).filter(Boolean);
      const created: SessionRecord = {
        id: `demo-${Date.now().toString(36)}`,
        title: req.title || parts[parts.length - 1] || preset.label,
        agentKind: preset.kind,
        activeAgent: preset.kind,
        presetId: preset.id,
        profile: preset.profile,
        cwd,
        status: "running",
        agentSessionId: null,
        pid: Math.floor(10000 + Math.random() * 20000),
        memoryMb: 92,
        createdAt: new Date().toISOString(),
        lastActiveAt: new Date().toISOString(),
        exitCode: null,
        shared: false,
      };
      sessions = [...sessions, created];
      return created;
    },
    wakeSession: async (sessionId: string) => {
      sessions = sessions.map((s) =>
        s.id === sessionId ? { ...s, status: "running", pid: 29110, memoryMb: 112 } : s,
      );
      return sessions.find((s) => s.id === sessionId)!;
    },
    sleepSession: async (sessionId: string) => {
      sessions = sessions.map((s) =>
        s.id === sessionId ? { ...s, status: "sleeping", pid: null, memoryMb: 0 } : s,
      );
      return sessions.find((s) => s.id === sessionId)!;
    },
    sleepIdleSessions: async (excludeSessionId?: string | null) => {
      sessions = sessions.map((s) =>
        s.id === excludeSessionId || s.status !== "running"
          ? s
          : { ...s, status: "sleeping", pid: null, memoryMb: 0 },
      );
      return [...sessions];
    },
    closeSession: async (sessionId: string) => {
      sessions = sessions.filter((s) => s.id !== sessionId);
    },
    renameSession: async (sessionId: string, title: string) => {
      sessions = sessions.map((s) => (s.id === sessionId ? { ...s, title } : s));
      return sessions.find((s) => s.id === sessionId)!;
    },
    reorderSessions: async (sessionIds: string[]) => {
      const map = new Map(sessions.map((s) => [s.id, s]));
      const next: SessionRecord[] = [];
      for (const id of sessionIds) {
        const s = map.get(id);
        if (s) next.push(s);
      }
      for (const s of sessions) {
        if (!sessionIds.includes(s.id)) next.push(s);
      }
      sessions = next;
      return [...sessions];
    },
    writePty: async (sessionId: string, data: string) => {
      const state = ptyStreams.get(sessionId) ?? { gen: 1, offset: 0 };
      const echoed = data === "\r" ? "\r\n\x1b[90m❯\x1b[0m " : data;
      const bytes = encoder.encode(echoed);
      state.offset += bytes.length;
      ptyStreams.set(sessionId, state);
      const listeners = ptySubscribers.get(sessionId);
      if (listeners) {
        const ev: PtyOutputEvent = {
          sessionId,
          dataB64: encodeBytesToBase64(bytes),
          generation: state.gen,
          endOffset: state.offset,
        };
        for (const cb of listeners) cb(ev);
      }
    },
    resizePty: async () => {},
    getScrollback: async (sessionId: string): Promise<ScrollbackSnapshot> => {
      const session = sessions.find((s) => s.id === sessionId);
      const bytes = encoder.encode(session ? makeBanner(session) : "");
      ptyStreams.set(sessionId, { gen: 1, offset: bytes.length });
      return {
        dataB64: encodeBytesToBase64(bytes),
        generation: 1,
        endOffset: bytes.length,
      };
    },
    pasteImage: async () => "[image-pasted.png]",
    pasteFiles: async (_sessionId: string, paths: string[]) => paths.join(" "),
    listAgentPresets: async () => presets,
    listRecentDirs: async () => config.recentDirs,
    pickDirectory: async (defaultPath?: string) => defaultPath ?? config.defaultCwd,
    getMcpStatus: async () => mcp,
    sleepMcpBackend: async (name: string) => {
      mcp = {
        ...mcp,
        backends: mcp.backends.map((b) =>
          b.name === name ? { ...b, running: false, pid: null, memoryMb: 0, remainingIdleMs: null } : b,
        ),
      };
      mcp.activeBackends = mcp.backends.filter((b) => b.running).length;
      mcp.totalMemoryMb = mcp.backends.reduce((sum, b) => sum + b.memoryMb, 0);
      return mcp;
    },
    sleepAllMcps: async () => {
      mcp = {
        ...mcp,
        activeBackends: 0,
        totalMemoryMb: 0,
        backends: mcp.backends.map((b) => ({
          ...b,
          running: false,
          pid: null,
          memoryMb: 0,
          remainingIdleMs: null,
        })),
      };
      return mcp;
    },
    getTelemetry: async () => getTelemetry(),
    getConfig: async () => ({ ...config }),
    updateConfig: async (next: VeloxConfig) => {
      config = { ...next };
      return { ...config };
    },
    getTeamStatus: async (): Promise<TeamSnapshot> => ({
      enabled: config.team.enabled,
      displayName: config.team.displayName,
      listenAddr: null,
      serverError: null,
      tailscaleIp: null,
      peers: [],
      localSessions: [],
      watchers: [],
    }),
    setSessionShared: async (sessionId: string, shared: boolean) => {
      sessions = sessions.map((s) => (s.id === sessionId ? { ...s, shared } : s));
      return sessions.find((s) => s.id === sessionId)!;
    },
    teamSubscribe: async () => {},
    teamUnsubscribe: async () => {},
    sendTeamTask: async (peer: string | null, sessionId: string, text: string): Promise<TaskTicket> => ({
      requestId: `mock-${Date.now()}`,
      direction: "outgoing",
      peer,
      sessionId,
      sessionTitle: sessions.find((s) => s.id === sessionId)?.title ?? null,
      fromName: config.team.displayName,
      fromSession: null,
      text,
      state: "delivered",
      message: null,
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
    }),
    listTeamTasks: async (): Promise<TaskTicket[]> => [],
    respondTeamTask: async (requestId: string, accept: boolean): Promise<TaskTicket> => ({
      requestId,
      direction: "incoming",
      peer: null,
      sessionId: "",
      sessionTitle: null,
      fromName: "",
      fromSession: null,
      text: "",
      state: accept ? "delivered" : "rejected",
      message: null,
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
    }),
    generateTeamToken: async () => "mock-team-token",
    subscribePtyOutput: async (sessionId: string, cb: PtyListener): Promise<UnlistenFn> => {
      let set = ptySubscribers.get(sessionId);
      if (!set) {
        set = new Set();
        ptySubscribers.set(sessionId, set);
      }
      set.add(cb);
      return () => {
        set?.delete(cb);
        if (set?.size === 0) ptySubscribers.delete(sessionId);
      };
    },
    onTeamUpdated: async (): Promise<UnlistenFn> => () => {},
    onTeamTasksUpdated: async (): Promise<UnlistenFn> => () => {},
    onTeamPtyOutput: async (): Promise<UnlistenFn> => () => {},
    onTeamPtySnapshot: async (): Promise<UnlistenFn> => () => {},
    onTeamPtyResize: async (): Promise<UnlistenFn> => () => {},
    onTeamSessionGone: async (): Promise<UnlistenFn> => () => {},
    onPtyOutput: async (): Promise<UnlistenFn> => () => {},
    onSessionUpdated: async (): Promise<UnlistenFn> => () => {},
    onSessionSpawnFailed: async (): Promise<UnlistenFn> => () => {},
    onTelemetryUpdated: async (): Promise<UnlistenFn> => () => {},
    onMcpUpdated: async (): Promise<UnlistenFn> => () => {},
  };
}

const tauriApi = {
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

  // Velox Online (team)
  getTeamStatus: () => invoke<TeamSnapshot>("get_team_status"),
  setSessionShared: (sessionId: string, shared: boolean) =>
    invoke<SessionRecord>("set_session_shared", { sessionId, shared }),
  /** Start mirroring a teammate's session; a `team-pty-snapshot` event follows. */
  teamSubscribe: (peer: string, sessionId: string) =>
    invoke<void>("team_subscribe", { peer, sessionId }),
  teamUnsubscribe: (peer: string, sessionId: string) =>
    invoke<void>("team_unsubscribe", { peer, sessionId }),
  /** `peer` null = a session on this machine. */
  sendTeamTask: (peer: string | null, sessionId: string, text: string) =>
    invoke<TaskTicket>("send_team_task", { peer, sessionId, text }),
  listTeamTasks: () => invoke<TaskTicket[]>("list_team_tasks"),
  respondTeamTask: (requestId: string, accept: boolean) =>
    invoke<TaskTicket>("respond_team_task", { requestId, accept }),
  generateTeamToken: () => invoke<string>("generate_team_token"),

  // Events
  subscribePtyOutput: async (sessionId: string, cb: PtyListener): Promise<UnlistenFn> => {
    let set = ptySubscribers.get(sessionId);
    if (!set) {
      set = new Set();
      ptySubscribers.set(sessionId, set);
    }
    set.add(cb);
    await ensureSharedPtyListener();
    return () => {
      set?.delete(cb);
      if (set?.size === 0) ptySubscribers.delete(sessionId);
      teardownSharedPtyListenerIfEmpty();
    };
  },
  onTeamUpdated: (cb: (ev: TeamSnapshot) => void): Promise<UnlistenFn> =>
    listen<TeamSnapshot>("team-updated", (e) => cb(e.payload)),
  onTeamTasksUpdated: (cb: (ev: TaskTicket[]) => void): Promise<UnlistenFn> =>
    listen<TaskTicket[]>("team-tasks-updated", (e) => cb(e.payload)),
  onTeamPtyOutput: (cb: (ev: TeamPtyOutputEvent) => void): Promise<UnlistenFn> =>
    listen<TeamPtyOutputEvent>("team-pty-output", (e) => cb(e.payload)),
  onTeamPtySnapshot: (cb: (ev: TeamPtySnapshotEvent) => void): Promise<UnlistenFn> =>
    listen<TeamPtySnapshotEvent>("team-pty-snapshot", (e) => cb(e.payload)),
  onTeamPtyResize: (cb: (ev: TeamPtyResizeEvent) => void): Promise<UnlistenFn> =>
    listen<TeamPtyResizeEvent>("team-pty-resize", (e) => cb(e.payload)),
  onTeamSessionGone: (cb: (ev: TeamSessionGoneEvent) => void): Promise<UnlistenFn> =>
    listen<TeamSessionGoneEvent>("team-session-gone", (e) => cb(e.payload)),
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

export const veloxApi =
  import.meta.env.DEV && !isTauriRuntime() ? createDevMockApi() : tauriApi;

export function decodeBase64ToBytes(b64: string): Uint8Array {
  const binary = atob(b64);
  const len = binary.length;
  const bytes = new Uint8Array(len);
  for (let i = 0; i < len; i += 1) {
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
