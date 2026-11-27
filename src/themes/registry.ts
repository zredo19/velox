import type { ITheme } from "@xterm/xterm";
import type { AgentKind } from "../types/ipc";
import { createAgyScene } from "./agy/agyScene";
import { createClaudeScene } from "./claude/claudeScene";
import { createCodexScene } from "./codex/codexScene";
import { createOpencodeScene } from "./opencode/opencodeScene";
import {
  createCatppuccinScene,
  createDraculaScene,
  createGruvboxScene,
  createMatrixScene,
  createNordScene,
  createSynthwaveScene,
  createTokyoNightScene,
} from "./custom/customScenes";
import type { AgentTheme, AgentThemeId, Scene, ThemeMeta, ThemePreference } from "./types";

const THEME_STORAGE_KEY = "velox.themePreference";

function hexToTransparentRgba(hex: string): string {
  const clean = hex.replace("#", "");
  const r = parseInt(clean.slice(0, 2), 16);
  const g = parseInt(clean.slice(2, 4), 16);
  const b = parseInt(clean.slice(4, 6), 16);
  return `rgba(${r}, ${g}, ${b}, 0)`;
}

function makeThemePair(
  id: AgentThemeId,
  base: string,
  xterm: ITheme,
  createScene: (() => Scene) | null,
): { animated: AgentTheme; static: AgentTheme } {
  const staticTheme: AgentTheme = {
    id,
    background: base,
    xterm,
    createScene: null,
  };
  const animatedTheme: AgentTheme = createScene
    ? {
        id,
        background: base,
        // Same RGB with zero alpha: xterm derives contrast colours from it, and the canvas shows through.
        xterm: { ...xterm, background: hexToTransparentRgba(base) },
        createScene,
      }
    : staticTheme;
  return { animated: animatedTheme, static: staticTheme };
}

/* ---- Agent Themes -------------------------------------------------------------------- */

const DEFAULT_PAIR = makeThemePair(
  "default",
  "#0B0D12",
  {
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
  },
  null,
);

const CLAUDE_PAIR = makeThemePair(
  "claude",
  "#17120F",
  {
    background: "#17120F",
    foreground: "#F2EAE4",
    cursor: "#D97757",
    cursorAccent: "#17120F",
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
  },
  createClaudeScene,
);

const AGY_PAIR = makeThemePair(
  "agy",
  "#0B0E17",
  {
    background: "#0B0E17",
    foreground: "#E8EDF8",
    cursor: "#8AB4F8",
    cursorAccent: "#0B0E17",
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
  },
  createAgyScene,
);

const CODEX_PAIR = makeThemePair(
  "codex",
  "#091310",
  {
    background: "#091310",
    foreground: "#E6F4EF",
    cursor: "#19C37D",
    cursorAccent: "#091310",
    selectionBackground: "rgba(25, 195, 125, 0.28)",
    black: "#12241E",
    red: "#F46376",
    green: "#19C37D",
    yellow: "#F2C94C",
    blue: "#60A5FA",
    magenta: "#C084FC",
    cyan: "#2DD4BF",
    white: "#D1EAE2",
    brightBlack: "#5D8074",
    brightRed: "#FB8494",
    brightGreen: "#5CF0B8",
    brightYellow: "#FDE047",
    brightBlue: "#93C5FD",
    brightMagenta: "#D8B4FE",
    brightCyan: "#5EEAD4",
    brightWhite: "#F4FBF8",
  },
  createCodexScene,
);

/* Authentic OpenCode warm monochrome charcoal & crisp off-white palette */
const OPENCODE_PAIR = makeThemePair(
  "opencode",
  "#141212",
  {
    background: "#141212",
    foreground: "#F4F2F0",
    cursor: "#F4F2F0",
    cursorAccent: "#141212",
    selectionBackground: "rgba(244, 242, 240, 0.22)",
    black: "#211E1D",
    red: "#E06C75",
    green: "#98C379",
    yellow: "#E5C07B",
    blue: "#7AA2F7",
    magenta: "#C678DD",
    cyan: "#56B6C2",
    white: "#DFDCD8",
    brightBlack: "#736D6A",
    brightRed: "#F28B94",
    brightGreen: "#B5DF97",
    brightYellow: "#F2D49B",
    brightBlue: "#9BBBF9",
    brightMagenta: "#D99CEF",
    brightCyan: "#7CD2DC",
    brightWhite: "#FFFFFF",
  },
  createOpencodeScene,
);

