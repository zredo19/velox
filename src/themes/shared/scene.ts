/**
 * The animated wallpaper shared by every agent: a field of tiny stars, dithered clouds and
 * halo (same technique as the sky in the owner's portfolio), faint drifting glyphs, a block
 * "emblem" top right and a strolling pixel mascot. Each agent only supplies colours, an
 * emblem and a mascot.
 *
 * Performance model: the loop presents ~60 frames a second, but a frame is a handful of
 * drawImage calls.
 *  - Stars that do not twinkle are painted once per resize on a static layer.
 *  - The emblem (hundreds of glyph tiles) and its halo are rendered into an offscreen
 *    keyframe 15 times a second, as before; each display frame cross-fades the two nearest
 *    keyframes, so breathing, sweeping light and pulse move smoothly for two drawImages.
 *  - The mascot is cached as a sprite, re-rendered only when its pose changes, and moved on
 *    whole device pixels (so it stays crisp) on the frames in between.
 *  - Ambient glyphs come from a pre-rendered atlas instead of fillText.
 *  - Nothing uses CSS / Web Animations: those run at the display's refresh rate (144 Hz on
 *    the owner's laptop), and every presented frame costs GPU-process time.
 */
import type { Rect, Scene, SceneLayer } from "../types";
import { context2d, createCanvas as makeCanvas, type AnyCanvas, type Ctx2D } from "./canvas";
import { bayer, mulberry32, smoothstep } from "./dither";

/* Emblem placement, as fractions of the terminal area. */
const EMBLEM_X = 0.78;
const EMBLEM_Y = 0.29;
const NARROW = 640;
const SPARKS = 7;

/* Halo and clouds use a finer dot grid than the emblem blocks. */
const DOT_CELL = 6;
const DOT = 3;
const HALO_REACH = 1.6;

const AMBIENT_COUNT = 60;
const AMBIENT_FONT_PX = 11;

/* Stars: per 10 000 CSS px², and the share of them that twinkles. */
const STAR_DENSITY = 0.75;
const STAR_MAX = 520;
const TWINKLE_SHARE = 0.16;

/** Keyframes per second of the emblem; display frames in between are cross-faded. */
const EMBLEM_KEY_FPS = 15;

const MASCOT_RIGHT = 48;
const MASCOT_BOTTOM = 96;
const MASCOT_STROLL = 40;
const MASCOT_STROLL_SPEED = 0.18;

const CLOUDS = [
  { x: 0.06, y: 0.12, s: 1.0, speed: 0.0055 },
  { x: 0.38, y: 0.06, s: 0.66, speed: 0.0042 },
  { x: 0.52, y: 0.4, s: 1.1, speed: 0.0048 },
  { x: 0.2, y: 0.34, s: 0.8, speed: 0.0034 },
];

const PUFFS: Array<[number, number, number]> = [
  [-1.6, 0.25, 0.72],
  [-0.85, -0.1, 1.0],
  [0, -0.38, 1.22],
  [0.9, -0.06, 1.0],
  [1.7, 0.28, 0.7],
  [0.15, 0.3, 0.92],
];

export interface Emblem {
  /** Called whenever the area changes. `radius` is the emblem's nominal radius in CSS px. */
  prepare(radius: number, dpr: number): void;
  /**
   * Draws the emblem at a fixed centre, in background CSS px. Must stay within 1.6 radii of
   * the centre. Rendered into cached keyframes, so whole-emblem motion belongs in `float`.
   */
  draw(ctx: Ctx2D, timeSec: number, centreX: number, centreY: number): void;
  /** The whole emblem hovers by `px * sin(speed * t)`, applied on every display frame. */
  float?: { px: number; speed: number };
}

export interface Mascot {
  width: number;
  height: number;
  /** Vertical bob, `bob * sin(bobSpeed * t)` px, applied on every display frame. */
  bob: number;
  bobSpeed: number;
  /** Changes whenever `draw` would paint different pixels; the sprite is cached on it. */
  pose(timeSec: number, walking: boolean): number;
  /** Paints the current pose with its top-left at `x`, `y`. */
  draw(
    ctx: Ctx2D,
    x: number,
    y: number,
    timeSec: number,
    walking: boolean,
  ): void;
}

