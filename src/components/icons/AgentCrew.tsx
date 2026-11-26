import { useEffect, useRef } from "react";

/**
 * The four agents as tiny, happy pixel critters, standing at the foot of the sidebar.
 * Every few seconds one of them hops or blinks (a short CSS animation added by a timer, so
 * nothing runs in between), and hovering one makes it hop. Nothing moves while the window
 * is hidden or unfocused.
 */

/* '.' empty, B body, L highlight, D mouth / visor, E eye, A antenna light, F thruster. */
interface Critter {
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
    name: "OpenCode",
    map: [
      ".....B.....",
      "....BLB....",
      "...BLLLB...",
      "..BBEBEBB..",
      ".BBBBBBBBB.",
      "BBBDBBBDBBB",
      ".BBBDDDBBB.",
      "..BBBBBBB..",
      "...BBBBB...",
      "....B.B....",
    ],
    body: ["#C9BBFF"],
    light: "#ECE6FF",
    dark: "#2A2340",
    eye: "#221B36",
    accent: "#C9BBFF",
    glow: "rgba(196, 181, 253, 0.32)",
  },
  {
    name: "Codex",
    map: [
      "...BBBBB...",
      "..BLLLLLB..",
      ".BBBBBBBBB.",
      ".BBEBBBEBB.",
      ".BBBBBBBBB.",
      ".BBDBBBDBB.",
      ".BBBDDDBBB.",
      "..BBBBBBB..",
      "...BBBBB...",
      "...B...B...",
    ],
    body: ["#1FC398"],
    light: "#8CEBCB",
    dark: "#06281D",
    eye: "#06281D",
    accent: "#1FC398",
    glow: "rgba(31, 195, 152, 0.32)",
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

export function AgentCrew() {
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
        <div
          key={critter.name}
          title={critter.name}
          className="velox-crew-critter"
          onAnimationEnd={(e) => {
            if (e.target === e.currentTarget || (e.target as Element).classList.contains("velox-crew-eyes")) {
              e.currentTarget.removeAttribute("data-act");
            }
          }}
        >
          <CritterArt critter={critter} />
        </div>
      ))}
    </div>
  );
}