/* ---- Custom Selectable Themes -------------------------------------------------------- */

const TOKYO_NIGHT_PAIR = makeThemePair(
  "tokyo-night",
  "#131522",
  {
    background: "#131522",
    foreground: "#C0CAF5",
    cursor: "#7AA2F7",
    cursorAccent: "#131522",
    selectionBackground: "rgba(122, 162, 247, 0.28)",
    black: "#1D2033",
    red: "#F7768E",
    green: "#9ECE6A",
    yellow: "#E0AF68",
    blue: "#7AA2F7",
    magenta: "#BB9AF7",
    cyan: "#7DCFFF",
    white: "#A9B1D6",
    brightBlack: "#565F89",
    brightRed: "#FF899D",
    brightGreen: "#B9F27C",
    brightYellow: "#FFCB7D",
    brightBlue: "#8DB6FF",
    brightMagenta: "#D2B8FF",
    brightCyan: "#A4E0FF",
    brightWhite: "#E4EAFF",
  },
  createTokyoNightScene,
);

const CATPPUCCIN_PAIR = makeThemePair(
  "catppuccin",
  "#181825",
  {
    background: "#181825",
    foreground: "#CDD6F4",
    cursor: "#F5E0DC",
    cursorAccent: "#181825",
    selectionBackground: "rgba(203, 166, 247, 0.28)",
    black: "#242537",
    red: "#F38BA8",
    green: "#A6E3A1",
    yellow: "#F9E2AF",
    blue: "#89B4FA",
    magenta: "#CBA6F7",
    cyan: "#94E2D5",
    white: "#BAC2DE",
    brightBlack: "#6C7086",
    brightRed: "#F7A6BD",
    brightGreen: "#C0EDBC",
    brightYellow: "#FCECC9",
    brightBlue: "#A6C8FC",
    brightMagenta: "#DFC4FA",
    brightCyan: "#B2EDE3",
    brightWhite: "#EEF2FF",
  },
  createCatppuccinScene,
);

const SYNTHWAVE_PAIR = makeThemePair(
  "synthwave",
  "#120B1F",
  {
    background: "#120B1F",
    foreground: "#F4EAFF",
    cursor: "#FF2E97",
    cursorAccent: "#120B1F",
    selectionBackground: "rgba(255, 46, 151, 0.30)",
    black: "#211536",
    red: "#FF3E78",
    green: "#72F1B8",
    yellow: "#FFD319",
    blue: "#36F9F6",
    magenta: "#FF2E97",
    cyan: "#00F0FF",
    white: "#DFD0F5",
    brightBlack: "#725C99",
    brightRed: "#FF6B98",
    brightGreen: "#9AF8CD",
    brightYellow: "#FFE36B",
    brightBlue: "#7CFCFA",
    brightMagenta: "#FF6CB8",
    brightCyan: "#70F7FF",
    brightWhite: "#FFFFFF",
  },
  createSynthwaveScene,
);

const NORD_PAIR = makeThemePair(
  "nord",
  "#181E28",
  {
    background: "#181E28",
    foreground: "#E5E9F0",
    cursor: "#88C0D0",
    cursorAccent: "#181E28",
    selectionBackground: "rgba(136, 192, 208, 0.28)",
    black: "#252D3C",
    red: "#BF616A",
    green: "#A3BE8C",
    yellow: "#EBCB8B",
    blue: "#81A1C1",
    magenta: "#B48EAD",
    cyan: "#88C0D0",
    white: "#D8DEE9",
    brightBlack: "#616E88",
    brightRed: "#D47982",
    brightGreen: "#B8D1A3",
    brightYellow: "#F3D9A4",
    brightBlue: "#9BB8D6",
    brightMagenta: "#C9A8C3",
    brightCyan: "#A3D4E2",
    brightWhite: "#ECEFF4",
  },
  createNordScene,
);

