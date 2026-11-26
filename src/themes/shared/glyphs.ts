/** Pre-rendered glyph tiles, so emblem frames only ever call drawImage / fillRect. */

import { context2d, createCanvas, type AnyCanvas } from "./canvas";

export const CELL = 10;
export const BLOCK = 9;

export const GLYPHS = ["t", "%", "S", "X", ";", ":", "."] as const;

export interface GlyphAtlas {
  canvas: AnyCanvas;
  /** Device-pixel size of one tile. */
  tile: number;
}

/** One tile per (background, glyph): columns are glyphs, rows are backgrounds. */
export function createGlyphAtlas(
  dpr: number,
  ink: string,
  backgrounds: readonly (string | null)[],
): GlyphAtlas {
  const tile = Math.ceil(BLOCK * dpr);
  const canvas = createCanvas(tile * GLYPHS.length, tile * backgrounds.length);
  const ctx = context2d(canvas);
  if (ctx) {
    ctx.font = `700 ${Math.round(tile * 0.9)}px 'JetBrains Mono', 'Cascadia Mono', Consolas, monospace`;
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    backgrounds.forEach((background, row) => {
      GLYPHS.forEach((glyph, col) => {
        if (background) {
          ctx.fillStyle = background;
          ctx.fillRect(col * tile, row * tile, tile, tile);
        }
        ctx.fillStyle = ink;
        ctx.fillText(glyph, col * tile + tile / 2, row * tile + tile / 2 + dpr * 0.4);
      });
    });
  }
  return { canvas, tile };
}

/** Compute the shimmer tick once per frame instead of per cell. */
export function shimmerTick(timeSec: number): number {
  return Math.floor((timeSec * 1000) / 150);
}

/** Stable per-cell glyph; about one cell in six changes on each tick, so the emblem shimmers. */
export function shimmerGlyphAtTick(cx: number, cy: number, tick: number): number {
  const hash = (Math.imul(cx, 73856093) ^ Math.imul(cy, 19349663)) >>> 0;
  const swap = (hash + tick) % 6 === 0 ? tick : 0;
  return (hash + swap) % GLYPHS.length;
}

/** Backwards-compatible helper when a caller passes timeSec directly. */
export function shimmerGlyph(cx: number, cy: number, timeSec: number): number {
  return shimmerGlyphAtTick(cx, cy, shimmerTick(timeSec));
}
