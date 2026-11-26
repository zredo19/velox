export type AgentKind = "claude" | "agy" | "codex" | "opencode" | "shell";

export type SessionStatus =
  | "starting"
  | "running"
  | "idle"
  | "blocked"
  | "sleeping"
  | "exited";

export interface AgentPreset {
  id: string;
  kind: AgentKind;
  label: string;
  profile: string | null;
  description: string;
  accentColor: string;
  command: string;
  args: string[];
  env: Record<string, string>;
  installed: boolean;
}

export interface SessionRecord {
  id: string;
  title: string;
  agentKind: AgentKind;
  activeAgent: AgentKind | null;
  presetId: string;
  profile: string | null;
  cwd: string;
  status: SessionStatus;
  agentSessionId: string | null;
  pid: number | null;
  memoryMb: number;
  createdAt: string;
  lastActiveAt: string;
  exitCode: number | null;
}

export interface PtyOutputEvent {
  sessionId: string;
  dataB64: string;
  /** PTY spawn generation; changes when the session is respawned. */
  generation: number;
  /** Byte offset just past this chunk in the PTY stream. */
  endOffset: number;
}

export interface ScrollbackSnapshot {
  dataB64: string;
  generation: number;
  endOffset: number;
}

export interface SessionUpdatedEvent {
  session: SessionRecord;
}

/** A tab's PTY could not be started in the background (create/wake return before it). */
export interface SessionSpawnFailedEvent {
  sessionId: string;
  message: string;
  /** True for a failed new tab (it was dropped); false for a failed wake (back to sleep). */
  removed: boolean;
}

export type McpGatewayMode =
  | "embedded"
  | "externalAttached"
  | "disabled"
  | "error";

export interface McpBackendStatus {
  name: string;
  running: boolean;
  pid: number | null;
  memoryMb: number;
  idleMs: number;
  remainingIdleMs: number | null;
  inFlight: number;
  cachedMethods: number;
  lastUsedAt: string | null;
}

export interface McpGatewaySnapshot {
  mode: McpGatewayMode;
  listenAddr: string;
  totalBackends: number;
  activeBackends: number;
  totalMemoryMb: number;
  backends: McpBackendStatus[];
  error: string | null;
}

export interface SystemTelemetry {
  appMemoryMb: number;
  sessionsMemoryMb: number;
  mcpMemoryMb: number;
  totalMemoryMb: number;
  activeSessions: number;
  sleepingSessions: number;
  sessionRam: Record<string, number>;
}

export interface VeloxConfig {
  scrollbackBytes: number;
  autoHibernateMinutes: number;
  lazyRestoreOnStartup: boolean;
  mcpGatewayEnabled: boolean;
  mcpGatewayPort: number;
  mcpBackendsPath: string;
  defaultCwd: string;
  fontFamily: string;
  fontSize: number;
  animatedBackgrounds: boolean;
  recentDirs: string[];
}

export interface CreateSessionRequest {
  presetId: string;
  cwd?: string;
  title?: string;
  cols: number;
  rows: number;
}