const DRACULA_PAIR = makeThemePair(
  "dracula",
  "#171824",
  {
    background: "#171824",
    foreground: "#F8F8F2",
    cursor: "#BD93F9",
    cursorAccent: "#171824",
    selectionBackground: "rgba(189, 147, 249, 0.30)",
    black: "#242638",
    red: "#FF5555",
    green: "#50FA7B",
    yellow: "#F1FA8C",
    blue: "#6272A4",
    magenta: "#BD93F9",
    cyan: "#8BE9FD",
    white: "#E2E2DC",
    brightBlack: "#6272A4",
    brightRed: "#FF7777",
    brightGreen: "#78FC9A",
    brightYellow: "#F6FCAD",
    brightBlue: "#8899CE",
    brightMagenta: "#D4B6FB",
    brightCyan: "#AEF0FE",
    brightWhite: "#FFFFFF",
  },
  createDraculaScene,
);

const GRUVBOX_PAIR = makeThemePair(
  "gruvbox",
  "#1B1917",
  {
    background: "#1B1917",
    foreground: "#EBDBB2",
    cursor: "#FABD2F",
    cursorAccent: "#1B1917",
    selectionBackground: "rgba(250, 189, 47, 0.28)",
    black: "#282828",
    red: "#FB4934",
    green: "#B8BB26",
    yellow: "#FABD2F",
    blue: "#83A598",
    magenta: "#D3869B",
    cyan: "#8EC07C",
    white: "#D5C4A1",
    brightBlack: "#7C6F64",
    brightRed: "#FC7262",
    brightGreen: "#CDD051",
    brightYellow: "#FCD062",
    brightBlue: "#A3BEB3",
    brightMagenta: "#E0A4B4",
    brightCyan: "#ABD29E",
    brightWhite: "#FBF1C7",
  },
  createGruvboxScene,
);

const MATRIX_PAIR = makeThemePair(
  "matrix",
  "#060E09",
  {
    background: "#060E09",
    foreground: "#D4FFE8",
    cursor: "#00FF7F",
    cursorAccent: "#060E09",
    selectionBackground: "rgba(0, 255, 127, 0.26)",
    black: "#0C1D13",
    red: "#FF5C77",
    green: "#00E676",
    yellow: "#C6FF00",
    blue: "#40C4FF",
    magenta: "#B388FF",
    cyan: "#1DE9B6",
    white: "#B2DFC8",
    brightBlack: "#3E6B52",
    brightRed: "#FF8599",
    brightGreen: "#69F0AE",
    brightYellow: "#DDFF66",
    brightBlue: "#80D8FF",
    brightMagenta: "#D1B3FF",
    brightCyan: "#64FFDA",
    brightWhite: "#EEFFF6",
  },
  createMatrixScene,
);

const THEME_MAP: Record<AgentThemeId, { animated: AgentTheme; static: AgentTheme }> = {
  default: DEFAULT_PAIR,
  claude: CLAUDE_PAIR,
  agy: AGY_PAIR,
  codex: CODEX_PAIR,
  opencode: OPENCODE_PAIR,
  "tokyo-night": TOKYO_NIGHT_PAIR,
  catppuccin: CATPPUCCIN_PAIR,
  synthwave: SYNTHWAVE_PAIR,
  nord: NORD_PAIR,
  dracula: DRACULA_PAIR,
  gruvbox: GRUVBOX_PAIR,
  matrix: MATRIX_PAIR,
};

