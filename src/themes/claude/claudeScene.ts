import { bayer } from "../shared/dither";
import {
  BLOCK,
  CELL,
  createGlyphAtlas,
  shimmerGlyphAtTick,
  shimmerTick,
  type GlyphAtlas,
} from "../shared/glyphs";
import { createSpriteScene, type Emblem, type Mascot } from "../shared/scene";
import type { Scene } from "../types";

/* Ray shades, from the centre of the burst to its tips. */
const SHADES = ["#EBC7B4", "#E59A7C", "#D97757", "#B3614A", "#8E4A38"] as const;
const GLYPH_INK = "rgba(23, 18, 15, 0.55)";

/* Was 0.75: the burst reads a little brighter now. */
const BURST_ALPHA = 0.86;
const BURST_TURN_SECONDS = 90;
const BREATH = 0.16;
const BREATH_SPEED = 2.6;
/* A soft highlight that travels around the burst. */
const SWEEP_SPEED = 1.6;
const SWEEP_DEPTH = 0.2;

/* Twelve irregular rays: [angle offset (rad), length, half width as a fraction of radius]. */
const RAYS: Array<[number, number, number]> = [
  [0.0, 1.0, 0.1],
  [0.55, 0.72, 0.08],
  [1.02, 0.9, 0.092],
  [1.6, 0.78, 0.086],
  [2.1, 0.96, 0.092],
  [2.58, 0.68, 0.074],
  [3.14, 1.04, 0.1],
  [3.66, 0.74, 0.08],
  [4.15, 0.88, 0.09],
  [4.72, 0.82, 0.086],
  [5.22, 0.98, 0.092],
  [5.76, 0.7, 0.074],
];
const MAX_RAY = Math.max(...RAYS.map((r) => r[1]));

interface BurstCell {
  cx: number;
  cy: number;
  px: number;
  py: number;
  dist: number;
  angle: number;
  dither: number;
}

function createBurst(): Emblem {
  let radius = 90;
  let atlas: GlyphAtlas | null = null;
  let atlasDpr = 0;
  // Per-ray scratch buffers, reused every frame.
  const dirX = new Float32Array(RAYS.length);
  const dirY = new Float32Array(RAYS.length);
  const reach = new Float32Array(RAYS.length);
  /* Every grid cell the burst can ever reach, with its polar coordinates: they only depend
   * on the centre and radius, so the per-frame loop skips hypot / atan2 / Bayer lookups. */
  let cells: BurstCell[] = [];
  let cellsKey = "";

  const cellsFor = (bx: number, by: number): BurstCell[] => {
    const key = `${bx},${by},${radius}`;
    if (key === cellsKey) return cells;
    cellsKey = key;
    const far = radius * MAX_RAY * (1 + BREATH);
    const x0 = Math.floor((bx - far) / CELL);
    const x1 = Math.ceil((bx + far) / CELL);
    const y0 = Math.floor((by - far) / CELL);
    const y1 = Math.ceil((by + far) / CELL);
    cells = [];
    for (let cy = y0; cy <= y1; cy++) {
      for (let cx = x0; cx <= x1; cx++) {
        const px = cx * CELL + CELL / 2 - bx;
        const py = cy * CELL + CELL / 2 - by;
        const dist = Math.hypot(px, py);
        if (dist > far) continue;
        cells.push({ cx, cy, px, py, dist, angle: Math.atan2(py, px), dither: bayer(cx, cy) - 0.5 });
      }
    }
    return cells;
  };

  return {
    prepare(nextRadius, dpr) {
      radius = nextRadius;
      cellsKey = "";
      if (!atlas || atlasDpr !== dpr) {
        atlas = createGlyphAtlas(dpr, GLYPH_INK, SHADES);
        atlasDpr = dpr;
      }
    },

    draw(ctx, t, bx, by) {
      if (!atlas) return;
      const turn = (t / BURST_TURN_SECONDS) * Math.PI * 2;
      let longest = 0;
      for (let i = 0; i < RAYS.length; i++) {
        const [angle, length] = RAYS[i];
        dirX[i] = Math.cos(angle + turn);
        dirY[i] = Math.sin(angle + turn);
        reach[i] = radius * length * (1 + BREATH * Math.sin(t * BREATH_SPEED + i * 1.9));
        if (reach[i] > longest) longest = reach[i];
      }

      const core = radius * 0.2;
      const sweep = t * SWEEP_SPEED;
      const tick = shimmerTick(t);
      const { canvas, tile } = atlas;

      for (const cell of cellsFor(bx, by)) {
        const { px, py, dist } = cell;
        if (dist > longest) continue;

        let inside = dist <= core;
        for (let i = 0; !inside && i < RAYS.length; i++) {
          const along = px * dirX[i] + py * dirY[i];
          if (along < 0 || along > reach[i]) continue;
          const across = Math.abs(px * dirY[i] - py * dirX[i]);
          // Rays are thick at the root and taper towards the tip.
          inside = across <= radius * RAYS[i][2] * (1.2 - 0.35 * (along / reach[i]));
        }
        if (!inside) continue;

        // Lower k is lighter, so the side facing the travelling highlight brightens.
        const light = Math.cos(cell.angle - sweep);
        const k = dist / longest + cell.dither * 0.18 - light * SWEEP_DEPTH;
        const shade = Math.min(SHADES.length - 1, Math.max(0, Math.floor(k * SHADES.length)));
        const glyph = shimmerGlyphAtTick(cell.cx, cell.cy, tick);
        ctx.drawImage(canvas, glyph * tile, shade * tile, tile, tile, cell.cx * CELL, cell.cy * CELL, BLOCK, BLOCK);
      }
    },
  };
}

