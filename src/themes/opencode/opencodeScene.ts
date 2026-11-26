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

/* Soft violet-lavender shades for the faceted OpenCode diamond prism. */
const SHADES = ["#F3ECFF", "#DDD6FE", "#C4B5FD", "#957AE6", "#6D54B8"] as const;
const GLYPH_INK = "rgba(16, 12, 28, 0.56)";

const PRISM_ALPHA = 0.88;
const SWEEP_SPEED = 1.5;
const BREATH_SPEED = 2.1;
const FLOAT_PX = 7;
const FLOAT_SPEED = 1.25;

interface PrismCell {
  cx: number;
  cy: number;
  manhattan: number;
  angle: number;
  inner: boolean;
  dither: number;
}

function createDiamondPrism(): Emblem {
  let radius = 90;
  let atlas: GlyphAtlas | null = null;
  let atlasDpr = 0;
  let cells: PrismCell[] = [];
  let cellsKey = "";

  const cellsFor = (bx: number, by: number): PrismCell[] => {
    const key = `${bx},${by},${radius}`;
    if (key === cellsKey) return cells;
    cellsKey = key;
    const far = radius * 1.12;
    const x0 = Math.floor((bx - far) / CELL);
    const x1 = Math.ceil((bx + far) / CELL);
    const y0 = Math.floor((by - far) / CELL);
    const y1 = Math.ceil((by + far) / CELL);
    cells = [];

    for (let cy = y0; cy <= y1; cy++) {
      for (let cx = x0; cx <= x1; cx++) {
        const dx = cx * CELL + CELL / 2 - bx;
        const dy = cy * CELL + CELL / 2 - by;
        const m = (Math.abs(dx) + Math.abs(dy)) / (radius * 1.08);
        if (m > 1.0) continue;

        const outerBand = m >= 0.72 && m <= 1.0;
        const innerGem = m <= 0.48;
        if (!outerBand && !innerGem) continue;

        cells.push({
          cx,
          cy,
          manhattan: m,
          angle: Math.atan2(dy, dx),
          inner: innerGem,
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
      const breath = 0.5 + 0.5 * Math.sin(t * BREATH_SPEED);

      for (const cell of cellsFor(bx, by)) {
        const light = Math.cos(cell.angle - sweep);
        const base = cell.inner ? 0.25 + cell.manhattan * 0.45 - breath * 0.12 : 0.56 + cell.manhattan * 0.2;
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

/* Pixel OpenCode diamond critter matching AgentCrew. */
const BODY_MAP = [
  ".....B.....",
  "....BLB....",
  "...BLLLB...",
  "..BBEBEBB..",
  ".BBBBBBBBB.",
  "BBBDBBBDBBB",
  ".BBBDDDBBB.",
  "..BBBBBBB..",
  "...BBBBB...",
] as const;

const FEET = ["....B.B....", "...B...B..."] as const;
const PX = 5;

const opencodeBlinking = (t: number) => t % 3.8 < 0.16;
const opencodeStep = (t: number, walking: boolean) =>
  walking && Math.floor(t * 3.4) % 2 === 1 ? 1 : 0;

const opencodeMascot: Mascot = {
  width: BODY_MAP[0].length * PX,
  height: (BODY_MAP.length + 1) * PX,
  bob: 4,
  bobSpeed: 2.1,
  pose: (t, walking) => opencodeStep(t, walking) * 2 + (opencodeBlinking(t) ? 1 : 0),
  draw(ctx, x, y, t, walking) {
    const blinking = opencodeBlinking(t);
    const step = opencodeStep(t, walking);
    const drawRows = (rows: readonly string[], rowOffset: number) => {
      rows.forEach((line, r) => {
        for (let c = 0; c < line.length; c++) {
          const ch = line[c];
          if (ch === ".") continue;
          if (ch === "E") ctx.fillStyle = blinking ? "#C9BBFF" : "#221B36";
          else if (ch === "L") ctx.fillStyle = "#ECE6FF";
          else if (ch === "D") ctx.fillStyle = "#2A2340";
          else ctx.fillStyle = "#C9BBFF";
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
    haloColor: "rgba(196, 181, 253, 0.34)",
    cloudLow: "rgba(167, 152, 219, 0.09)",
    cloudHigh: "rgba(167, 152, 219, 0.16)",
    glow: "rgba(167, 139, 250, 0.08)",
    ambientColors: ["#C4B5FD", "#A78BFA", "#DDD6FE"],
    ambientGlyphs: ["◇", "◆", "{", "}", ".", ":", "+", "*"],
    sparkColors: ["#ECE6FF", "#C4B5FD", "#A78BFA"],
    emblemAlpha: PRISM_ALPHA,
    createEmblem: createDiamondPrism,
    mascot: opencodeMascot,
  });
}
