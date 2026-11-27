import { context2d, createCanvas } from "../shared/canvas";
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
import { OPENAI_BLOSSOM_PATH } from "./logo";

/* Iconic OpenAI emerald-mint shades for the sampled Blossom emblem. */
const SHADES = ["#B6FCE0", "#5CF0B8", "#19C37D", "#12915D", "#0B5F3E"] as const;
const GLYPH_INK = "rgba(7, 20, 15, 0.56)";

const BLOSSOM_ALPHA = 0.9;
const SWEEP_SPEED = 1.5;
const TURN_SECONDS = 120;
const FLOAT_PX = 6;
const FLOAT_SPEED = 1.2;
const SIZE = 2.15;

interface BlossomCell {
  dx: number;
  dy: number;
  dist: number;
  angle: number;
  dither: number;
}

/** Sample the real official OpenAI Blossom SVG Path2D into grid cells once per resize. */
function sampleBlossomCells(radius: number): BlossomCell[] {
  const ctx = context2d(createCanvas(1, 1));
  if (!ctx) return [];
  const path = new Path2D(OPENAI_BLOSSOM_PATH);
  const count = Math.ceil((radius * SIZE) / CELL);
  const unit = 24 / count;
  const cells: BlossomCell[] = [];

  for (let row = 0; row < count; row++) {
    for (let col = 0; col < count; col++) {
      if (!ctx.isPointInPath(path, (col + 0.5) * unit, (row + 0.5) * unit)) continue;
      const dx = (col - count / 2) * CELL;
      const dy = (row - count / 2) * CELL;
      const dist = Math.hypot(dx, dy) / (radius * SIZE * 0.52);
      cells.push({
        dx,
        dy,
        dist: Math.min(1, dist),
        angle: Math.atan2(dy, dx),
        dither: bayer(col, row) - 0.5,
      });
    }
  }
  return cells;
}

function createBlossomEmblem(): Emblem {
  let cells: BlossomCell[] = [];
  let atlas: GlyphAtlas | null = null;
  let atlasDpr = 0;

  return {
    prepare(nextRadius, dpr) {
      cells = sampleBlossomCells(nextRadius);
      if (!atlas || atlasDpr !== dpr) {
        atlas = createGlyphAtlas(dpr, GLYPH_INK, SHADES);
        atlasDpr = dpr;
      }
    },

    float: { px: FLOAT_PX, speed: FLOAT_SPEED },

    draw(ctx, t, centreX, centreY) {
      if (!atlas) return;
      const { canvas, tile } = atlas;
      const tick = shimmerTick(t);
      const sweep = t * SWEEP_SPEED;
      const turn = (t / TURN_SECONDS) * Math.PI * 2;
      const cosT = Math.cos(turn);
      const sinT = Math.sin(turn);
      const x0 = Math.round(centreX);
      const y0 = Math.round(centreY);

      for (const cell of cells) {
        // Gentle orbital turn around the blossom centre
        const rx = cell.dx * cosT - cell.dy * sinT;
        const ry = cell.dx * sinT + cell.dy * cosT;
        const light = Math.cos(cell.angle + turn - sweep);
        const k = 0.25 + cell.dist * 0.45 + cell.dither * 0.16 - light * 0.26;
        const shade = Math.min(SHADES.length - 1, Math.max(0, Math.floor(k * SHADES.length)));
        const glyph = shimmerGlyphAtTick(Math.round(cell.dx / CELL), Math.round(cell.dy / CELL), tick);
        ctx.drawImage(
          canvas,
          glyph * tile,
          shade * tile,
          tile,
          tile,
          Math.round(x0 + rx),
          Math.round(y0 + ry),
          BLOCK,
          BLOCK,
        );
      }
    },
  };
}

/* OpenAI Blossom pixel critter matching AgentCrew. */
const BODY_MAP = [
  "...LL.LL...",
  ".BBB...BBB.",
  "BBLBBBBBLLB",
  ".BBEBBBEBB.",
  "BBBBBBBBBBB",
  ".BBDBBBDBB.",
  "BB.BDDDB.BB",
  ".BBB...BBB.",
  "...BB.BB...",
] as const;

const FEET = ["...B...B...", "..B.....B.."] as const;
const PX = 5;

const codexBlinking = (t: number) => t % 4.4 < 0.16;
const codexStep = (t: number, walking: boolean) => (walking && Math.floor(t * 3.2) % 2 === 1 ? 1 : 0);

const codexMascot: Mascot = {
  width: BODY_MAP[0].length * PX,
  height: (BODY_MAP.length + 1) * PX,
  bob: 3,
  bobSpeed: 2.0,
  pose: (t, walking) => codexStep(t, walking) * 2 + (codexBlinking(t) ? 1 : 0),
  draw(ctx, x, y, t, walking) {
    const blinking = codexBlinking(t);
    const step = codexStep(t, walking);
    const drawRows = (rows: readonly string[], rowOffset: number) => {
      rows.forEach((line, r) => {
        for (let c = 0; c < line.length; c++) {
          const ch = line[c];
          if (ch === ".") continue;
          if (ch === "E") ctx.fillStyle = blinking ? "#19C37D" : "#06291C";
          else if (ch === "L") ctx.fillStyle = "#86F0C5";
          else if (ch === "D") ctx.fillStyle = "#06291C";
          else ctx.fillStyle = "#19C37D";
          ctx.fillRect(x + c * PX, y + (r + rowOffset) * PX, PX - 0.5, PX - 0.5);
        }
      });
    };
    drawRows(BODY_MAP, 0);
    drawRows([FEET[step]], BODY_MAP.length);
  },
};

export function createCodexScene(): Scene {
  return createSpriteScene({
    seed: 71,
    haloColor: "rgba(25, 195, 125, 0.36)",
    cloudLow: "rgba(116, 198, 166, 0.09)",
    cloudHigh: "rgba(116, 198, 166, 0.16)",
    glow: "rgba(16, 163, 127, 0.09)",
    ambientColors: ["#19C37D", "#5CF0B8", "#34D399"],
    ambientGlyphs: ["{", "}", "<", ">", "/", "+", ":", ";"],
    sparkColors: ["#86F0C5", "#19C37D", "#5CF0B8"],
    emblemAlpha: BLOSSOM_ALPHA,
    createEmblem: createBlossomEmblem,
    mascot: codexMascot,
  });
}
