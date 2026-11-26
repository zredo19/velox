/**
 * What an agent is doing right now, read from its terminal screen and window title.
 * The Claude and Antigravity rules are ported from herdr's detection manifests
 * (src/detect/manifests/*.toml, Apache-2.0).
 */
import type { AgentKind } from "../types/ipc";

export type AgentActivity = "working" | "blocked" | "idle";

const CLAUDE_BUSY_TITLE = /^[⠀-⣿◐-◓] /;
const CLAUDE_IDLE_TITLE = /^✳ /;
const CLAUDE_INTERRUPT_LINE = /^\s*[⏸⏵].*esc to interrupt(?:\s|·|$)/;
const CLAUDE_SPINNER_LINE = /^\s*[*·✢✶✻✽]\s+\S.*…(?:\s+\(\d+[smh](?:\s|·)|\s*$)/;
const CLAUDE_BG_SHELLS = /^\s*[⏸⏵].*·\s+[1-9]\d*\s+shells?\s+(?:·|$)/;
const CLAUDE_BG_AGENTS = /^\s*[*·✢✶✻✽]\s+Waiting for [1-9]\d* background agents? to finish\s*$/;
const HORIZONTAL_RULE = /^\s*[─━]{8,}/;

const AGY_SPINNER_LINE = /^\s*[⠀-⣿]+\s+\p{L}+ing\b/u;
const AGY_BG_TASKS = /·\s*[1-9]\d*\s+task/i;

const lower = (lines: string[]) => lines.join("\n").toLowerCase();
const has = (text: string, ...needles: string[]) => needles.some((n) => text.includes(n));

function detectClaude(title: string, lines: string[]): AgentActivity | null {
  const filled = lines.filter((l) => l.trim());
  const rules = lines.flatMap((l, i) => (HORIZONTAL_RULE.test(l) ? [i] : []));
  const footer = lower(rules.length ? lines.slice(rules[rules.length - 1] + 1) : lines);
  const screen = lower(lines);

  // The transcript viewer says nothing about the live turn.
  if (lower(filled.slice(-3)).includes("showing detailed transcript")) return null;

  // A question or confirmation form. Checked first: its footer is unambiguous.
  if (
    footer.includes("esc to cancel") &&
    (footer.includes("enter to confirm") ||
      (footer.includes("enter to select") &&
        has(footer, "arrow keys", "arrows to navigate", "↑/↓ to navigate", "↑↓ to navigate")))
  ) {
    return "blocked";
  }

  if (CLAUDE_BUSY_TITLE.test(title)) return "working";
  const recent = filled.slice(-12);
  if (recent.some((l) => CLAUDE_INTERRUPT_LINE.test(l) || CLAUDE_SPINNER_LINE.test(l))) {
    return "working";
  }
  if (filled.slice(-5).some((l) => CLAUDE_BG_SHELLS.test(l))) return "working";
  if (recent.some((l) => CLAUDE_BG_AGENTS.test(l))) return "working";

  // The input box is on screen, between the last two horizontal rules.
  if (rules.length >= 2) {
    const box = lines.slice(rules[rules.length - 2] + 1, rules[rules.length - 1]);
    if (box.some((l) => /^\s*[❯>]/.test(l))) return "idle";
  }

  if (screen.includes("do you want to proceed?") && /^\s*❯?\s*\d\.\s*(yes|no)\b/im.test(screen)) {
    return "blocked";
  }
  if (
    !lines.some((l) => /^\s*❯\s*$/.test(l)) &&
    (has(
      screen,
      "waiting for permission",
      "do you want to allow this connection?",
      "tab to amend",
      "ctrl+e to explain",
      "review your answers",
      "skip interview and plan immediately",
    ) ||
      (has(screen, "do you want to", "would you like to") && has(screen, "yes", "❯")))
  ) {
    return "blocked";
  }
  return CLAUDE_IDLE_TITLE.test(title) ? "idle" : null;
}

function detectAgy(lines: string[]): AgentActivity {
  const screen = lower(lines);
  const bottom = lines.filter((l) => l.trim()).slice(-5);
  if (
    screen.includes("requesting permission for:") &&
    has(screen, "do you want to proceed?", "tab amend", "edit command")
  ) {
    return "blocked";
  }
  // Its selection prompts end in "↑/↓ Navigate · enter Confirm".
  if (/navigate.*enter\s+(to\s+)?(confirm|select)/.test(lower(bottom))) return "blocked";
  if (lines.some((l) => AGY_SPINNER_LINE.test(l))) return "working";
  return bottom.some((l) => AGY_BG_TASKS.test(l)) ? "working" : "idle";
}

/** Returns `null` when the screen gives no answer; callers keep the previous state. */
export function detectActivity(
  kind: AgentKind,
  title: string,
  lines: string[],
): AgentActivity | null {
  if (kind === "claude") return detectClaude(title, lines);
  if (kind === "agy") return detectAgy(lines);
  if (kind === "shell") return "idle";

  const bottom = lower(lines.filter((l) => l.trim()).slice(-8));
  if (has(bottom, "permission required", "allow command?", "[y/n]")) return "blocked";
  return has(bottom, "esc interrupt", "esc to interrupt") ? "working" : "idle";
}