export interface SceneStyle {
  seed: number;
  haloColor: string;
  cloudLow: string;
  cloudHigh: string;
  /** Soft wash around the emblem, painted once as the CSS backdrop. */
  glow: string;
  ambientColors: readonly string[];
  ambientGlyphs: readonly string[];
  sparkColors: readonly string[];
  /** Overall opacity of the emblem. */
  emblemAlpha: number;
  createEmblem: () => Emblem;
  mascot: Mascot;
}

interface Sprite {
  canvas: AnyCanvas;
  /** CSS px */
  width: number;
  height: number;
}

interface Star {
  x: number;
  y: number;
  /** Device px side: 1 or 2. */
  size: number;
  alpha: number;
  twinkle: boolean;
  phase: number;
  speed: number;
  /** A brighter star with a faint 4-point glint while it peaks. */
  glint: boolean;
}

/** Runs `plot` once for every dot-grid cell of an offscreen canvas of the given CSS size. */
function rasterise(
  width: number,
  height: number,
  dpr: number,
  plot: (ctx: Ctx2D, cx: number, cy: number) => void,
): Sprite {
  const canvas = makeCanvas(width * dpr, height * dpr);
  const ctx = context2d(canvas);
  if (ctx) {
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    for (let cy = 0; cy * DOT_CELL < height; cy++) {
      for (let cx = 0; cx * DOT_CELL < width; cx++) plot(ctx, cx, cy);
    }
  }
  return { canvas, width, height };
}

