/**
 * Workaround for @xterm/addon-webgl 0.18 with a transparent background.
 *
 * 1. The renderer decides a cell needs a background rectangle when its `bg` word is
 *    non-zero, but that word also carries the italic and dim flags. Dim or italic text on
 *    the default background therefore got an opaque block in the theme colour, hiding
 *    the animated wallpaper. Cells on the default background need no rectangle at all
 *    (the viewport rectangle already paints it), so those are collapsed to zero size.
 *
 * 2. Cells with an explicit background colour are always painted fully opaque. Claude Code
 *    draws the messages you send on a grey block, which then covered the wallpaper. While
 *    the theme background is transparent (an animated scene is showing), those blocks are
 *    made translucent instead: neutral greys mostly see-through, coloured ones (diffs,
 *    highlights) a little stronger so they stay readable. Inverse-video cells (cursors,
 *    selected menu items) stay opaque.
 */

const FG_INVERSE = 0x4000000;
const COLOR_MODE_MASK = 0x3000000;

/** Opacity of explicit cell backgrounds over an animated wallpaper. */
const NEUTRAL_BG_ALPHA = 0.3;
const COLORED_BG_ALPHA = 0.55;
/** Max - min channel (0..1) under which a colour counts as a neutral grey. */
const NEUTRAL_SPREAD = 0.1;

type UpdateRectangle = (
  this: unknown,
  vertices: { attributes: Float32Array },
  offset: number,
  fg: number,
  bg: number,
  ...rest: number[]
) => void;

interface RectangleRendererProto {
  _updateRectangle?: UpdateRectangle;
  veloxPatched?: boolean;
}

interface RectangleRendererInstance {
  _themeService?: { colors?: { background?: { rgba?: number } } };
}

/** Call after the WebGL addon is loaded. Patches the prototype once for every terminal. */
export function fixTransparentCellBackgrounds(webglAddon: object): void {
  const renderer = (
    webglAddon as { _renderer?: { _rectangleRenderer?: { value?: object } } }
  )._renderer?._rectangleRenderer?.value;
  const proto = renderer && (Object.getPrototypeOf(renderer) as RectangleRendererProto);
  const original = proto?._updateRectangle;
  if (!proto || !original) {
    console.warn("velox: xterm internals changed; dim text may cover the background");
    return;
  }
  if (proto.veloxPatched) return;
  proto.veloxPatched = true;

  proto._updateRectangle = function (vertices, offset, fg, bg, ...rest) {
    original.call(this, vertices, offset, fg, bg, ...rest);
    if (fg & FG_INVERSE) return;
    const attributes = vertices.attributes;
    // Attribute layout is x, y, width, height, r, g, b, a.
    if ((bg & COLOR_MODE_MASK) === 0) {
      attributes[offset + 2] = 0;
      attributes[offset + 3] = 0;
      return;
    }

    // Only while the terminal itself is see-through (theme background alpha 0).
    const themeBg = (this as RectangleRendererInstance)._themeService?.colors?.background?.rgba;
    if (themeBg === undefined || (themeBg & 0xff) !== 0) return;

    const r = attributes[offset + 4];
    const g = attributes[offset + 5];
    const b = attributes[offset + 6];
    const spread = Math.max(r, g, b) - Math.min(r, g, b);
    const alpha = spread < NEUTRAL_SPREAD ? NEUTRAL_BG_ALPHA : COLORED_BG_ALPHA;
    // The renderer blends with SRC_ALPHA / ONE_MINUS_SRC_ALPHA onto a cleared buffer, which
    // stores (rgb * a, a * a), and the page composites that as premultiplied colour. Feeding
    // it sqrt(alpha) and rgb * sqrt(alpha) stores exactly (rgb * alpha, alpha).
    const root = Math.sqrt(alpha);
    attributes[offset + 4] = r * root;
    attributes[offset + 5] = g * root;
    attributes[offset + 6] = b * root;
    attributes[offset + 7] = root;
  };
}
