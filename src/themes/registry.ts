import type { ITheme } from "@xterm/xterm";
import type { AgentKind } from "../types/ipc";
import { createAgyScene } from "./agy/agyScene";
import { createClaudeScene } from "./claude/claudeScene";
import { createCodexScene } from "./codex/codexScene";
import { createOpencodeScene } from "./opencode/opencodeScene";
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

const CODEX_BASE = "#091311";

const CODEX_XTERM: ITheme = {
  background: CODEX_BASE,
  foreground: "#E6F4F0",
  cursor: "#1FC398",
  cursorAccent: CODEX_BASE,
  selectionBackground: "rgba(31, 195, 152, 0.28)",
  black: "#132420",
  red: "#F46376",
  green: "#34D399",
  yellow: "#F2C94C",
  blue: "#60A5FA",
  magenta: "#C084FC",
  cyan: "#2DD4BF",
  white: "#D1EAE3",
  brightBlack: "#5D8077",
  brightRed: "#FB8494",
  brightGreen: "#6EE7B7",
  brightYellow: "#FDE047",
  brightBlue: "#93C5FD",
  brightMagenta: "#D8B4FE",
  brightCyan: "#5EEAD4",
  brightWhite: "#F4FBF9",
};

const OPENCODE_BASE = "#0E0C18";

const OPENCODE_XTERM: ITheme = {
  background: OPENCODE_BASE,
  foreground: "#ECE8F8",
  cursor: "#C4B5FD",
  cursorAccent: OPENCODE_BASE,
  selectionBackground: "rgba(196, 181, 253, 0.28)",
  black: "#1C182D",
  red: "#F46B84",
  green: "#6EE7B7",
  yellow: "#F5C96B",
  blue: "#81A9FA",
  magenta: "#C4B5FD",
  cyan: "#67E8F9",
  white: "#DBD4F0",
  brightBlack: "#746B96",
  brightRed: "#FB8DA0",
  brightGreen: "#A7F3D0",
  brightYellow: "#FDE68A",
  brightBlue: "#A5C4FF",
  brightMagenta: "#DDD6FE",
  brightCyan: "#A5F3FC",
  brightWhite: "#F9F7FF",
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

const CODEX_ANIMATED: AgentTheme = {
  id: "codex",
  background: CODEX_BASE,
  xterm: { ...CODEX_XTERM, background: "rgba(9, 19, 17, 0)" },
  createScene: createCodexScene,
};

const CODEX_STATIC: AgentTheme = {
  id: "codex",
  background: CODEX_BASE,
  xterm: CODEX_XTERM,
  createScene: null,
};

const OPENCODE_ANIMATED: AgentTheme = {
  id: "opencode",
  background: OPENCODE_BASE,
  xterm: { ...OPENCODE_XTERM, background: "rgba(14, 12, 24, 0)" },
  createScene: createOpencodeScene,
};

const OPENCODE_STATIC: AgentTheme = {
  id: "opencode",
  background: OPENCODE_BASE,
  xterm: OPENCODE_XTERM,
  createScene: null,
};

/** Returns stable object references, so effects that depend on a theme do not re-run needlessly. */
export function getAgentTheme(
  kind: AgentKind | null | undefined,
  animated: boolean,
): AgentTheme {
  if (kind === "claude") return animated ? CLAUDE_ANIMATED : CLAUDE_STATIC;
  if (kind === "agy") return animated ? AGY_ANIMATED : AGY_STATIC;
  if (kind === "codex") return animated ? CODEX_ANIMATED : CODEX_STATIC;
  if (kind === "opencode") return animated ? OPENCODE_ANIMATED : OPENCODE_STATIC;
  return DEFAULT_THEME;
}
