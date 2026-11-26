import { context2d, createCanvas } from "../shared/canvas";
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
import { ANTIGRAVITY_PATH, ANTIGRAVITY_RAMPS } from "./logo";

const GLYPH_INK = "rgba(11, 14, 23, 0.55)";

/* Same travelling highlight as the Claude burst. */
const SWEEP_SPEED = 1.6;
/* Anti-gravity: the logo hovers, and blocks stream off both feet. */
const FLOAT_PX = 8;
const FLOAT_SPEED = 1.3;
const TRAIL_BLOCKS = 5;
/** Logo side as a multiple of the scene radius. */
const SIZE = 2.1;

interface Cell {
  /** Offset of the block from the emblem centre, CSS px. */
  dx: number;
  dy: number;
  angle: number;
  /** Atlas row. */
  shade: number;
}

/* Colours are snapped to this step per channel, so each block is a single drawImage. */
const COLOR_STEP = 20;

function snap(rgb: string): string {
  const [r, g, b] = (rgb.match(/\d+/g) ?? []).map((v) =>
    Math.min(255, Math.round(Number(v) / COLOR_STEP) * COLOR_STEP),
  );
  return `rgb(${r}, ${g}, ${b})`;
}

function mix(a: string, b: string, k: number): string {
  const pa = parseInt(a.slice(1), 16);
  const pb = parseInt(b.slice(1), 16);
  const channel = (shift: number) =>
    Math.round(((pa >> shift) & 255) * (1 - k) + ((pb >> shift) & 255) * k);
  return `rgb(${channel(16)}, ${channel(8)}, ${channel(0)})`;
}

/** Colour of the ramp at `t` in 0..1, as a hex string. */
function ramp(stops: readonly string[], t: number): string {
  const at = Math.min(0.9999, Math.max(0, t)) * (stops.length - 1);
  const i = Math.floor(at);
  const rgb = mix(stops[i], stops[i + 1], at - i).match(/\d+/g) ?? [];
  return `#${rgb.map((v) => Number(v).toString(16).padStart(2, "0")).join("")}`;
}

/** The blocks of the logo, sampled once per size from the real vector shape. */
function sampleCells(radius: number): { cells: Cell[]; palette: string[] } {
  const palette: string[] = [];
  const ctx = context2d(createCanvas(1, 1));
  if (!ctx) return { cells: [], palette };
  const path = new Path2D(ANTIGRAVITY_PATH);
  const count = Math.ceil((radius * SIZE) / CELL);
  const unit = 24 / count; // viewBox units per block
  const cells: Cell[] = [];
  for (let row = 0; row < count; row++) {
    for (let col = 0; col < count; col++) {
      const u = (col + 0.5) / count;
      const v = (row + 0.5) / count;
      if (!ctx.isPointInPath(path, (col + 0.5) * unit, (row + 0.5) * unit)) continue;
      const side = Math.min(1, Math.max(0, (u - 0.38) / 0.24));
      const color = snap(
        mix(ramp(ANTIGRAVITY_RAMPS.left, v), ramp(ANTIGRAVITY_RAMPS.right, v), side),
      );
      let shade = palette.indexOf(color);
      if (shade < 0) shade = palette.push(color) - 1;
      const dx = (col - count / 2) * CELL;
      const dy = (row - count / 2) * CELL;
      cells.push({ dx, dy, angle: Math.atan2(dy, dx), shade });
    }
  }
  return { cells, palette };
}

function createArch(): Emblem {
  let radius = 90;
  let cells: Cell[] = [];
  let atlas: GlyphAtlas | null = null;

  return {
    prepare(nextRadius, dpr) {
      radius = nextRadius;
      const sampled = sampleCells(radius);
      cells = sampled.cells;
      atlas = createGlyphAtlas(dpr, GLYPH_INK, sampled.palette);
    },

    /* The hover runs on the compositor, so it glides instead of stepping at 15 fps. */
    float: { px: FLOAT_PX, speed: FLOAT_SPEED },

    draw(ctx, t, centreX, centreY) {
      if (!atlas) return;
      const { canvas, tile } = atlas;
      const x0 = Math.round(centreX);
      const y0 = Math.round(centreY);

      // Thrust trails under the two feet (they float with the logo).
      const foot = radius * SIZE * 0.36;
      const base = y0 + radius * SIZE * 0.47;
      for (let side = 0; side < 2; side++) {
        ctx.fillStyle = side === 0 ? "#64B6F6" : "#3886FB";
        for (let i = 0; i < TRAIL_BLOCKS; i++) {
          const life = (t * 0.9 + i / TRAIL_BLOCKS + side * 0.1) % 1;
          const size = Math.max(2, BLOCK * (1 - life));
          ctx.globalAlpha = 0.55 * (1 - life);
          ctx.fillRect(
            x0 + (side === 0 ? -foot : foot) - size / 2 + Math.sin(t * 3 + i) * 3,
            base + life * radius * 0.38,
            size,
            size,
          );
        }
      }

      const tick = shimmerTick(t);
      const sweep = t * SWEEP_SPEED;
      for (const cell of cells) {
        const light = Math.cos(cell.angle - sweep);
        ctx.globalAlpha = 0.8 + 0.2 * light;
        const glyph = shimmerGlyphAtTick(cell.dx / CELL, cell.dy / CELL, tick);
        ctx.drawImage(
          canvas,
          glyph * tile,
          cell.shade * tile,
          tile,
          tile,
          x0 + cell.dx,
          y0 + cell.dy,
          BLOCK,
          BLOCK,
        );
      }
      ctx.globalAlpha = 1;
    },
  };
}

