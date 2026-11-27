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

interface ShapeCell {
  cx: number;
  cy: number;
  dx: number;
  dy: number;
  nx: number;
  ny: number;
  dist: number;
  angle: number;
  dither: number;
}

function createShapeEmblem(config: {
  shades: readonly string[];
  ink: string;
  sweepSpeed: number;
  floatPx?: number;
  floatSpeed?: number;
  test: (nx: number, ny: number, dist: number, angle: number) => boolean;
  shadeAt: (cell: ShapeCell, t: number, sweep: number) => number;
}): Emblem {
  let radius = 90;
  let atlas: GlyphAtlas | null = null;
  let atlasDpr = 0;
  let cells: ShapeCell[] = [];
  let cellsKey = "";

  const cellsFor = (bx: number, by: number): ShapeCell[] => {
    const key = `${bx},${by},${radius}`;
    if (key === cellsKey) return cells;
    cellsKey = key;
    const far = radius * 1.15;
    const x0 = Math.floor((bx - far) / CELL);
    const x1 = Math.ceil((bx + far) / CELL);
    const y0 = Math.floor((by - far) / CELL);
    const y1 = Math.ceil((by + far) / CELL);
    cells = [];

    for (let cy = y0; cy <= y1; cy++) {
      for (let cx = x0; cx <= x1; cx++) {
        const dx = cx * CELL + CELL / 2 - bx;
        const dy = cy * CELL + CELL / 2 - by;
        const nx = dx / radius;
        const ny = dy / radius;
        const dist = Math.hypot(nx, ny);
        if (dist > 1.12) continue;
        const angle = Math.atan2(ny, nx);
        if (!config.test(nx, ny, dist, angle)) continue;
        cells.push({
          cx,
          cy,
          dx,
          dy,
          nx,
          ny,
          dist,
          angle,
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
        atlas = createGlyphAtlas(dpr, config.ink, config.shades);
        atlasDpr = dpr;
      }
    },

    float: config.floatPx
      ? { px: config.floatPx, speed: config.floatSpeed ?? 1.2 }
      : undefined,

    draw(ctx, t, bx, by) {
      if (!atlas) return;
      const { canvas, tile } = atlas;
      const tick = shimmerTick(t);
      const sweep = t * config.sweepSpeed;
      const count = config.shades.length;

      for (const cell of cellsFor(bx, by)) {
        const k = config.shadeAt(cell, t, sweep);
        const shade = Math.min(count - 1, Math.max(0, Math.floor(k * count)));
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

function makePixelMascot(config: {
  bodyMap: readonly string[];
  feet: readonly [string, string];
  colors: Record<string, string>;
  blinkBg: string;
  bob?: number;
  bobSpeed?: number;
  px?: number;
}): Mascot {
  const px = config.px ?? 5;
  const blinking = (t: number) => t % 4.1 < 0.16;
  const step = (t: number, walking: boolean) => (walking && Math.floor(t * 3.2) % 2 === 1 ? 1 : 0);

  return {
    width: config.bodyMap[0].length * px,
    height: (config.bodyMap.length + 1) * px,
    bob: config.bob ?? 3,
    bobSpeed: config.bobSpeed ?? 2.0,
    pose: (t, walking) => step(t, walking) * 2 + (blinking(t) ? 1 : 0),
    draw(ctx, x, y, t, walking) {
      const blink = blinking(t);
      const s = step(t, walking);
      const paint = (rows: readonly string[], offset: number) => {
        rows.forEach((line, r) => {
          for (let c = 0; c < line.length; c++) {
            const ch = line[c];
            if (ch === ".") continue;
            ctx.fillStyle = ch === "E" && blink ? config.blinkBg : (config.colors[ch] ?? "#FFFFFF");
            ctx.fillRect(x + c * px, y + (r + offset) * px, px - 0.5, px - 0.5);
          }
        });
      };
      paint(config.bodyMap, 0);
      paint([config.feet[s]], config.bodyMap.length);
    },
  };
}

/* ---- 1. Tokyo Night: Neon Torii Gate & Lantern Spirit -------------------------------- */

const tokyoMascot = makePixelMascot({
  bodyMap: [
    "...AAAAA...",
    "..LLLLLLL..",
    ".BBBBBBBBB.",
    ".BBEBBBEBB.",
    ".BBBBBBBBB.",
    ".BBDBBBDBB.",
    "..BBBBBBB..",
    "...AAAAA...",
  ],
  feet: ["...B...B...", "..B.....B.."],
  colors: {
    B: "#7AA2F7",
    L: "#B4D0FF",
    A: "#BB9AF7",
    E: "#131522",
    D: "#131522",
  },
  blinkBg: "#7AA2F7",
  bob: 4,
});

export function createTokyoNightScene(): Scene {
  return createSpriteScene({
    seed: 109,
    haloColor: "rgba(122, 162, 247, 0.36)",
    cloudLow: "rgba(122, 162, 247, 0.09)",
    cloudHigh: "rgba(187, 154, 247, 0.16)",
    glow: "rgba(122, 162, 247, 0.09)",
    ambientColors: ["#7AA2F7", "#BB9AF7", "#7DCFFF"],
    ambientGlyphs: ["{", "}", "λ", ":", ".", "+", "/", "*"],
    sparkColors: ["#7DCFFF", "#BB9AF7", "#F7768E"],
    emblemAlpha: 0.9,
    createEmblem: () =>
      createShapeEmblem({
        shades: ["#C0CAF5", "#7DCFFF", "#7AA2F7", "#BB9AF7", "#565F89"],
        ink: "rgba(19, 21, 34, 0.56)",
        sweepSpeed: 1.35,
        floatPx: 6,
        floatSpeed: 1.15,
        // Iconic minimalist Torii gate silhouette
        test: (nx, ny) => {
          const topBeam = ny >= -0.68 && ny <= -0.44 && Math.abs(nx) <= 0.98 - Math.max(0, ny + 0.58) * 0.6;
          const midBeam = ny >= -0.26 && ny <= -0.1 && Math.abs(nx) <= 0.74;
          const centerPost = ny >= -0.46 && ny <= -0.1 && Math.abs(nx) <= 0.1;
          const leftPillar = ny >= -0.46 && ny <= 0.82 && Math.abs(nx + 0.46) <= 0.11;
          const rightPillar = ny >= -0.46 && ny <= 0.82 && Math.abs(nx - 0.46) <= 0.11;
          return topBeam || midBeam || centerPost || leftPillar || rightPillar;
        },
        shadeAt: (cell, _t, sweep) =>
          0.35 + (cell.ny + 0.8) * 0.28 + cell.dither * 0.16 - Math.cos(cell.angle - sweep) * 0.24,
      }),
    mascot: tokyoMascot,
  });
}

/* ---- 2. Catppuccin Mocha: Pastel Crescent Moon & Cozy Cat ---------------------------- */

const catppuccinMascot = makePixelMascot({
  bodyMap: [
    ".BB.....BB.",
    ".BBB...BBB.",
    ".BLLLLLLLB.",
    ".BBEBBBEBB.",
    ".BBBBABBBB.",
    ".BBDBBBDBB.",
    "..BBBBBBB..",
    "..BBBBBBB..",
  ],
  feet: ["..BB...BB..", ".BB.....BB."],
  colors: {
    B: "#CBA6F7",
    L: "#E2CCFF",
    A: "#F5C2E7",
    E: "#181825",
    D: "#313244",
  },
  blinkBg: "#CBA6F7",
  bob: 3,
});

export function createCatppuccinScene(): Scene {
  return createSpriteScene({
    seed: 131,
    haloColor: "rgba(203, 166, 247, 0.34)",
    cloudLow: "rgba(203, 166, 247, 0.09)",
    cloudHigh: "rgba(245, 194, 231, 0.15)",
    glow: "rgba(203, 166, 247, 0.08)",
    ambientColors: ["#CBA6F7", "#F5C2E7", "#89B4FA", "#94E2D5"],
    ambientGlyphs: ["✦", "·", "{", "}", "+", ":", ".", "*"],
    sparkColors: ["#F5C2E7", "#CBA6F7", "#F9E2AF"],
    emblemAlpha: 0.9,
    createEmblem: () =>
      createShapeEmblem({
        shades: ["#F5E0DC", "#F5C2E7", "#CBA6F7", "#89B4FA", "#585B70"],
        ink: "rgba(24, 24, 37, 0.55)",
        sweepSpeed: 1.25,
        floatPx: 7,
        floatSpeed: 1.1,
        // Dreamy crescent moon + tiny companion star
        test: (nx, ny, dist) => {
          const inMoon = dist <= 0.94 && Math.hypot(nx - 0.36, ny + 0.18) >= 0.72;
          const inStar = Math.abs(nx - 0.44) + Math.abs(ny + 0.24) <= 0.2;
          return inMoon || inStar;
        },
        shadeAt: (cell, _t, sweep) =>
          0.26 + cell.dist * 0.42 + cell.dither * 0.16 - Math.cos(cell.angle - sweep) * 0.26,
      }),
    mascot: catppuccinMascot,
  });
}

/* ---- 3. Synthwave '84: Striped Retrowave Sun & Visor Bot ----------------------------- */

const synthwaveMascot = makePixelMascot({
  bodyMap: [
    "....AAA....",
    "..BBBBBBB..",
    ".BDDDDDDDB.",
    ".BEEEEEEBB.",
    ".BDDDDDDDB.",
    ".BBBBBBBBB.",
    "..BBBBBBB..",
    "..BB...BB..",
  ],
  feet: ["..AA...AA..", ".AA.....AA."],
  colors: {
    B: "#FF2E97",
    A: "#FFD319",
    D: "#120B1F",
    E: "#00F0FF",
  },
  blinkBg: "#120B1F",
  bob: 6,
  bobSpeed: 1.8,
});

export function createSynthwaveScene(): Scene {
  return createSpriteScene({
    seed: 157,
    haloColor: "rgba(255, 46, 151, 0.38)",
    cloudLow: "rgba(189, 147, 249, 0.10)",
    cloudHigh: "rgba(255, 46, 151, 0.16)",
    glow: "rgba(255, 46, 151, 0.10)",
    ambientColors: ["#FF2E97", "#00F0FF", "#FFD319"],
    ambientGlyphs: ["/", "\\", "=", "+", ":", ".", "*", "^"],
    sparkColors: ["#00F0FF", "#FF2E97", "#FFD319"],
    emblemAlpha: 0.92,
    createEmblem: () =>
      createShapeEmblem({
        shades: ["#FFF275", "#FFD319", "#FF8C42", "#FF2E97", "#9D2EC5"],
        ink: "rgba(18, 11, 31, 0.52)",
        sweepSpeed: 1.5,
        floatPx: 6,
        floatSpeed: 1.25,
        // Classic 80s sliced sunset disc
        test: (_nx, ny, dist) => {
          if (dist > 0.96) return false;
          if (ny > 0.08) {
            const stripe = (ny - 0.08) * 5.2;
            const frac = stripe - Math.floor(stripe);
            const cutWidth = 0.18 + (ny - 0.08) * 0.42;
            if (frac < cutWidth) return false;
          }
          return true;
        },
        shadeAt: (cell, t) =>
          (cell.ny + 0.96) / 1.92 + cell.dither * 0.12 - Math.sin(t * 2 + cell.nx * 2) * 0.06,
      }),
    mascot: synthwaveMascot,
  });
}

/* ---- 4. Nord Frost: Crystalline Snowflake & Arctic Spirit ---------------------------- */

const nordMascot = makePixelMascot({
  bodyMap: [
    ".BB.....BB.",
    ".BLLLLLLLB.",
    ".BBBBBBBBB.",
    ".BBEBBBEBB.",
    ".BBBBABBBB.",
    "..BBBBBBB..",
    "..BLLLLLB..",
    "...BB.BB...",
  ],
  feet: ["...B...B...", "..B.....B.."],
  colors: {
    B: "#88C0D0",
    L: "#D8DEE9",
    A: "#5E81AC",
    E: "#181E28",
  },
  blinkBg: "#88C0D0",
  bob: 3,
});

export function createNordScene(): Scene {
  return createSpriteScene({
    seed: 181,
    haloColor: "rgba(136, 192, 208, 0.34)",
    cloudLow: "rgba(129, 161, 193, 0.10)",
    cloudHigh: "rgba(216, 222, 233, 0.16)",
    glow: "rgba(136, 192, 208, 0.08)",
    ambientColors: ["#88C0D0", "#81A1C1", "#E5E9F0"],
    ambientGlyphs: ["*", "+", "·", ":", ".", "<", ">", "/"],
    sparkColors: ["#ECEFF4", "#88C0D0", "#8FBCBB"],
    emblemAlpha: 0.88,
    createEmblem: () =>
      createShapeEmblem({
        shades: ["#ECEFF4", "#D8DEE9", "#88C0D0", "#81A1C1", "#5E81AC"],
        ink: "rgba(24, 30, 40, 0.56)",
        sweepSpeed: 1.2,
        floatPx: 5,
        floatSpeed: 1.0,
        // 6-fold polar snowflake crystal
        test: (_nx, _ny, dist, angle) => {
          if (dist > 0.98) return false;
          if (dist < 0.22) return true;
          const sector = ((angle % (Math.PI / 3)) + Math.PI / 3) % (Math.PI / 3);
          const dAng = Math.min(sector, Math.PI / 3 - sector);
          const perp = dist * Math.sin(dAng);
          const onArm = perp <= 0.11;
          const onBranch = dist > 0.42 && dist < 0.78 && Math.abs(perp - (dist - 0.42) * 0.75) < 0.075;
          return onArm || onBranch;
        },
        shadeAt: (cell, _t, sweep) =>
          0.22 + cell.dist * 0.52 + cell.dither * 0.16 - Math.cos(cell.angle - sweep) * 0.24,
      }),
    mascot: nordMascot,
  });
}

/* ---- 5. Dracula: Gothic Crest & Pixel Bat -------------------------------------------- */

const draculaMascot = makePixelMascot({
  bodyMap: [
    ".B.......B.",
    "BBB..B..BBB",
    "BBBBBBBBBBB",
    "BBBEBBBEBBB",
    ".BBBBBBBBB.",
    "..BBAAABB..",
    "...BBBBB...",
    "....B.B....",
  ],
  feet: ["....B.B....", "...B...B..."],
  colors: {
    B: "#BD93F9",
    A: "#FF79C6",
    E: "#50FA7B",
  },
  blinkBg: "#171824",
  bob: 6,
  bobSpeed: 2.3,
});

export function createDraculaScene(): Scene {
  return createSpriteScene({
    seed: 211,
    haloColor: "rgba(189, 147, 249, 0.36)",
    cloudLow: "rgba(189, 147, 249, 0.09)",
    cloudHigh: "rgba(255, 121, 198, 0.14)",
    glow: "rgba(189, 147, 249, 0.08)",
    ambientColors: ["#BD93F9", "#FF79C6", "#50FA7B", "#8BE9FD"],
    ambientGlyphs: ["{", "}", ";", "+", ":", ".", "*", "^"],
    sparkColors: ["#FF79C6", "#BD93F9", "#50FA7B"],
    emblemAlpha: 0.9,
    createEmblem: () =>
      createShapeEmblem({
        shades: ["#F8F8F2", "#FF79C6", "#BD93F9", "#8E63D6", "#58418C"],
        ink: "rgba(23, 24, 36, 0.56)",
        sweepSpeed: 1.45,
        floatPx: 7,
        floatSpeed: 1.35,
        // Winged gothic emblem inside an eclipse ring
        test: (nx, ny, dist) => {
          const ring = dist >= 0.78 && dist <= 0.96;
          const ax = Math.abs(nx);
          const wing =
            ax <= 0.68 &&
            ny >= -0.26 + ax * 0.25 &&
            ny <= 0.3 - ax * 0.12 &&
            !(ax > 0.25 && ny > 0.08 && Math.hypot(ax - 0.42, ny - 0.26) < 0.24);
          const ears = ax >= 0.06 && ax <= 0.2 && ny >= -0.42 && ny <= -0.18;
          return ring || wing || ears;
        },
        shadeAt: (cell, _t, sweep) =>
          0.32 + cell.dist * 0.38 + cell.dither * 0.16 - Math.cos(cell.angle - sweep) * 0.26,
      }),
    mascot: draculaMascot,
  });
}

/* ---- 6. Gruvbox Dark: Retro Solar Compass & Amber Bot -------------------------------- */

const gruvboxMascot = makePixelMascot({
  bodyMap: [
    "....AAA....",
    "..BLLLLLB..",
    ".BBBBBBBBB.",
    ".BBEBBBEBB.",
    ".BBBBBBBBB.",
    ".BBDDDDDBB.",
    "..BBBBBBB..",
    "...BB.BB...",
  ],
  feet: ["...B...B...", "..B.....B.."],
  colors: {
    B: "#FABD2F",
    L: "#FBF1C7",
    A: "#FE8019",
    E: "#1B1917",
    D: "#282828",
  },
  blinkBg: "#FABD2F",
  bob: 3,
});

export function createGruvboxScene(): Scene {
  return createSpriteScene({
    seed: 239,
    haloColor: "rgba(250, 189, 47, 0.34)",
    cloudLow: "rgba(213, 196, 161, 0.08)",
    cloudHigh: "rgba(250, 189, 47, 0.14)",
    glow: "rgba(254, 128, 25, 0.08)",
    ambientColors: ["#FABD2F", "#FE8019", "#B8BB26", "#83A598"],
    ambientGlyphs: ["$", ">", "{", "}", "+", ":", ".", ";"],
    sparkColors: ["#FBF1C7", "#FABD2F", "#FE8019"],
    emblemAlpha: 0.9,
    createEmblem: () =>
      createShapeEmblem({
        shades: ["#FBF1C7", "#FABD2F", "#FE8019", "#D65D0E", "#9D4208"],
        ink: "rgba(27, 25, 23, 0.56)",
        sweepSpeed: 1.3,
        floatPx: 5,
        floatSpeed: 1.1,
        // Retro sun compass: double ring + 4 cardinal spikes + solid center
        test: (nx, ny, dist) => {
          const core = dist <= 0.32;
          const ring = dist >= 0.58 && dist <= 0.74;
          const cardinal =
            (Math.abs(nx) <= 0.1 && Math.abs(ny) <= 1.0) ||
            (Math.abs(ny) <= 0.1 && Math.abs(nx) <= 1.0);
          return core || ring || cardinal;
        },
        shadeAt: (cell, _t, sweep) =>
          0.26 + cell.dist * 0.46 + cell.dither * 0.16 - Math.cos(cell.angle - sweep) * 0.25,
      }),
    mascot: gruvboxMascot,
  });
}

/* ---- 7. Matrix Rain: Phosphor Digital Core & Cyber Bug ------------------------------- */

const matrixMascot = makePixelMascot({
  bodyMap: [
    ".A.......A.",
    "..B.....B..",
    ".BLLLLLLLB.",
    ".BBEBBBEBB.",
    "BBBBBBBBBBB",
    ".BBDBBBDBB.",
    "..BBBBBBB..",
    "..B.B.B.B..",
  ],
  feet: ["..B.B.B.B..", ".B..B.B..B."],
  colors: {
    B: "#00E676",
    L: "#80FFBF",
    A: "#39FF14",
    E: "#041208",
    D: "#041208",
  },
  blinkBg: "#00E676",
  bob: 3,
});

export function createMatrixScene(): Scene {
  return createSpriteScene({
    seed: 277,
    haloColor: "rgba(0, 255, 127, 0.32)",
    cloudLow: "rgba(0, 230, 118, 0.08)",
    cloudHigh: "rgba(57, 255, 20, 0.14)",
    glow: "rgba(0, 255, 127, 0.08)",
    ambientColors: ["#00FF7F", "#39FF14", "#00C853"],
    ambientGlyphs: ["0", "1", "ﾊ", "ﾐ", "ﾋ", ":", ".", "+"],
    sparkColors: ["#B9F6CA", "#00FF7F", "#39FF14"],
    emblemAlpha: 0.9,
    createEmblem: () =>
      createShapeEmblem({
        shades: ["#D4FFE8", "#69F0AE", "#00E676", "#00A152", "#005C2E"],
        ink: "rgba(4, 18, 8, 0.62)",
        sweepSpeed: 1.8,
        floatPx: 4,
        floatSpeed: 1.0,
        // Circular digital matrix disc with vertical rain gaps
        test: (nx, _ny, dist) => {
          if (dist > 0.96) return false;
          const colBand = Math.abs(((nx * 7) % 1 + 1) % 1 - 0.5);
          return colBand < 0.34;
        },
        shadeAt: (cell, t) => {
          const colSpeed = 1.6 + (((Math.abs(cell.cx * 17) % 7) / 7) * 1.6);
          const drop = ((cell.ny + 1) * 0.5 - t * colSpeed * 0.35) % 1;
          const trail = drop < 0 ? drop + 1 : drop;
          return trail * 0.85 + cell.dither * 0.12;
        },
      }),
    mascot: matrixMascot,
  });
}