export const THEME_OPTIONS: readonly ThemeMeta[] = [
  {
    id: "auto",
    label: "Auto (Per Agent)",
    description: "Matches Claude, Antigravity, Codex & OpenCode automatically",
    category: "auto",
    swatches: ["#17120F", "#0B0E17", "#D97757", "#19C37D"],
  },
  {
    id: "tokyo-night",
    label: "Tokyo Night",
    description: "Deep twilight indigo with neon Torii gate",
    category: "custom",
    swatches: ["#131522", "#1A1E30", "#7AA2F7", "#BB9AF7"],
  },
  {
    id: "catppuccin",
    label: "Catppuccin Mocha",
    description: "Cozy pastel dark with crescent moon & cat",
    category: "custom",
    swatches: ["#181825", "#242537", "#CBA6F7", "#F5C2E7"],
  },
  {
    id: "synthwave",
    label: "Synthwave '84",
    description: "Retro neon violet with sliced sunset disc",
    category: "custom",
    swatches: ["#120B1F", "#1E1333", "#FF2E97", "#00F0FF"],
  },
  {
    id: "nord",
    label: "Nord Frost",
    description: "Arctic polar night with crystalline snowflake",
    category: "custom",
    swatches: ["#181E28", "#232B3A", "#88C0D0", "#D8DEE9"],
  },
  {
    id: "dracula",
    label: "Dracula",
    description: "Gothic purple & neon pink eclipse crest",
    category: "custom",
    swatches: ["#171824", "#222436", "#BD93F9", "#FF79C6"],
  },
  {
    id: "gruvbox",
    label: "Gruvbox Dark",
    description: "Warm retro amber & solar compass",
    category: "custom",
    swatches: ["#1B1917", "#282522", "#FABD2F", "#FE8019"],
  },
  {
    id: "matrix",
    label: "Matrix Rain",
    description: "CRT phosphor green digital core",
    category: "custom",
    swatches: ["#060E09", "#0C1C12", "#00E676", "#69F0AE"],
  },
  {
    id: "claude",
    label: "Claude Espresso",
    description: "Warm terracotta sunburst & crab",
    category: "agent",
    swatches: ["#17120F", "#231B17", "#D97757", "#F0A080"],
  },
  {
    id: "agy",
    label: "Antigravity Navy",
    description: "Cosmic navy hovering arch & thruster bot",
    category: "agent",
    swatches: ["#0B0E17", "#171E36", "#4285F4", "#9ED0FF"],
  },
  {
    id: "codex",
    label: "Codex Emerald",
    description: "OpenAI Blossom emerald & mint glow",
    category: "agent",
    swatches: ["#091310", "#13251F", "#19C37D", "#7CEAD0"],
  },
  {
    id: "opencode",
    label: "OpenCode Mono",
    description: "Minimalist charcoal & crisp off-white grid",
    category: "agent",
    swatches: ["#141212", "#221F1E", "#F4F2F0", "#A8A29E"],
  },
  {
    id: "default",
    label: "Velox Slate",
    description: "Clean minimalist dark with no wallpaper",
    category: "custom",
    swatches: ["#0B0D12", "#171B26", "#4E8DF5", "#9CC2FF"],
  },
];

const VALID_PREFS = new Set<string>(THEME_OPTIONS.map((t) => t.id));

export function loadThemePreference(): ThemePreference {
  try {
    const raw = window.localStorage.getItem(THEME_STORAGE_KEY);
    if (raw && VALID_PREFS.has(raw)) return raw as ThemePreference;
  } catch {
    // Ignore storage errors
  }
  return "auto";
}

export function saveThemePreference(pref: ThemePreference): void {
  try {
    window.localStorage.setItem(THEME_STORAGE_KEY, pref);
  } catch {
    // Ignore storage errors
  }
}

export function getThemeById(id: AgentThemeId, animated: boolean): AgentTheme {
  const pair = THEME_MAP[id] ?? DEFAULT_PAIR;
  return animated ? pair.animated : pair.static;
}

/** Returns stable object references, so effects that depend on a theme do not re-run needlessly. */
export function getAgentTheme(
  kind: AgentKind | null | undefined,
  animated: boolean,
  preference: ThemePreference = "auto",
): AgentTheme {
  if (preference !== "auto") {
    return getThemeById(preference, animated);
  }
  if (kind === "claude") return getThemeById("claude", animated);
  if (kind === "agy") return getThemeById("agy", animated);
  if (kind === "codex") return getThemeById("codex", animated);
  if (kind === "opencode") return getThemeById("opencode", animated);
  return getThemeById("default", animated);
}
