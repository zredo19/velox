import { memo, useEffect, useRef } from "react";
import type { AgentKind } from "../../types/ipc";

/**
 * The four agents as tiny, happy pixel critters, standing at the foot of the sidebar.
 * Every few seconds one of them hops or blinks (a short CSS animation added by a timer, so
 * nothing runs in between), hovering makes it hop, and clicking launches that agent.
 * Nothing moves while the window is hidden or unfocused.
 */

/* '.' empty, B body, L highlight, D mouth / visor / shadow, E eye, A antenna light, F thruster. */
interface Critter {
  kind: AgentKind;
  name: string;
  map: readonly string[];
  /** Body colour per row (one entry is used for every row). */
  body: readonly string[];
  light: string;
  dark: string;
  eye: string;
  accent: string;
  glow: string;
}

const CREW: readonly Critter[] = [
  {
    kind: "claude",
    name: "Claude",
    map: [
      "B.........B",
      "BB.......BB",
      ".BB.....BB.",
      "..BLLLLLB..",
      ".BBEBBBEBB.",
      ".BBBBBBBBB.",
      ".BBDBBBDBB.",
      "..BBDDDBB..",
      "..B.B.B.B..",
    ],
    body: ["#F08F69"],
    light: "#F7BDA2",
    dark: "#3B2219",
    eye: "#1E1410",
    accent: "#F08F69",
    glow: "rgba(240, 143, 105, 0.35)",
  },
  {
    kind: "agy",
    name: "Antigravity",
    map: [
      ".....A.....",
      ".....B.....",
      "..BBBBBBB..",
      ".BDDDDDDDB.",
      ".BDEDDDEDB.",
      ".BDDEEEDDB.",
      ".BBBBBBBBB.",
      "B.BBBBBBB.B",
      "..BB...BB..",
      "..F.....F..",
    ],
    body: ["#9A6FC4", "#9A6FC4", "#7D78D8", "#6A86F2", "#5A8FF6", "#4D94F8", "#4A9AF0", "#4FB0E2", "#4FBCB8", "#4FBCB8"],
    light: "#C9D8FF",
    dark: "#0B0E17",
    eye: "#A8DAFF",
    accent: "#F6912E",
    glow: "rgba(110, 160, 255, 0.35)",
  },
  {
    /* Blocky OpenCode "O" critter: crisp off-white frame, dark inner counter & lower block */
    kind: "opencode",
    name: "OpenCode",
    map: [
      ".LLLLLLLLL.",
      ".BBBBBBBBB.",
      ".BB.....BB.",
      ".BB.E.E.BB.",
      ".BB.....BB.",
      ".BBDDDDDBB.",
      ".BBDDDDDBB.",
      ".BBBBBBBBB.",
      ".BBBBBBBBB.",
      "..BB...BB..",
    ],
    body: ["#EDEAE6"],
    light: "#FFFFFF",
    dark: "#5C5855",
    eye: "#FDFCFC",
    accent: "#9E9994",
    glow: "rgba(244, 242, 240, 0.28)",
  },
  {
    /* OpenAI Blossom-inspired Codex critter in iconic OpenAI emerald */
    kind: "codex",
    name: "Codex",
    map: [
      "...LL.LL...",
      ".BBB...BBB.",
      "BBLBBBBBLLB",
      ".BBEBBBEBB.",
      "BBBBBBBBBBB",
      ".BBDBBBDBB.",
      "BB.BDDDB.BB",
      ".BBB...BBB.",
      "...BB.BB...",
      "...B...B...",
    ],
    body: ["#19C37D"],
    light: "#86F0C5",
    dark: "#06291C",
    eye: "#06291C",
    accent: "#38E0A0",
    glow: "rgba(25, 195, 125, 0.38)",
  },
];

const COLS = 11;
const ROWS = 10;
/** CSS px per art pixel: 3 device pixels at the usual 150 % scale, so edges stay crisp. */
const PIXEL = 2;