/* A little hovering bot. a = antenna light, X = shell, v = visor, E = eye. */
const BOT_MAP = [
  ".....aa.....",
  ".....XX.....",
  "..XXXXXXXX..",
  ".XXXXXXXXXX.",
  ".XvvvvvvvvX.",
  ".XvEEvvEEvX.",
  ".XvvvvvvvvX.",
  ".XXXXXXXXXX.",
  "X.XXXXXXXX.X",
  "..XX....XX..",
] as const;

const PX = 5;
/* Shell colour per row, top to bottom. */
const SHELL = ["#8F64B4", "#8F64B4", "#706ECE", "#706ECE", "#4A81F0", "#4A81F0", "#4285F4", "#3886FB", "#47A8DC", "#43AEAB"];
const LIGHTS = ["#F6912E", "#EF5442", "#DBB131", "#7CC251"];
const FLAMES = ["#F6912E", "#DBB131", "#EF5442"];
const FLAME_ROWS = 2;

const botBlinking = (t: number) => t % 4 < 0.16;

const bot: Mascot = {
  width: BOT_MAP[0].length * PX,
  height: (BOT_MAP.length + FLAME_ROWS) * PX,
  bob: 7,
  bobSpeed: 1.7,
  pose: (t) => Math.floor(t * 9) * 8 + (Math.floor(t * 2) % LIGHTS.length) * 2 + (botBlinking(t) ? 1 : 0),
  draw(ctx, x, y, t) {
    const top = y;
    const blinking = botBlinking(t);
    BOT_MAP.forEach((line, r) => {
      for (let c = 0; c < line.length; c++) {
        const ch = line[c];
        if (ch === ".") continue;
        if (ch === "a") ctx.fillStyle = LIGHTS[Math.floor(t * 2) % LIGHTS.length];
        else if (ch === "v") ctx.fillStyle = "#0B0E17";
        else if (ch === "E") ctx.fillStyle = blinking ? "#0B0E17" : "#9ED0FF";
        else ctx.fillStyle = SHELL[r];
        ctx.fillRect(x + c * PX, top + r * PX, PX - 0.5, PX - 0.5);
      }
    });
    // Thrusters flicker under both legs.
    const flicker = Math.floor(t * 9);
    for (const column of [2, 8]) {
      const length = 1 + ((flicker + column) % FLAME_ROWS);
      for (let i = 0; i < length; i++) {
        ctx.fillStyle = FLAMES[(flicker + i + column) % FLAMES.length];
        ctx.fillRect(x + column * PX, top + (BOT_MAP.length + i) * PX, PX * 2 - 0.5, PX - 0.5);
      }
    }
  },
};

export function createAgyScene(): Scene {
  return createSpriteScene({
    seed: 53,
    haloColor: "rgba(66, 133, 244, 0.38)",
    cloudLow: "rgba(148, 163, 200, 0.10)",
    cloudHigh: "rgba(148, 163, 200, 0.17)",
    glow: "rgba(66, 133, 244, 0.09)",
    ambientColors: ["#4285F4", "#8F64B4", "#43AEAB", "#4285F4", "#F6912E"],
    ambientGlyphs: ["{", "<", "^", "+", "%", ".", ":", "}"],
    sparkColors: ["#9ED0FF", "#F6912E", "#7CC251", "#EF5442", "#8F64B4"],
    // Was 0.7 + 0.2 * light per block; now 0.8 + 0.2 * light, times this: a touch brighter.
    emblemAlpha: 0.97,
    createEmblem: createArch,
    mascot: bot,
  });
}
