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

/* Emerald-mint shades for the geometric hexagon core. */
const SHADES = ["#B2F5DF", "#5EEAD4", "#1FC398", "#118A6B", "#0B5D49"] as const;
const GLYPH_INK = "rgba(8, 20, 17, 0.56)";

const HEX_ALPHA = 0.88;
const PULSE_SPEED = 2.0;
const SWEEP_SPEED = 1.45;
const FLOAT_PX = 6;
const FLOAT_SPEED = 1.15;

interface HexCell {
  cx: number;
  cy: number;
  dx: number;
  dy: number;
  normDist: number;
  angle: number;
  ring: boolean;
  dither: number;
}

/** Pointy-top hexagon SDF distance normalized to radius 1. */
function hexDist(x: number, y: number): number {
  const ax = Math.abs(x);
  const ay = Math.abs(y);
  return Math.max(ax * 0.8660254 + ay * 0.5, ay);
}

function createHexCore(): Emblem {
  let radius = 90;
  let atlas: GlyphAtlas | null = null;
  let atlasDpr = 0;
  let cells: HexCell[] = [];
  let cellsKey = "";

  const cellsFor = (bx: number, by: number): HexCell[] => {
    const key = `${bx},${by},${radius}`;
    if (key === cellsKey) return cells;
    cellsKey = key;
    const outer = radius * 1.08;
    const x0 = Math.floor((bx - outer) / CELL);
    const x1 = Math.ceil((bx + outer) / CELL);
    const y0 = Math.floor((by - outer) / CELL);
    const y1 = Math.ceil((by + outer) / CELL);
    cells = [];

    for (let cy = y0; cy <= y1; cy++) {
      for (let cx = x0; cx <= x1; cx++) {
        const dx = cx * CELL + CELL / 2 - bx;
        const dy = cy * CELL + CELL / 2 - by;
        const hd = hexDist(dx / radius, dy / radius);
        if (hd > 1.02) continue;

        // Outer hexagonal frame + inner code-bracket chevrons (< >) + pulsing core
        const inFrame = hd >= 0.72 && hd <= 0.98;
        const nx = dx / radius;
        const ny = dy / radius;
        const absY = Math.abs(ny);
        const leftChev = Math.abs(nx + 0.34 - absY * 0.62) < 0.115 && absY < 0.34;
        const rightChev = Math.abs(nx - 0.34 + absY * 0.62) < 0.115 && absY < 0.34;
        const centerDot = Math.hypot(nx, ny) < 0.14;

        if (!inFrame && !leftChev && !rightChev && !centerDot) continue;

        cells.push({
          cx,
          cy,
          dx,
          dy,
          normDist: Math.min(1, hd),
          angle: Math.atan2(dy, dx),
          ring: inFrame,
          dither: bayer(cx, cy) - 0.5,
        });
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

    float: { px: FLOAT_PX, speed: FLOAT_SPEED },

    draw(ctx, t, bx, by) {
      if (!atlas) return;
      const { canvas, tile } = atlas;
      const tick = shimmerTick(t);
      const sweep = t * SWEEP_SPEED;
      const pulse = 0.5 + 0.5 * Math.sin(t * PULSE_SPEED);

      for (const cell of cellsFor(bx, by)) {
        const light = Math.cos(cell.angle - sweep);
        const base = cell.ring ? 0.52 + cell.normDist * 0.25 : 0.22 - pulse * 0.15;
        const k = base + cell.dither * 0.16 - light * 0.24;
        const shade = Math.min(SHADES.length - 1, Math.max(0, Math.floor(k * SHADES.length)));
        const glyph = shimmerGlyphAtTick(cell.cx, cell.cy, tick);
        ctx.drawImage(
          canvas,
          glyph * tile,
          shade * tile,
          tile,
          tile,
          cell.cx * CELL,
          cell.cy * CELL,
          BLOCK,
          BLOCK,
        );
      }
    },
  };
}

/* Pixel Codex critter matching AgentCrew. */
const BODY_MAP = [
  "...BBBBB...",
  "..BLLLLLB..",
  ".BBBBBBBBB.",
  ".BBEBBBEBB.",
  ".BBBBBBBBB.",
  ".BBDBBBDBB.",
  ".BBBDDDBBB.",
  "..BBBBBBB..",
  "...BBBBB...",
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
          if (ch === "E") ctx.fillStyle = blinking ? "#1FC398" : "#06281D";
          else if (ch === "L") ctx.fillStyle = "#8CEBCB";
          else if (ch === "D") ctx.fillStyle = "#06281D";
          else ctx.fillStyle = "#1FC398";
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
    haloColor: "rgba(31, 195, 152, 0.34)",
    cloudLow: "rgba(120, 196, 178, 0.09)",
    cloudHigh: "rgba(120, 196, 178, 0.16)",
    glow: "rgba(16, 163, 127, 0.08)",
    ambientColors: ["#1FC398", "#5EEAD4", "#34D399"],
    ambientGlyphs: ["<", ">", "/", "{", "}", "+", ":", ";"],
    sparkColors: ["#8CEBCB", "#1FC398", "#5EEAD4"],
    emblemAlpha: HEX_ALPHA,
    createEmblem: createHexCore,
    mascot: codexMascot,
  });
}
