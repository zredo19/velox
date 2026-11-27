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
import { OPENCODE_INNER_BLOCK_PATH, OPENCODE_OUTER_PATH } from "./logo";

/* Authentic OpenCode monochrome warm off-white + charcoal-slate inner counter shades. */
const OUTER_SHADES = ["#FFFFFF", "#F0ECE8", "#DCD6D0", "#B8B0AA"] as const;
const INNER_SHADES = ["#6E6864", "#55504C", "#423E3B"] as const;
const ALL_SHADES = [...OUTER_SHADES, ...INNER_SHADES] as const;
const GLYPH_INK = "rgba(18, 16, 16, 0.58)";

const MARK_ALPHA = 0.9;
const SWEEP_SPEED = 1.35;
const FLOAT_PX = 6;
const FLOAT_SPEED = 1.1;
const SIZE = 2.05;

interface OpenCodeCell {
  dx: number;
  dy: number;
  angle: number;
  inner: boolean;
  dither: number;
}

/** Sample the real OpenCode "O" vector paths directly into crisp terminal-grid cells. */
function sampleOpenCodeCells(radius: number): OpenCodeCell[] {
  const ctx = context2d(createCanvas(1, 1));
  if (!ctx) return [];
  const outerPath = new Path2D(OPENCODE_OUTER_PATH);
  const innerPath = new Path2D(OPENCODE_INNER_BLOCK_PATH);
  const count = Math.ceil((radius * SIZE) / CELL);
  const unit = 24 / count;
  const cells: OpenCodeCell[] = [];

  for (let row = 0; row < count; row++) {
    for (let col = 0; col < count; col++) {
      const px = (col + 0.5) * unit;
      const py = (row + 0.5) * unit;
      const inOuter = ctx.isPointInPath(outerPath, px, py, "evenodd");
      const inInner = !inOuter && ctx.isPointInPath(innerPath, px, py);
      if (!inOuter && !inInner) continue;
      const dx = (col - count / 2) * CELL;
      const dy = (row - count / 2) * CELL;
      cells.push({
        dx,
        dy,
        angle: Math.atan2(dy, dx),
        inner: inInner,
        dither: bayer(col, row) - 0.5,
      });
    }
  }
  return cells;
}

function createOpenCodeEmblem(): Emblem {
  let cells: OpenCodeCell[] = [];
  let atlas: GlyphAtlas | null = null;
  let atlasDpr = 0;

  return {
    prepare(nextRadius, dpr) {
      cells = sampleOpenCodeCells(nextRadius);
      if (!atlas || atlasDpr !== dpr) {
        atlas = createGlyphAtlas(dpr, GLYPH_INK, ALL_SHADES);
        atlasDpr = dpr;
      }
    },

    float: { px: FLOAT_PX, speed: FLOAT_SPEED },

    draw(ctx, t, centreX, centreY) {
      if (!atlas) return;
      const { canvas, tile } = atlas;
      const tick = shimmerTick(t);
      const sweep = t * SWEEP_SPEED;
      const x0 = Math.round(centreX);
      const y0 = Math.round(centreY);

      for (const cell of cells) {
        const light = Math.cos(cell.angle - sweep);
        let shade: number;
        if (cell.inner) {
          const k = 0.45 + cell.dither * 0.22 - light * 0.2;
          const idx = Math.min(INNER_SHADES.length - 1, Math.max(0, Math.floor(k * INNER_SHADES.length)));
          shade = OUTER_SHADES.length + idx;
        } else {
          const k = 0.35 + cell.dither * 0.18 - light * 0.28;
          shade = Math.min(OUTER_SHADES.length - 1, Math.max(0, Math.floor(k * OUTER_SHADES.length)));
        }
        const glyph = shimmerGlyphAtTick(Math.round(cell.dx / CELL), Math.round(cell.dy / CELL), tick);
        ctx.drawImage(
          canvas,
          glyph * tile,
          shade * tile,
          tile,
          tile,
          x0 + cell.dx,
          y0 + cell.dy,
          BLOCK,
          BLOCK,
        );
      }
    },
  };
}

/* Blocky OpenCode "O" pixel mascot matching AgentCrew. */
const BODY_MAP = [
  ".LLLLLLLLL.",
  ".BBBBBBBBB.",
  ".BB.....BB.",
  ".BB.E.E.BB.",
  ".BB.....BB.",
  ".BBDDDDDBB.",
  ".BBDDDDDBB.",
  ".BBBBBBBBB.",
  ".BBBBBBBBB.",
] as const;

const FEET = ["..BB...BB..", ".BB.....BB."] as const;
const PX = 5;

const opencodeBlinking = (t: number) => t % 3.8 < 0.16;
const opencodeStep = (t: number, walking: boolean) =>
  walking && Math.floor(t * 3.4) % 2 === 1 ? 1 : 0;

const opencodeMascot: Mascot = {
  width: BODY_MAP[0].length * PX,
  height: (BODY_MAP.length + 1) * PX,
  bob: 3,
  bobSpeed: 2.0,
  pose: (t, walking) => opencodeStep(t, walking) * 2 + (opencodeBlinking(t) ? 1 : 0),
  draw(ctx, x, y, t, walking) {
    const blinking = opencodeBlinking(t);
    const step = opencodeStep(t, walking);
    const drawRows = (rows: readonly string[], rowOffset: number) => {
      rows.forEach((line, r) => {
        for (let c = 0; c < line.length; c++) {
          const ch = line[c];
          if (ch === ".") continue;
          if (ch === "E") ctx.fillStyle = blinking ? "#161414" : "#FDFCFC";
          else if (ch === "L") ctx.fillStyle = "#FFFFFF";
          else if (ch === "D") ctx.fillStyle = "#5C5855";
          else ctx.fillStyle = "#E8E4DF";
          ctx.fillRect(x + c * PX, y + (r + rowOffset) * PX, PX - 0.5, PX - 0.5);
        }
      });
    };
    drawRows(BODY_MAP, 0);
    drawRows([FEET[step]], BODY_MAP.length);
  },
};

export function createOpencodeScene(): Scene {
  return createSpriteScene({
    seed: 97,
    haloColor: "rgba(232, 228, 223, 0.24)",
    cloudLow: "rgba(180, 174, 168, 0.08)",
    cloudHigh: "rgba(220, 214, 208, 0.14)",
    glow: "rgba(244, 240, 236, 0.06)",
    ambientColors: ["#EDEAE6", "#A8A29E", "#78716C"],
    ambientGlyphs: ["[", "]", "{", "}", "_", ":", ".", "/"],
    sparkColors: ["#FFFFFF", "#D6D1CC", "#9E9893"],
    emblemAlpha: MARK_ALPHA,
    createEmblem: createOpenCodeEmblem,
    mascot: opencodeMascot,
  });
}
