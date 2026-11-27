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
  /** Velox Online: teammates can watch this session (read-only) and send it task requests. */
  shared: boolean;
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
  team: TeamConfig;
}

export interface CreateSessionRequest {
  presetId: string;
  cwd?: string;
  title?: string;
  cols: number;
  rows: number;
}

// ---------------------------------------------------------------------------
// Velox Online (team). Mirrors src-tauri/src/domain/config.rs (TeamConfig),
// src-tauri/src/team/protocol.rs and src-tauri/src/team/api.rs. CONTRACT.
// ---------------------------------------------------------------------------

export interface PeerConfig {
  /** Label shown in the UI and used by MCP tools (e.g. "Benjamin"). */
  name: string;
  /** Tailscale MagicDNS name or IP (e.g. "benjamin" or "100.101.102.103"). */
  host: string;
  port: number;
}

export interface TeamConfig {
  enabled: boolean;
  displayName: string;
  listenPort: number;
  /** Empty = auto-detect the Tailscale IP. "127.0.0.1" for local testing. */
  bindHost: string;
  /** Shared secret; must be identical on every teammate. */
  token: string;
  autoApproveLocal: boolean;
  peers: PeerConfig[];
}

export interface RemoteSessionInfo {
  id: string;
  title: string;
  agentKind: AgentKind;
  activeAgent: AgentKind | null;
  status: SessionStatus;
  /** Basename of the session folder (never the full path). */
  folder: string;
  lastActiveAt: string;
  /** Latest agent action, e.g. "Edit src/team/hub.rs". */
  summary: string | null;
  /** PTY size of the source terminal; mirrors render at exactly this size. */
  cols: number;
  rows: number;
}

export interface LocalSessionInfo extends RemoteSessionInfo {
  shared: boolean;
}

export interface PeerStatus {
  name: string;
  host: string;
  port: number;
  online: boolean;
  displayName: string | null;
  lastError: string | null;
  sessions: RemoteSessionInfo[];
}

export interface TeamSnapshot {
  enabled: boolean;
  displayName: string;
  /** "100.123.172.42:43120" while this machine's team server listens. */
  listenAddr: string | null;
  serverError: string | null;
  tailscaleIp: string | null;
  peers: PeerStatus[];
  localSessions: LocalSessionInfo[];
  /** Teammates currently connected to this machine. */
  watchers: string[];
}

export type TaskState =
  | "pending"
  | "accepted"
  | "delivered"
  | "rejected"
  | "failed"
  | "expired";

export type TaskDirection = "outgoing" | "incoming";

export interface TaskTicket {
  requestId: string;
  direction: TaskDirection;
  /** Outgoing: target peer (null = this machine). Incoming: requester (null = local agent). */
  peer: string | null;
  sessionId: string;
  sessionTitle: string | null;
  fromName: string;
  fromSession: string | null;
  text: string;
  state: TaskState;
  message: string | null;
  createdAt: string;
  updatedAt: string;
}

/** `team-pty-output`: live bytes of a remote session you subscribed to. */
export interface TeamPtyOutputEvent {
  peer: string;
  sessionId: string;
  generation: number;
  endOffset: number;
  dataB64: string;
}

/** `team-pty-snapshot`: first frame after `teamSubscribe` (reset the mirror, then write). */
export interface TeamPtySnapshotEvent {
  peer: string;
  sessionId: string;
  generation: number;
  endOffset: number;
  dataB64: string;
  cols: number;
  rows: number;
}

/** `team-pty-resize`: the source terminal changed size. */
export interface TeamPtyResizeEvent {
  peer: string;
  sessionId: string;
  cols: number;
  rows: number;
}

/** `team-session-gone`: the remote session was unshared, closed or slept. */
export interface TeamSessionGoneEvent {
  peer: string;
  sessionId: string;
}