/* The crab. X = body, o = eye. Legs are drawn separately so it can walk. */
const BODY_MAP = [
  "..XXXXXXXXXX..",
  "..XoXXXXXXoX..",
  "..XoXXXXXXoX..",
  "XXXXXXXXXXXXXX",
  "XXXXXXXXXXXXXX",
  "..XXXXXXXXXX..",
  "..XXXXXXXXXX..",
] as const;

const LEGS = [
  ["...X.X..X.X...", "...X.X..X.X..."],
  ["...X.X..X.X...", "..X...XX...X.."],
] as const;

const PX = 5;

const crabBlinking = (t: number) => t % 4 < 0.16;
const crabStep = (t: number, walking: boolean) => (walking && Math.floor(t * 3) % 2 === 1 ? 1 : 0);

const crab: Mascot = {
  width: BODY_MAP[0].length * PX,
  height: (BODY_MAP.length + 2) * PX,
  bob: 2,
  bobSpeed: 2.2,
  pose: (t, walking) => crabStep(t, walking) * 2 + (crabBlinking(t) ? 1 : 0),
  draw(ctx, x, y, t, walking) {
    const top = y;
    const blinking = crabBlinking(t);
    const step = crabStep(t, walking);
    const paint = (rows: readonly string[], rowOffset: number) => {
      rows.forEach((line, r) => {
        for (let c = 0; c < line.length; c++) {
          const ch = line[c];
          if (ch === ".") continue;
          // A blink keeps only the lower half of each eye.
          const eye = ch === "o" && !(blinking && r === 1);
          ctx.fillStyle = eye ? "#17120F" : "#D97757";
          ctx.fillRect(x + c * PX, top + (r + rowOffset) * PX, PX - 0.5, PX - 0.5);
        }
      });
    };
    paint(BODY_MAP, 0);
    paint(LEGS[step], BODY_MAP.length);
  },
};

export function createClaudeScene(): Scene {
  return createSpriteScene({
    seed: 37,
    haloColor: "rgba(217, 119, 87, 0.38)",
    cloudLow: "rgba(184, 168, 158, 0.10)",
    cloudHigh: "rgba(184, 168, 158, 0.17)",
    glow: "rgba(217, 119, 87, 0.07)",
    ambientColors: ["#D97757"],
    ambientGlyphs: ["{", "<", ";", "+", "%", ".", ":", "}"],
    sparkColors: ["#EBC7B4"],
    emblemAlpha: BURST_ALPHA,
    createEmblem: createBurst,
    mascot: crab,
  });
}
