import type { ITheme } from "@xterm/xterm";
import type { AgentKind } from "../types/ipc";
import { createAgyScene } from "./agy/agyScene";
import { createClaudeScene } from "./claude/claudeScene";
import type { AgentTheme } from "./types";

const DEFAULT_XTERM: ITheme = {
  background: "#0B0D12",
  foreground: "#E6EAF2",
  cursor: "#9CC2FF",
  cursorAccent: "#0B0D12",
  selectionBackground: "rgba(78, 141, 245, 0.28)",
  black: "#181C26",
  red: "#F43F5E",
  green: "#10B981",
  yellow: "#F59E0B",
  blue: "#4E8DF5",
  magenta: "#C084FC",
  cyan: "#22D3EE",
  white: "#D6DCE8",
  brightBlack: "#5A657D",
  brightRed: "#FB7185",
  brightGreen: "#34D399",
  brightYellow: "#FBBF24",
  brightBlue: "#7CB4FF",
  brightMagenta: "#D8B4FE",
  brightCyan: "#67E8F9",
  brightWhite: "#F8FAFC",
};

const CLAUDE_BASE = "#17120F";

const CLAUDE_XTERM: ITheme = {
  background: CLAUDE_BASE,
  foreground: "#F2EAE4",
  cursor: "#D97757",
  cursorAccent: CLAUDE_BASE,
  selectionBackground: "rgba(217, 119, 87, 0.30)",
  black: "#231B17",
  red: "#F0556A",
  green: "#7FBF7F",
  yellow: "#E8B45A",
  blue: "#7FA7E8",
  magenta: "#C99BE0",
  cyan: "#6FC4C0",
  white: "#E3D8D0",
  brightBlack: "#85746A",
  brightRed: "#FF8093",
  brightGreen: "#A3D9A0",
  brightYellow: "#F5CB7E",
  brightBlue: "#A6C4F5",
  brightMagenta: "#DDBBEF",
  brightCyan: "#98DDD8",
  brightWhite: "#FBF7F4",
};

const DEFAULT_THEME: AgentTheme = {
  id: "default",
  background: "#0B0D12",
  xterm: DEFAULT_XTERM,
  createScene: null,
};

const CLAUDE_ANIMATED: AgentTheme = {
  id: "claude",
  background: CLAUDE_BASE,
  // Same RGB with zero alpha: xterm derives contrast colours from it, and the canvas shows through.
  xterm: { ...CLAUDE_XTERM, background: "rgba(23, 18, 15, 0)" },
  createScene: createClaudeScene,
};

const CLAUDE_STATIC: AgentTheme = {
  id: "claude",
  background: CLAUDE_BASE,
  xterm: CLAUDE_XTERM,
  createScene: null,
};

const AGY_BASE = "#0B0E17";

const AGY_XTERM: ITheme = {
  background: AGY_BASE,
  foreground: "#E8EDF8",
  cursor: "#8AB4F8",
  cursorAccent: AGY_BASE,
  selectionBackground: "rgba(66, 133, 244, 0.32)",
  black: "#181F33",
  red: "#EF5A63",
  green: "#7CC251",
  yellow: "#DBB131",
  blue: "#5B8DEF",
  magenta: "#A87BD0",
  cyan: "#43AEAB",
  white: "#D8DFEF",
  brightBlack: "#6B7794",
  brightRed: "#FF7B82",
  brightGreen: "#9AD878",
  brightYellow: "#F2C94C",
  brightBlue: "#8AB4F8",
  brightMagenta: "#C6A0E8",
  brightCyan: "#6FD0CD",
  brightWhite: "#F8FAFF",
};

const AGY_ANIMATED: AgentTheme = {
  id: "agy",
  background: AGY_BASE,
  xterm: { ...AGY_XTERM, background: "rgba(11, 14, 23, 0)" },
  createScene: createAgyScene,
};

const AGY_STATIC: AgentTheme = {
  id: "agy",
  background: AGY_BASE,
  xterm: AGY_XTERM,
  createScene: null,
};

/** Returns stable object references, so effects that depend on a theme do not re-run needlessly. */
export function getAgentTheme(
  kind: AgentKind | null | undefined,
  animated: boolean,
): AgentTheme {
  if (kind === "claude") return animated ? CLAUDE_ANIMATED : CLAUDE_STATIC;
  if (kind === "agy") return animated ? AGY_ANIMATED : AGY_STATIC;
  return DEFAULT_THEME;
}