const ACT_MIN_MS = 2600;
const ACT_MAX_MS = 5200;
/** Every so often the whole crew hops in a wave. */
const WAVE_CHANCE = 0.12;
const WAVE_STAGGER_MS = 110;

function CritterArt({ critter }: { critter: Critter }) {
  const pixels: React.ReactNode[] = [];
  const eyes: React.ReactNode[] = [];
  critter.map.forEach((line, y) => {
    for (let x = 0; x < line.length; x++) {
      const ch = line[x];
      if (ch === ".") continue;
      const fill =
        ch === "L"
          ? critter.light
          : ch === "D"
            ? critter.dark
            : ch === "E"
              ? critter.eye
              : ch === "A" || ch === "F"
                ? critter.accent
                : critter.body[Math.min(y, critter.body.length - 1)];
      const rect = <rect key={`${x}-${y}`} x={x} y={y} width="1" height="1" fill={fill} />;
      // Antigravity's smile is drawn in its eye colour: only the top row of E's are eyes.
      if (ch === "E" && !(critter.name === "Antigravity" && y > 4)) eyes.push(rect);
      else pixels.push(rect);
    }
  });
  return (
    <svg
      viewBox={`0 0 ${COLS} ${ROWS}`}
      width={COLS * PIXEL}
      height={ROWS * PIXEL}
      shapeRendering="crispEdges"
      aria-hidden="true"
      style={{ filter: `drop-shadow(0 0 4px ${critter.glow})` }}
    >
      {pixels}
      <g className="velox-crew-eyes">{eyes}</g>
    </svg>
  );
}

export const AgentCrew = memo(function AgentCrew({
  onCritterClick,
}: {
  onCritterClick?: (kind: AgentKind) => void;
}) {
  const rootRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    let timer = 0;
    const play = (el: HTMLElement | undefined, act: "hop" | "blink", delay = 0) => {
      if (!el) return;
      window.setTimeout(() => {
        // Restart the animation even if the same one just ran.
        el.removeAttribute("data-act");
        void el.offsetWidth;
        el.setAttribute("data-act", act);
      }, delay);
    };
    const next = () => {
      timer = window.setTimeout(() => {
        const critters = Array.from(
          rootRef.current?.querySelectorAll<HTMLElement>(".velox-crew-critter") ?? [],
        );
        if (critters.length && !document.hidden && document.hasFocus()) {
          if (Math.random() < WAVE_CHANCE) {
            critters.forEach((el, i) => play(el, "hop", i * WAVE_STAGGER_MS));
          } else {
            const el = critters[Math.floor(Math.random() * critters.length)];
            play(el, Math.random() < 0.55 ? "blink" : "hop");
          }
        }
        next();
      }, ACT_MIN_MS + Math.random() * (ACT_MAX_MS - ACT_MIN_MS));
    };
    next();
    return () => window.clearTimeout(timer);
  }, []);

  return (
    <div
      ref={rootRef}
      className="velox-crew relative flex items-end justify-around px-5 pt-2.5 pb-2"
      aria-label="Agents"
    >
      {CREW.map((critter) => (
        <button
          key={critter.name}
          type="button"
          title={onCritterClick ? `Launch ${critter.name}` : critter.name}
          aria-label={onCritterClick ? `Launch ${critter.name}` : critter.name}
          className="velox-crew-critter bg-transparent border-0 p-0 focus:outline-none"
          onClick={(e) => {
            const el = e.currentTarget;
            el.removeAttribute("data-act");
            void el.offsetWidth;
            el.setAttribute("data-act", "hop");
            onCritterClick?.(critter.kind);
          }}
          onAnimationEnd={(e) => {
            if (
              e.target === e.currentTarget ||
              (e.target as Element).classList.contains("velox-crew-eyes")
            ) {
              e.currentTarget.removeAttribute("data-act");
            }
          }}
        >
          <CritterArt critter={critter} />
        </button>
      ))}
    </div>
  );
});