export function createSpriteScene(style: SceneStyle): Scene {
  const rand = mulberry32(style.seed);
  const ambient = Array.from({ length: AMBIENT_COUNT }, (_, i) => ({
    x: rand(),
    y: rand(),
    glyph: Math.floor(rand() * style.ambientGlyphs.length),
    color: i % style.ambientColors.length,
    pixel: rand() < 0.45,
    phase: rand() * Math.PI * 2,
    speed: 0.4 + rand() * 1.2,
  }));
  // Normalised once, so the sky keeps its constellations across resizes.
  const starRand = mulberry32(style.seed * 7919 + 1);
  const starSeeds = Array.from({ length: STAR_MAX }, () => {
    const twinkle = starRand() < TWINKLE_SHARE;
    return {
      x: starRand(),
      // Denser towards the top, like a night sky over a horizon.
      y: Math.pow(starRand(), 1.35),
      big: starRand() < 0.18,
      alpha: 0.24 + starRand() * 0.36,
      twinkle,
      phase: starRand() * Math.PI * 2,
      speed: 0.6 + starRand() * 1.4,
      glint: twinkle && starRand() < 0.22,
    };
  });
  const emblem = style.createEmblem();
  const mascot = style.mascot;

  let width = 0;
  let height = 0;
  let left = 0;
  let dpr = 1;
  let radius = 90;
  let clouds: Sprite[] = [];
  let halo: Sprite | null = null;
  let stars: Star[] = [];
  let twinklers: Star[] = [];
  let glyphAtlas: { canvas: AnyCanvas; tile: number } | null = null;
  // Cached layout geometry: avoids per-frame object allocations in the 60fps loop.
  let cachedCentre = { x: 0, y: 0 };
  let cachedFullBox: Rect = { x: 0, y: 0, width: 0, height: 0 };
  let cachedStarBox: Rect = { x: 0, y: 0, width: 0, height: 0 };
  let cachedWalkerBox: Rect = { x: 0, y: 0, width: 0, height: 0 };

  /* Rare pixel shooting star: one brief diagonal streak every ~14 seconds. */
  const SHOOT_PERIOD_SEC = 14.5;
  const SHOOT_DURATION_SEC = 0.72;
  const drawShootingStar = (ctx: Ctx2D, t: number) => {
    const cycle = Math.floor(t / SHOOT_PERIOD_SEC);
    const local = t - cycle * SHOOT_PERIOD_SEC;
    if (local > SHOOT_DURATION_SEC || width < NARROW) return;
    // Deterministic per-cycle start position in the upper half of the sky.
    const h1 = ((Math.imul(cycle + style.seed, 2654435761) >>> 0) % 1000) / 1000;
    const h2 = ((Math.imul(cycle * 3 + style.seed, 2246822519) >>> 0) % 1000) / 1000;
    const progress = local / SHOOT_DURATION_SEC;
    const ease = 1 - (1 - progress) * (1 - progress);
    const startX = left + (width - left) * (0.15 + h1 * 0.55);
    const startY = height * (0.06 + h2 * 0.24);
    const travel = Math.min(220, width * 0.22);
    const headX = startX + travel * ease;
    const headY = startY + travel * 0.42 * ease;
    const fade = Math.sin(progress * Math.PI);
    const px = 2 / dpr;

    ctx.fillStyle = "#FFFFFF";
    const steps = 7;
    for (let i = 0; i < steps; i++) {
      const k = i / steps;
      ctx.globalAlpha = fade * (1 - k) * 0.65;
      const sx = Math.round((headX - k * 34) * dpr) / dpr;
      const sy = Math.round((headY - k * 14.3) * dpr) / dpr;
      ctx.fillRect(sx, sy, px, px);
    }
    ctx.globalAlpha = 1;
  };

  /* Clouds and halo never change shape, so they are rasterised once per resize
   * and only moved or faded afterwards. */
  const buildCloud = (b: number): Sprite => {
    const w = b * 6;
    const h = b * 3;
    const ccx = w / 2 / DOT_CELL;
    const ccy = (h * 0.58) / DOT_CELL;
    const bc = b / DOT_CELL;
    const bottom = ccy + bc * 0.55;
    return rasterise(w, h, dpr, (ctx, cx, cy) => {
      const flat = cy > bottom ? 1 - smoothstep(0, 2.2, cy - bottom) : 1;
      if (flat <= 0) return;
      let v = 0;
      for (const [px, py, pr] of PUFFS) {
        const d = Math.hypot(cx - (ccx + px * bc), cy - (ccy + py * bc)) / (pr * bc);
        if (d < 1) v = Math.max(v, (1 - smoothstep(0.3, 1, d)) * 0.86 * flat);
      }
      if (v <= bayer(cx, cy)) return;
      ctx.fillStyle = v > 0.7 ? style.cloudHigh : style.cloudLow;
      ctx.fillRect(cx * DOT_CELL, cy * DOT_CELL, DOT, DOT);
    });
  };

  const buildHalo = (): Sprite => {
    const far = radius * HALO_REACH;
    const mid = far / DOT_CELL;
    return rasterise(far * 2, far * 2, dpr, (ctx, cx, cy) => {
      const d = Math.hypot(cx - mid, cy - mid) * DOT_CELL;
      if (d > far) return;
      // Dense near the core and thinning outwards, so the Bayer pattern reads as a glow.
      const v = 0.85 * Math.exp(-Math.max(0, d - radius * 0.25) / (radius * 0.5));
      if (v <= bayer(cx, cy)) return;
      ctx.fillStyle = style.haloColor;
      ctx.fillRect(cx * DOT_CELL, cy * DOT_CELL, DOT, DOT);
    });
  };

  /** One tile per (colour, glyph), so ambient glyphs are a drawImage instead of fillText. */
  const buildGlyphAtlas = () => {
    const tile = Math.ceil(AMBIENT_FONT_PX * 1.4 * dpr);
    const canvas = makeCanvas(tile * style.ambientGlyphs.length, tile * style.ambientColors.length);
    const ctx = context2d(canvas);
    if (ctx) {
      ctx.font = `${AMBIENT_FONT_PX * dpr}px 'JetBrains Mono', Consolas, monospace`;
      ctx.textBaseline = "middle";
      style.ambientColors.forEach((color, row) => {
        ctx.fillStyle = color;
        style.ambientGlyphs.forEach((glyph, col) => {
          ctx.fillText(glyph, col * tile + 1, row * tile + tile / 2);
        });
      });
    }
    return { canvas, tile };
  };

  const buildStars = () => {
    const count = Math.min(STAR_MAX, Math.round(((width * height) / 10000) * STAR_DENSITY));
    stars = starSeeds.slice(0, count).map((s) => ({
      // Whole device pixels: a 1-px star that straddles two pixels turns into a grey smudge.
      x: Math.round(s.x * width * dpr) / dpr,
      y: Math.round(s.y * height * dpr) / dpr,
      size: s.big ? 2 : 1,
      alpha: s.alpha + (s.big ? 0.12 : 0),
      twinkle: s.twinkle,
      phase: s.phase,
      speed: s.speed,
      glint: s.glint,
    }));
    twinklers = stars.filter((s) => s.twinkle);
  };

  const drawStar = (ctx: Ctx2D, s: Star, alpha: number) => {
    const px = s.size / dpr;
    ctx.globalAlpha = alpha;
    ctx.fillRect(s.x, s.y, px, px);
    if (s.glint && alpha > 0.55) {
      const arm = 2 / dpr;
      ctx.globalAlpha = (alpha - 0.55) * 0.9;
      ctx.fillRect(s.x - arm, s.y, px + arm * 2, px);
      ctx.fillRect(s.x, s.y - arm, px, px + arm * 2);
    }
  };

  /** Rounds a CSS coordinate to the device-pixel grid of a layer whose box starts at `origin`. */
  const snap = (value: number, origin: number) => origin + Math.round((value - origin) * dpr) / dpr;

  const drawClouds = (ctx: Ctx2D, t: number) => {
    for (let i = 0; i < clouds.length; i++) {
      const c = CLOUDS[i];
      const sprite = clouds[i];
      const span = width + sprite.width;
      const raw = c.x * width + t * c.speed * width + sprite.width / 2;
      // Whole device pixels keep the dither dots crisp while the cloud glides.
      const x = snap((raw % span) - sprite.width, 0);
      const y = snap(c.y * height - sprite.height * 0.58, 0);
      ctx.drawImage(sprite.canvas, x, y, sprite.width, sprite.height);
    }
  };

  const drawAmbient = (ctx: Ctx2D, t: number) => {
    const areaWidth = width - left;
    const atlas = glyphAtlas;
    for (const a of ambient) {
      // Only over the terminal area: behind the sidebar text they would read as noise.
      const x = left + a.x * areaWidth;
      const y = a.y * height;
      ctx.globalAlpha = 0.1 + 0.2 * (0.5 + 0.5 * Math.sin(t * a.speed + a.phase));
      if (a.pixel || !atlas) {
        ctx.fillStyle = style.ambientColors[a.color];
        ctx.fillRect(x, y, 2.5, 2.5);
      } else {
        const size = atlas.tile / dpr;
        ctx.drawImage(
          atlas.canvas,
          a.glyph * atlas.tile,
          a.color * atlas.tile,
          atlas.tile,
          atlas.tile,
          x,
          y - size / 2,
          size,
          size,
        );
      }
    }
    ctx.globalAlpha = 1;
  };

  /* Small squares orbiting the emblem and blinking; a few fillRects, drawn live. */
  const drawSparks = (ctx: Ctx2D, t: number, x: number, y: number) => {
    for (let i = 0; i < SPARKS; i++) {
      const angle = (i / SPARKS) * Math.PI * 2 + t * (i % 2 ? 0.35 : -0.25);
      const orbit = radius * (0.8 + 0.45 * ((i * 0.37) % 1) + 0.08 * Math.sin(t * 1.3 + i));
      ctx.fillStyle = style.sparkColors[i % style.sparkColors.length];
      ctx.globalAlpha = Math.max(0, Math.sin(t * 2.4 + i * 2.1)) * 0.75;
      ctx.fillRect(x + Math.cos(angle) * orbit, y + Math.sin(angle) * orbit, 3, 3);
    }
    ctx.globalAlpha = 1;
  };

  /* The night sky behind everything: painted once, it never moves. */
  const starfield: SceneLayer = {
    fps: 0,
    box: () => cachedFullBox,
    draw(ctx) {
      ctx.fillStyle = "#FFFFFF";
      for (const s of stars) if (!s.twinkle) drawStar(ctx, s, s.alpha);
      ctx.globalAlpha = 1;
    },
  };

  /* A few stars twinkle, clouds glide (about one device pixel per frame), rare shooting
   * stars streak by, and faint glyphs breathe: all slow enough for 15 fps. */
  const sky: SceneLayer = {
    fps: 15,
    box: () => cachedFullBox,
    draw(ctx, t) {
      ctx.fillStyle = "#FFFFFF";
      for (const s of twinklers) {
        const wave = 0.5 + 0.5 * Math.sin(t * s.speed + s.phase);
        // Mostly calm, with a soft swell now and then.
        drawStar(ctx, s, 0.08 + (s.alpha + 0.35) * wave * wave);
      }
      ctx.globalAlpha = 1;
      drawShootingStar(ctx, t);
      drawClouds(ctx, t);
      drawAmbient(ctx, t);
    },
  };

  /* Emblem keyframes: `a` holds frame `key`, `b` the one after it. */
  const keyframes = {
    a: null as AnyCanvas | null,
    b: null as AnyCanvas | null,
    key: Number.NaN,
  };

  /** Halo and emblem at `t`, with the same transform as the layer, so a blit is 1:1. */
  const renderKeyframe = (target: AnyCanvas, t: number) => {
    const ctx = context2d(target);
    if (!ctx) return;
    const box = cachedStarBox;
    const { x, y } = cachedCentre;
    ctx.setTransform(dpr, 0, 0, dpr, -box.x * dpr, -box.y * dpr);
    ctx.clearRect(box.x, box.y, box.width, box.height);
    if (halo) {
      // The whole keyframe is blitted at emblemAlpha; the halo keeps its own strength.
      ctx.globalAlpha = Math.min(1, (0.85 + 0.15 * Math.sin(t * 1.4)) / style.emblemAlpha);
      ctx.drawImage(halo.canvas, x - halo.width / 2, y - halo.height / 2, halo.width, halo.height);
    }
    ctx.globalAlpha = 1;
    emblem.draw(ctx, t, x, y);
    ctx.globalAlpha = 1;
  };

  const star: SceneLayer = {
    fps: 60,
    box: () => cachedStarBox,
    draw(ctx, t, live) {
      const { a, b } = keyframes;
      if (!a || !b) return;
      const frame = t * EMBLEM_KEY_FPS;
      const key = Math.floor(frame);
      if (!live) {
        // Stepping a few times a second: nothing to interpolate, draw this moment.
        renderKeyframe(a, t);
        keyframes.key = Number.NaN;
      } else if (key !== keyframes.key) {
        if (key === keyframes.key + 1) {
          keyframes.a = b;
          keyframes.b = a;
        } else {
          renderKeyframe(a, key / EMBLEM_KEY_FPS);
        }
        renderKeyframe(keyframes.b!, (key + 1) / EMBLEM_KEY_FPS);
        keyframes.key = key;
      }
      const mix = live ? frame - key : 0;
      const float = emblem.float;
      // Whole device pixels, so the glyph tiles are copied 1:1 and stay sharp.
      const lift = float ? Math.round(Math.sin(t * float.speed) * float.px * dpr) : 0;
      const { x, y } = cachedCentre;

      ctx.save();
      ctx.setTransform(1, 0, 0, 1, 0, 0);
      // Adding two premultiplied frames weighted (1 - mix) and mix is an exact linear
      // interpolation: blocks shared by both stay solid, the rest fade in or out.
      ctx.globalAlpha = style.emblemAlpha * (1 - mix);
      ctx.drawImage(keyframes.a!, 0, lift);
      if (mix > 0.001) {
        ctx.globalCompositeOperation = "lighter";
        ctx.globalAlpha = style.emblemAlpha * mix;
        ctx.drawImage(keyframes.b!, 0, lift);
      }
      ctx.restore();
      drawSparks(ctx, t, x, y);
    },
  };

  /* The mascot sprite, cached per pose. */
  const pad = 2;
  let mascotSprite: AnyCanvas | null = null;
  let mascotPose = Number.NaN;

  const walker: SceneLayer = {
    fps: 60,
    box: () => cachedWalkerBox,
    draw(ctx, t) {
      if (!mascotSprite) return;
      const walking = Math.abs(Math.cos(t * MASCOT_STROLL_SPEED)) > 0.35;
      const pose = mascot.pose(t, walking);
      if (pose !== mascotPose) {
        mascotPose = pose;
        const sctx = context2d(mascotSprite);
        if (sctx) {
          sctx.setTransform(dpr, 0, 0, dpr, 0, 0);
          sctx.clearRect(0, 0, mascot.width + pad * 2, mascot.height + pad * 2);
          mascot.draw(sctx, pad, pad, t, walking);
        }
      }
      const box = cachedWalkerBox;
      const stroll = Math.sin(t * MASCOT_STROLL_SPEED) * MASCOT_STROLL;
      const bob = Math.sin(t * mascot.bobSpeed) * mascot.bob;
      ctx.globalAlpha = 0.9;
      // Whole device pixels of this layer's canvas, so the pixel art is copied 1:1.
      ctx.drawImage(
        mascotSprite,
        snap(width - MASCOT_RIGHT - mascot.width - MASCOT_STROLL + stroll - pad, box.x),
        snap(height - MASCOT_BOTTOM - mascot.height + bob - pad, box.y),
        mascotSprite.width / dpr,
        mascotSprite.height / dpr,
      );
      ctx.globalAlpha = 1;
    },
  };

  return {
    layers: [starfield, sky, star, walker],

    resize(w, h, nextDpr, contentLeft) {
      width = w;
      height = h;
      dpr = nextDpr;
      left = Math.min(Math.max(0, contentLeft), w);
      const area = w - left;
      radius = area < NARROW ? 70 : Math.min(210, Math.max(90, area * 0.125));
      const base = Math.min(70, Math.max(42, area * 0.04));

      cachedCentre = { x: left + area * EMBLEM_X, y: height * EMBLEM_Y };
      cachedFullBox = { x: 0, y: 0, width, height };
      const half = Math.ceil(radius * HALO_REACH + 8);
      cachedStarBox = {
        x: Math.floor(cachedCentre.x) - half,
        y: Math.floor(cachedCentre.y) - half,
        width: half * 2,
        height: half * 2,
      };
      cachedWalkerBox = {
        x: width - MASCOT_RIGHT - mascot.width - MASCOT_STROLL * 2 - 2,
        y: height - MASCOT_BOTTOM - mascot.height - mascot.bob - 2,
        width: mascot.width + MASCOT_STROLL * 2 + 4,
        height: mascot.height + mascot.bob * 2 + 4,
      };

      clouds = area < NARROW ? [] : CLOUDS.map((c) => buildCloud(base * c.s));
      halo = buildHalo();
      glyphAtlas = buildGlyphAtlas();
      buildStars();
      emblem.prepare(radius, dpr);

      keyframes.a = makeCanvas(Math.round(cachedStarBox.width * dpr), Math.round(cachedStarBox.height * dpr));
      keyframes.b = makeCanvas(Math.round(cachedStarBox.width * dpr), Math.round(cachedStarBox.height * dpr));
      keyframes.key = Number.NaN;
      mascotSprite = makeCanvas((mascot.width + pad * 2) * dpr, (mascot.height + pad * 2) * dpr);
      mascotPose = Number.NaN;
    },

    backdrop() {
      const { x, y } = cachedCentre;
      const reach = Math.round(Math.max(width - left, height) * 0.75);
      return `radial-gradient(circle ${reach}px at ${Math.round(x)}px ${Math.round(y)}px, ${style.glow}, transparent)`;
    },
  };
}
